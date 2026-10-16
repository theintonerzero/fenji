//! A console attached to a running server (M4 hosting spec §3.1, §3.5): `fenji-server console`
//! connects to the console socket of its config's data folder and takes the server's hello,
//! refusing a server that speaks another protocol. Then it shows the full-screen console, or plain
//! lines, which leave on Ctrl-C, `exit` or `quit` while the server runs on, and on SIGTERM,
//! SIGHUP, SIGINT and SIGQUIT too, giving the terminal back (Review Focus 2). `--run` runs one
//! command for a script, prints its answer, and exits 0 if it ran, or 1. Linux and macOS: Windows
//! has no console socket in M4.
use std::process::ExitCode;

use crate::config::Config;

#[cfg(unix)]
pub use self::unix::run_once;

/// `fenji-server console`: attaches to the server that runs on `cfg`'s data folder. With `run`, it
/// runs that command, prints its answer's lines and exits 0 if it ran, or 1. Otherwise it's the
/// full-screen console, or plain lines with `plain` or without a terminal, until the host leaves
/// or the server says goodbye.
///
/// # Errors
/// If no server answers on the data folder's console socket, or the server speaks another protocol.
#[cfg(unix)]
pub async fn console(cfg: &Config, run: Option<&str>, plain: bool) -> anyhow::Result<ExitCode> {
    unix::console(cfg, run, plain).await
}

/// `fenji-server console` on Windows, which has no console socket in M4.
///
/// # Errors
/// Always.
#[cfg(not(unix))]
pub async fn console(_cfg: &Config, _run: Option<&str>, _plain: bool) -> anyhow::Result<ExitCode> {
    anyhow::bail!("attaching isn't available on Windows; run the server in its own window")
}

#[cfg(unix)]
mod unix {
    use std::io::{self, IsTerminal, Write};
    use std::path::Path;
    use std::process::ExitCode;
    use std::time::Duration;

    use anyhow::{Context, anyhow, bail};
    use fenji_console::text::clean;
    use fenji_console::wire::{self, ConsoleMsg, INPUT_MAX, PROTO, ServerMsg};
    use fenji_console::{Colours, Ending, Hello, Mode, Session, run_plain, run_tui};
    use tokio::io::{AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;
    use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
    use tokio::sync::{mpsc, watch};

    use crate::config::{self, Config};
    use crate::console::socket::{self, Line};
    use crate::console::{self, Front};
    use crate::{lock, server};

    /// How long a server has to say hello.
    const HELLO_WAIT: Duration = Duration::from_secs(10);

    /// How long a console left by a signal has to give its terminal back. A terminal that takes no
    /// more output, as a frozen link's doesn't, would keep it from ending: it ends without.
    const LEAVE_WAIT: Duration = Duration::from_secs(2);

    /// How long a console that ended with an error waits for a signal to leave: a terminal that
    /// hangs up fails the console's reads and writes as its SIGHUP comes.
    const HANGUP_WAIT: Duration = Duration::from_millis(200);

    /// The server's messages that may wait for the console.
    const FROM_SERVER: usize = 64;

    /// The command lines the console may send ahead of the socket.
    const TO_SERVER: usize = 16;

    pub(super) async fn console(
        cfg: &Config,
        run: Option<&str>,
        plain: bool,
    ) -> anyhow::Result<ExitCode> {
        let socket = socket::path(&config::data_folder(&cfg.store.path));
        if let Some(line) = run {
            let (ok, lines) = run_once(&socket, line).await?;
            let mut out = io::stdout().lock();
            for line in &lines {
                writeln!(out, "{}", clean(line))?;
            }
            out.flush()?;
            return Ok(exit(ok));
        }
        let session = Link::connect(&socket).await?.session();
        let left = leave_signals()?;
        let term = std::env::var("TERM").ok();
        let terminal = (io::stdin().is_terminal(), io::stdout().is_terminal());
        let front = console::front(terminal.0, terminal.1, term.as_deref(), plain);
        let leave = left_now(left.clone());
        // Off the runtime's workers either way, as `run`'s own console is: a terminal that takes
        // no more holds the console's thread alone, and the signals still come.
        let shown = async move {
            match front {
                Front::Full => {
                    let tui = run_tui(session, Mode::Attached, Colours::from_env(), leave);
                    server::held(tui).await
                }
                Front::Plain => {
                    let input = BufReader::new(tokio::io::stdin());
                    let (out, err) = (io::stdout(), io::stderr());
                    let lines = run_plain(session, input, out, err, true, Mode::Attached, leave);
                    server::held(lines).await
                }
            }
        };
        let ended = tokio::select! {
            ended = shown => settle(ended, left).await,
            // Left, but its terminal takes nothing more: there's nothing to give back, or to say.
            () = stuck(left.clone()) => return Ok(ExitCode::SUCCESS),
        };
        let (note, ok) = ending(&ended);
        if let Some(note) = note {
            server::say(&note);
        }
        Ok(exit(ok))
    }

    /// The host leaving the console by a signal: from now on SIGTERM, SIGHUP, SIGINT and SIGQUIT
    /// no longer end the process at once, with its terminal still the console's (Review Focus 2),
    /// but turn this value true, which ends the console as Ctrl-C ends the full-screen one.
    fn leave_signals() -> io::Result<watch::Receiver<bool>> {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate())?;
        let mut hangup = signal(SignalKind::hangup())?;
        let mut interrupt = signal(SignalKind::interrupt())?;
        let mut quit = signal(SignalKind::quit())?;
        let (leave, left) = watch::channel(false);
        tokio::spawn(async move {
            tokio::select! {
                _ = term.recv() => {}
                _ = hangup.recv() => {}
                _ = interrupt.recv() => {}
                _ = quit.recv() => {}
            }
            leave.send_replace(true);
        });
        Ok(left)
    }

