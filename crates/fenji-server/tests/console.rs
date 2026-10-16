//! The host console through `fenji-server` itself (M4 hosting spec §3.1, §3.5): as plain lines,
//! commands from stdin, answers on stdout and the log on stderr; and `fenji-server console`,
//! attached to a running server.
//!
//! The tests that start processes are here rather than in `tests/attach.rs`: a process being
//! started holds a copy of every open file of the test's until it runs, so a socket or a lock that
//! test has just let go of would live on for a moment.
#![cfg(unix)]
use std::ffi::OsStr;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Output, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process};
use rustix::termios::{LocalModes, Winsize};

/// The signals that leave an attached console as Ctrl-C leaves the full-screen one (M4 hosting
/// plan, Task 11's review 1, Important 1).
const LEAVE: [(Signal, &str); 4] = [
    (Signal::TERM, "SIGTERM"),
    (Signal::HUP, "SIGHUP"),
    (Signal::INT, "SIGINT"),
    (Signal::QUIT, "SIGQUIT"),
];

/// What an attached console says as the host leaves it.
const LEFT: &str = "Left the console. The server runs on.";

/// Waits for `child` to stop by itself, 30 s at most: its exit status, or `what` and a panic.
fn exit_of(child: &mut Child, what: impl Fn() -> String) -> ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "still running after 30 s:\n{}",
            what()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Waits, 60 s at most, until `done` is true, or `what` and a panic.
