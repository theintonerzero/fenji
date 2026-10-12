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

/// What the server's bytes complete, as the client reads them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientEvent {
    /// The reply `ok`: the client is in `QUEUE` (transport.md 6.6).
    Accepted,
    /// Any other reply: the close reason, at most 255 bytes (6.6).
    Rejected(Vec<u8>),
    /// A type-1 message in `QUEUE` or `WORK`: the server closed the connection, and this is the
    /// reason, at most 255 bytes (7.4).
    Closed(Vec<u8>),
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

/// Whether `m` moves the client to `WORK`: `PROTOG2C_SET_NET_CONNECTED`, whose handler calls
/// `set_working` (transport.md 10.3) once it has read the billing fields from its second argument.
/// Indexing nil, a boolean or a number fails first (`net.luac`); a table works, and Lua indexes a
/// string through its string library. Lua indexes its handlers by value, so 414 and 414.0 are one
/// id.
fn is_set_net_connected(m: &Message) -> bool {
    if m.ty != Stc::Script.to_u8() {
        return false;
    }
    let Ok(values) = pickle::decode(&m.body) else {
        return false;
    };
    let id = match values.first() {
        Some(Value::Int(id)) => *id == SET_NET_CONNECTED,
        Some(Value::Float(id)) => *id == SET_NET_CONNECTED as f64,
        _ => false,
    };
    id && matches!(values.get(2), Some(Value::Table(_) | Value::Str(_)))
}

/// A close reason as the client keeps it: at most 255 bytes (transport.md 6.6, 7.4).
fn reason(body: &[u8]) -> Vec<u8> {
    body[..body.len().min(MAX_REASON)].to_vec()
}

/// The largest server message the simulator takes: what the inner header's 24-bit length can say.
/// The real client keeps no smaller limit of its own (transport.md 7.1), so the server's
/// `MAX_INBOUND` doesn't apply here.
pub const CLIENT_MAX_MESSAGE: usize = 0xFF_FFFF;

