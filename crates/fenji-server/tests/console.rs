//! The host console through `fenji-server` itself (M4 hosting spec §3.1, §3.5): as plain lines,
//! commands from stdin, answers on stdout and the log on stderr; and `fenji-server console`,
//! attached to a running server.
//!
//! The tests that start processes are here rather than in `tests/attach.rs`: a process being
//! started holds a copy of every open file of the test's until it runs, so a socket or a lock that
//! test has just let go of would live on for a moment.
#![cfg(unix)]
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

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
        let log = tempfile::NamedTempFile::new().unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_fenji-server"))
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
fn a_second_server_on_a_data_folder_in_use_refuses_before_it_listens() {
    // Review Focus 5: the second is given the first's gate port, so a start that listened before
    // it claimed the data folder would fail there instead.
    let dir = tempfile::tempdir().unwrap();
    let first = Plain::start(dir.path());
    let gate = first.running();
    let mut second = Plain::with(&first.config, &[("FENJI_LOGIN_BIND", &gate)]);
    assert!(!second.wait().success());
    let said = second.said();
    assert!(
        said.contains("a server is already using this data folder ("),
        "{said}"
    );
    assert!(!said.contains("can't listen"), "{said}");
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
