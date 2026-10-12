//! The server's side of the gate handshake (FUN_14029C330 and FUN_14029ADD0 on the client;
//! docs/protocol/transport.md §6). Send the hello, read the client's auth message, then accept
//! (`ok`, the client goes to `QUEUE`) or reject (the reason becomes the client's close reason).
//! Client data is transformed but never Oodle-coded; server data goes through `StreamCodec`.
use crate::codec::StreamCodec;
use crate::frame::{DownWriter, Frame, SEND_CHUNK, UpReader};
use crate::header::{HEADER_LEN, split_message, write_message};
use crate::hello::{CLIENT_MAGIC, HELLO_LEN, Variants, build};
use crate::variant::Cipher;
use crate::{Error, Result};

/// The auth body's fixed part: 16 bytes from the `gate_dictionary.bin` image (DAT_1431FC8E0), then
/// 6 machine bytes (transport.md 6.2–6.4).
pub const AUTH_FIXED: usize = 22;
/// The largest message body this server takes from a client: the client's own auth payload is at
/// most 0x4000 bytes (FUN_14029A900), and nothing it sends early is known to be larger.
pub const MAX_INBOUND: usize = 1 << 20;
/// The client keeps at most this much of a close reason (spike-0 4.28).
pub const MAX_REASON: usize = 255;

/// The client's first message (type 0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthMessage {
    /// The first 16 bytes of the `gate_dictionary.bin` image at DAT_1431FC8E0 (transport.md 6.3).
    pub dict_id: [u8; 16],
    /// The `SendARP` MAC address, or a marker when that fails (F5).
    pub machine: [u8; 6],
    /// `net_client_create`'s payload: `ks.pickle_pack` of the login fields (login.md 2.6).
    pub payload: Vec<u8>,
}

impl AuthMessage {
    pub fn to_body(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(AUTH_FIXED + self.payload.len());
        b.extend_from_slice(&self.dict_id);
        b.extend_from_slice(&self.machine);
        b.extend_from_slice(&self.payload);
        b
    }

