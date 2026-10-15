//! The console as plain lines (M4 hosting spec §3.1): commands from stdin, a pipe or a terminal
//! in its usual mode, for systemd, scripts, screen readers and simple terminals.
use std::collections::HashSet;
use std::io::{self, Write};

use tokio::io::{AsyncBufRead, AsyncBufReadExt};

use crate::text::{clean, plain};
use crate::wire::{ConsoleMsg, INPUT_MAX, ServerMsg};
use crate::{Ending, Session};

/// Runs each non-empty line of `input` as a command and writes each answer's lines to `out`. With
/// `print_logs`, the server's log goes to `err` as `HH:MM:SS LEVEL message`. Everything printed is
/// cleaned of hidden characters.
///
/// At the end of the input it waits for the answers still due, then ends with
/// [`Ending::InputClosed`]. A line over [`INPUT_MAX`] bytes, or with a hidden character, isn't
/// sent, and `err` says why.
pub async fn run_plain(
    session: Session,
    input: impl AsyncBufRead + Unpin,
    mut out: impl Write,
    mut err: impl Write,
    print_logs: bool,
) -> io::Result<Ending> {
    let Session {
        mut from_server,
        to_server,
        ..
    } = session;
    let mut lines = Lines::new(input);
    let mut input_open = true;
    // A line read and not yet sent: no more is read until it is.
    let mut queued: Option<String> = None;
    let mut waiting = HashSet::new();
    let mut last_id = 0;
    loop {
        if !input_open && queued.is_none() && waiting.is_empty() {
            return Ok(Ending::InputClosed);
        }
        // No branch goes first, so neither a flood from the server nor a fast pipe of commands
        // keeps the other waiting.
        tokio::select! {
            msg = from_server.recv() => match msg {
                None => return Ok(Ending::Lost),
                Some(ServerMsg::Bye { reason }) => return Ok(Ending::ServerStopped(reason)),
                Some(ServerMsg::Answer { id, lines, .. }) => {
                    for line in &lines {
                        writeln!(out, "{}", clean(line))?;
                    }
                    out.flush()?;
                    waiting.remove(&id);
                }
                Some(ServerMsg::Log { line }) if print_logs => {
                    writeln!(err, "{line}")?;
                    err.flush()?;
                }
                Some(ServerMsg::Skipped { lines }) if print_logs => {
                    writeln!(err, "… {lines} lines skipped")?;
                    err.flush()?;
                }
                Some(_) => {}
            },
            permit = to_server.reserve(), if queued.is_some() => {
                let Ok(permit) = permit else {
                    return Ok(Ending::Lost);
                };
                if let Some(line) = queued.take() {
                    last_id += 1;
                    permit.send(ConsoleMsg::Run { id: last_id, line });
                    waiting.insert(last_id);
                }
            }
            read = lines.next(), if input_open && queued.is_none() => match read? {
                None => input_open = false,
                Some(Read::TooLong) => {
                    writeln!(err, "that line is too long: a command is at most {INPUT_MAX} bytes")?;
                    err.flush()?;
                }
                Some(Read::Line(line)) => match line.trim() {
                    "" => {}
                    line if !plain(line) => {
                        writeln!(err, "commands take plain text only")?;
                        err.flush()?;
                    }
                    line => queued = Some(line.to_owned()),
                },
            },
        }
    }
}

/// A line of the input.
enum Read {
    Line(String),
    /// One over [`INPUT_MAX`] bytes, which was let go.
    TooLong,
}

/// The input's lines, of which at most [`INPUT_MAX`] bytes are kept, so a long one can't take the
/// memory. Cancel-safe: a line read in part stays here until its end comes.
struct Lines<R> {
    input: R,
    line: Vec<u8>,
    too_long: bool,
}

impl<R: AsyncBufRead + Unpin> Lines<R> {
    fn new(input: R) -> Self {
        Lines {
            input,
            line: Vec::new(),
            too_long: false,
        }
    }

    /// The next line, without its line ending; `None` at the input's end.
    async fn next(&mut self) -> io::Result<Option<Read>> {
        loop {
            let buf = self.input.fill_buf().await?;
            if buf.is_empty() {
                // A last line without its newline counts.
                let any = !self.line.is_empty() || self.too_long;
                return Ok(any.then(|| self.take()));
            }
            let (part, used) = match buf.iter().position(|&b| b == b'\n') {
                Some(end) => (&buf[..end], end + 1),
                None => (buf, buf.len()),
            };
            if self.too_long || self.line.len() + part.len() > INPUT_MAX {
                self.too_long = true;
                self.line.clear();
            } else {
                self.line.extend_from_slice(part);
            }
            let ended = used > part.len();
            self.input.consume(used);
            if ended {
                return Ok(Some(self.take()));
            }
        }
    }

