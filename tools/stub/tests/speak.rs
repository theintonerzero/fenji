use std::path::Path;
use std::time::Duration;

use fenji_proto::client::{Client, ClientEvent};
use fenji_proto::handshake::{AuthMessage, Message};
use fenji_proto::hello::Variants;
use fenji_proto::packet::set_net_connected;
use fenji_proto::variant::AES128;
use fenji_stub::server::{Mode, serve};
use fenji_stub::speak::{AuthReply, SpeakOpts};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

async fn start(dir: &Path, auth_reply: AuthReply) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let opts = SpeakOpts {
        variants: Variants {
            down: AES128,
            up: AES128,
        },
        auth_reply,
    };
    tokio::spawn(serve(listener, dir.to_path_buf(), Mode::Speak(opts)));
    addr
}

fn auth() -> AuthMessage {
    AuthMessage {
        dict_id: [0x22; 16],
        machine: [0x02, 0, 0, 0, 0, 0x01],
        payload: b"test".to_vec(),
    }
}

/// Reads until `client` reports events or `ms` pass.
async fn pump(s: &mut TcpStream, client: &mut Client, ms: u64) -> Vec<ClientEvent> {
    let mut buf = [0u8; 4096];
    let deadline = tokio::time::Instant::now() + Duration::from_millis(ms);
    loop {
        let read = tokio::time::timeout_at(deadline, s.read(&mut buf)).await;
        let Ok(Ok(n)) = read else { return Vec::new() };
        if n == 0 {
            return Vec::new();
        }
        let (events, out) = client.receive(&buf[..n]).unwrap();
        if !out.is_empty() {
            s.write_all(&out).await.unwrap();
        }
        if !events.is_empty() {
            return events;
        }
    }
}

/// Reads until `done` holds for the events so far, or `ms` pass; returns all events.
async fn pump_until(
    s: &mut TcpStream,
    client: &mut Client,
    ms: u64,
    done: impl Fn(&[ClientEvent]) -> bool,
) -> Vec<ClientEvent> {
    let mut all = Vec::new();
    let mut buf = [0u8; 4096];
    let deadline = tokio::time::Instant::now() + Duration::from_millis(ms);
    while !done(&all) {
        let read = tokio::time::timeout_at(deadline, s.read(&mut buf)).await;
        let Ok(Ok(n)) = read else { break };
        if n == 0 {
            break;
        }
        let (events, out) = client.receive(&buf[..n]).unwrap();
        if !out.is_empty() {
            s.write_all(&out).await.unwrap();
        }
        all.extend(events);
    }
    all
}

fn read_events(dir: &Path) -> String {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "events"))
        .map(|e| std::fs::read_to_string(e.path()).unwrap())
        .collect()
}

/// The events log once it holds every line in `want`, or as it is after `ms` (the asserts that
/// follow then show what is missing). Polling, not a fixed sleep, so a slow machine can't fail it.
async fn events_with(dir: &Path, want: &[&str], ms: u64) -> String {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(ms);
    loop {
        let ev = read_events(dir);
        if want.iter().all(|w| ev.contains(w)) || tokio::time::Instant::now() >= deadline {
            return ev;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn speaks_hello_auth_ok_then_work() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = start(tmp.path(), AuthReply::Ok).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    let mut client = Client::new(auth());
    let work = ClientEvent::Message(Message {
        ty: 0,
        body: set_net_connected(),
    });
    let events = pump_until(&mut s, &mut client, 5000, |e| e.contains(&work)).await;
    assert_eq!(events, [ClientEvent::Accepted, work]);
    // The client's answer: script message 402 to the gate (transport.md 10.3).
    s.write_all(&client.send(0xff, &[0x01, 0x04, 0x92, 0x01]).unwrap())
        .await
        .unwrap();
    s.write_all(&client.time_sync(99)).await.unwrap();
    let ev = events_with(
        tmp.path(),
        &["\tcall\tgateway id=402 args=0", "\ttime_sync\tclient_ms=99"],
        5000,
    )
    .await;
    assert!(ev.contains("\thello\tdown=3 up=3"), "{ev}");
    assert!(
        ev.contains(
            "\tauth\tdict_id=22222222222222222222222222222222 machine=020000000001 payload_len=4"
        ),
        "{ev}"
    );
    assert!(ev.contains("\tsent\tok (stored frame)"), "{ev}");
    assert!(ev.contains("\tsent\twork (SET_NET_CONNECTED)"), "{ev}");
    assert!(ev.contains("\tcall\tgateway id=402 args=0"), "{ev}");
    assert!(ev.contains("\ttime_sync\tclient_ms=99"), "{ev}");
}

#[tokio::test]
async fn undecodable_script_messages_and_other_opcodes_are_logged() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = start(tmp.path(), AuthReply::Ok).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    let mut client = Client::new(auth());
    let work = ClientEvent::Message(Message {
        ty: 0,
        body: set_net_connected(),
    });
    pump_until(&mut s, &mut client, 5000, |e| e.contains(&work)).await;
    // A script message whose pickle the decoder refuses (0D, "magic"), another opcode, then a
    // good call: each gets its own line, and decoding goes on.
    for (ty, body) in [
        (0u8, &[0x01, 0x0D, 0x00][..]),
        (4, &[][..]),
        (0xff, &[0x01, 0x04, 0x92, 0x01][..]),
    ] {
        s.write_all(&client.send(ty, body).unwrap()).await.unwrap();
    }
    let ev = events_with(tmp.path(), &["\tcall\tgateway id=402 args=0"], 5000).await;
    assert!(
        ev.contains("\tbad_call\tscript len=3 head=010d00 error=pickle: \"magic\" objects (0D) aren't supported"),
        "{ev}"
    );
    assert!(ev.contains("\tmessage\tty=4 len=0 head=\n"), "{ev}");
    assert!(ev.contains("\tcall\tgateway id=402 args=0"), "{ev}");
    assert!(!ev.contains("\terror\t"), "{ev}");
}

#[tokio::test]
async fn auth_reply_none_stays_silent() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = start(tmp.path(), AuthReply::None).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    let mut client = Client::new(auth());
    assert!(pump(&mut s, &mut client, 500).await.is_empty());
    let ev = read_events(tmp.path());
    assert!(ev.contains("\tauth\t"), "{ev}");
    assert!(!ev.contains("\tsent\t"), "{ev}");
}

#[tokio::test]
async fn auth_reply_reject_sends_the_reason() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = start(tmp.path(), AuthReply::Reject).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    let mut client = Client::new(auth());
    assert_eq!(
        pump(&mut s, &mut client, 5000).await,
        [ClientEvent::Rejected(
            fenji_stub::speak::REJECT_REASON.to_vec()
        )]
    );
    let ev = events_with(tmp.path(), &["\tsent\treject (stored frame)"], 5000).await;
    assert!(ev.contains("\tsent\treject (stored frame)"), "{ev}");
}