    /// Done once the host has left the console by a signal.
    async fn left_now(mut left: watch::Receiver<bool>) {
        let gone = left.wait_for(|left| *left).await.is_err();
        if gone {
            // The signals' task ended without one: there's none to wait for.
            std::future::pending::<()>().await;
        }
    }

    /// Done once the host has left the console by a signal and [`LEAVE_WAIT`] has passed, the
    /// console still running.
    async fn stuck(left: watch::Receiver<bool>) {
        left_now(left).await;
        tokio::time::sleep(LEAVE_WAIT).await;
    }

    /// How the console ended, with the host's leaving counted: after a signal to leave, any ending
    /// but the server's goodbye is the host's, whatever the terminal did meanwhile. An error waits
    /// [`HANGUP_WAIT`] for such a signal, as a terminal that hangs up fails the console's reads as
    /// its SIGHUP comes.
    async fn settle(ended: io::Result<Ending>, left: watch::Receiver<bool>) -> io::Result<Ending> {
        if ended.is_err() {
            let _ = tokio::time::timeout(HANGUP_WAIT, left_now(left.clone())).await;
        }
        match ended {
            Ok(Ending::ServerStopped(reason)) => Ok(Ending::ServerStopped(reason)),
            _ if *left.borrow() => Ok(Ending::Detached),
            ended => ended,
        }
    }

