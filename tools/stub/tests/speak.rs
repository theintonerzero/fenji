use std::path::Path;
use std::time::Duration;

use fenji_proto::client::{Client, ClientEvent};
use fenji_proto::handshake::{AuthMessage, Message};
use fenji_proto::hello::Variants;
use fenji_proto::packet::set_net_connected;
use fenji_proto::pickle::{Value, decode, encode};
use fenji_proto::variant::AES128;
use fenji_stub::server::{Mode, serve};
use fenji_stub::speak::{AuthReply, SpeakOpts};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

async fn start(dir: &Path, auth_reply: AuthReply) -> std::net::SocketAddr {
    start_with(dir, auth_reply, 0).await
}

async fn start_with(dir: &Path, auth_reply: AuthReply, work_pad: usize) -> std::net::SocketAddr {
    start_opts(dir, auth_reply, work_pad, true).await
}

async fn start_opts(
    dir: &Path,
    auth_reply: AuthReply,
    work_pad: usize,
    capture_auth: bool,
) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let opts = SpeakOpts {
        variants: Variants {
            down: AES128,
            up: AES128,
        },
        auth_reply,
        work_pad,
        capture_auth,
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
    assert!(
        ev.contains("\tsent\twork (SET_NET_CONNECTED); state Work"),
        "{ev}"
    );
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
    // A script message whose pickle the decoder refuses (tag 11, unknown), another opcode, then a
    // good call: each gets its own line, and decoding goes on.
    for (ty, body) in [
        (0u8, &[0x01, 0x11][..]),
        (4, &[][..]),
        (0xff, &[0x01, 0x04, 0x92, 0x01][..]),
    ] {
        s.write_all(&client.send(ty, body).unwrap()).await.unwrap();
    }
    let ev = events_with(tmp.path(), &["\tcall\tgateway id=402 args=0"], 5000).await;
    assert!(
        ev.contains("\tbad_call\tscript len=2 head=0111 error=pickle: unknown tag 0x11"),
        "{ev}"
    );
    assert!(ev.contains("\tmessage\tty=4 len=0 head=\n"), "{ev}");
    assert!(ev.contains("\tcall\tgateway id=402 args=0"), "{ev}");
    assert!(!ev.contains("\terror\t"), "{ev}");
}

#[tokio::test]
async fn the_work_message_can_carry_padding() {
    // --work-pad: a fifth argument, which the handler ignores, makes the message long enough
    // for the stored packet's FC prefix (transport.md 5.2, #88). The client still reaches WORK.
    let tmp = tempfile::tempdir().unwrap();
    let addr = start_with(tmp.path(), AuthReply::Ok, 300).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    let mut client = Client::new(auth());
    let mut values = decode(&set_net_connected()).unwrap();
    values.push(Value::Str(vec![b'.'; 300]));
    let body = encode(&values).unwrap();
    assert!(4 + body.len() >= 0xFC, "past the one-byte prefix");
    let line = format!("\tsent\twork (SET_NET_CONNECTED, {}-byte body)", body.len());
    let work = ClientEvent::Message(Message { ty: 0, body });
    let events = pump_until(&mut s, &mut client, 5000, |e| e.contains(&work)).await;
    assert_eq!(events, [ClientEvent::Accepted, work]);
    // In WORK, so the client may answer.
    s.write_all(&client.send(0xff, &[0x01, 0x04, 0x92, 0x01]).unwrap())
        .await
        .unwrap();
    let ev = events_with(tmp.path(), &[&line, "\tcall\tgateway id=402 args=0"], 5000).await;
    assert!(ev.contains(&format!("{line}; state Work")), "{ev}");
    assert!(ev.contains("\tcall\tgateway id=402 args=0"), "{ev}");
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

/// The file of `dir` with extension `ext` once it holds `want`, or as it is after `ms`.
async fn file_with(dir: &Path, ext: &str, want: &str, ms: u64) -> String {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(ms);
    loop {
        let text: String = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == ext))
            .map(|e| std::fs::read_to_string(e.path()).unwrap())
            .collect();
        if text.contains(want) || tokio::time::Instant::now() >= deadline {
            return text;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// --no-auth-capture (#102): the auth can hold a real platform ticket, so no file keeps it. The
/// session log skips exactly the frames that carried it, and records what follows.
#[tokio::test]
async fn without_auth_capture_no_file_keeps_the_auth() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = start_opts(tmp.path(), AuthReply::Ok, 0, false).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    // A real platform ticket can run to kilobytes: the auth then spans frames and reads.
    let ticket = [b"fenji1-".as_slice(), &[b'x'; 2993]].concat();
    let mut client = Client::new(AuthMessage {
        payload: ticket.clone(),
        ..auth()
    });
    // The hello by hand, so the auth frames the client answers with are known.
    let mut hello = [0u8; fenji_proto::hello::HELLO_LEN];
    s.read_exact(&mut hello).await.unwrap();
    let (_, up) = client.receive(&hello).unwrap();
    for piece in up.chunks(500) {
        s.write_all(piece).await.unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let work = ClientEvent::Message(Message {
        ty: 0,
        body: set_net_connected(),
    });
    pump_until(&mut s, &mut client, 5000, |e| e.contains(&work)).await;
    let call = client.send(0xff, &[0x01, 0x04, 0x92, 0x01]).unwrap();
    s.write_all(&call).await.unwrap();
    let ev = events_with(tmp.path(), &["\tcall\tgateway id=402 args=0"], 5000).await;
    assert!(
        ev.contains(&format!(
            "\tauth\t(not captured) payload_len={}",
            ticket.len()
        )),
        "{ev}"
    );
    assert!(!ev.contains("dict_id") && !ev.contains("machine"), "{ev}");
    drop(s);
    let log = file_with(tmp.path(), "log", "# closed after", 5000).await;
    assert!(
        log.contains(&format!("[auth frames not captured: {} bytes]", up.len())),
        "{log}"
    );
    // Inbound bytes are recorded from the auth's end on, and the call after it is there.
    let first = log.lines().find(|l| l.starts_with("# recv")).unwrap_or("");
    assert!(
        first.ends_with(&format!("at offset {}", up.len())),
        "{first}\n{log}"
    );
    assert!(
        log.contains(&format!("# recv {} bytes", call.len())),
        "{log}"
    );
    assert!(
        log.contains(&format!("# closed after {} bytes", up.len() + call.len())),
        "{log}"
    );
    assert!(
        log.contains("# sent 64 bytes at offset 0"),
        "the hello is still logged"
    );
    let names: Vec<String> = std::fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().all(|n| !n.ends_with(".auth.bin")), "{names:?}");
    // No stub file holds any 8 bytes of the auth's frames, raw or as the log's hex, nor its
    // identifiers or the ticket.
    let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>();
    for name in &names {
        let bytes = std::fs::read(tmp.path().join(name)).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        for at in (0..up.len() - 8).step_by(4) {
            let window = &up[at..at + 8];
            assert!(
                !bytes.windows(8).any(|w| w == window),
                "{name}: raw auth bytes at {at}"
            );
            assert!(
                !text.contains(&hex(window).join(" ")),
                "{name}: auth bytes as hex at {at}"
            );
        }
        let ticket_hex = hex(&ticket[..12]).concat();
        for secret in [
            "22222222222222222222222222222222",
            "020000000001",
            "fenji1-xxxx",
            ticket_hex.as_str(),
        ] {
            assert!(!text.contains(secret), "{name} holds {secret}");
        }
    }
}

#[test]
fn no_auth_capture_needs_speak() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_fenji-stub"))
        .args(["--no-auth-capture", "--listen", "127.0.0.1:0"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("--speak"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
