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

/// Waits, 60 s at most, until four consoles attach to the server on `config` at once, all held
/// open until the last is in: a server takes four, so none of its places is kept by a console
/// that has gone (#359, the hosting plan's Task 11). One it hasn't seen go yet keeps its place a
/// moment more (tests/attach.rs's `a_console_that_goes_gives_its_place_back_at_once`).
fn all_four_places_come_free(config: &Path) {
    until(
        || {
            let consoles: Vec<_> = (0..4).map(|_| AttachedPlain::start(config)).collect();
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let said: Vec<String> = consoles.iter().map(AttachedPlain::said).collect();
                // Attached, each is sent the server's log, its start among it.
                if said.iter().all(|s| s.contains("Fenji is running")) {
                    return true;
                }
                if said.iter().any(|s| s.contains("turned the console away")) {
                    return false;
                }
                assert!(Instant::now() < deadline, "not attached: {said:?}");
                std::thread::sleep(Duration::from_millis(20));
            }
        },
        || "four consoles never attached at once".into(),
    );
}

/// `fenji-server console` full-screen on `config`'s server, in a pseudo-terminal of 100 × 30 as a
/// host's terminal would be: what it writes there is kept. Nothing answers what it might ask the
/// terminal, and it asks nothing (#359). Killed if the test ends first.
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
        let mut reading = master.try_clone().unwrap();
        let kept = Arc::clone(&out);
        std::thread::spawn(move || {
            let mut buf = [0; 65536];
            while let Ok(n @ 1..) = reading.read(&mut buf) {
                kept.lock().unwrap().extend_from_slice(&buf[..n]);
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

/// `fenji-server` full-screen in a window of 80 × 24: a pseudo-terminal, which the test can close as
/// a window closes. Nothing answers what the program might ask the terminal. Linux, for
/// util-linux's `setsid --ctty`. Killed if the test ends first.
#[cfg(target_os = "linux")]
struct Window {
    child: Child,
    master: Option<File>,
    /// The program's end of the terminal, for its mode and its flow.
    terminal: File,
    /// True while the window is open: the thread reading the terminal goes on.
    open: Arc<std::sync::atomic::AtomicBool>,
    reader: Option<std::thread::JoinHandle<()>>,
    out: Arc<Mutex<Vec<u8>>>,
}

/// How a [`Window`]'s program holds its terminal.
#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Held {
    /// As its controlling terminal, as a terminal window or an SSH session gives it: closing the
    /// window hangs the terminal up, and the kernel sends the program SIGHUP.
    Controlling,
    /// The same, but with SIGHUP ignored from the start, as `trap '' HUP` leaves it: the hang-up's
    /// SIGHUP does nothing.
    ControllingHangupIgnored,
    /// As a terminal it was given, not its controlling one: closing the window ends its reads and
    /// fails its writes, and no signal comes.
    Given,
}

/// What a terminal is asked where its cursor is with.
#[cfg(target_os = "linux")]
const CURSOR_ASKED: &[u8] = b"\x1b[6n";

#[cfg(target_os = "linux")]
impl Window {
    /// `fenji-server --config <config> <args>` in the window, its controlling terminal.
    fn open(config: &Path, args: &[&str]) -> Window {
        Window::holding(config, args, Held::Controlling)
    }

    /// `fenji-server --config <config> <args>` in the window, holding its terminal as `held` says.
    fn holding(config: &Path, args: &[&str], held: Held) -> Window {
        use std::sync::atomic::{AtomicBool, Ordering};

        use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
        // Close-on-exec: a master the server inherited would keep the terminal from hanging up.
        let flags = OpenptFlags::RDWR | OpenptFlags::NOCTTY | OpenptFlags::CLOEXEC;
        let master = openpt(flags).unwrap();
        // Read without waiting, so that the window closes at once, whatever the console is doing.
        rustix::fs::fcntl_setfl(&master, rustix::fs::OFlags::NONBLOCK).unwrap();
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
            ws_row: 24,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        rustix::termios::tcsetwinsize(&terminal, size).unwrap();
        let server = env!("CARGO_BIN_EXE_fenji-server");
        let mut program = match held {
            // A session of its own whose controlling terminal is its stdin: this terminal.
            Held::Controlling => {
                let mut setsid = Command::new("setsid");
                setsid.arg("--ctty").arg(server);
                setsid
            }
            // The shell ignores SIGHUP, and what it runs keeps it ignored.
            Held::ControllingHangupIgnored => {
                let mut sh = Command::new("sh");
                sh.args(["-c", "trap '' HUP; exec setsid --ctty \"$@\"", "sh", server]);
                sh
            }
            Held::Given => Command::new(server),
        };
        let child = program
            .arg("--config")
            .arg(config)
            .args(args)
            .env("TERM", "xterm-256color")
            .stdin(terminal.try_clone().unwrap())
            .stdout(terminal.try_clone().unwrap())
            .stderr(terminal.try_clone().unwrap())
            .spawn()
            .unwrap();
        let master = File::from(master);
        let out = Arc::new(Mutex::new(Vec::new()));
        let open = Arc::new(AtomicBool::new(true));
        let mut reading = master.try_clone().unwrap();
        let (kept, still) = (Arc::clone(&out), Arc::clone(&open));
        let reader = std::thread::spawn(move || {
            let mut buf = [0; 65536];
            while still.load(Ordering::SeqCst) {
                let n = match reading.read(&mut buf) {
                    Ok(n @ 1..) => n,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                    // The program's side is closed: it has ended.
                    Ok(0) | Err(_) => return,
                };
                kept.lock().unwrap().extend_from_slice(&buf[..n]);
            }
        });
        Window {
            child,
            master: Some(master),
            terminal,
            open,
            reader: Some(reader),
            out,
        }
    }

    /// Whether the terminal is in its usual, cooked mode: lines, echo and signals from keys.
    fn cooked(&self) -> bool {
        let modes = rustix::termios::tcgetattr(&self.terminal)
            .unwrap()
            .local_modes;
        modes.contains(LocalModes::ICANON | LocalModes::ECHO | LocalModes::ISIG)
    }

    fn shown(&self) -> String {
        String::from_utf8_lossy(&self.out.lock().unwrap()).into_owned()
    }

    /// Types `keys` in the window, as the terminal sends them.
    fn type_keys(&self, keys: &[u8]) {
        let mut master = self.master.as_ref().expect("the window is open");
        master.write_all(keys).unwrap();
    }

    /// Waits, 60 s at most, until the program has drawn `text`.
    fn drawn(&self, text: &str) {
        until(
            || self.shown().contains(text),
            || format!("{text:?} not drawn: {:?}", self.shown()),
        );
    }

    /// Closes the window at once: the last handle on the terminal's master end goes, and the kernel
    /// hangs the terminal up.
    fn close(&mut self) {
        self.open.store(false, std::sync::atomic::Ordering::SeqCst);
        if let Some(reader) = self.reader.take() {
            reader.join().unwrap();
        }
        self.master.take();
    }
}

#[cfg(target_os = "linux")]
impl Drop for Window {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // The test's own end of the terminal keeps it from ending: the reading stops here.
        self.open.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

/// `fenji-server --config <config> <args>` full-screen in a terminal of 100 × 30 that it writes to
/// without waiting: its stdout is an open of the terminal of its own, non-blocking, while its keys'
/// reads wait as usual. Once the test stops the terminal's output, as a frozen link stops it
/// ([`Stalled::stalls_after`]), the next frame fails with EAGAIN, which is neither a hang-up nor
/// the end of its keys: the console ends as one that failed. Its stderr is a file of its own, for
/// what it says then. Linux. Killed if the test ends first.
#[cfg(target_os = "linux")]
struct Stalled {
    child: Child,
    master: File,
    /// The program's end of the terminal, which it reads its keys from: for the terminal's flow.
    terminal: File,
    out: Arc<Mutex<Vec<u8>>>,
    said: tempfile::NamedTempFile,
}

#[cfg(target_os = "linux")]
impl Stalled {
    fn start(config: &Path, args: &[&str]) -> Stalled {
        use rustix::fs::OFlags;
        use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
        let master =
            openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY | OpenptFlags::CLOEXEC).unwrap();
        grantpt(&master).unwrap();
        unlockpt(&master).unwrap();
        let name = ptsname(&master, Vec::new()).unwrap();
        let open = |flags: OFlags| {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags((OFlags::NOCTTY | flags).bits() as i32)
                .open(OsStr::from_bytes(name.as_bytes()))
                .unwrap()
        };
        let terminal = open(OFlags::empty());
        let screen = open(OFlags::NONBLOCK);
        let size = Winsize {
            ws_row: 30,
            ws_col: 100,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        rustix::termios::tcsetwinsize(&terminal, size).unwrap();
        let said = tempfile::NamedTempFile::new().unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_fenji-server"))
            .arg("--config")
            .arg(config)
            .args(args)
            .env("TERM", "xterm-256color")
            .stdin(terminal.try_clone().unwrap())
            .stdout(screen)
            .stderr(said.reopen().unwrap())
            .spawn()
            .unwrap();
        let master = File::from(master);
        let out = Arc::new(Mutex::new(Vec::new()));
        let mut reading = master.try_clone().unwrap();
        let kept = Arc::clone(&out);
        std::thread::spawn(move || {
            let mut buf = [0; 65536];
            while let Ok(n @ 1..) = reading.read(&mut buf) {
                kept.lock().unwrap().extend_from_slice(&buf[..n]);
            }
        });
        Stalled {
            child,
            master,
            terminal,
            out,
            said,
        }
    }

    /// Once it has drawn `text`: the terminal takes no more output, and a key makes a frame due at
    /// once, which fails.
    fn stalls_after(&mut self, text: &str) {
        let shown = || String::from_utf8_lossy(&self.out.lock().unwrap()).into_owned();
        until(
            || shown().contains(text),
            || format!("{text:?} not drawn: {:?}", shown()),
        );
        rustix::termios::tcflow(&self.terminal, rustix::termios::Action::OOff).unwrap();
        self.master.write_all(b"x").unwrap();
    }

    /// What it has said on its stderr.
    fn said(&self) -> String {
        String::from_utf8_lossy(&std::fs::read(self.said.path()).unwrap()).into_owned()
    }
}

#[cfg(target_os = "linux")]
impl Drop for Stalled {
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
fn the_end_of_plain_lines_input_leaves_the_server_running() {
    // The hosting plan's Task 10 (#359): the end of stdin ends the plain console, and the server
    // runs on until it's stopped, as under `nohup … < /dev/null`, which the guide shows. Nothing
    // had tried it through `fenji-server` itself.
    let dir = tempfile::tempdir().unwrap();
    let mut server = Plain::start(dir.path());
    server.type_line("online");
    assert_eq!(server.answer(), "nobody is online");
    drop(server.stdin.take());
    // A server its input's end stopped would be stopping by now, and an idle one stops in well
    // under a second.
    std::thread::sleep(Duration::from_secs(1));
    assert!(
        server.child.try_wait().unwrap().is_none(),
        "it stopped:\n{}",
        server.said()
    );
    let online = server.console(&["--run", "online"], "");
    assert!(online.status.success(), "{online:?}");
    assert_eq!(text(&online.stdout), "nobody is online\n");
    kill_process(Pid::from_child(&server.child), Signal::TERM).unwrap();
    let status = server.wait();
    let said = server.said();
    assert!(status.success(), "{status}:\n{said}");
    assert!(said.contains("shutting down signal=SIGTERM"), "{said}");
    // Its input's end is no failure of the console's.
    assert!(!said.contains("the console's lines have stopped"), "{said}");
}

#[test]
fn plain_lines_whose_answers_go_unread_are_left_once_the_server_has_stopped() {
    // #359 (the hosting plan's Task 10, carry line 6): once the server has stopped, `run` waits
    // a few seconds at most for its full-screen console (#356), but it waited on its plain lines
    // without a bound. With stdout taking no more, as a pipe nobody reads, the write of an answer waited for
    // ever, and so did `run`: the server stopped, and the process never ended.
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("fenji.toml");
    std::fs::write(
        &config,
        "[server]\nname = \"Fenji\"\nlogin_bind = \"127.0.0.1:0\"\napi_bind = \"127.0.0.1:0\"\n",
    )
    .unwrap();
    let log = tempfile::NamedTempFile::new().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_fenji-server"))
        .arg("--config")
        .arg(&config)
        .args(["run", "--plain"])
        .env("RUST_LOG", "info")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(log.reopen().unwrap())
        .spawn()
        .unwrap();
    let said = || String::from_utf8_lossy(&std::fs::read(log.path()).unwrap()).into_owned();
    // Held open, and never read.
    let _stdout = child.stdout.take().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    until(|| said().contains("Fenji is running"), said);
    // Far more answers than the pipe holds (64 KiB on Linux): each `help` is a line a command.
    for _ in 0..200 {
        writeln!(stdin, "help").unwrap();
    }
    stdin.flush().unwrap();
    // The console's write of an answer waits on the pipe by now.
    std::thread::sleep(Duration::from_secs(1));
    kill_process(Pid::from_child(&child), Signal::TERM).unwrap();
    let status = exit_of(&mut child, said);
    let _ = child.kill();
    assert!(status.success(), "{status}:\n{}", said());
    let said = said();
    assert!(said.contains("shutting down signal=SIGTERM"), "{said}");
    assert!(said.contains("it's left behind"), "{said}");
    assert!(!dir.path().join("fenji.db-wal").exists(), "{said}");
}

/// A config for a server of its own in the folder it's written to.
const CONFIG: &str =
    "[server]\nname = \"Fenji\"\nlogin_bind = \"127.0.0.1:0\"\napi_bind = \"127.0.0.1:0\"\n";

/// `fenji-server console --run <line>` on `config`'s server.
fn console_run(config: &Path, line: &str) -> Command {
    let mut console = Command::new(env!("CARGO_BIN_EXE_fenji-server"));
    console
        .arg("--config")
        .arg(config)
        .args(["console", "--run", line]);
    console
}

/// A pipe for a child's stderr that nobody reads, as a terminal over a frozen link is: full before
/// the child starts, so that its first write waits, for as long as the reading end, returned with
/// it, is held.
fn full_pipe() -> (std::io::PipeReader, std::io::PipeWriter) {
    use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
    let (unread, mut pipe) = std::io::pipe().unwrap();
    let flags = fcntl_getfl(&pipe).unwrap();
    fcntl_setfl(&pipe, flags | OFlags::NONBLOCK).unwrap();
    for chunk in [&[b'.'; 4096][..], b"."] {
        while pipe.write(chunk).is_ok() {}
    }
    // A write waits for room again, as one to a terminal does.
    fcntl_setfl(&pipe, flags).unwrap();
    (unread, pipe)
}

/// A server in a folder of its own, as plain lines, whose log nobody reads: its stderr is a
/// [`full_pipe`] (#359). Killed if the test ends first.
struct Unread {
    child: Child,
    dir: tempfile::TempDir,
    config: PathBuf,
    stdin: Option<ChildStdin>,
    /// What it prints on stdout, a line at a time: the commands' answers.
    answers: mpsc::Receiver<String>,
    /// Held, so that a write to its stderr waits rather than fails.
    _unread: std::io::PipeReader,
}

impl Unread {
    /// The server, up and serving, with its signals caught. Its runtime has `workers` threads, or
    /// as many as it would.
    fn start(workers: Option<&str>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("fenji.toml");
        std::fs::write(&config, CONFIG).unwrap();
        let (unread, stderr) = full_pipe();
        let mut server = Command::new(env!("CARGO_BIN_EXE_fenji-server"));
        server
            .arg("--config")
            .arg(&config)
            .args(["run", "--plain"])
            .env("RUST_LOG", "info")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(stderr);
        match workers {
            Some(workers) => server.env("TOKIO_WORKER_THREADS", workers),
            None => server.env_remove("TOKIO_WORKER_THREADS"),
        };
        let mut child = server.spawn().unwrap();
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let (lines, answers) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if lines.send(line).is_err() {
                    break;
                }
            }
        });
        until(
            || {
                console_run(&config, "online")
                    .output()
                    .unwrap()
                    .status
                    .success()
            },
            || "never answered".into(),
        );
        Unread {
            child,
            dir,
            config,
            stdin,
            answers,
            _unread: unread,
        }
    }

    /// Types `online` on its stdin `n` times, each command logging a line, and waits, 60 s at
    /// most, for each answer: a line of the log that waited would hold its command, and the
    /// answer. Typed from a thread of their own, in case the server reads no more.
    fn serves(&mut self, n: usize) {
        let mut stdin = self.stdin.take().unwrap();
        let typing = std::thread::spawn(move || {
            for _ in 0..n {
                if writeln!(stdin, "online").is_err() {
                    break;
                }
            }
            let _ = stdin.flush();
            stdin
        });
        let deadline = Instant::now() + Duration::from_secs(60);
        for answered in 0..n {
            let left = deadline.saturating_duration_since(Instant::now());
            let answer = self.answers.recv_timeout(left).unwrap_or_else(|e| {
                panic!("{answered} of {n} commands answered ({e}): the log held the server")
            });
            assert_eq!(answer, "nobody is online");
        }
        self.stdin = Some(typing.join().unwrap());
    }

    /// Waits for it to stop by itself, 15 s at most, and cleanly: exit 0, and its store closed.
    fn stops(&mut self, what: &str) {
        let began = Instant::now();
        let status = exit_of(&mut self.child, || format!("still running: {what}"));
        assert!(status.success(), "{status}: {what}");
        assert!(
            began.elapsed() < Duration::from_secs(15),
            "{:?}: {what}",
            began.elapsed()
        );
        assert!(!self.dir.path().join("fenji.db-wal").exists(), "{what}");
    }
}

impl Drop for Unread {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn a_plain_server_whose_log_goes_unread_serves_on_and_stops_on_sigterm() {
    // #359 (Task 7's review, Important 1, and its re-review): under `--plain` the log goes to
    // stderr. A stderr that took no more, a terminal over a frozen link or a pipe nobody reads,
    // held each thread that logged: the commands' answers stopped, and once each of the
    // runtime's workers had a line waiting, at once on a one-CPU host's only one, no signal was
    // seen. No line waits now: one there's no room for is skipped, and counted.
    for workers in [Some("1"), None] {
        let what = format!("SIGTERM, {workers:?} workers");
        let mut server = Unread::start(workers);
        // Each logs a line: more than the log's queue holds.
        server.serves(10_000);
        kill_process(Pid::from_child(&server.child), Signal::TERM).unwrap();
        server.stops(&what);
    }
}

#[test]
fn a_plain_server_whose_log_goes_unread_stops_on_the_consoles_stop() {
    // #359 (Task 7's re-review, Important 1 (b)): the console's `stop` logs its line, then stops
    // the server.
    for workers in [Some("1"), None] {
        let what = format!("the console's stop, {workers:?} workers");
        let mut server = Unread::start(workers);
        server.serves(10_000);
        let mut stop = console_run(&server.config, "stop")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        server.stops(&what);
        assert!(exit_of(&mut stop, || what.clone()).success(), "{what}");
        let mut said = String::new();
        stop.stdout
            .take()
            .unwrap()
            .read_to_string(&mut said)
            .unwrap();
        assert_eq!(said, "stopping the server\n", "{what}");
    }
}

#[test]
fn a_plain_server_that_fails_with_its_log_unread_still_exits() {
    // #359 (Task 7's re-review, new Minor): Rust's `main` printed the error `run` ended with to
    // stderr, waiting on it, and on std's lock of it, which the log's writer may hold through a
    // write nobody reads: the process never exited. The error goes through the log's queue now,
    // after its last lines, and the process exits after its grace.
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("fenji.toml");
    std::fs::write(&config, CONFIG).unwrap();
    // Its data folder is another's: `run` ends at once, with an error.
    let _held = fenji_server::lock::take(dir.path(), Some(&config)).unwrap();
    let (_unread, stderr) = full_pipe();
    let mut child = Command::new(env!("CARGO_BIN_EXE_fenji-server"))
        .arg("--config")
        .arg(&config)
        .args(["run", "--plain"])
        .env("RUST_LOG", "info")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()
        .unwrap();
    let began = Instant::now();
    let status = exit_of(&mut child, || "still running after its error".into());
    assert_eq!(status.code(), Some(1), "{status}");
    assert!(
        began.elapsed() < Duration::from_secs(15),
        "{:?}",
        began.elapsed()
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

/// A config for a [`Window`]'s server, its data folder `dir`, on ports the system picks.
#[cfg(target_os = "linux")]
fn window_config(dir: &Path) -> PathBuf {
    let config = dir.join("fenji.toml");
    let binds = "login_bind = \"127.0.0.1:0\"\napi_bind = \"127.0.0.1:0\"\n";
    std::fs::write(&config, format!("[server]\nname = \"Fenji\"\n{binds}")).unwrap();
    config
}

/// The log of a full-screen server whose data folder is `dir`: its files in `logs`.
#[cfg(target_os = "linux")]
fn logs_of(dir: &Path) -> String {
    std::fs::read_dir(dir.join("logs"))
        .unwrap()
        .map(|f| std::fs::read_to_string(f.unwrap().path()).unwrap())
        .collect()
}

/// The CPU time process `pid` has used, in clock ticks (100 a second here): its utime and stime in
/// /proc.
#[cfg(target_os = "linux")]
fn cpu_ticks(pid: u32) -> u64 {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    // After the program's name, in parentheses that may hold anything: the state, then the rest,
    // utime being the 12th field from it and stime the 13th.
    let fields: Vec<&str> = stat[stat.rfind(')').unwrap() + 2..]
        .split_whitespace()
        .collect();
    fields[11].parse::<u64>().unwrap() + fields[12].parse::<u64>().unwrap()
}

/// Closes `window` on a server whose data folder is `dir`: the server stops on the hang-up, its
/// store closed and its log saying why, and its process ends with success, within 15 s.
#[cfg(target_os = "linux")]
fn closed_cleanly(mut window: Window, dir: &Path) {
    let wal = dir.join("fenji.db-wal");
    assert!(wal.exists(), "the store is open");
    window.close();
    let closed = Instant::now();
    let status = exit_of(&mut window.child, || {
        "still running after its window closed".into()
    });
    let took = closed.elapsed();
    assert!(status.success(), "{status}");
    assert!(took < Duration::from_secs(15), "it took {took:?}");
    assert!(!wal.exists() && !dir.join("fenji.db-shm").exists());
    // Its log, in the data folder's logs as the full-screen console has it, says why it stopped.
    let logs = logs_of(dir);
    assert!(logs.contains("shutting down signal=SIGHUP"), "{logs}");
}

#[cfg(target_os = "linux")]
#[test]
fn a_console_left_behind_gives_the_terminal_its_mode_back() {
    // #359: once the server has stopped, `run` waits a few seconds at most for a console that
    // hasn't ended, then leaves it to its thread, caught in a write to a terminal that has stopped reading, as
    // over a stalled SSH link. The terminal was left raw: once the link came back, the shell after
    // the server read no lines and echoed nothing, until `reset`. `run` gives it its mode back
    // before it leaves the console, without a write.
    let dir = tempfile::tempdir().unwrap();
    let mut window = Window::open(&window_config(dir.path()), &["run"]);
    window.drawn("online");
    assert!(!window.cooked(), "the console holds the terminal raw");
    // The terminal takes no more output: the console's next frame, at the next second, waits.
    rustix::termios::tcflow(&window.terminal, rustix::termios::Action::OOff).unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    kill_process(Pid::from_child(&window.child), Signal::TERM).unwrap();
    let status = exit_of(&mut window.child, || logs_of(dir.path()));
    assert!(status.success(), "{status}");
    let logs = logs_of(dir.path());
    assert!(logs.contains("it's left behind"), "{logs}");
    assert!(window.cooked(), "the terminal is left raw");
}

#[cfg(target_os = "linux")]
#[test]
fn an_attached_console_left_by_a_signal_gives_the_terminal_its_mode_back() {
    // #359 (Task 2's review, Minor 5): an attached console whose terminal takes no more output,
    // as over a stalled SSH link, is left 2 s after a signal to leave, its thread caught in a
    // write to that terminal. It exited 0 with the terminal raw: once the link came back, the
    // shell after it read no lines and echoed nothing, until `reset`. It gives the terminal its
    // mode back first, without a write, as `run` does with a console it leaves behind.
    let dir = tempfile::tempdir().unwrap();
    let server = Plain::start(dir.path());
    server.running();
    let mut console = Window::holding(&server.config, &["console"], Held::Given);
    console.drawn("attached");
    assert!(!console.cooked(), "the console holds the terminal raw");
    // The terminal takes no more output, and a key makes a frame due at once: its write waits,
    // well before the signal, so the console is caught in it, however loaded the machine (Task 7's
    // review, Minor 4). Without the frame, it would leave as usual, and its give-back gives the
    // mode back before its own write waits.
    rustix::termios::tcflow(&console.terminal, rustix::termios::Action::OOff).unwrap();
    console.type_keys(b"x");
    std::thread::sleep(Duration::from_millis(1000));
    kill_process(Pid::from_child(&console.child), Signal::TERM).unwrap();
    let status = exit_of(&mut console.child, || "still running after SIGTERM".into());
    assert_eq!(status.code(), Some(0), "{status}");
    assert!(console.cooked(), "the terminal is left raw");
}

#[cfg(target_os = "linux")]
#[test]
fn a_full_screen_console_that_fails_suggests_plain_lines_with_the_servers_config() {
    // #359 (#404's review): as it closes, `run`'s full-screen console suggests plain lines, with
    // the `--config` that `main` gives `run`. The server runs on, and SIGTERM stops it.
    let dir = tempfile::tempdir().unwrap();
    let config = window_config(dir.path());
    let mut server = Stalled::start(&config, &["run"]);
    server.stalls_after("online");
    until(
        || server.said().contains("gives the console as plain lines"),
        || format!("no note: {:?}", server.said()),
    );
    let plain = format!(
        "`fenji-server --config {} --plain` gives the console as plain lines",
        config.display()
    );
    assert!(server.said().contains(&plain), "{}", server.said());
    kill_process(Pid::from_child(&server.child), Signal::TERM).unwrap();
    let status = exit_of(&mut server.child, || logs_of(dir.path()));
    assert!(status.success(), "{status}");
}

#[cfg(target_os = "linux")]
#[test]
fn an_attached_full_screen_console_that_fails_suggests_plain_lines_with_its_config() {
    // #359 (#404's review): an attached full-screen console that fails suggests plain lines, with
    // the `--config` that `main` gives it, and exits 1.
    let dir = tempfile::tempdir().unwrap();
    let server = Plain::start(dir.path());
    server.running();
    let mut console = Stalled::start(&server.config, &["console"]);
    console.stalls_after("attached");
    let status = exit_of(&mut console.child, || "the console runs on".into());
    assert_eq!(status.code(), Some(1), "{status}");
    let plain = format!(
        "`fenji-server --config {} console --plain` gives the console as plain lines",
        server.config.display()
    );
    assert!(console.said().contains(&plain), "{}", console.said());
}

#[cfg(target_os = "linux")]
#[test]
fn the_full_screen_console_runs_what_is_typed_and_ctrl_c_stops_the_server() {
    // #359: on Linux and macOS the console reads its keys itself, rather than through crossterm's
    // event source. A command typed and Enter, a cursor key on the way, then Ctrl-C.
    let dir = tempfile::tempdir().unwrap();
    let mut window = Window::open(&window_config(dir.path()), &["run"]);
    window.drawn("online");
    window.type_keys(b"onlie\x1b[Dn\x1b[F\r");
    window.drawn("nobody is online");
    window.type_keys(b"\x03");
    let status = exit_of(&mut window.child, || logs_of(dir.path()));
    assert!(status.success(), "{status}");
    let logs = logs_of(dir.path());
    assert!(
        logs.contains("shutting down, as the console asked"),
        "{logs}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_server_whose_terminal_hangs_up_with_sighup_ignored_runs_on_without_spinning() {
    // #359: with SIGHUP ignored on purpose (`trap '' HUP`), closing the window doesn't stop the
    // server, and crossterm 0.29 read the hung-up terminal over and over without end: a core spun
    // for as long as the server served. The console ends on the hang-up now, and the server runs
    // on without it, says so in its log, and stops cleanly on Ctrl-C's SIGINT as before.
    let dir = tempfile::tempdir().unwrap();
    let config = window_config(dir.path());
    let mut window = Window::holding(&config, &["run"], Held::ControllingHangupIgnored);
    window.drawn("online");
    window.close();
    let pid = window.child.id();
    // Settled: the console has ended, and anything that would spin is spinning.
    std::thread::sleep(Duration::from_secs(1));
    let before = cpu_ticks(pid);
    std::thread::sleep(Duration::from_secs(2));
    let used = cpu_ticks(pid) - before;
    let running = window.child.try_wait().unwrap().is_none();
    assert!(running, "it stopped:\n{}", logs_of(dir.path()));
    // A core spinning uses 200 ticks in 2 s; a server at rest, a few.
    assert!(used < 50, "{used} ticks in 2 s:\n{}", logs_of(dir.path()));
    let logs = logs_of(dir.path());
    assert!(logs.contains("the console's terminal has gone"), "{logs}");
    // How to reach it, with the config it was started with (#359, Task 7's re-review).
    let attach = format!(
        "`fenji-server --config {} console` attaches to it",
        config.display()
    );
    assert!(logs.contains(&attach), "{logs}");
    // It serves, and answers a console.
    let online = Command::new(env!("CARGO_BIN_EXE_fenji-server"))
        .arg("--config")
        .arg(&config)
        .args(["console", "--run", "online"])
        .output()
        .unwrap();
    assert!(online.status.success(), "{online:?}");
    kill_process(Pid::from_child(&window.child), Signal::INT).unwrap();
    let status = exit_of(&mut window.child, || logs_of(dir.path()));
    assert!(status.success(), "{status}");
    let logs = logs_of(dir.path());
    assert!(logs.contains("shutting down signal=SIGINT"), "{logs}");
    assert!(!dir.path().join("fenji.db-wal").exists(), "{logs}");
}

#[cfg(target_os = "linux")]
#[test]
fn an_attached_full_screen_console_whose_terminal_hangs_up_exits_0() {
    // #359: a hang-up failed the console's writes, and with no SIGHUP to say the host had left, as
    // a terminal that isn't the console's controlling one sends none, it exited 1, as a console
    // that failed. A terminal that has gone is the host leaving, however it went: 0.
    let dir = tempfile::tempdir().unwrap();
    let server = Plain::start(dir.path());
    server.running();
    for held in [
        Held::Given,
        Held::Controlling,
        Held::ControllingHangupIgnored,
    ] {
        let mut console = Window::holding(&server.config, &["console"], held);
        console.drawn("attached");
        console.close();
        let status = exit_of(&mut console.child, || format!("{held:?}: still running"));
        assert_eq!(status.code(), Some(0), "{held:?}: {status}");
    }
    // The server runs on, and answers a console. One it hasn't seen go yet holds its place a
    // moment more (tests/attach.rs's `a_console_that_goes_gives_its_place_back_at_once`).
    until(
        || server.console(&["--run", "online"], "").status.success(),
        || "no console is answered".into(),
    );
}

#[cfg(target_os = "linux")]
#[test]
fn closing_the_window_of_a_full_screen_server_stops_it_and_ends_its_process() {
    // The hosting plan's final review, Important 1: closing the window stops the server cleanly,
    // and the process ends, here once the console is up and drawing.
    let dir = tempfile::tempdir().unwrap();
    let window = Window::open(&window_config(dir.path()), &["run"]);
    // The status strip's word: the console is up, and the server runs.
    window.drawn("online");
    closed_cleanly(window, dir.path());
}

#[cfg(target_os = "linux")]
#[test]
fn the_console_draws_at_once_in_a_terminal_that_never_answers() {
    // #359: as it started, the console cleared the screen with ratatui's Terminal::clear, which asks
    // the terminal where its cursor is and waits up to 2 s for the answer. A terminal that never
    // answered cost the 2 s, then ended the console. It clears the screen without asking now.
    let dir = tempfile::tempdir().unwrap();
    let window = Window::open(&window_config(dir.path()), &["run"]);
    window.drawn("\x1b[?1049h");
    let took = Instant::now();
    // The status strip's word: the first frame is drawn.
    while !window.shown().contains("online") {
        assert!(
            took.elapsed() < Duration::from_secs(1),
            "not drawn within 1 s of taking the screen: {:?}",
            window.shown()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let asked = window
        .shown()
        .as_bytes()
        .windows(CURSOR_ASKED.len())
        .any(|w| w == CURSOR_ASKED);
    assert!(!asked, "it asked where the cursor is");
}

#[cfg(target_os = "linux")]
#[test]
fn closing_the_window_as_the_console_starts_still_ends_the_process() {
    // The fix pass's re-review, NB1: a window closed as the console started, then waiting for the
    // terminal's answer to where its cursor was, hung the terminal up under a read (crossterm's)
    // that never returned. The server stopped cleanly on its SIGHUP, but `run` waited for its
    // console without end, the process spinning a core. The console asks nothing now (#359): closed
    // as the console takes the screen, the server stops cleanly and the process ends.
    let dir = tempfile::tempdir().unwrap();
    let window = Window::open(&window_config(dir.path()), &["run"]);
    window.drawn("\x1b[?1049h");
    closed_cleanly(window, dir.path());
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
    // As Rust's `main` would print it, through the log's stderr now (#359).
    assert!(
        said.contains("Error: another fenji-server holds this data folder ("),
        "{said}"
    );
    // To attach, with the config it was given (#359, Task 2's review, Minor 2).
    let attach = format!("`fenji-server --config {} console`", other.display());
    assert!(said.contains(&attach), "{said}");
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
    // And none of the four that left by a signal keeps its place.
    all_four_places_come_free(&server.config);
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
    // And none of the four that left by a signal keeps its place.
    all_four_places_come_free(&server.config);
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
