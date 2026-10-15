//! The host console as plain lines, through `fenji-server` itself (M4 hosting spec §3.1): commands
//! from stdin, answers on stdout, the log on stderr.
#![cfg(unix)]
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// `fenji-server` run with `--plain` on a config in `dir` with free loopback ports, stdin and stdout
/// piped and its log in a file; killed if the test ends first.
struct Plain {
    child: Child,
    stdin: Option<std::process::ChildStdin>,
    /// The lines it prints on stdout.
    out: mpsc::Receiver<String>,
    log: tempfile::NamedTempFile,
}

impl Plain {
    fn start(dir: &Path) -> Self {
        let config = dir.join("fenji.toml");
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
}

impl Drop for Plain {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
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
