use std::path::Path;
use std::time::Duration;

use fenji_proto::client::{Client, ClientEvent};
use fenji_proto::handshake::AuthMessage;
use fenji_proto::hello::Variants;
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

fn read_events(dir: &Path) -> String {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "events"))
        .map(|e| std::fs::read_to_string(e.path()).unwrap())
        .collect()
}

#[tokio::test]
async fn speaks_hello_auth_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = start(tmp.path(), AuthReply::Ok).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    let mut client = Client::new(auth());
    assert_eq!(
        pump(&mut s, &mut client, 5000).await,
        [ClientEvent::Accepted]
    );
    s.write_all(&client.time_sync(99)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let ev = read_events(tmp.path());
    assert!(ev.contains("\thello\tdown=3 up=3"), "{ev}");
    assert!(
        ev.contains(
            "\tauth\tdict_id=22222222222222222222222222222222 machine=020000000001 payload_len=4"
        ),
        "{ev}"
    );
    assert!(ev.contains("\tsent\tok (stored frame)"), "{ev}");
    assert!(ev.contains("\ttime_sync\tclient_ms=99"), "{ev}");
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
    tokio::time::sleep(Duration::from_millis(200)).await;
    let ev = read_events(tmp.path());
    assert!(ev.contains("\tsent\treject (stored frame)"), "{ev}");
}
