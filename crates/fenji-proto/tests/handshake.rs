use fenji_proto::Error;
use fenji_proto::client::{Client, ClientEvent};
use fenji_proto::codec::StoredCodec;
use fenji_proto::frame::{SEND_CHUNK, UpWriter};
use fenji_proto::handshake::{AuthMessage, Event, MAX_INBOUND, Message, Server, State};
use fenji_proto::header::write_message;
use fenji_proto::hello::Variants;
use fenji_proto::packet::{script_body, set_net_connected};
use fenji_proto::pickle::{Table, Value, decode};
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

/// Runs hello, auth, `accept` and `begin_work`: both sides in WORK.
fn to_work(seed: u8) -> (Server, Client) {
    let (mut server, mut client, _) = connect(seed);
    let (events, _) = client
        .receive(&server.accept(&StoredCodec).unwrap())
        .unwrap();
    assert_eq!(events, [ClientEvent::Accepted]);
    let (events, _) = client
        .receive(&server.begin_work(&StoredCodec).unwrap())
        .unwrap();
    assert_eq!(events.len(), 1);
    (server, client)
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
    let (mut server, mut client) = to_work(3);
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
    // From its header, in the message's first frame: the server never buffers the body.
    let (mut server, mut client) = to_work(6);
    let up = client.send(1, &vec![0; MAX_INBOUND + 1]).unwrap();
    assert_eq!(
        server.receive(&up[..2 + SEND_CHUNK]),
        Err(Error::BodyTooLong(MAX_INBOUND + 1))
    );
    assert_eq!(server.state(), State::Closed);
}

#[test]
fn the_simulator_takes_server_messages_larger_than_the_servers_own_limit() {
    // The real client keeps no smaller limit for server messages than the header's 24 bits
    // (transport.md 7.1), so the simulator mustn't apply the server's MAX_INBOUND to them.
    let (mut server, mut client) = to_work(41);
    let big = vec![7u8; MAX_INBOUND + 1];
    let bytes = server.send(0, &big, &StoredCodec).unwrap();
    let (events, _) = client.receive(&bytes).unwrap();
    assert!(matches!(&events[..], [ClientEvent::Message(m)] if m.body.len() == big.len()));
}

#[test]
fn a_closed_simulator_sends_no_time_syncs() {
    let (mut server, mut client) = to_work(42);
    let close = server.send(1, b"bye", &StoredCodec).unwrap();
    client.receive(&close).unwrap();
    assert!(client.time_sync(5).is_empty());
}

#[test]
fn an_auth_message_never_shows_its_payload_or_machine_bytes() {
    // The payload carries the ticket (and the MAC and fingerprint): one `?event` in a log line
    // must not print them (M3 spec D3).
    let a = AuthMessage {
        dict_id: [0; 16],
        machine: [0xAB, 0xCD, 0xEF, 0x01, 0x23, 0x45],
        payload: b"fenji1-secretticket".to_vec(),
    };
    for shown in [format!("{a:?}"), format!("{:?}", Event::Auth(a.clone()))] {
        assert!(
            !shown.contains("secret") && !shown.contains("115, 101, 99"),
            "{shown}"
        );
        assert!(
            !shown.to_lowercase().contains("ab, cd") && !shown.contains("171"),
            "{shown}"
        );
    }
}