fn until(done: impl Fn() -> bool, what: impl Fn() -> String) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !done() {
        assert!(Instant::now() < deadline, "not within 60 s:\n{}", what());
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// `fenji-server console` as plain lines on `config`'s server: its stdin held open, its stderr
/// kept as it comes; killed if the test ends first.
struct AttachedPlain {
    child: Child,
    _stdin: ChildStdin,
    err: Arc<Mutex<Vec<u8>>>,
}

impl AttachedPlain {
    fn start(config: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_fenji-server"))
            .arg("--config")
            .arg(config)
            .args(["console", "--plain"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let err = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&err);
        let mut stderr = child.stderr.take().unwrap();
        std::thread::spawn(move || {
            let mut buf = [0; 4096];
            while let Ok(n @ 1..) = stderr.read(&mut buf) {
                kept.lock().unwrap().extend_from_slice(&buf[..n]);
            }
        });
        AttachedPlain {
            child,
            _stdin: stdin,
            err,
        }
    }

    fn said(&self) -> String {
        String::from_utf8_lossy(&self.err.lock().unwrap()).into_owned()
    }
}

impl Drop for AttachedPlain {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `fenji-server console` full-screen on `config`'s server, in a pseudo-terminal of 100 × 30 as a
/// host's terminal would be: what it writes there is kept, and its cursor queries are answered.
/// Killed if the test ends first.
struct InTerminal {
    child: Child,
    master: File,
    out: Arc<Mutex<Vec<u8>>>,
}

impl InTerminal {
    /// `fenji-server console` on `config`'s server.
    fn start(config: &Path) -> Self {
        InTerminal::spawn(config, &["console"])
    }

    /// `fenji-server --config <config> <args>`.
    fn spawn(config: &Path, args: &[&str]) -> Self {
        use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
        let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
        grantpt(&master).unwrap();
        unlockpt(&master).unwrap();
        let name = ptsname(&master, Vec::new()).unwrap();
        let terminal = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(rustix::fs::OFlags::NOCTTY.bits() as i32)
            .open(OsStr::from_bytes(name.as_bytes()))
            .unwrap();
        let size = Winsize {
            ws_row: 30,
            ws_col: 100,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        rustix::termios::tcsetwinsize(&terminal, size).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_fenji-server"))
            .arg("--config")
            .arg(config)
            .args(args)
            .env("TERM", "xterm-256color")
            .stdin(terminal.try_clone().unwrap())
            .stdout(terminal.try_clone().unwrap())
            .stderr(terminal)
            .spawn()
            .unwrap();
        let master = File::from(master);
        let out = Arc::new(Mutex::new(Vec::new()));
        let (mut reading, mut answering) =
            (master.try_clone().unwrap(), master.try_clone().unwrap());
        let kept = Arc::clone(&out);
        std::thread::spawn(move || {
            const ASKED: &[u8] = b"\x1b[6n";
            let mut buf = [0; 65536];
            let mut answered = 0;
            while let Ok(n @ 1..) = reading.read(&mut buf) {
                let mut out = kept.lock().unwrap();
                out.extend_from_slice(&buf[..n]);
                // ratatui asks where the cursor is as it clears the screen: a terminal answers.
                let asked = out.windows(ASKED.len()).filter(|w| *w == ASKED).count();
                for _ in answered..asked {
                    let _ = answering.write_all(b"\x1b[1;1R");
                }
                answered = asked;
            }
        });
        InTerminal { child, master, out }
    }

    fn output(&self) -> Vec<u8> {
        self.out.lock().unwrap().clone()
    }

    fn shown(&self) -> String {
        String::from_utf8_lossy(&self.output()).into_owned()
    }

    /// Whether the terminal is in its usual, cooked mode: lines, echo and signals from keys.
    fn cooked(&self) -> bool {
        let modes = rustix::termios::tcgetattr(&self.master)
            .unwrap()
            .local_modes;
        modes.contains(LocalModes::ICANON | LocalModes::ECHO | LocalModes::ISIG)
    }
}

impl Drop for InTerminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Where `needle` last is in `bytes`.
fn last(bytes: &[u8], needle: &[u8]) -> Option<usize> {
    bytes.windows(needle.len()).rposition(|w| w == needle)
}

/// `fenji-server` run with `--plain` on a config with free loopback ports, stdin and stdout piped
/// and its log in a file; killed if the test ends first.
struct Plain {
    child: Child,
    stdin: Option<std::process::ChildStdin>,
    /// The lines it prints on stdout.
    out: mpsc::Receiver<String>,
    log: tempfile::NamedTempFile,
    config: PathBuf,
}

impl Plain {
    /// A server on a config in `dir`, its data folder.
    fn start(dir: &Path) -> Self {
        let config = dir.join("fenji.toml");
        std::fs::write(
            &config,
            "[server]\nname = \"Fenji\"\nlogin_bind = \"127.0.0.1:0\"\napi_bind = \"127.0.0.1:0\"\n",
        )
        .unwrap();
        Plain::with(&config, &[])
    }

    /// A server on `config`, with `env` set.
    fn with(config: &Path, env: &[(&str, &str)]) -> Self {
        Plain::spawn(
            Command::new(env!("CARGO_BIN_EXE_fenji-server")),
            config,
            env,
        )
    }

    /// A server on `config` started by `nohup`, which starts it with SIGHUP ignored so that it
    /// outlives its terminal, as the write-up shows a host.
    fn nohup(config: &Path) -> Self {
        let mut nohup = Command::new("nohup");
        nohup.arg(env!("CARGO_BIN_EXE_fenji-server"));
        Plain::spawn(nohup, config, &[])
    }

    /// `program` run as the server on `config`, with `env` set.
    fn spawn(mut program: Command, config: &Path, env: &[(&str, &str)]) -> Self {
        let log = tempfile::NamedTempFile::new().unwrap();
        let mut child = program
            .arg("--config")
            .arg(config)
            .args(["run", "--plain"])
            .env("RUST_LOG", "info")
            .envs(env.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(log.reopen().unwrap())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let (lines, out) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if lines.send(line).is_err() {
                    break;
                }
            }
        });
        Plain {
            child,
            stdin,
            out,
            log,
            config: config.to_owned(),
        }
    }

    fn said(&self) -> String {
        String::from_utf8_lossy(&std::fs::read(self.log.path()).unwrap()).into_owned()
    }

    fn type_line(&mut self, line: &str) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
    }

    /// The next line it prints on stdout, within 60 s.
    fn answer(&self) -> String {
        self.out
            .recv_timeout(Duration::from_secs(60))
            .unwrap_or_else(|e| panic!("no answer ({e}):\n{}", self.said()))
    }

    /// Its exit status once it stops, within 30 s.
    fn wait(&mut self) -> ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "still running after 30 s:\n{}",
                self.said()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The gate's address, once the server says it runs (within 60 s).
    fn running(&self) -> String {
        const RUNNING: &str = "Fenji is running gate=";
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let said = self.said();
            if let Some(at) = said.find(RUNNING) {
                let rest = &said[at + RUNNING.len()..];
                return rest.split_whitespace().next().unwrap().to_owned();
            }
            assert!(Instant::now() < deadline, "not running after 60 s:\n{said}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// `fenji-server --config <its config> console <args>`, given `input`: what it did.
    fn console(&self, args: &[&str], input: &str) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_fenji-server"))
            .arg("--config")
            .arg(&self.config)
            .arg("console")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(input.as_bytes()).unwrap();
        drop(stdin);
        child.wait_with_output().unwrap()
    }
}

impl Drop for Plain {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn plain_lines_run_commands_and_stop_ends_the_server_with_its_input_still_open() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = Plain::start(dir.path());
    server.type_line("online");
    assert_eq!(server.answer(), "nobody is online");
    // A blank line is no command: the next answer is the next command's.
    server.type_line("");
    server.type_line("stop");
    assert_eq!(server.answer(), "stopping the server");
    // Stdin stays open: `stop` ends it, not the end of its input.
    let status = server.wait();
    assert!(status.success(), "{status}:\n{}", server.said());
    let said = server.said();
    assert!(
        said.contains("Fenji is running"),
        "the log is on stderr: {said}"
    );
    assert!(
        !said.contains("nobody is online"),
        "answers aren't in the log: {said}"
    );
}

#[test]
fn console_run_gives_a_script_the_answer_and_an_exit_code_that_follows_it() {
    // M4 hosting plan, Task 11's carry-forwards 2 and 6.
    let dir = tempfile::tempdir().unwrap();
    let server = Plain::start(dir.path());
    server.running();
    let online = server.console(&["--run", "online"], "");
    assert!(online.status.success(), "{online:?}");
    assert_eq!(text(&online.stdout), "nobody is online\n");
    // Refused by the world: exit 1, and the answer says why.
    let kick = server.console(&["--run", "kick Nobody"], "");
    assert_eq!(kick.status.code(), Some(1), "{kick:?}");
    assert!(text(&kick.stdout).contains("Nobody"), "{kick:?}");
    // A blank command sends nothing, and is no failure.
    let blank = server.console(&["--run", ""], "");
    assert!(blank.status.success(), "{blank:?}");
    assert_eq!(text(&blank.stdout), "");
}

#[test]
fn a_full_screen_run_refuses_an_open_data_folder_before_it_writes_its_log() {
    // The hosting plan's final review, Minor 1: the full-screen console logs to data/logs, so a
    // run refused for its data folder (one every user may write in here, another user's when a
    // host types sudo) is refused before that file too, and writes nothing there.
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    std::fs::create_dir(&data).unwrap();
    std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o777)).unwrap();
    let config = dir.path().join("fenji.toml");
    std::fs::write(
        &config,
        "[server]\nname = \"Fenji\"\nlogin_bind = \"127.0.0.1:0\"\napi_bind = \"127.0.0.1:0\"\n\
         [store]\npath = \"data/fenji.db\"\n",
    )
    .unwrap();
    let mut run = InTerminal::spawn(&config, &["run"]);
    let out = Arc::clone(&run.out);
    let shown = move || String::from_utf8_lossy(&out.lock().unwrap()).into_owned();
    let status = exit_of(&mut run.child, shown);
    assert!(!status.success(), "{status}");
    until(|| run.shown().contains("chmod o-w"), || run.shown());
    assert_eq!(
        std::fs::read_dir(&data).unwrap().count(),
        0,
        "{}",
        run.shown()
    );
}

