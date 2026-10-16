//! Attaching to a running server (M4 hosting spec §3.5): the console socket in the data folder,
//! one server per data folder, and the attached console's side. No test here starts a process:
//! one being started holds a copy of the test's open files until it runs, which would keep a
//! socket or a lock a test has just let go of. `fenji-server console` itself is in
//! `tests/console.rs`.
#![cfg(unix)]
use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use fenji_console::wire::{self, ConsoleMsg, ServerMsg, State};
use fenji_server::clock::TestClock;
use fenji_server::config::Config;
use fenji_server::console;
use fenji_server::console::log::LogHub;
use fenji_server::server::{GateLimits, RunningServer, start};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::task::JoinHandle;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;

const NOW: i64 = 1_800_000_000_000;

/// How long a test waits for what a server sends.
const WAIT: Duration = Duration::from_secs(30);

/// A server on free loopback ports, its console socket served from its own log hub, which this
/// test's thread logs to.
struct Started {
    server: RunningServer,
    serving: JoinHandle<()>,
    _dir: tempfile::TempDir,
    _log: tracing::subscriber::DefaultGuard,
    _asked: tracing::Dispatch,
}

impl Deref for Started {
    type Target = RunningServer;

    fn deref(&self) -> &RunningServer {
        &self.server
    }
}

/// A server in a temp folder, with its socket served.
async fn started_with_socket() -> Started {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().to_owned();
    started_at(dir, &data).await
}

/// A server whose data folder is `data`, in `dir`, with its socket served.
async fn started_at(dir: tempfile::TempDir, data: &Path) -> Started {
    // While one dispatcher alone is registered, tracing-core keeps each place's interest as the
    // first thread to log there saw it (M4 hosting plan, Task 10's finding): a second one makes it
    // ask them all, so this test's lines reach its hub whatever other threads did first.
    let _asked = tracing::Dispatch::new(tracing_subscriber::registry());
    let hub = LogHub::new(500);
    let _log = tracing::subscriber::set_default(
        tracing_subscriber::registry()
            .with(LevelFilter::INFO)
            .with(hub.layer()),
    );
    let claim = console::socket::claim(data, None).await.unwrap();
    let cfg = Config::parse(
        "[server]\nname = \"Fenji\"\nlogin_bind = \"127.0.0.1:0\"\napi_bind = \"127.0.0.1:0\"\n",
        data,
    )
    .unwrap();
    let server = start(&cfg, Arc::new(TestClock::at(NOW)), GateLimits::default())
        .await
        .unwrap();
    let serving = tokio::spawn(console::socket::serve(claim, server.console(), hub));
    Started {
        server,
        serving,
        _dir: dir,
        _log,
        _asked,
    }
}

/// A console that speaks the socket's JSON lines by hand.
struct Raw {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
}

impl Raw {
    async fn connect(socket: &Path) -> Raw {
        let (read, writer) = UnixStream::connect(socket).await.unwrap().into_split();
        Raw {
            reader: BufReader::new(read),
            writer,
        }
    }

    /// The next message, which must come within [`WAIT`].
    async fn next(&mut self) -> ServerMsg {
        self.try_next()
            .await
            .expect("the server closed the console")
    }

    /// The next message within [`WAIT`], or `None` once the server has closed the console. Each
    /// line must be within the wire's limit, as `decode_server` checks.
    async fn try_next(&mut self) -> Option<ServerMsg> {
        let mut line = String::new();
        let read = tokio::time::timeout(WAIT, self.reader.read_line(&mut line))
            .await
            .expect("a message within 30 s");
        match read {
            Ok(0) | Err(_) => None,
            Ok(_) => {
                Some(wire::decode_server(&line).unwrap_or_else(|e| panic!("{e}: {:.200}", line)))
            }
        }
    }

    async fn send(&mut self, msg: &ConsoleMsg) {
        self.send_raw(&wire::encode(msg)).await;
    }

    /// `line` and a line ending, as they are.
    async fn send_raw(&mut self, line: &str) {
        self.writer
            .write_all(format!("{line}\n").as_bytes())
            .await
            .unwrap();
    }