#[test]
fn an_empty_reject_reason_is_refused() {
    // The client looks at its first message only once 5 bytes have arrived (transport.md 6.6), so
    // a bare 4-byte header would leave it waiting. The session can still answer.
    let (mut server, mut client, _) = connect(10);
    assert!(server.reject(b"", &StoredCodec).is_err());
    assert_eq!(server.state(), State::Authed);
    let (events, _) = client
        .receive(&server.reject(b"x", &StoredCodec).unwrap())
        .unwrap();
    assert_eq!(events, [ClientEvent::Rejected(b"x".to_vec())]);
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

#[test]
fn begin_work_sends_set_net_connected() {
    let (mut server, mut client, _) = connect(10);
    let down = server.accept(&StoredCodec).unwrap();
    assert_eq!(client.receive(&down).unwrap().0, [ClientEvent::Accepted]);
    let down = server.begin_work(&StoredCodec).unwrap();
    assert_eq!(server.state(), State::Work);
    let (events, _) = client.receive(&down).unwrap();
    assert_eq!(
        events,
        [ClientEvent::Message(Message {
            ty: 0,
            body: set_net_connected()
        })]
    );
    // transport.md 10.3: (414, nil, the billing table, nil), the billing fields all 0.
    let billing = Table {
        array: vec![],
        hash: [
            "times_remain",
            "coins_remain",
            "bind_coins_remain",
            "cards_endtime",
        ]
        .iter()
        .map(|k| (Value::Str(k.as_bytes().to_vec()), Value::Int(0)))
        .collect(),
    };
    assert_eq!(
        decode(&set_net_connected()).unwrap(),
        [
            Value::Int(414),
            Value::Nil,
            Value::Table(billing),
            Value::Nil
        ]
    );
    // In WORK the client's messages still arrive as messages.
    let up = client.send(0xff, &[0x01, 0x04, 0x92, 0x01]).unwrap();
    assert_eq!(
        server.receive(&up).unwrap(),
        [Event::Message(Message {
            ty: 0xff,
            body: vec![0x01, 0x04, 0x92, 0x01]
        })]
    );
}

#[test]
fn the_client_sends_only_in_work() {
    // transport.md 7.3 and 10.3: the real client sends only in WORK, which the scripts' handler
    // for 414 starts.
    let gateway_402 = [0x01, 0x04, 0x92, 0x01];
    let (mut server, mut client, _) = connect(12);
    client
        .receive(&server.accept(&StoredCodec).unwrap())
        .unwrap();
    assert!(client.send(0xff, &gateway_402).is_err(), "in QUEUE");
    let other = script_body(415, &[]).unwrap();
    client
        .receive(&server.send(0, &other, &StoredCodec).unwrap())
        .unwrap();
    assert!(
        client.send(0xff, &gateway_402).is_err(),
        "after another script message"
    );
    client
        .receive(&server.begin_work(&StoredCodec).unwrap())
        .unwrap();
    assert!(client.send(0xff, &gateway_402).is_ok(), "in WORK");
}

#[test]
fn begin_work_needs_queued() {
    let (mut server, _, _) = connect(11);
    assert!(server.begin_work(&StoredCodec).is_err(), "before accept");
    server.accept(&StoredCodec).unwrap();
    server.begin_work(&StoredCodec).unwrap();
    assert!(server.begin_work(&StoredCodec).is_err(), "twice");
}

/// How many of the client's bytes the frames carrying its auth took (#102): the stub keeps them
/// out of its session log. A time sync after the auth, in the same read, isn't counted.
#[test]
fn the_server_counts_the_bytes_the_auth_took() {
    let (mut server, hello) = Server::start(fill(7), V).unwrap();
    let mut client = Client::new(auth());
    let (_, up) = client.receive(&hello).unwrap();
    assert_eq!(server.auth_through(), None);
    let mut bytes = up.clone();
    bytes.extend_from_slice(&client.time_sync(7));
    let got = server.receive(&bytes).unwrap();
    assert!(
        matches!(got.as_slice(), [Event::Auth(_), Event::TimeSync(7)]),
        "{got:?}"
    );
    assert_eq!(server.auth_through(), Some(up.len() as u64));

    // The same count when the bytes come one at a time.
    let (mut server, hello) = Server::start(fill(7), V).unwrap();
    let mut client = Client::new(auth());
    let (_, up) = client.receive(&hello).unwrap();
    for b in &up {
        server.receive(std::slice::from_ref(b)).unwrap();
    }
    assert_eq!(server.auth_through(), Some(up.len() as u64));
}

/// `begin_work` with the caller's body, such as a padded one (#102): the client gets it, and the
/// server is in `Work`, as with `begin_work`.
#[test]
fn begin_work_with_sends_the_body_and_reaches_work() {
    let (mut server, mut client, _) = connect(8);
    let body = set_net_connected();
    assert!(
        server.begin_work_with(&body, &StoredCodec).is_err(),
        "not before accept"
    );
    client
        .receive(&server.accept(&StoredCodec).unwrap())
        .unwrap();
    let down = server.begin_work_with(&body, &StoredCodec).unwrap();
    assert_eq!(server.state(), State::Work);
    let (events, _) = client.receive(&down).unwrap();
    assert_eq!(
        events,
        [ClientEvent::Message(Message {
            ty: 0,
            body: body.clone()
        })]
    );
    assert!(
        server.begin_work_with(&body, &StoredCodec).is_err(),
        "only once"
    );
}