    pub fn parse(body: &[u8]) -> Result<Self> {
        if body.len() < AUTH_FIXED {
            return Err(Error::BadAuth(format!(
                "{} bytes, at least 22 expected",
                body.len()
            )));
        }
        Ok(Self {
            dict_id: body[..16].try_into().expect("16 bytes"),
            machine: body[16..22].try_into().expect("6 bytes"),
            payload: body[22..].to_vec(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub ty: u8,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Auth(AuthMessage),
    TimeSync(u64),
    Message(Message),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    AwaitAuth,
    Authed,
    Queued,
    Closed,
}

impl State {
    fn name(self) -> &'static str {
        match self {
            Self::AwaitAuth => "AwaitAuth",
            Self::Authed => "Authed",
            Self::Queued => "Queued",
            Self::Closed => "Closed",
        }
    }
}

/// Splits whole messages off the front of `stream`. Refuses a header that announces more than
/// `MAX_INBOUND` bytes before waiting for them.
pub(crate) fn drain_messages(stream: &mut Vec<u8>) -> Result<Vec<Message>> {
    let mut out = Vec::new();
    let mut at = 0;
    loop {
        if let Some(h) = stream.get(at..at + HEADER_LEN) {
            let len = crate::header::Header::from_bytes(h.try_into().expect("4 bytes")).len;
            if len > MAX_INBOUND {
                return Err(Error::BodyTooLong(len));
            }
        }
        let Some((h, body, used)) = split_message(&stream[at..]) else {
            break;
        };
        out.push(Message {
            ty: h.ty,
            body: body.to_vec(),
        });
        at += used;
    }
    stream.drain(..at);
    Ok(out)
}

/// Encodes `msg` into frames: chunks of at most `SEND_CHUNK` raw bytes, each through `codec`, then
/// the variant transform.
pub(crate) fn frame_stream(
    msg: &[u8],
    codec: &dyn StreamCodec,
    transform: impl Fn(&mut [u8]),
    writer: &mut DownWriter,
) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for chunk in msg.chunks(SEND_CHUNK) {
        let mut payload = Vec::with_capacity(chunk.len() + 5);
        codec.encode(chunk, &mut payload)?;
        transform(&mut payload);
        writer.data(&payload, &mut out)?;
    }
    Ok(out)
}

pub struct Server {
    state: State,
    down_cipher: Cipher,
    up_cipher: Cipher,
    down: DownWriter,
    up: UpReader,
    raw_in: Vec<u8>,
    stream_in: Vec<u8>,
}

impl Server {
    /// A session, and the hello to send first. `fill` should be fresh random bytes.
    pub fn start(fill: [u8; HELLO_LEN], v: Variants) -> Result<(Self, [u8; HELLO_LEN])> {
        let hello = build(fill, v, CLIENT_MAGIC)?;
        let server = Self {
            state: State::AwaitAuth,
            down_cipher: Cipher::new(v.down, &hello)?,
            up_cipher: Cipher::new(v.up, &hello)?,
            down: DownWriter::new(),
            up: UpReader::new(),
            raw_in: Vec::new(),
            stream_in: Vec::new(),
        };
        Ok((server, hello))
    }

    pub fn state(&self) -> State {
        self.state
    }

    /// Feeds bytes read from the client; returns the events they complete. After an error the
    /// session is `Closed`.
    pub fn receive(&mut self, bytes: &[u8]) -> Result<Vec<Event>> {
        let got = self.receive_inner(bytes);
        if got.is_err() {
            self.state = State::Closed;
        }
        got
    }

    fn receive_inner(&mut self, bytes: &[u8]) -> Result<Vec<Event>> {
        if self.state == State::Closed {
            return Err(self.wrong("receive"));
        }
        self.raw_in.extend_from_slice(bytes);
        let mut events = Vec::new();
        let mut at = 0;
        // Messages are taken frame by frame, so events keep the client's order however the bytes
        // were split into reads.
        while let Some((frame, used)) = self.up.read(&self.raw_in[at..]) {
            at += used;
            match frame {
                Frame::TimeSync(ms) => events.push(Event::TimeSync(ms)),
                Frame::Data(mut body) => {
                    self.up_cipher.decrypt(&mut body);
                    self.stream_in.extend_from_slice(&body);
                    for m in drain_messages(&mut self.stream_in)? {
                        events.push(self.take(m)?);
                    }
                }
            }
        }
        self.raw_in.drain(..at);
        Ok(events)
    }

    fn take(&mut self, m: Message) -> Result<Event> {
        match self.state {
            State::AwaitAuth if m.ty == 0 => {
                self.state = State::Authed;
                Ok(Event::Auth(AuthMessage::parse(&m.body)?))
            }
            State::AwaitAuth => Err(Error::BadAuth(format!("first message has type {}", m.ty))),
            State::Queued => Ok(Event::Message(m)),
            _ => Err(self.wrong("take a message")),
        }
    }

    /// Accepts the auth: the 2-byte body `ok`, which moves the client to `QUEUE`.
    pub fn accept(&mut self, codec: &dyn StreamCodec) -> Result<Vec<u8>> {
        if self.state != State::Authed {
            return Err(self.wrong("accept"));
        }
        let out = self.write(0, b"ok", codec)?;
        self.state = State::Queued;
        Ok(out)
    }

    /// Refuses the auth. The client shows `reason` (at most 255 bytes kept) and closes.
    pub fn reject(&mut self, reason: &[u8], codec: &dyn StreamCodec) -> Result<Vec<u8>> {
        if self.state != State::Authed {
            return Err(self.wrong("reject"));
        }
        let reason = &reason[..reason.len().min(MAX_REASON)];
        // A 2-byte reason "ok" would be read as acceptance.
        if reason == b"ok" {
            return Err(Error::BadAuth("a reason of \"ok\" would accept".into()));
        }
        let out = self.write(0, reason, codec)?;
        self.state = State::Closed;
        Ok(out)
    }

    /// One message to a client in `Queued`.
    pub fn send(&mut self, ty: u8, body: &[u8], codec: &dyn StreamCodec) -> Result<Vec<u8>> {
        if self.state != State::Queued {
            return Err(self.wrong("send"));
        }
        self.write(ty, body, codec)
    }

    pub fn time_sync(&mut self, millis: u64) -> Vec<u8> {
        let mut out = Vec::new();
        self.down.time_sync(millis, &mut out);
        out
    }

    fn write(&mut self, ty: u8, body: &[u8], codec: &dyn StreamCodec) -> Result<Vec<u8>> {
        let mut msg = Vec::with_capacity(HEADER_LEN + body.len());
        write_message(ty, body, &mut msg)?;
        let cipher = &self.down_cipher;
        frame_stream(&msg, codec, |b| cipher.encrypt(b), &mut self.down)
    }

    fn wrong(&self, action: &'static str) -> Error {
        Error::WrongState {
            action,
            state: self.state.name(),
        }
    }
}