    /// Whether a log line containing `text` comes before the server says goodbye.
    async fn until_log_contains(&mut self, text: &str) -> bool {
        loop {
            match self.try_next().await {
                Some(ServerMsg::Log { line }) if line.message().contains(text) => return true,
                Some(ServerMsg::Bye { .. }) | None => return false,
                Some(_) => {}
            }
        }
    }

    /// The answer to command `id`, its parts put together: whether it ran, and its lines.
    async fn answer(&mut self, id: u64) -> (bool, Vec<String>) {
        let mut lines = Vec::new();
        loop {
            match self.next().await {
                ServerMsg::Answer {
                    id: got,
                    ok,
                    lines: part,
                    more,
                } if got == id => {
                    lines.extend(part);
                    if !more {
                        return (ok, lines);
                    }
                }
                ServerMsg::Bye { reason } => panic!("goodbye before answer {id}: {reason}"),
                _ => {}
            }
        }
    }

    /// The reason of the goodbye that comes next, after whatever else.
    async fn until_bye(&mut self) -> String {
        loop {
            if let ServerMsg::Bye { reason } = self.next().await {
                return reason;
            }
        }
    }
}

/// The status line of the account API's answer to `GET /api/v1/server`, within [`WAIT`].
async fn api_status(api: std::net::SocketAddr) -> String {
    let asked = async {
        let mut tcp = tokio::net::TcpStream::connect(api).await.unwrap();
        tcp.write_all(b"GET /api/v1/server HTTP/1.1\r\nHost: fenji\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut answer = String::new();
        BufReader::new(tcp).read_line(&mut answer).await.unwrap();
        answer.trim_end().to_owned()
    };
    tokio::time::timeout(WAIT, asked)
        .await
        .expect("the API answers")
}

fn run(id: u64, line: &str) -> ConsoleMsg {
    ConsoleMsg::Run {
        id,
        line: line.into(),
    }
}

#[tokio::test]
async fn a_console_attaches_sees_the_backlog_and_runs_a_command() {
    let s = started_with_socket().await; // a server in a temp folder, with its socket served
    tracing::info!("before the console");
    let mut c = Raw::connect(&s.data().join("console.sock")).await; // a test helper: JSON lines
    assert!(matches!(c.next().await, ServerMsg::Hello { proto: 1, .. }));
    assert!(c.until_log_contains("before the console").await);
    c.send(&ConsoleMsg::Run {
        id: 9,
        line: "online".into(),
    })
    .await;
    assert_eq!(c.answer(9).await, (true, vec!["nobody is online".into()]));
}

#[tokio::test]
async fn a_second_server_on_the_same_data_folder_refuses_before_opening_anything() {
    // Review Focus 5.
    let s = started_with_socket().await;
    let e = console::socket::claim(&s.data(), None).await.unwrap_err();
    assert!(
        format!("{e:#}").contains("already using this data folder"),
        "{e:#}"
    );
}

#[tokio::test]
async fn a_server_whose_lock_file_was_deleted_keeps_its_data_folder_by_its_socket() {
    // Spec §3.5: a server that answers on the socket uses the folder, whatever happened to its
    // lock file (a host tidying the folder).
    let s = started_with_socket().await;
    std::fs::remove_file(s.data().join("server.lock")).unwrap();
    let e = console::socket::claim(&s.data(), None).await.unwrap_err();
    assert!(
        format!("{e:#}").contains("already using this data folder"),
        "{e:#}"
    );
    assert!(s.data().join("console.sock").exists(), "its socket stays");
}

#[tokio::test]
async fn a_stale_socket_is_replaced() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("console.sock"), b"").unwrap();
    let claim = console::socket::claim(dir.path(), None).await.unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(dir.path().join("console.sock"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    drop(claim);
    assert!(!dir.path().join("console.sock").exists());
}

#[tokio::test]
async fn the_fifth_console_and_long_lines_are_refused() {
    let s = started_with_socket().await;
    let mut held = Vec::new();
    for _ in 0..4 {
        let mut c = Raw::connect(&s.data().join("console.sock")).await;
        assert!(matches!(c.next().await, ServerMsg::Hello { .. }));
        held.push(c);
    }
    let mut fifth = Raw::connect(&s.data().join("console.sock")).await;
    assert_eq!(
        fifth.next().await,
        ServerMsg::Bye {
            reason: "too many consoles (4)".into()
        }
    );
    held[0].send_raw(&"x".repeat(5000)).await;
    assert!(held[0].until_bye().await.contains("line too long"));
}

#[tokio::test]
async fn a_console_that_goes_gives_its_place_back_at_once() {
    // Found by Task 11's fix round 1: four consoles that came and went, as by Ctrl-C or a signal,
    // held every place until something was logged, so an idle server turned the next away.
    let s = started_with_socket().await;
    let socket = s.data().join("console.sock");
    for _ in 0..4 {
        let mut c = Raw::connect(&socket).await;
        assert!(matches!(c.next().await, ServerMsg::Hello { .. }));
    }
    // Time for the server to see them go: nothing is logged meanwhile.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut fifth = Raw::connect(&socket).await;
    let first = fifth.next().await;
    assert!(matches!(first, ServerMsg::Hello { .. }), "{first:?}");
}

#[tokio::test]
async fn a_console_that_shuts_its_side_still_gets_its_answers() {
    // A console that sends a command and then sends no more, its side of the socket shut, isn't
    // gone: its answer still comes.
    let s = started_with_socket().await;
    let mut c = Raw::connect(&s.data().join("console.sock")).await;
    c.send(&run(1, "online")).await;
    c.writer.shutdown().await.unwrap();
    assert_eq!(c.answer(1).await, (true, vec!["nobody is online".into()]));
}

#[tokio::test]
async fn run_once_prints_the_answer_and_its_status() {
    let s = started_with_socket().await;
    let (ok, out) = console::attach::run_once(&s.data().join("console.sock"), "online")
        .await
        .unwrap();
    assert!(ok);
    assert_eq!(out, ["nobody is online"]);
    let (ok, _) = console::attach::run_once(&s.data().join("console.sock"), "dance")
        .await
        .unwrap();
    assert!(!ok);
}

#[tokio::test]
async fn an_attached_console_sees_the_status_change() {
    // Spec §4: the status's changes reach an attached console, as maintenance begins.
    let s = started_with_socket().await;
    let mut c = Raw::connect(&s.data().join("console.sock")).await;
    let first = loop {
        if let ServerMsg::Status { status } = c.next().await {
            break status;
        }
    };
    assert_eq!(first.state, State::Online);
    assert!(s.console().run_line("maintenance on Updating").await.ok);
    let changed = loop {
        if let ServerMsg::Status { status } = c.next().await {
            break status;
        }
    };
    assert_eq!(changed.state, State::Maintenance);
    assert_eq!(changed.maintenance.as_deref(), Some("Updating"));
}

#[tokio::test]
async fn a_console_that_stops_reading_holds_up_nothing_and_is_told_what_it_lost() {
    // Carry-forward 6 and spec §3.4: logging and the server never wait on a console. One that
    // has stopped reading fills its socket and its queue, then loses lines, and is told how many.
    let s = started_with_socket().await;
    let socket = s.data().join("console.sock");
    let mut stalled = Raw::connect(&socket).await;
    assert!(matches!(stalled.next().await, ServerMsg::Hello { .. }));
    // Far more than the socket's buffer, the console's queue and the log's queue hold.
    let filler = "x".repeat(200);
    for n in 0..20_000 {
        tracing::info!(n, filler, "flood");
        if n % 100 == 0 {
            tokio::task::yield_now().await;
        }
    }
    // The server answers meanwhile: another console's command, and the account API.
    let mut other = Raw::connect(&socket).await;
    other.send(&run(1, "online")).await;
    let answered = tokio::time::timeout(Duration::from_secs(10), other.answer(1))
        .await
        .expect("the server answers another console");
    assert_eq!(answered, (true, vec!["nobody is online".into()]));
    assert_eq!(api_status(s.api_addr).await, "HTTP/1.1 200 OK");
    // Reading again, the stalled console is told what it lost.
    let lost = loop {
        if let ServerMsg::Skipped { lines } = stalled.next().await {
            break lines;
        }
    };
    assert!(lost > 0);
}

#[tokio::test]
async fn a_console_that_stops_reading_doesnt_hold_up_the_servers_end() {
    // Carry-forward 6: `fenji-server run` waits for its socket's end, which waits for each
    // console's goodbye; one that will never take it is let go.
    let Started {
        server,
        serving,
        _dir,
        _log,
        _asked,
    } = started_with_socket().await;
    let socket = server.data().join("console.sock");
    let mut stalled = Raw::connect(&socket).await;
    assert!(matches!(stalled.next().await, ServerMsg::Hello { .. }));
    let filler = "x".repeat(200);
    for n in 0..20_000 {
        tracing::info!(n, filler, "flood");
        if n % 100 == 0 {
            tokio::task::yield_now().await;
        }
    }
    let console = server.console();
    let served = tokio::spawn(server.serve_until(std::future::pending()));
    assert!(console.run_line("stop").await.ok);
    served.await.unwrap().unwrap();
    tokio::time::timeout(Duration::from_secs(20), serving)
        .await
        .expect("the socket's end doesn't wait on the stalled console")
        .unwrap();
    assert!(!socket.exists());
}

#[tokio::test]
async fn run_once_stop_returns_once_another_server_could_start() {
    // The fix pass's re-review, NB2: `--run stop` returned at its own console's end, but the
    // server held its claim on the data folder on for the other consoles' goodbyes, 5 s at most
    // with one that had stopped reading, and a server started then was refused. The guide says a
    // script can start it again as soon as `--run stop` returns.
    let Started {
        server,
        serving,
        _dir,
        _log,
        _asked,
    } = started_with_socket().await;
    let data = server.data();
    let socket = data.join("console.sock");
    let mut stalled = Raw::connect(&socket).await;
    assert!(matches!(stalled.next().await, ServerMsg::Hello { .. }));
    let filler = "x".repeat(200);
    for n in 0..20_000 {
        tracing::info!(n, filler, "flood");
        if n % 100 == 0 {
            tokio::task::yield_now().await;
        }
    }
    let served = tokio::spawn(server.serve_until(std::future::pending()));
    let (ok, _) = console::attach::run_once(&socket, "stop").await.unwrap();
    assert!(ok);
    // As a server starting would: the data folder's lock is free.
    let lock = std::fs::File::open(data.join("server.lock")).unwrap();
    assert!(lock.try_lock().is_ok(), "the data folder is still claimed");
    drop(lock);
    served.await.unwrap().unwrap();
    serving.await.unwrap();
}

#[tokio::test]
async fn a_stale_socket_left_by_a_killed_server_is_replaced() {
    // After `kill -9`, the socket file stays and nobody listens on it.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("console.sock");
    drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
    assert!(path.exists());
    let claim = console::socket::claim(dir.path(), None).await.unwrap();
    assert!(claim.unattachable().is_none());
    // It answers now, and the claim left nothing else in the data folder but its lock.
    UnixStream::connect(&path).await.unwrap();
    let mut names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, ["console.sock", "server.lock"]);
}

