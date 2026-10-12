//! Real-client fixtures from sandboxed runs (docs/protocol/transport.md §9), scrubbed: the machine
//! bytes and the machine fingerprint in the auth payload are synthetic.
use fenji_proto::codec::StoredCodec;
use fenji_proto::frame::{Frame, UpReader};
use fenji_proto::handshake::{AuthMessage, Event, MAX_INBOUND, Server};
use fenji_proto::header::split_message;
use fenji_proto::hello::{CLIENT_MAGIC, check};
use fenji_proto::packet::Up;
use fenji_proto::pickle::{Value, decode};
use fenji_proto::variant::Cipher;

fn unhex(s: &str) -> Vec<u8> {
    let s: String = s.split_whitespace().collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn hello(hex: &str) -> [u8; 64] {
    unhex(hex).try_into().unwrap()
}

fn s(x: &str) -> Value {
    Value::Str(x.as_bytes().to_vec())
}

/// m2-r1's auth message, read with the frame, cipher and header layers alone (no `Server`).
fn r2_auth() -> AuthMessage {
    let hello = hello(include_str!("fixtures/r2-hello.hex"));
    let up = unhex(include_str!("fixtures/r2-up.hex"));
    let v = check(&hello, CLIENT_MAGIC).unwrap();
    let cipher = Cipher::new(v.up, &hello).unwrap();
    let mut reader = UpReader::new();
    let mut stream = Vec::new();
    let mut at = 0;
    while let Some((frame, used)) = reader.read(&up[at..]) {
        at += used;
        if let Frame::Data(mut body) = frame {
            cipher.decrypt(&mut body);
            stream.extend(body);
        }
    }
    assert_eq!(at, up.len(), "whole frames");
    let (h, body, used) = split_message(&stream, MAX_INBOUND)
        .unwrap()
        .expect("one whole message");
    assert_eq!((h.ty(), used), (0, stream.len()), "one auth message");
    AuthMessage::parse(body).unwrap()
}

/// The scrubbing both auth fixtures share: synthetic machine bytes, and payload value 9, the
/// machine fingerprint, zeroed (payloads.md). The other values are the run's own synthetic
/// `gj_uinfo`/`gj_ticket` values and client facts.
fn assert_scrubbed(auth: &AuthMessage) {
    assert_eq!(auth.machine, [0x02, 0, 0, 0, 0, 0x01]);
    let values = decode(&auth.payload).unwrap();
    assert_eq!(values.len(), 10);
    assert_eq!(
        values[8],
        Value::Str(vec![0; 20]),
        "fingerprint must be zeroed"
    );
}

#[test]
fn r2_client_auth_decodes() {
    assert!(!r2_auth().payload.is_empty());
}

#[test]
fn fixture_is_scrubbed() {
    assert_scrubbed(&r2_auth());
}

#[test]
fn r2_payload_decodes_to_the_login_fields() {
    let values = decode(&r2_auth().payload).unwrap();
    assert_eq!(values[0], s("fenji-dev"), "the ticket (gj_ticket)");
    assert_eq!(values[2], s("tester"), "the account (gj_uinfo)");
    assert_eq!(values[5], s("2.1.19.42851"), "the client version");
    assert_eq!(values[9], s("en-GB"), "the language");
}

/// m2-r4b: the stub's hello, its two stored frames after the auth (`ok`, then
/// SET_NET_CONNECTED), and the client's first three frames: the auth (scrubbed), a time sync and
/// GATE_SCRIPT 402.
struct R4b {
    hello: [u8; 64],
    down: Vec<u8>,
    up: Vec<u8>,
}

/// The client's auth frame: 2 length bytes and 99 body bytes.
const R4B_AUTH_FRAME: usize = 101;

fn r4b() -> R4b {
    R4b {
        hello: hello(include_str!("fixtures/r4b-hello.hex")),
        down: unhex(include_str!("fixtures/r4b-down.hex")),
        up: unhex(include_str!("fixtures/r4b-up.hex")),
    }
}

/// A `Server` that sends this hello. `hello::build` turns a fill's first two words w into
/// (w % SEEDS) * 27 + the variant; a hello word h was built that way, so the fill word h / 27
/// gives h back.
fn server_for(hello: &[u8; 64]) -> Server {
    let mut fill = *hello;
    for w in fill.as_chunks_mut::<4>().0.iter_mut().take(2) {
        *w = (u32::from_le_bytes(*w) / 27).to_le_bytes();
    }
    let (server, sent) = Server::start(fill, check(hello, CLIENT_MAGIC).unwrap()).unwrap();
    assert_eq!(&sent, hello);
    server
}

/// Takes the server through the auth, `accept` and `begin_work`; returns the auth and the
/// bytes it sent.
fn r4b_to_work(server: &mut Server, up: &[u8]) -> (AuthMessage, Vec<u8>) {
    let events = server.receive(&up[..R4B_AUTH_FRAME]).unwrap();
    let [Event::Auth(auth)] = events.as_slice() else {
        panic!("{events:?}")
    };
    let mut sent = server.accept(&StoredCodec).unwrap();
    sent.extend(server.begin_work(&StoredCodec).unwrap());
    (auth.clone(), sent)
}

#[test]
fn r4b_the_server_sends_what_the_client_accepted() {
    // In m2-r4b the real client took these bytes to WORK ("[net]net connected...") and
    // answered with GATE_SCRIPT 402.
    let r = r4b();
    let mut server = server_for(&r.hello);
    let (_, sent) = r4b_to_work(&mut server, &r.up);
    assert_eq!(sent, r.down);
}

#[test]
fn r4b_client_gateway_402_decodes() {
    let r = r4b();
    let mut server = server_for(&r.hello);
    r4b_to_work(&mut server, &r.up);
    let events = server.receive(&r.up[R4B_AUTH_FRAME..]).unwrap();
    let [Event::TimeSync(ms), Event::Message(m)] = events.as_slice() else {
        panic!("{events:?}")
    };
    assert_eq!(*ms, 1_791_778_386_550, "the client's clock, ms since 1970");
    assert_eq!(
        Up::decode(m.ty, &m.body).unwrap(),
        Up::Script {
            gateway: true,
            id: Value::Int(402),
            args: vec![]
        }
    );
}

#[test]
fn r4b_fixture_is_scrubbed() {
    let r = r4b();
    let (auth, _) = r4b_to_work(&mut server_for(&r.hello), &r.up);
    assert_scrubbed(&auth);
}

#[test]
#[ignore = "needs m2-r5's auth body (local only): FENJI_M2R5_AUTH=$PWD/re/stub-logs/m2-r5/<stem>.auth.bin cargo test -p fenji-proto --test fixtures -- --ignored"]
fn r5_compressed_auth_payload_decodes() {
    // The server-list path's auth payload is over 128 bytes, so the client compressed it
    // (payloads.md P.1, P.5, P.6).
    // An absolute path: cargo runs this test from crates/fenji-proto.
    let path = std::env::var_os("FENJI_M2R5_AUTH").expect("set FENJI_M2R5_AUTH");
    let auth = AuthMessage::parse(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(auth.payload[0], 0xff, "compressed");
    let values = decode(&auth.payload).unwrap();
    assert_eq!(values.len(), 10);
    assert_eq!(values[5], s("2.1.19.42851"), "the client version");
    assert_eq!(values[9], s("de-DE"), "the language");
}