#[test]
fn a_hangup_stops_the_server_cleanly() {
    // The hosting plan's final review, Important 1: closing the terminal or window a server runs
    // in, or losing its SSH session, sends it SIGHUP, which killed it at once, without its saves,
    // without the shutdown's reason for the players, and with its write-ahead log left behind.
    // It stops as `stop` does now.
    let dir = tempfile::tempdir().unwrap();
    let mut server = Plain::start(dir.path());
    server.running();
    kill_process(Pid::from_child(&server.child), Signal::HUP).unwrap();
    let status = server.wait();
    assert!(status.success(), "{status}:\n{}", server.said());
    let said = server.said();
    assert!(said.contains("shutting down signal=SIGHUP"), "{said}");
    for left in ["fenji.db-wal", "fenji.db-shm"] {
        assert!(!dir.path().join(left).exists(), "{left} is left:\n{said}");
    }
}

#[test]
fn a_server_nohup_started_runs_on_through_a_hangup() {
    // `nohup` starts a server with SIGHUP ignored, so that it outlives its terminal (the hosting
    // plan's final review, Important 1): it stays so, and the server runs on until it's stopped.
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("fenji.toml");
    std::fs::write(
        &config,
        "[server]\nname = \"Fenji\"\nlogin_bind = \"127.0.0.1:0\"\napi_bind = \"127.0.0.1:0\"\n",
    )
    .unwrap();
    let mut server = Plain::nohup(&config);
    server.running();
    kill_process(Pid::from_child(&server.child), Signal::HUP).unwrap();
    // A server that took the hangup would be stopping by now, and an idle one stops in well
    // under a second.
    std::thread::sleep(Duration::from_secs(1));
    assert!(
        server.child.try_wait().unwrap().is_none(),
        "it stopped:\n{}",
        server.said()
    );
    let online = server.console(&["--run", "online"], "");
    assert!(online.status.success(), "{online:?}");
    assert!(
        !server.said().contains("shutting down"),
        "{}",
        server.said()
    );
    server.type_line("stop");
    assert_eq!(server.answer(), "stopping the server");
    assert!(server.wait().success(), "{}", server.said());
}