#[tokio::test]
async fn the_claim_makes_a_missing_data_folder_owner_only() {
    // A config from `init` names a data folder no one has made yet, and the claim comes first.
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let _claim = console::socket::claim(&data, None).await.unwrap();
    let mode = std::fs::metadata(&data).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o700);
    assert!(data.join("console.sock").exists());
}

#[tokio::test]
async fn a_data_folder_too_deep_for_a_socket_still_starts_and_keeps_one_server_to_it() {
    // Carry-forward 7: a socket's path is at most 107 bytes on Linux, 103 on macOS.
    let dir = tempfile::tempdir().unwrap();
    let deep = dir.path().join("d".repeat(60)).join("e".repeat(60));
    let s = started_at(dir, &deep).await;
    assert!(!deep.join("console.sock").exists());
    // One server per data folder holds without the socket.
    let e = console::socket::claim(&deep, None).await.unwrap_err();
    assert!(
        format!("{e:#}").contains("already using this data folder"),
        "{e:#}"
    );
    // A console can't attach, and says why.
    let e = console::attach::run_once(&deep.join("console.sock"), "online")
        .await
        .unwrap_err();
    assert!(format!("{e:#}").contains("too long"), "{e:#}");
    // Once the server has stopped, the folder is free.
    let Started {
        server,
        serving,
        _dir,
        _log,
        _asked,
    } = s;
    server.shutdown().await.unwrap();
    tokio::time::timeout(WAIT, serving).await.unwrap().unwrap();
    let claim = console::socket::claim(&deep, None).await.unwrap();
    let why = claim
        .unattachable()
        .expect("it says why consoles can't attach");
    assert!(why.contains("too long"), "{why}");
}