    fn take(&mut self) -> Read {
        let line = std::mem::take(&mut self.line);
        if std::mem::take(&mut self.too_long) {
            Read::TooLong
        } else {
            Read::Line(String::from_utf8_lossy(&line).into_owned())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::tests::help;
    use crate::wire::{ConsoleMsg, INPUT_MAX, Level, LogLine, Part, ServerMsg};
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncWriteExt, BufReader};
    use tokio::sync::mpsc;

    /// A writer that keeps what was flushed apart from what was written since.
    #[derive(Clone, Default)]
    struct Flushed(Arc<Mutex<(Vec<u8>, Vec<u8>)>>);

    impl Write for Flushed {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().1.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            let mut written = self.0.lock().unwrap();
            let pending = std::mem::take(&mut written.1);
            written.0.extend(pending);
            Ok(())
        }
    }

    impl Flushed {
        /// What was flushed, and what was written after.
        fn text(&self) -> (String, String) {
            let written = self.0.lock().unwrap();
            let text = |bytes: &[u8]| String::from_utf8(bytes.to_vec()).unwrap();
            (text(&written.0), text(&written.1))
        }
    }

    #[tokio::test]
    async fn refusals_reach_err_at_once() {
        // Review M5: like everything else plain mode writes, a refusal is flushed.
        let (_to_console, from_server) = mpsc::channel(8);
        let (to_server, _console_out) = mpsc::channel(8);
        let session = Session::for_test(help(), from_server, to_server);
        let err = Flushed::default();
        let input = format!("{}\nannounce \u{1b}[2J\n", "x".repeat(INPUT_MAX + 1));
        let ending = run_plain(
            session,
            input.as_bytes(),
            std::io::sink(),
            err.clone(),
            true,
        )
        .await
        .unwrap();
        assert_eq!(ending, Ending::InputClosed);
        let (flushed, pending) = err.text();
        assert!(pending.is_empty(), "written, not flushed: {pending:?}");
        assert!(
            flushed.contains("too long") && flushed.contains("plain text only"),
            "{flushed:?}"
        );
    }

    #[tokio::test]
    async fn a_flood_from_the_server_doesnt_hold_up_a_typed_command() {
        // Review M1: with the server's queue never empty, the input still takes its turns.
        const FLOOD: usize = 10_000;
        let (to_console, from_server) = mpsc::channel(FLOOD);
        for n in 0..FLOOD {
            to_console
                .try_send(ServerMsg::Log {
                    line: LogLine {
                        at_ms: 0,
                        level: Level::Info,
                        parts: vec![Part {
                            text: format!("line {n}"),
                            name: false,
                        }],
                    },
                })
                .unwrap();
        }
        let (to_server, mut console_out) = mpsc::channel(8);
        let session = Session::for_test(help(), from_server, to_server);
        let server = tokio::spawn(async move {
            let ConsoleMsg::Run { id, line } = console_out.recv().await.unwrap();
            assert_eq!(line, "stop");
            let unread = to_console.max_capacity() - to_console.capacity();
            to_console
                .send(ServerMsg::Answer {
                    id,
                    ok: true,
                    lines: vec![],
                })
                .await
                .unwrap();
            unread
        });
        // Typed through tokio's own I/O, which shares the task's budget as stdin does.
        let (mut typed, input) = tokio::io::duplex(64);
        typed.write_all(b"stop\n").await.unwrap();
        drop(typed);
        let ending = run_plain(
            session,
            BufReader::new(input),
            std::io::sink(),
            std::io::sink(),
            true,
        )
        .await
        .unwrap();
        let unread = server.await.unwrap();
        assert_eq!(ending, Ending::InputClosed);
        assert!(
            unread > FLOOD / 2,
            "`stop` went out only after {} of the {FLOOD} log lines",
            FLOOD - unread
        );
    }

