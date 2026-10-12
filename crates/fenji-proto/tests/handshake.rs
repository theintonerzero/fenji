use fenji_proto::client::{Client, ClientEvent};
use fenji_proto::codec::StoredCodec;
use fenji_proto::frame::{SEND_CHUNK, UpWriter};
use fenji_proto::handshake::{AuthMessage, Event, MAX_INBOUND, Message, Server, State};
use fenji_proto::header::write_message;
use fenji_proto::hello::Variants;
use fenji_proto::variant::{AES128, Cipher};

const V: Variants = Variants {
    down: AES128,
    up: AES128,
};

fn auth() -> AuthMessage {
    AuthMessage {
        dict_id: [0x11; 16],
        machine: [0x02, 0, 0, 0, 0, 0x01],
        payload: b"synthetic payload".to_vec(),
    }
}

fn fill(seed: u8) -> [u8; 64] {
    core::array::from_fn(|i| (i as u8).wrapping_mul(29).wrapping_add(seed))
}

/// Runs hello and auth; returns the pair and the server's auth event.
fn connect(seed: u8) -> (Server, Client, AuthMessage) {
    let (mut server, hello) = Server::start(fill(seed), V).unwrap();
    let mut client = Client::new(auth());
    let (events, up) = client.receive(&hello).unwrap();
    assert!(events.is_empty());
    let got = server.receive(&up).unwrap();
    let [Event::Auth(a)] = got.as_slice() else {
        panic!("{got:?}")
    };
    assert_eq!(server.state(), State::Authed);
    (server, client, a.clone())
}

#[test]
fn hello_auth_ok() {
    let (mut server, mut client, a) = connect(1);
    assert_eq!(a, auth());
    let down = server.accept(&StoredCodec).unwrap();
    assert_eq!(server.state(), State::Queued);
    let (events, _) = client.receive(&down).unwrap();
    assert_eq!(events, [ClientEvent::Accepted]);
}

#[test]
fn reject_carries_the_reason() {
    let (mut server, mut client, _) = connect(2);
    let down = server.reject(b"server full", &StoredCodec).unwrap();
    assert_eq!(server.state(), State::Closed);
    let (events, _) = client.receive(&down).unwrap();
    assert_eq!(events, [ClientEvent::Rejected(b"server full".to_vec())]);
}

#[test]
fn large_messages_cross_frames_both_ways() {
    let (mut server, mut client, _) = connect(3);
    let down = server.accept(&StoredCodec).unwrap();
    client.receive(&down).unwrap();
    let big: Vec<u8> = (0..10_000u32).map(|i| (i * 31) as u8).collect();
    let down = server.send(9, &big, &StoredCodec).unwrap();
    let (events, _) = client.receive(&down).unwrap();
    assert_eq!(
        events,
        [ClientEvent::Message(Message {
            ty: 9,
            body: big.clone()
        })]
    );
    let up = client.send(4, &big).unwrap();
    let events = server.receive(&up).unwrap();
    assert_eq!(events, [Event::Message(Message { ty: 4, body: big })]);
}

#[test]
fn time_syncs_both_ways() {
    let (mut server, mut client, _) = connect(4);
    let up = client.time_sync(123_456);
    assert_eq!(server.receive(&up).unwrap(), [Event::TimeSync(123_456)]);
    let down = server.time_sync(654_321);
    let (events, _) = client.receive(&down).unwrap();
    assert_eq!(events, [ClientEvent::TimeSync(654_321)]);
}

#[test]
fn byte_at_a_time_matches_one_read() {
    let (mut server, hello) = Server::start(fill(5), V).unwrap();
    let mut client = Client::new(auth());
    let mut up = Vec::new();
    for b in hello {
        let (events, bytes) = client.receive(&[b]).unwrap();
        assert!(events.is_empty());
        up.extend(bytes);
    }
    up.extend(client.time_sync(7));
    let mut events = Vec::new();
    for b in &up {
        events.extend(server.receive(std::slice::from_ref(b)).unwrap());
    }
    assert_eq!(events, [Event::Auth(auth()), Event::TimeSync(7)]);
    // The same bytes in one read give the same events in the same order.
    let (mut again, _) = Server::start(fill(5), V).unwrap();
    assert_eq!(again.receive(&up).unwrap(), events);
}

#[test]
fn oversized_inbound_message_is_refused() {
    let (mut server, mut client, _) = connect(6);
    let down = server.accept(&StoredCodec).unwrap();
    client.receive(&down).unwrap();
    let up = client.send(1, &vec![0; MAX_INBOUND + 1]).unwrap();
    assert!(server.receive(&up).is_err());
}

#[test]
fn junk_never_panics() {
    // A deterministic xorshift stream: 200 junk inputs of up to 4 KiB, before and after the hello.
    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for _ in 0..200 {
        let len = (next() % 4096) as usize;
        let junk: Vec<u8> = (0..len).map(|_| next() as u8).collect();
        let (mut server, _) = Server::start(fill(7), V).unwrap();
        let _ = server.receive(&junk);
        let mut client = Client::new(auth());
        let _ = client.receive(&junk);
    }
}

#[test]
fn send_before_accept_is_refused() {
    let (mut server, _, _) = connect(8);
    assert!(server.send(1, b"x", &StoredCodec).is_err());
}

#[test]
fn a_frame_boundary_inside_a_header_and_several_messages_per_frame() {
    // The real client cuts one outbound message stream into frames (transport.md 4.5, 7.2), so
    // a header can straddle two frames and one frame can hold several messages.
    for one_read in [true, false] {
        let (mut server, hello) = Server::start(fill(9), V).unwrap();
        let cipher = Cipher::new(AES128, &hello).unwrap();
        let mut w = UpWriter::new();
        let mut frames = |stream: &[u8]| {
            let mut out = Vec::new();
            for c in stream.chunks(SEND_CHUNK) {
                let mut c = c.to_vec();
                cipher.encrypt(&mut c);
                w.data(&c, &mut out).unwrap();
            }
            out
        };
        let mut s = Vec::new();
        write_message(0, &auth().to_body(), &mut s).unwrap();
        assert_eq!(server.receive(&frames(&s)).unwrap(), [Event::Auth(auth())]);
        server.accept(&StoredCodec).unwrap();
        let a = vec![0xaa; SEND_CHUNK - 6]; // A (4 + 0x7fa bytes) ends 2 bytes before the cut
        let mut s = Vec::new();
        write_message(5, &a, &mut s).unwrap();
        write_message(6, b"bee", &mut s).unwrap(); // B's header straddles the cut
        write_message(7, b"", &mut s).unwrap(); // C shares B's frame
        let up = frames(&s);
        let got: Vec<Event> = if one_read {
            server.receive(&up).unwrap()
        } else {
            up.iter()
                .flat_map(|b| server.receive(std::slice::from_ref(b)).unwrap())
                .collect()
        };
        assert_eq!(
            got,
            [
                Event::Message(Message { ty: 5, body: a }),
                Event::Message(Message {
                    ty: 6,
                    body: b"bee".to_vec()
                }),
                Event::Message(Message {
                    ty: 7,
                    body: vec![]
                }),
            ],
            "one_read {one_read}"
        );
    }
}