    fn exit(ok: bool) -> ExitCode {
        if ok {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }

    /// What the host is told as an attached console ends, and whether it ended as it should.
    fn ending(ended: &io::Result<Ending>) -> (Option<String>, bool) {
        match ended {
            Ok(Ending::Detached) => (Some("Left the console. The server runs on.".into()), true),
            Ok(Ending::ServerStopped(reason)) => (
                Some(format!("The server closed the console: {reason}.")),
                true,
            ),
            Ok(Ending::InputClosed) => (None, true),
            Ok(Ending::Lost) => (
                Some("The console lost the server: it closed without a goodbye.".into()),
                false,
            ),
            Err(e) => (Some(format!("The console has closed: {e}.")), false),
        }
    }

    /// Runs `line` on the server whose console socket is `socket`: whether it ran, and its
    /// answer's lines, an answer in parts put together. Everything else the server sends is let
    /// go. A blank line is no command, and nothing is sent (carry-forward 2).
    ///
    /// # Errors
    /// If no server answers there, or it speaks another protocol, or it closes the console before
    /// it answers.
    pub async fn run_once(socket: &Path, line: &str) -> anyhow::Result<(bool, Vec<String>)> {
        let mut link = Link::connect(socket).await?;
        let line = line.trim();
        if line.is_empty() {
            return Ok((true, Vec::new()));
        }
        if line.len() > INPUT_MAX {
            bail!("that command is too long: a command is at most {INPUT_MAX} bytes");
        }
        let id = 1;
        link.send(&ConsoleMsg::Run {
            id,
            line: line.into(),
        })
        .await
        .context("can't send the command to the server")?;
        let mut lines = Vec::new();
        loop {
            let next = link.next().await;
            match next.context("lost the server before it answered")? {
                Some(ServerMsg::Answer {
                    id: answered,
                    ok,
                    lines: part,
                    more,
                }) if answered == id => {
                    lines.extend(part);
                    if !more {
                        return Ok((ok, lines));
                    }
                }
                Some(ServerMsg::Bye { reason }) => bail!(
                    "the server closed the console before it answered: {}",
                    clean(&reason)
                ),
                Some(_) => {}
                None => bail!("the server closed the console before it answered"),
            }
        }
    }

    /// A console's connection to a server, after its hello.
    struct Link {
        hello: Hello,
        reader: BufReader<OwnedReadHalf>,
        writer: OwnedWriteHalf,
        buf: Vec<u8>,
    }

    impl Link {
        /// Connects to the console socket at `socket` and takes the server's hello: refused if
        /// the server turns the console away, or speaks another protocol.
        async fn connect(socket: &Path) -> anyhow::Result<Link> {
            let stream = UnixStream::connect(socket)
                .await
                .map_err(|e| unreachable(socket, e))?;
            let (read, writer) = stream.into_split();
            let mut reader = BufReader::new(read);
            let mut buf = Vec::new();
            let first = tokio::time::timeout(HELLO_WAIT, next(&mut reader, &mut buf))
                .await
                .map_err(|_| {
                    anyhow!(
                        "the server didn't say hello within {} s",
                        HELLO_WAIT.as_secs()
                    )
                })?
                .context("can't read the server's hello")?;
            let hello = match first.map(Hello::try_from) {
                Some(Ok(hello)) => hello,
                Some(Err(ServerMsg::Bye { reason })) => {
                    bail!("the server turned the console away: {}", clean(&reason))
                }
                Some(Err(_)) => bail!("the server didn't say hello first"),
                None => bail!("the server closed the console at once"),
            };
            if hello.proto != PROTO {
                bail!(
                    "the server speaks the console's protocol {}, and this fenji-server speaks \
                     {PROTO}: attach with the fenji-server of the server's version, {}",
                    hello.proto,
                    clean(&hello.version)
                );
            }
            Ok(Link {
                hello,
                reader,
                writer,
                buf,
            })
        }

        /// The server's next message; `None` once it has closed the console.
        async fn next(&mut self) -> io::Result<Option<ServerMsg>> {
            next(&mut self.reader, &mut self.buf).await
        }

        async fn send(&mut self, msg: &ConsoleMsg) -> io::Result<()> {
            send(&mut self.writer, msg).await
        }

        /// The link as a console's session: a task hands the console what the server sends, and
        /// another the server what the console sends.
        fn session(self) -> Session {
            let Link {
                hello,
                mut reader,
                mut writer,
                mut buf,
            } = self;
            let (to_console, from_server) = mpsc::channel(FROM_SERVER);
            let (to_server, mut from_console) = mpsc::channel(TO_SERVER);
            tokio::spawn(async move {
                while let Ok(Some(msg)) = next(&mut reader, &mut buf).await {
                    if to_console.send(msg).await.is_err() {
                        return;
                    }
                }
            });
            tokio::spawn(async move {
                while let Some(msg) = from_console.recv().await {
                    if send(&mut writer, &msg).await.is_err() {
                        return;
                    }
                }
            });
            Session {
                hello,
                from_server,
                to_server,
            }
        }
    }

    /// Why no console could attach at `socket`.
    fn unreachable(socket: &Path, e: io::Error) -> anyhow::Error {
        if let Some(why) = socket::too_long(socket) {
            return anyhow!("can't attach to the server: {why}");
        }
        let data = lock::shown(socket.parent().unwrap_or(Path::new(".")));
        match e.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => anyhow!(
                "no server is running with this data folder ({})",
                data.display()
            ),
            io::ErrorKind::PermissionDenied => anyhow!(
                "can't attach to the server with this data folder ({}): its console socket is \
                 another user's; attach as the user the server runs as",
                data.display()
            ),
            _ => anyhow::Error::new(e).context(format!(
                "can't attach to the server at {}",
                socket.display()
            )),
        }
    }