#[test]
fn console_run_stop_returns_once_the_server_has_stopped() {
    // The hosting plan's final review, Minor 2: `--run stop` returned as soon as the server
    // answered, before its stop, so a script that started the server again found the data folder
    // in use. It reads on to the server's goodbye, which comes once the world has saved everyone
    // and the store has closed: by then SQLite has folded its write-ahead log in and removed it.
    let dir = tempfile::tempdir().unwrap();
    let mut server = Plain::start(dir.path());
    server.running();
    let wal = dir.path().join("fenji.db-wal");
    assert!(wal.exists(), "the store is open: {}", server.said());
    let stop = server.console(&["--run", "stop"], "");
    let (wal_left, shm_left) = (wal.exists(), dir.path().join("fenji.db-shm").exists());
    assert!(stop.status.success(), "{stop:?}");
    assert_eq!(text(&stop.stdout), "stopping the server\n");
    assert!(
        !wal_left && !shm_left,
        "returned before the store closed:\n{}",
        server.said()
    );
    assert!(server.wait().success(), "{}", server.said());
}

#[test]
fn a_second_server_on_a_data_folder_in_use_refuses_before_it_opens_its_database_or_listens() {
    // Review Focus 5 (Task 11's review 1, Minor 6a). The second server's config names another
    // database in the same data folder, which opening its store would make, and it's given the
    // first's gate port, which a start that listened first would fail on instead.
    let dir = tempfile::tempdir().unwrap();
    let first = Plain::start(dir.path());
    let gate = first.running();
    let other = dir.path().join("other.toml");
    std::fs::write(
        &other,
        "[server]\nname = \"Fenji\"\nlogin_bind = \"127.0.0.1:0\"\napi_bind = \"127.0.0.1:0\"\n\
         [store]\npath = \"other.db\"\n",
    )
    .unwrap();
    let mut second = Plain::with(&other, &[("FENJI_LOGIN_BIND", &gate)]);
    assert!(!second.wait().success());
    let said = second.said();
    assert!(
        said.contains("a server is already using this data folder ("),
        "{said}"
    );
    assert!(
        !dir.path().join("other.db").exists(),
        "its store never opened"
    );
    assert!(!said.contains("can't listen"), "{said}");
}

