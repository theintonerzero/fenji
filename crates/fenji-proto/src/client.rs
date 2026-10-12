//! A stand-in for the real client's transport (FUN_14029ADD0 and its callees), for tests and the
//! bot client (parent spec §9). It reads only `StoredCodec` packets: it has no Oodle decoder.
use crate::codec::{StoredCodec, StreamCodec};
use crate::frame::{DownReader, Frame, SEND_CHUNK, UpWriter};
use crate::handshake::{AuthMessage, MAX_REASON, Message, drain_messages};
use crate::header::write_message;
use crate::hello::{CLIENT_MAGIC, HELLO_LEN, check};
use crate::opcode::Stc;
use crate::packet::SET_NET_CONNECTED;
use crate::pickle::{self, Value};
use crate::variant::Cipher;
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientEvent {
    Accepted,
    Rejected(Vec<u8>),
    TimeSync(u64),
    Message(Message),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CState {
    AwaitHello,
    InAuth,
    Queued,
    Work,
    Closed,
}

/// Whether `m` is `PROTOG2C_SET_NET_CONNECTED`, whose script handler calls `set_working`
/// (transport.md 10.3). Lua indexes its handler table by value, so 414 and 414.0 are one id.
fn is_set_net_connected(m: &Message) -> bool {
    if m.ty != Stc::Script.to_u8() {
        return false;
    }
    match pickle::decode(&m.body).ok().as_deref() {
        Some([Value::Int(id), ..]) => *id == SET_NET_CONNECTED,
        Some([Value::Float(id), ..]) => *id == SET_NET_CONNECTED as f64,
        _ => false,
    }
}

pub struct Client {
    auth: AuthMessage,
    state: CState,
    down_cipher: Option<Cipher>,
    up_cipher: Option<Cipher>,
    down: DownReader,
    up: UpWriter,
    raw_in: Vec<u8>,
    stream_in: Vec<u8>,
}

impl Client {
    pub fn new(auth: AuthMessage) -> Self {
        Self {
            auth,
            state: CState::AwaitHello,
            down_cipher: None,
            up_cipher: None,
            down: DownReader::new(),
            up: UpWriter::new(),
            raw_in: Vec::new(),
            stream_in: Vec::new(),
        }
    }

    /// Feeds bytes from the server: the events, and the bytes the client sends in reply.
    pub fn receive(&mut self, bytes: &[u8]) -> Result<(Vec<ClientEvent>, Vec<u8>)> {
        self.raw_in.extend_from_slice(bytes);
        let mut events = Vec::new();
        let mut out = Vec::new();
        if self.state == CState::AwaitHello {
            let Some(h) = self.raw_in.get(..HELLO_LEN) else {
                return Ok((events, out));
            };
            let hello: [u8; HELLO_LEN] = h.try_into().expect("64 bytes");
            let v = check(&hello, CLIENT_MAGIC)?;
            self.down_cipher = Some(Cipher::new(v.down, &hello)?);
            self.up_cipher = Some(Cipher::new(v.up, &hello)?);
            self.raw_in.drain(..HELLO_LEN);
            self.state = CState::InAuth;
            let body = self.auth.to_body();
            out = self.frames(0, &body)?;
        }
        let mut at = 0;
        while let Some((frame, used)) = self.down.read(&self.raw_in[at..]) {
            at += used;
            match frame {
                Frame::TimeSync(ms) => events.push(ClientEvent::TimeSync(ms)),
                Frame::Data(mut body) => {
                    self.down_cipher
                        .as_ref()
                        .expect("after hello")
                        .decrypt(&mut body);
                    let raw = StoredCodec.decode(&body)?;
                    self.stream_in.extend_from_slice(&raw);
                    for m in drain_messages(&mut self.stream_in)? {
                        events.push(self.take(m)?);
                    }
                }
            }
        }
        self.raw_in.drain(..at);
        Ok((events, out))
    }

    fn take(&mut self, m: Message) -> Result<ClientEvent> {
        match self.state {
            CState::InAuth if m.body == b"ok" => {
                self.state = CState::Queued;
                Ok(ClientEvent::Accepted)
            }
            CState::InAuth => {
                self.state = CState::Closed;
                let n = m.body.len().min(MAX_REASON);
                Ok(ClientEvent::Rejected(m.body[..n].to_vec()))
            }
            CState::Queued => {
                if is_set_net_connected(&m) {
                    self.state = CState::Work;
                }
                Ok(ClientEvent::Message(m))
            }
            CState::Work => Ok(ClientEvent::Message(m)),
            _ => Err(Error::WrongState {
                action: "take a message",
                state: "Closed",
            }),
        }
    }

    /// One message to the server. Like the real client, only in `WORK` (transport.md 7.3).
    pub fn send(&mut self, ty: u8, body: &[u8]) -> Result<Vec<u8>> {
        if self.state != CState::Work {
            return Err(Error::WrongState {
                action: "send",
                state: "not Work",
            });
        }
        self.frames(ty, body)
    }

    pub fn time_sync(&mut self, millis: u64) -> Vec<u8> {
        let mut out = Vec::new();
        self.up.time_sync(millis, &mut out);
        out
    }

    fn frames(&mut self, ty: u8, body: &[u8]) -> Result<Vec<u8>> {
        let mut msg = Vec::new();
        write_message(ty, body, &mut msg)?;
        let cipher = self.up_cipher.as_ref().expect("after hello");
        let mut out = Vec::new();
        for chunk in msg.chunks(SEND_CHUNK) {
            let mut c = chunk.to_vec();
            cipher.encrypt(&mut c);
            self.up.data(&c, &mut out)?;
        }
        Ok(out)
    }
}