    /// The server's next message, a line that isn't one let go; `None` once the server has
    /// closed the console.
    async fn next(
        reader: &mut BufReader<OwnedReadHalf>,
        buf: &mut Vec<u8>,
    ) -> io::Result<Option<ServerMsg>> {
        loop {
            match socket::read_line(reader, buf).await? {
                None => return Ok(None),
                Some(Line::Text(line)) => {
                    if let Ok(msg) = wire::decode_server(&line) {
                        return Ok(Some(msg));
                    }
                }
                Some(Line::TooLong { ended }) => {
                    if !ended {
                        socket::rest_of_line(reader).await?;
                    }
                }
            }
        }
    }

    async fn send(writer: &mut OwnedWriteHalf, msg: &ConsoleMsg) -> io::Result<()> {
        writer
            .write_all(format!("{}\n", wire::encode(msg)).as_bytes())
            .await
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn an_attached_console_says_how_it_ended_and_fails_only_when_it_lost_the_server() {
            let (left, ok) = ending(&Ok(Ending::Detached));
            assert!(ok);
            assert_eq!(
                left.as_deref(),
                Some("Left the console. The server runs on.")
            );
            let (bye, ok) = ending(&Ok(Ending::ServerStopped("the server is stopping".into())));
            assert!(ok);
            assert_eq!(
                bye.as_deref(),
                Some("The server closed the console: the server is stopping.")
            );
            assert_eq!(ending(&Ok(Ending::InputClosed)), (None, true));
            assert!(!ending(&Ok(Ending::Lost)).1);
            assert!(!ending(&Err(io::Error::other("a panic ended the console"))).1);
        }

        #[tokio::test]
        async fn a_leave_signal_makes_the_ending_the_hosts_leaving() {
            // Task 11's review 1, Important 1: whatever the terminal did as the signal came.
            let (leave, left) = tokio::sync::watch::channel(false);
            let stopping = || Ending::ServerStopped("the server is stopping".into());
            // Without one, an ending is as it was.
            let closed = settle(Ok(Ending::InputClosed), left.clone()).await;
            assert_eq!(closed.unwrap(), Ending::InputClosed);
            // A terminal that hangs up fails the console's reads as its SIGHUP comes: an error
            // waits a moment for the signal.
            let hangup = tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
                leave.send_replace(true);
                leave
            });
            let failed = settle(Err(io::Error::other("Input/output error")), left.clone()).await;
            assert_eq!(failed.unwrap(), Ending::Detached);
            let _leave = hangup.await.unwrap();
            // Once left, any ending but the server's goodbye is the host's leaving.
            assert_eq!(
                settle(Ok(Ending::Lost), left.clone()).await.unwrap(),
                Ending::Detached
            );
            assert_eq!(settle(Ok(stopping()), left).await.unwrap(), stopping());
        }
    }
}