    #[tokio::test]
    async fn plain_mode_runs_lines_and_prints_answers() {
        let (to_console, from_server) = mpsc::channel(8);
        let (to_server, mut console_out) = mpsc::channel(8);
        let session = Session::for_test(help(), from_server, to_server);
        let answer = tokio::spawn(async move {
            let ConsoleMsg::Run { id, line } = console_out.recv().await.unwrap();
            assert_eq!(line, "online");
            to_console
                .send(ServerMsg::Answer {
                    id,
                    ok: true,
                    lines: vec!["nobody is online".into()],
                })
                .await
                .unwrap();
        });
        let mut out = Vec::new();
        let ending = run_plain(session, &b"online\n"[..], &mut out, std::io::sink(), true)
            .await
            .unwrap();
        answer.await.unwrap();
        assert_eq!(ending, Ending::InputClosed);
        assert_eq!(String::from_utf8(out).unwrap(), "nobody is online\n");
    }

    #[tokio::test]
    async fn plain_mode_prints_the_log_cleaned_and_ends_with_the_server() {
        // Review Focus 1: a player's name can't act on the host's terminal.
        let (to_console, from_server) = mpsc::channel(8);
        let (to_server, _console_out) = mpsc::channel(8);
        let session = Session::for_test(help(), from_server, to_server);
        for msg in [
            ServerMsg::Log {
                line: LogLine {
                    at_ms: 3_723_000,
                    level: Level::Warn,
                    parts: vec![
                        Part {
                            text: "chat from ".into(),
                            name: false,
                        },
                        Part {
                            text: "Mai\u{1b}[2J".into(),
                            name: true,
                        },
                    ],
                },
            },
            ServerMsg::Skipped { lines: 37 },
            ServerMsg::Bye {
                reason: "the server is stopping".into(),
            },
        ] {
            to_console.send(msg).await.unwrap();
        }
        // Input that stays open: the server's goodbye ends the session.
        let (_open, input) = tokio::io::duplex(64);
        let mut err = Vec::new();
        let ending = run_plain(
            session,
            BufReader::new(input),
            std::io::sink(),
            &mut err,
            true,
        )
        .await
        .unwrap();
        assert_eq!(
            ending,
            Ending::ServerStopped("the server is stopping".into())
        );
        assert_eq!(
            String::from_utf8(err).unwrap(),
            "01:02:03 WARN  chat from Mai\\u{1b}[2J\n… 37 lines skipped\n"
        );
    }

    #[tokio::test]
    async fn without_print_logs_the_log_stays_off_and_a_lost_server_ends_it() {
        let (to_console, from_server) = mpsc::channel(8);
        let (to_server, _console_out) = mpsc::channel(8);
        let session = Session::for_test(help(), from_server, to_server);
        to_console
            .send(ServerMsg::Log {
                line: LogLine {
                    at_ms: 0,
                    level: Level::Info,
                    parts: vec![Part {
                        text: "listening".into(),
                        name: false,
                    }],
                },
            })
            .await
            .unwrap();
        drop(to_console);
        let (_open, input) = tokio::io::duplex(64);
        let mut err = Vec::new();
        let ending = run_plain(
            session,
            BufReader::new(input),
            std::io::sink(),
            &mut err,
            false,
        )
        .await
        .unwrap();
        assert_eq!(ending, Ending::Lost);
        assert!(err.is_empty());
    }

    #[tokio::test]
    async fn blank_overlong_and_hidden_lines_are_not_sent() {
        let (to_console, from_server) = mpsc::channel(8);
        let (to_server, mut console_out) = mpsc::channel(8);
        let session = Session::for_test(help(), from_server, to_server);
        let answer = tokio::spawn(async move {
            let ConsoleMsg::Run { id, line } = console_out.recv().await.unwrap();
            assert_eq!(line, "online");
            to_console
                .send(ServerMsg::Answer {
                    id,
                    ok: false,
                    lines: vec!["a\u{1b}b".into()],
                })
                .await
                .unwrap();
            assert!(console_out.recv().await.is_none(), "nothing else was sent");
        });
        let input = format!(
            "\n   \r\n{}\nannounce \u{1b}[2J\n  online\r\n",
            "x".repeat(INPUT_MAX + 1)
        );
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let ending = run_plain(session, input.as_bytes(), &mut out, &mut err, true)
            .await
            .unwrap();
        assert_eq!(ending, Ending::InputClosed);
        answer.await.unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "a\\u{1b}b\n");
        let err = String::from_utf8(err).unwrap();
        assert!(err.contains("too long"), "{err}");
        assert!(err.contains("plain text only"), "{err}");
    }
}