#[test]
fn a_signal_leaves_an_attached_full_screen_console_with_the_terminal_given_back() {
    // Task 11's review 1, Important 1 (Review Focus 2): SIGTERM, SIGHUP, SIGINT and SIGQUIT to
    // an attached console end it as Ctrl-C does. The host's terminal is back on its main screen,
    // with a cursor and in cooked mode, and the server runs on.
    let dir = tempfile::tempdir().unwrap();
    let server = Plain::start(dir.path());
    server.running();
    for (signal, name) in LEAVE {
        let mut console = InTerminal::start(&server.config);
        until(
            || console.shown().contains("attached"),
            || format!("{name}: not drawn: {:?}", console.shown()),
        );
        assert!(!console.cooked(), "{name}: the console holds the terminal");
        kill_process(Pid::from_child(&console.child), signal).unwrap();
        let status = exit_of(&mut console.child, || name.to_string());
        assert_eq!(status.code(), Some(0), "{name}: {status}");
        let out = console.output();
        let took = last(&out, b"\x1b[?1049h").expect("it took the alternate screen");
        let gave = last(&out, b"\x1b[?1049l");
        assert!(
            gave.is_some_and(|at| at > took),
            "{name}: not on the main screen"
        );
        let cursor = last(&out, b"\x1b[?25h");
        assert!(cursor.is_some_and(|at| at > took), "{name}: no cursor");
        // The wheel's ↑ and ↓ (alternate scroll) off while it held the terminal, and on again
        // (the hosting plan's final review B, Minor 5).
        let off = last(&out, b"\x1b[?1007l");
        assert!(
            off.is_some_and(|at| at > took),
            "{name}: the wheel's left on"
        );
        let on = last(&out, b"\x1b[?1007h");
        assert!(
            on.is_some_and(|at| gave.is_some_and(|gave| at > gave)),
            "{name}: the wheel isn't given back"
        );
        assert!(console.cooked(), "{name}: not in cooked mode");
        assert!(
            console.shown().contains(LEFT),
            "{name}: {:?}",
            console.shown()
        );
    }
    // The server runs on, and answers a console. One it hasn't seen go yet holds its place a
    // moment more (tests/attach.rs's `a_console_that_goes_gives_its_place_back_at_once`).
    until(
        || server.console(&["--run", "online"], "").status.success(),
        || "no console is answered".into(),
    );
}

#[test]
fn a_signal_leaves_an_attached_plain_console() {
    // Task 11's review 1, Important 1: as plain lines too, SIGINT (Ctrl-C) among them, which
    // killed the console with 130 before.
    let dir = tempfile::tempdir().unwrap();
    let server = Plain::start(dir.path());
    server.running();
    for (signal, name) in LEAVE {
        let mut console = AttachedPlain::start(&server.config);
        // Attached: the server's log comes on stderr.
        until(
            || console.said().contains("Fenji is running"),
            || format!("{name}: {}", console.said()),
        );
        kill_process(Pid::from_child(&console.child), signal).unwrap();
        let status = exit_of(&mut console.child, || name.to_string());
        assert_eq!(status.code(), Some(0), "{name}: {status}");
        assert!(console.said().contains(LEFT), "{name}: {}", console.said());
    }
    // The server runs on, and answers a console. One it hasn't seen go yet holds its place a
    // moment more (tests/attach.rs's `a_console_that_goes_gives_its_place_back_at_once`).
    until(
        || server.console(&["--run", "online"], "").status.success(),
        || "no console is answered".into(),
    );
}

#[test]
fn an_attached_plain_console_leaves_on_exit_and_stop_ends_the_server() {
    // Task 11's carry-forward 1: `exit` ends the attachment and never reaches the server as a
    // command, and nothing typed after it is read.
    let dir = tempfile::tempdir().unwrap();
    let mut server = Plain::start(dir.path());
    server.running();
    let left = server.console(&["--plain"], "online\nexit\nbackup\n");
    assert!(left.status.success(), "{left:?}");
    assert_eq!(text(&left.stdout), "nobody is online\n");
    let said = server.said();
    assert!(said.contains("command=online"), "{said}");
    assert!(!said.contains("command=exit"), "{said}");
    assert!(
        !said.contains("command=backup"),
        "nothing after exit: {said}"
    );
    // `stop` from a console stops the server, which takes its socket away.
    let stop = server.console(&["--run", "stop"], "");
    assert!(stop.status.success(), "{stop:?}");
    assert_eq!(text(&stop.stdout), "stopping the server\n");
    assert!(server.wait().success(), "{}", server.said());
    assert!(!dir.path().join("console.sock").exists());
    let gone = server.console(&["--run", "online"], "");
    assert_eq!(gone.status.code(), Some(1), "{gone:?}");
    assert!(
        text(&gone.stderr).contains("no server is running with this data folder ("),
        "{gone:?}"
    );
}