#[tokio::test]
async fn a_line_that_isnt_a_message_is_refused_by_its_id_or_else_ignored() {
    let s = started_with_socket().await;
    let mut c = Raw::connect(&s.data().join("console.sock")).await;
    assert!(matches!(c.next().await, ServerMsg::Hello { .. }));
    c.send_raw(r#"{"type":"dance","id":4}"#).await;
    let (ok, lines) = c.answer(4).await;
    assert!(!ok && !lines.is_empty(), "{lines:?}");
    // Without an id there's nothing to answer: the next answer is the next command's.
    c.send_raw("not json at all").await;
    c.send_raw(r#"{"type":"run","line":"online"}"#).await;
    c.send(&run(5, "online")).await;
    loop {
        if let ServerMsg::Answer { id, .. } = c.next().await {
            assert_eq!(id, 5);
            break;
        }
    }
}

#[tokio::test]
async fn the_server_stopping_says_goodbye_and_takes_its_socket_away() {
    let Started {
        server,
        serving,
        _dir,
        _log,
        _asked,
    } = started_with_socket().await;
    let data = server.data();
    let socket = data.join("console.sock");
    let mut c = Raw::connect(&socket).await;
    let served = tokio::spawn(server.serve_until(std::future::pending()));
    c.send(&run(1, "stop")).await;
    assert_eq!(
        c.answer(1).await,
        (true, vec!["stopping the server".into()])
    );
    assert_eq!(c.until_bye().await, "the server is stopping");
    assert_eq!(c.try_next().await, None, "closed after the goodbye");
    served.await.unwrap().unwrap();
    tokio::time::timeout(WAIT, serving).await.unwrap().unwrap();
    assert!(!socket.exists());
    // The data folder is free for the next server.
    console::socket::claim(&data, None).await.unwrap();
}

#[tokio::test]
async fn a_log_line_too_long_for_the_socket_reaches_a_console_cut() {
    // Carry-forward 1: a packet's hex or an error's chain can pass the wire's 4,096 bytes; it's
    // cut rather than ending the console.
    let s = started_with_socket().await;
    let mut c = Raw::connect(&s.data().join("console.sock")).await;
    tracing::info!("{}", "y".repeat(10_000));
    let line = loop {
        if let ServerMsg::Log { line } = c.next().await
            && line.message().starts_with("yyy")
        {
            break line;
        }
    };
    let message = line.message();
    assert!(message.ends_with('…'), "{:.50}…", message);
    assert!(message.len() > 3_000, "cut no more than it must");
    c.send(&run(1, "online")).await;
    assert_eq!(c.answer(1).await, (true, vec!["nobody is online".into()]));
}

#[tokio::test]
async fn an_answer_too_long_for_the_socket_comes_whole_in_parts() {
    // Carry-forward 1: `invites` lists every invite ever made, labels of up to 64 characters.
    let s = started_with_socket().await;
    for n in 0..60 {
        let label = format!("{n:02} {}", "z".repeat(61));
        assert!(s.console().run_line(&format!("invite {label}")).await.ok);
    }
    let whole = s.console().run_line("invites").await;
    assert_eq!(whole.lines.len(), 61, "the header and 60 invites");
    let socket = s.data().join("console.sock");
    let mut c = Raw::connect(&socket).await;
    c.send(&run(1, "invites")).await;
    let (mut parts, mut lines) = (0, Vec::new());
    loop {
        if let ServerMsg::Answer {
            id: 1,
            ok,
            lines: part,
            more,
        } = c.next().await
        {
            assert!(ok);
            parts += 1;
            lines.extend(part);
            if !more {
                break;
            }
        }
    }
    assert!(parts > 1, "one line can't hold it");
    assert_eq!(lines, whole.lines);
    let (ok, out) = console::attach::run_once(&socket, "invites").await.unwrap();
    assert!(ok);
    assert_eq!(out, whole.lines);
}

#[test]
fn the_longest_hello_and_status_a_config_allows_fit_one_line() {
    // Carry-forward 1: these go whole, so the config's limits must keep them within the wire's: a
    // server name of 32 characters and a maintenance message of 200, at four bytes each, which
    // JSON leaves as they are.
    let wide = |n| "𝕏".repeat(n);
    assert!(
        Config::parse(
            &format!("[server]\nname = \"{}\"\n", wide(33)),
            Path::new("/")
        )
        .is_err()
    );
    assert!(fenji_server::status::check_message(&wide(201)).is_err());
    let hello = ServerMsg::Hello {
        version: env!("CARGO_PKG_VERSION").into(),
        proto: wire::PROTO,
        server: wide(32),
        commands: fenji_server::commands::console_help(),
    };
    let status = ServerMsg::Status {
        status: wire::Status {
            state: State::Maintenance,
            server: wide(32),
            version: env!("CARGO_PKG_VERSION").into(),
            started_ms: i64::MIN,
            online: u32::MAX,
            max: u32::MAX,
            maintenance: Some(wide(200)),
            gate_port: u16::MAX,
            api_port: u16::MAX,
        },
    };
    for msg in [hello, status] {
        let lines = wire::encode_server(msg.clone());
        assert_eq!(lines, [wire::encode(&msg)]);
        assert!(lines[0].len() <= wire::LINE_MAX, "{} bytes", lines[0].len());
    }
}

/// A server of the test's own at `path`: it says hello with `proto`, then hears the console out
/// until it leaves, and gives back what it heard.
fn fake_server(path: &Path, proto: u32) -> JoinHandle<String> {
    let listener = tokio::net::UnixListener::bind(path).unwrap();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (read, mut write) = stream.into_split();
        let hello = ServerMsg::Hello {
            version: "9.9.9".into(),
            proto,
            server: "Fake".into(),
            commands: vec![],
        };
        write
            .write_all(format!("{}\n", wire::encode(&hello)).as_bytes())
            .await
            .unwrap();
        let mut heard = String::new();
        let _ = BufReader::new(read).read_to_string(&mut heard).await;
        heard
    })
}