/// One connection, from the client's side: it answers the hello with its auth message (§6), reads
/// the reply, and sends only in `WORK` (7.3).
pub struct Client {
    auth: AuthMessage,
    state: CState,
    down_cipher: Option<Cipher>,
    up_cipher: Option<Cipher>,
    down: DownReader,
    up: UpWriter,
    raw_in: Vec<u8>,
    stream_in: Vec<u8>,
    /// Queued messages not yet cut into frames: one stream (transport.md 7.2).
    stream_out: Vec<u8>,
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
            stream_out: Vec::new(),
        }
    }

    /// Feeds bytes from the server: the events, and the bytes the client sends in reply. After an
    /// error, or once the connection is closed, the client is closed and keeps nothing, as the
    /// real client closes its socket.
    pub fn receive(&mut self, bytes: &[u8]) -> Result<(Vec<ClientEvent>, Vec<u8>)> {
        let got = self.receive_inner(bytes);
        if got.is_err() || self.state == CState::Closed {
            self.state = CState::Closed;
            self.raw_in = Vec::new();
            self.stream_in = Vec::new();
            self.stream_out = Vec::new();
        }
        got
    }

    fn receive_inner(&mut self, bytes: &[u8]) -> Result<(Vec<ClientEvent>, Vec<u8>)> {
        if self.state == CState::Closed {
            return Err(Error::WrongState {
                action: "receive",
                state: "Closed",
            });
        }
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
            write_message(0, &self.auth.to_body(), &mut self.stream_out)?;
            out = self.flush()?;
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
                    // The reply is read only once 5 bytes are buffered (transport.md 6.6).
                    if self.state == CState::InAuth && self.stream_in.len() < 5 {
                        continue;
                    }
                    for m in drain_messages(&mut self.stream_in, CLIENT_MAX_MESSAGE)? {
                        events.push(self.take(m)?);
                        if self.state == CState::Closed {
                            return Ok((events, out));
                        }
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
                Ok(ClientEvent::Rejected(reason(&m.body)))
            }
            // In states 2 and 3, type 1 closes the connection (transport.md 7.4).
            CState::Queued | CState::Work if m.ty == Stc::Close.to_u8() => {
                self.state = CState::Closed;
                Ok(ClientEvent::Closed(reason(&m.body)))
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

    /// Queues one message to the server. Like the real client, only in `WORK` (transport.md 7.3).
    pub fn queue(&mut self, ty: u8, body: &[u8]) -> Result<()> {
        if self.state != CState::Work {
            return Err(Error::WrongState {
                action: "send",
                state: "not Work",
            });
        }
        write_message(ty, body, &mut self.stream_out)
    }

    /// Everything queued, cut into frames of at most `SEND_CHUNK` bytes as the real client cuts
    /// its one outbound stream (transport.md 4.5, 7.2): a frame can hold several messages, and a
    /// message can span frames.
    pub fn flush(&mut self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        if self.stream_out.is_empty() {
            return Ok(out);
        }
        let cipher = self
            .up_cipher
            .as_ref()
            .expect("queued only after the hello");
        for chunk in self.stream_out.chunks(SEND_CHUNK) {
            let mut c = chunk.to_vec();
            cipher.encrypt(&mut c);
            self.up.data(&c, &mut out)?;
        }
        self.stream_out.clear();
        Ok(out)
    }

    /// One message, framed at once: `queue`, then `flush`.
    pub fn send(&mut self, ty: u8, body: &[u8]) -> Result<Vec<u8>> {
        self.queue(ty, body)?;
        self.flush()
    }

    /// A time-sync frame (transport.md 4.6); nothing once the connection is closed.
    pub fn time_sync(&mut self, millis: u64) -> Vec<u8> {
        let mut out = Vec::new();
        if self.state != CState::Closed {
            self.up.time_sync(millis, &mut out);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::DownWriter;
    use crate::handshake::{Event, Server, frame_stream};
    use crate::hello::{Variants, build};
    use crate::pickle::{Table, encode};
    use crate::variant::AES128;

    const V: Variants = Variants {
        down: AES128,
        up: AES128,
    };
    const GATEWAY_402: [u8; 4] = [0x01, 0x04, 0x92, 0x01];

    fn auth() -> AuthMessage {
        AuthMessage {
            dict_id: [0x11; 16],
            machine: [0x02, 0, 0, 0, 0, 0x01],
            payload: b"payload".to_vec(),
        }
    }

    fn fill() -> [u8; HELLO_LEN] {
        core::array::from_fn(|i| (i as u8).wrapping_mul(29).wrapping_add(3))
    }

    /// The server's side written by hand, so a test can send what `Server` won't.
    struct Down {
        cipher: Cipher,
        w: DownWriter,
    }

    impl Down {
        /// Message-stream bytes in stored frames.
        fn frame(&mut self, stream: &[u8]) -> Vec<u8> {
            let cipher = &self.cipher;
            frame_stream(stream, &StoredCodec, |b| cipher.encrypt(b), &mut self.w).unwrap()
        }

        fn message(&mut self, ty: u8, body: &[u8]) -> Vec<u8> {
            let mut m = Vec::new();
            write_message(ty, body, &mut m).unwrap();
            self.frame(&m)
        }
    }

    /// A client past the hello, waiting for the reply to its auth.
    fn connect() -> (Client, Down) {
        let hello = build(fill(), V, CLIENT_MAGIC).unwrap();
        let mut client = Client::new(auth());
        let (events, out) = client.receive(&hello).unwrap();
        assert!(events.is_empty() && !out.is_empty());
        let down = Down {
            cipher: Cipher::new(AES128, &hello).unwrap(),
            w: DownWriter::new(),
        };
        (client, down)
    }

    fn queued() -> (Client, Down) {
        let (mut c, mut d) = connect();
        assert_eq!(
            c.receive(&d.message(0, b"ok")).unwrap().0,
            [ClientEvent::Accepted]
        );
        (c, d)
    }

    /// (id, nil, billing, nil), as `PROTOG2C_SET_NET_CONNECTED` would carry it.
    fn work_body(id: Value, billing: Value) -> Vec<u8> {
        encode(&[id, Value::Nil, billing, Value::Nil]).unwrap()
    }

    /// A `Server` and a client, both in `WORK`.
    fn working() -> (Server, Client) {
        let (mut server, hello) = Server::start(fill(), V).unwrap();
        let mut client = Client::new(auth());
        let (_, up) = client.receive(&hello).unwrap();
        server.receive(&up).unwrap();
        let (events, _) = client
            .receive(&server.accept(&StoredCodec).unwrap())
            .unwrap();
        assert_eq!(events, [ClientEvent::Accepted]);
        client
            .receive(&server.begin_work(&StoredCodec).unwrap())
            .unwrap();
        (server, client)
    }

    #[test]
    fn the_reply_is_read_once_five_bytes_are_there() {
        // transport.md 6.6: the client reads its first message only once 5 bytes are buffered.
        // A bare 4-byte header waits; with one byte more it is read: not `ok`, so a rejection
        // with an empty reason.
        let (mut c, mut d) = connect();
        let mut empty = Vec::new();
        write_message(0, b"", &mut empty).unwrap();
        assert_eq!(c.receive(&d.frame(&empty)).unwrap().0, []);
        assert_eq!(
            c.receive(&d.frame(&[0x00])).unwrap().0,
            [ClientEvent::Rejected(vec![])]
        );
        // And a whole message: `ok` cut after 5 bytes waits for its sixth.
        let (mut c, mut d) = connect();
        let mut ok = Vec::new();
        write_message(0, b"ok", &mut ok).unwrap();
        assert_eq!(c.receive(&d.frame(&ok[..5])).unwrap().0, []);
        assert_eq!(
            c.receive(&d.frame(&ok[5..])).unwrap().0,
            [ClientEvent::Accepted]
        );
    }

    #[test]
    fn a_type_1_message_closes_the_connection_in_queue_and_work() {
        // transport.md 7.4: in states 2 and 3 a type-1 message closes the connection, its body,
        // cut to 255 bytes, the close reason.
        let (mut c, mut d) = queued();
        assert_eq!(
            c.receive(&d.message(1, &[b'x'; 300])).unwrap().0,
            [ClientEvent::Closed(vec![b'x'; 255])]
        );
        assert!(c.receive(&d.message(0, b"later")).is_err());
        let (mut server, mut c) = working();
        let (events, _) = c
            .receive(&server.send(1, b"bye", &StoredCodec).unwrap())
            .unwrap();
        assert_eq!(events, [ClientEvent::Closed(b"bye".to_vec())]);
        assert!(c.send(0xff, &GATEWAY_402).is_err());
    }

    #[test]
    fn queued_messages_share_one_outbound_stream() {
        // transport.md 4.5, 7.2: the client cuts one outbound stream into frames of at most
        // SEND_CHUNK bytes, so a frame can hold several messages and a message can span frames.
        let (mut server, mut c) = working();
        c.queue(0xff, &GATEWAY_402).unwrap();
        c.queue(0, b"abc").unwrap();
        let out = c.flush().unwrap();
        assert_eq!(out.len(), 2 + 8 + 7, "one frame");
        let got = server.receive(&out).unwrap();
        assert_eq!(got.len(), 2, "{got:?}");
        // 0x803 + 5 bytes of stream: a full frame, then 8 bytes holding the second header.
        c.queue(0, &[0; SEND_CHUNK - 1]).unwrap();
        c.queue(0, b"x").unwrap();
        let out = c.flush().unwrap();
        assert_eq!(out.len(), 2 + SEND_CHUNK + 2 + 8);
        let got = server.receive(&out).unwrap();
        assert_eq!(
            got.last(),
            Some(&Event::Message(Message {
                ty: 0,
                body: b"x".to_vec()
            }))
        );
        assert!(c.flush().unwrap().is_empty(), "nothing left");
    }

    #[test]
    fn an_error_closes_the_client_and_drops_what_it_buffered() {
        // As the real client closes its socket (transport.md 5.3): a stored packet whose prefix
        // promises 5 bytes and holds 2, with a good frame behind it.
        let (mut c, mut d) = connect();
        let mut p = vec![0x05, 1, 2];
        d.cipher.encrypt(&mut p);
        let mut bytes = Vec::new();
        d.w.data(&p, &mut bytes).unwrap();
        bytes.extend(d.message(0, b"ok"));
        assert!(c.receive(&bytes).is_err());
        assert!(c.raw_in.is_empty() && c.stream_in.is_empty());
        assert!(matches!(
            c.receive(&d.message(0, b"ok")),
            Err(Error::WrongState { .. })
        ));
    }

    #[test]
    fn a_bad_hello_closes_the_client() {
        // transport.md 2.4: a hello that fails the check closes the connection.
        let mut c = Client::new(auth());
        assert!(matches!(
            c.receive(&[0; HELLO_LEN]),
            Err(Error::BadHello(_))
        ));
        assert!(c.raw_in.is_empty());
        assert!(matches!(
            c.receive(&[0; HELLO_LEN]),
            Err(Error::WrongState { .. })
        ));
    }

    #[test]
    fn only_script_message_414_moves_to_work() {
        // Type 4 holding (414, ...) isn't the script message. (414.0, ...) is: Lua indexes its
        // handlers by value.
        let table = || Value::Table(Table::default());
        let (mut c, mut d) = queued();
        c.receive(&d.message(4, &work_body(Value::Int(414), table())))
            .unwrap();
        assert!(c.send(0xff, &GATEWAY_402).is_err());
        c.receive(&d.message(0, &work_body(Value::Int(415), table())))
            .unwrap();
        assert!(c.send(0xff, &GATEWAY_402).is_err());
        c.receive(&d.message(0, &work_body(Value::Float(414.0), table())))
            .unwrap();
        assert!(c.send(0xff, &GATEWAY_402).is_ok());
    }

    #[test]
    fn work_needs_a_billing_argument_the_handler_can_index() {
        // net.lua's handler reads the billing fields before `set_working`: indexing nil, a
        // boolean or a number fails there. A table works, and Lua indexes a string through its
        // string library.
        for (billing, works) in [
            (Value::Nil, false),
            (Value::Bool(true), false),
            (Value::Int(5), false),
            (Value::Str(b"s".to_vec()), true),
            (Value::Table(Table::default()), true),
        ] {
            let (mut c, mut d) = queued();
            let body = work_body(Value::Int(414), billing.clone());
            c.receive(&d.message(0, &body)).unwrap();
            assert_eq!(c.send(0xff, &GATEWAY_402).is_ok(), works, "{billing:?}");
        }
    }
}