#[tokio::test]
async fn run_once_with_a_blank_line_sends_nothing() {
    // Carry-forward 2: a blank `--run` is no command, as the front ends skip blank lines.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("console.sock");
    let heard = fake_server(&path, wire::PROTO);
    let ran = tokio::time::timeout(WAIT, console::attach::run_once(&path, "  "))
        .await
        .expect("it doesn't wait for an answer")
        .unwrap();
    assert_eq!(ran, (true, vec![]));
    assert_eq!(heard.await.unwrap(), "", "nothing was sent");
}

#[tokio::test]
async fn a_server_of_another_protocol_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("console.sock");
    let heard = fake_server(&path, wire::PROTO + 1);
    let e = tokio::time::timeout(WAIT, console::attach::run_once(&path, "online"))
        .await
        .expect("it doesn't wait for an answer")
        .unwrap_err();
    assert!(format!("{e:#}").contains("protocol 2"), "{e:#}");
    assert_eq!(heard.await.unwrap(), "", "no command for it");
}

#[tokio::test]
async fn with_no_server_running_attaching_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("console.sock");
    let none = console::attach::run_once(&path, "online")
        .await
        .unwrap_err();
    // A socket file left behind, which nobody answers on, is no server either.
    drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
    let stale = console::attach::run_once(&path, "online")
        .await
        .unwrap_err();
    for e in [none, stale] {
        let e = format!("{e:#}");
        assert!(
            e.contains("no server is running with this data folder ("),
            "{e}"
        );
    }
}
