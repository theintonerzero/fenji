//! The console service (M4 hosting spec §3.1, §3.3, §3.9; §8, HA1 and HA5): the one place the host
//! console's commands run, whichever front end the host typed them in, and what every console is
//! sent.
//! - [`ConsoleService::run_line`] runs a line. `tp`, `kick` and `announce` (`say`) run in the
//!   world, as an admin's do (`WorldIn::Console`); the rest run here, against the store, the
//!   online registry, maintenance, the backups and the stop.
//! - [`session`] and [`connect`] give a console its feed: the log's last lines, the server's
//!   status, then each line as it's logged, each change of the status and each answer, and a
//!   goodbye once the server has stopped.
//!
//! Every answer is cleaned of hidden characters: names, labels and reasons reach the host's
//! terminal.
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::{Arc, Weak};
use std::time::Duration;

use fenji_console::Session;
use fenji_console::text::clean;
use fenji_console::view::width;
use fenji_console::wire::{self, ConsoleMsg, Hello, ServerMsg, State};
use fenji_store::{Account, Store};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::MissedTickBehavior;

use crate::accounts::is_admin;
use crate::cli;
use crate::clock::Clock;
use crate::commands::{self, Command, CommandError, ConsoleCommand, MaintenanceArg};
use crate::config::Config;
use crate::console::log::{LogHub, LogItem, LogSubscription, QUEUE};
use crate::online::Online;
use crate::status::{DEFAULT_MESSAGE, Maintenance, Status};
use crate::world::{ConsoleReply, MAX_LINE_CHARS, WorldHandle, WorldIn};

/// How long a command waits for the world's answer: a tick runs it, and an invite's store write
/// takes a moment more.
pub const WORLD_WAIT: Duration = Duration::from_secs(10);

/// Why the server says goodbye to a console: it has stopped (M4 hosting plan, Task 11's value).
pub const STOPPING: &str = "the server is stopping";

/// How a maintenance announcement starts (M4 hosting spec §3.9).
const MAINTENANCE_SAYS: &str = "Maintenance: ";

/// What the world announces when maintenance ends.
const MAINTENANCE_OVER: &str = "Maintenance is over.";

/// The messages that may wait for a console beyond the log hub's own queue, which tells a console
/// that falls behind how many lines it lost.
const TO_CONSOLE: usize = 64;

/// The command lines a console may send ahead of their answers.
const TO_SERVER: usize = 16;

/// How long a console's goodbye waits for the answers to commands still running as the server
/// stopped.
const ANSWERS_WAIT: Duration = Duration::from_secs(2);

/// What a command line came to: whether it did what was asked, and what it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub ok: bool,
    pub lines: Vec<String>,
}

impl Answer {
    fn done(lines: impl IntoIterator<Item = String>) -> Answer {
        Answer {
            ok: true,
            lines: lines.into_iter().collect(),
        }
    }

    fn line(text: impl Into<String>) -> Answer {
        Answer::done([text.into()])
    }

    fn failed(text: impl Into<String>) -> Answer {
        Answer {
            ok: false,
            lines: vec![text.into()],
        }
    }

    /// Each line as the host's terminal may show it ([`clean`]).
    fn cleaned(mut self) -> Answer {
        for line in &mut self.lines {
            if let Cow::Owned(cleaned) = clean(line) {
                *line = cleaned;
            }
        }
        self
    }
}

/// What the service runs commands against: the server's parts, as `server::start` holds them.
pub(crate) struct Parts {
    pub cfg: Config,
    pub store: Store,
    pub status: Arc<Status>,
    pub online: Arc<Online>,
    pub world: WorldHandle,
    pub clock: Arc<dyn Clock>,
    /// The pin of the server's own key, under `tls = "self"`.
    pub pin: Option<String>,
    /// The ports the gate and the account API listen on.
    pub gate_port: u16,
    pub api_port: u16,
    /// How long a command waits for the world's answer: [`WORLD_WAIT`].
    pub world_wait: Duration,
}

/// The host console's commands and status, shared by every console (`Arc<ConsoleService>`).
pub struct ConsoleService {
    cfg: Config,
    store: Store,
    status: Arc<Status>,
    online: Arc<Online>,
    world: WorldHandle,
    clock: Arc<dyn Clock>,
    pin: Option<String>,
    gate_port: u16,
    api_port: u16,
    world_wait: Duration,
    /// `[access].admins`, lower-cased, as the gate compares them.
    admins: Vec<String>,
    /// The status consoles show, as it last changed.
    facts: watch::Sender<wire::Status>,
    /// True once the server is stopping, whatever stopped it: the console's `stop` sets it, and
    /// `RunningServer::serve_until` stops on it as on a signal.
    stopping: watch::Sender<bool>,
    /// True once the server has stopped: each console is told goodbye.
    stopped: watch::Sender<bool>,
}

impl ConsoleService {
    pub(crate) fn new(parts: Parts) -> Arc<ConsoleService> {
        let Parts {
            cfg,
            store,
            status,
            online,
            world,
            clock,
            pin,
            gate_port,
            api_port,
            world_wait,
        } = parts;
        let admins = cfg
            .access
            .admins
            .iter()
            .map(|a| a.to_ascii_lowercase())
            .collect();
        let facts = facts(&cfg, &status, &online, [gate_port, api_port], false);
        Arc::new(ConsoleService {
            cfg,
            store,
            status,
            online,
            world,
            clock,
            pin,
            gate_port,
            api_port,
            world_wait,
            admins,
            facts: watch::Sender::new(facts),
            stopping: watch::Sender::new(false),
            stopped: watch::Sender::new(false),
        })
    }

    /// The first message a console gets: the server's version, the protocol's, its name and the
    /// commands it takes.
    pub fn hello(&self) -> ServerMsg {
        ServerMsg::from(self.greeting())
    }

    fn greeting(&self) -> Hello {
        Hello {
            version: env!("CARGO_PKG_VERSION").into(),
            proto: wire::PROTO,
            server: self.cfg.server.name.clone(),
            commands: commands::console_help(),
        }
    }

    /// The server's status as consoles show it: refreshed every second and when maintenance
    /// changes, and sent only when it changed.
    pub fn status(&self) -> watch::Receiver<wire::Status> {
        self.facts.subscribe()
    }

    /// True once the server is stopping.
    pub fn stopping(&self) -> watch::Receiver<bool> {
        self.stopping.subscribe()
    }

    /// The server is stopping, for whatever reason: its status says so, and `stop` answers so.
    pub(crate) fn begin_stopping(&self) {
        self.stopping.send_replace(true);
        self.refresh();
    }

    /// The server has stopped: each console is told goodbye, after the lines logged as it stopped.
    pub(crate) fn end(&self) {
        self.stopped.send_replace(true);
    }

    /// Sends consoles the status, if it changed.
    pub(crate) fn refresh(&self) {
        let ports = [self.gate_port, self.api_port];
        let stopping = *self.stopping.borrow();
        let now = facts(&self.cfg, &self.status, &self.online, ports, stopping);
        self.facts.send_if_modified(|was| {
            if *was == now {
                return false;
            }
            *was = now.clone();
            true
        });
    }

    /// Runs one line the host typed, whichever console it came from, and says what it came to,
    /// cleaned of hidden characters. A blank line is no command, and is skipped. Every other line
    /// is logged with `by = "console"`: here, or by the world for the commands it runs.
    pub async fn run_line(&self, line: &str) -> Answer {
        let line = line.trim();
        if line.is_empty() {
            return Answer::done([]);
        }
        let parsed = commands::parse_console(line);
        let in_world = matches!(
            parsed,
            Ok(ConsoleCommand::Shared(
                Command::Tp { .. } | Command::Kick { .. } | Command::Announce { .. }
            ))
        );
        if !in_world {
            tracing::info!(by = "console", command = line, "an admin's command");
        }
        let answer = match parsed {
            Ok(command) => self.run(command).await.unwrap_or_else(|e| {
                let error = format!("{e:#}");
                tracing::error!(
                    by = "console",
                    command = line,
                    error,
                    "a console command failed"
                );
                Answer::failed(error)
            }),
            Err(e) => Answer::failed(e.to_string()),
        };
        if in_world && !answer.ok {
            let why = answer.lines.join(" ");
            tracing::warn!(
                by = "console",
                command = line,
                why,
                "an admin's command the world didn't answer"
            );
        }
        answer.cleaned()
    }

    async fn run(&self, command: ConsoleCommand) -> anyhow::Result<Answer> {
        use ConsoleCommand as C;
        let now = self.clock.now_ms();
        Ok(match command {
            // Made here rather than in the world, so that it gives a link for each of the
            // machine's addresses as `invite create` does (carry-forward 8).
            C::Shared(Command::Invite { label }) => self.invite(label.as_deref(), now).await?,
            C::Shared(order) => self.ask_world(order).await,
            C::Online => self.online(),
            C::Accounts => listed(
                cli::account_rows(&self.store).await?,
                "no accounts yet: type invite for a link to send a friend",
            ),
            C::Invites => listed(
                cli::invite_rows(&self.store, now).await?,
                "no invites yet: type invite to make one",
            ),
            C::Revoke { id } => {
                let (ok, line) = cli::revoke(&self.store, id, now).await?;
                Answer {
                    ok,
                    lines: vec![line],
                }
            }
            C::Maintenance(arg) => self.maintenance(arg, now).await?,
            C::Backup => {
                let dir = crate::backup::folder(&self.cfg.store.path);
                let keep = crate::backup::DEFAULT_KEEP;
                let made = crate::backup::run(&self.store, &dir, keep, now).await?;
                Answer::line(format!("backed up to {}", made.display()))
            }
            C::Stop => self.stop(),
            C::Help { command } => help(command.as_deref()),
            C::Motd(motd) => self.set_motd(motd).await?,
            C::MotdShow => {
                let motd = self.status.motd();
                Answer::line(if motd.is_empty() {
                    "no MOTD; motd <text> sets one".into()
                } else {
                    format!("the MOTD: {motd}")
                })
            }
            C::Op { account } => self.op(&account).await?,
            C::Deop { account } => self.deop(&account).await?,
            C::Ops => self.ops().await?,
            C::Ban { account, reason } => self.ban(&account, reason.as_deref(), now).await?,
            C::Pardon { account } => self.pardon(&account).await?,
            C::Bans => self.bans().await?,
        })
    }

    /// Runs `order` in the world, as an admin's, and gives its answer, done or refused as the
    /// world says; a world that has stopped, or doesn't answer within [`WORLD_WAIT`], is answered
    /// for (carry-forward 10).
    async fn ask_world(&self, order: Command) -> Answer {
        let (reply, answer) = oneshot::channel();
        if !self.world.send(WorldIn::Console { order, reply }) {
            return Answer::failed("the world has stopped");
        }
        match tokio::time::timeout(self.world_wait, answer).await {
            Ok(Ok(ConsoleReply { ok, text })) => Answer {
                ok,
                lines: vec![text],
            },
            Ok(Err(_)) => Answer::failed("the world stopped before it answered"),
            Err(_) => Answer::failed("the world didn't answer in time"),
        }
    }

    /// Announces `text` on the marquee: what the world said of it.
    async fn announce(&self, text: String) -> Vec<String> {
        self.ask_world(Command::Announce { text }).await.lines
    }

    /// A new invite with `label`, as `invite create` makes one: its links, one for each of the
    /// machine's addresses where no one address is set (Amendment A, HA4), and its code.
    async fn invite(&self, label: Option<&str>, now: i64) -> anyhow::Result<Answer> {
        let pin = self.pin.as_deref();
        let bases = crate::invite::bases(&self.cfg, pin, self.api_port);
        let made = crate::invite::create(&self.store, None, label, now, None).await?;
        let ports = [self.gate_port, self.api_port];
        let lines = cli::invite_lines(&self.cfg, pin, ports, &bases, &made.code, None);
        Ok(Answer::done(lines))
    }

    /// Who is connected: each account, the character it plays or `lobby`, the scene and since
    /// when. Never an address.
    fn online(&self) -> Answer {
        let list = self.online.list();
        if list.is_empty() {
            return Answer::line("nobody is online");
        }
        let mut rows = vec![cells(["account", "character", "scene", "since"])];
        for c in list {
            let (character, scene) = match c.playing {
                Some(p) => (p.character, p.scene.to_string()),
                None => ("lobby".into(), "-".into()),
            };
            rows.push(vec![c.username, character, scene, cli::time(c.since_ms)]);
        }
        Answer::done(table(&rows))
    }

    async fn maintenance(&self, arg: MaintenanceArg, now: i64) -> anyhow::Result<Answer> {
        match arg {
            MaintenanceArg::Show => Ok(Answer::done(self.maintenance_lines())),
            MaintenanceArg::On { lasts, message } => {
                let message = message.unwrap_or_else(|| DEFAULT_MESSAGE.into());
                // Turned on again, it keeps the time it began, and the time the host expects to be
                // back unless given a new duration, which runs from now.
                let was = self.status.maintenance();
                let since_ms = was.as_ref().map_or(now, |m| m.since_ms);
                let until_ms = match lasts {
                    Some(lasts) => Some(cli::ms_after(now, lasts)),
                    None => was.and_then(|m| m.until_ms),
                };
                let note = maintenance_note(&message);
                let m = Maintenance {
                    message,
                    since_ms,
                    until_ms,
                };
                self.status.set_maintenance(&self.store, Some(m)).await?;
                self.refresh();
                let mut lines = self.maintenance_lines();
                lines.extend(self.announce(note).await);
                Ok(Answer::done(lines))
            }
            MaintenanceArg::Off => {
                if self.status.maintenance().is_none() {
                    return Ok(Answer::line("maintenance was off already"));
                }
                self.status.set_maintenance(&self.store, None).await?;
                self.refresh();
                let mut lines = vec!["maintenance is off".to_owned()];
                lines.extend(self.announce(MAINTENANCE_OVER.into()).await);
                Ok(Answer::done(lines))
            }
        }
    }

    /// Maintenance as `maintenance` alone shows it: off, or on with its message and times.
    fn maintenance_lines(&self) -> Vec<String> {
        let Some(m) = self.status.maintenance() else {
            return vec!["maintenance is off".into()];
        };
        let mut lines = vec![
            format!("maintenance is on: {}", m.message),
            format!("since {}", cli::time(m.since_ms)),
        ];
        if let Some(until) = m.until_ms {
            lines.push(format!("expected back: {}", cli::time(until)));
        }
        lines
    }

    fn stop(&self) -> Answer {
        if self.stopping.send_replace(true) {
            return Answer::line("the server is stopping already");
        }
        self.refresh();
        Answer::line("stopping the server")
    }

    /// Sets the MOTD (`Some`) or gives the config's back (`None`), and says what's in effect
    /// afterwards (carry-forward 6).
    async fn set_motd(&self, motd: Option<String>) -> anyhow::Result<Answer> {
        let off = motd.is_none();
        self.status.set_motd(&self.store, motd).await?;
        let now = self.status.motd();
        Ok(Answer::line(match (now.is_empty(), off) {
            (true, _) => "no MOTD now".into(),
            (false, false) => format!("the MOTD is now: {now}"),
            (false, true) => format!("the MOTD is the config's again: {now}"),
        }))
    }

    /// The account called `name`, in any case, or the answer that there's none.
    async fn account(&self, name: &str) -> anyhow::Result<Result<Account, Answer>> {
        Ok(self
            .store
            .account_by_username(name)
            .await?
            .ok_or_else(|| Answer::failed(format!("no account {name}"))))
    }

    /// Tells the world whether `account` is an admin's now, as [`is_admin`] says after a change:
    /// its player in the world, if any, runs commands from the next tick, or no longer does. A
    /// world that has stopped has no player to tell.
    async fn tell_admin(&self, account: &Account) -> anyhow::Result<bool> {
        let admin = is_admin(&self.store, &self.admins, account).await?;
        let _ = self.world.send(WorldIn::SetAdmin {
            account: account.id,
            admin,
        });
        Ok(admin)
    }

    async fn op(&self, name: &str) -> anyhow::Result<Answer> {
        let account = match self.account(name).await? {
            Ok(account) => account,
            Err(none) => return Ok(none),
        };
        let made = self.store.op(account.id).await?;
        self.tell_admin(&account).await?;
        let u = &account.username;
        Ok(Answer::line(if made {
            format!("{u} is an op now")
        } else {
            format!("{u} is an op already")
        }))
    }

    /// Takes the account's op away. It didn't do what was asked if `[access].admins` names the
    /// account, which stays an admin's.
    async fn deop(&self, name: &str) -> anyhow::Result<Answer> {
        let account = match self.account(name).await? {
            Ok(account) => account,
            Err(none) => return Ok(none),
        };
        let was = self.store.deop(account.id).await?;
        let admin = self.tell_admin(&account).await?;
        let u = &account.username;
        Ok(match (was, admin) {
            (true, false) => Answer::line(format!("{u} is no longer an op")),
            (false, false) => Answer::line(format!("{u} isn't an op")),
            (true, true) => Answer::failed(format!(
                "{u} is no longer an op, but [access].admins names it, so it stays an admin"
            )),
            (false, true) => Answer::failed(format!(
                "{u} isn't an op; [access].admins names it, so it stays an admin"
            )),
        })
    }

    /// The admins: `[access].admins`' names and the store's ops, each marked with why it's one.
    async fn ops(&self) -> anyhow::Result<Answer> {
        // By name in any case: the name as the account has it, else as the config writes it,
        // whether the config names it, and whether it's an op.
        let mut admins: BTreeMap<String, (String, bool, bool)> = BTreeMap::new();
        for name in &self.cfg.access.admins {
            let admin = admins.entry(name.to_ascii_lowercase()).or_default();
            admin.0.clone_from(name);
            admin.1 = true;
        }
        for name in self.store.ops().await? {
            let admin = admins.entry(name.to_ascii_lowercase()).or_default();
            admin.0 = name;
            admin.2 = true;
        }
        if admins.is_empty() {
            return Ok(Answer::line("no admins: op <account> makes one"));
        }
        let rows: Vec<Vec<String>> = admins
            .into_values()
            .map(|(name, named, op)| {
                let why = match (named, op) {
                    (true, true) => "[access].admins, op",
                    (true, false) => "[access].admins",
                    (false, _) => "op",
                };
                vec![name, why.into()]
            })
            .collect();
        Ok(Answer::done(table(&rows)))
    }

    /// Bans the account with `reason`, none being an empty one (carry-forward 6), and lets it go
    /// at once wherever it's connected (carry-forwards 2 and 10).
    async fn ban(&self, name: &str, reason: Option<&str>, now: i64) -> anyhow::Result<Answer> {
        let account = match self.account(name).await? {
            Ok(account) => account,
            Err(none) => return Ok(none),
        };
        let new = self
            .store
            .ban(account.id, reason.unwrap_or(""), now)
            .await?;
        // The registry first, so that the connection closes with the ban's reason, and the
        // world's kick, which comes after, finds its link gone (carry-forward 12).
        let online = self.online.ban(account.id);
        let _ = self.world.send(WorldIn::Ban {
            account: account.id,
        });
        let u = &account.username;
        let mut lines = vec![if new {
            format!("banned {u}")
        } else {
            format!("{u} was banned already; the ban's reason is updated")
        }];
        if online {
            lines.push(format!("{u} was online, and was let go"));
        }
        Ok(Answer::done(lines))
    }

    async fn pardon(&self, name: &str) -> anyhow::Result<Answer> {
        let account = match self.account(name).await? {
            Ok(account) => account,
            Err(none) => return Ok(none),
        };
        let u = &account.username;
        Ok(Answer::line(if self.store.pardon(account.id).await? {
            format!("pardoned {u}")
        } else {
            format!("{u} isn't banned")
        }))
    }

    /// The bans: each account, the reason or `-`, and since when.
    async fn bans(&self) -> anyhow::Result<Answer> {
        let bans = self.store.bans().await?;
        if bans.is_empty() {
            return Ok(Answer::line("nobody is banned"));
        }
        let mut rows = vec![cells(["account", "reason", "since"])];
        for b in bans {
            let reason = if b.reason.is_empty() {
                "-".into()
            } else {
                b.reason
            };
            rows.push(vec![b.username, reason, cli::time(b.since)]);
        }
        Ok(Answer::done(table(&rows)))
    }
}

/// The status consoles show.
fn facts(
    cfg: &Config,
    status: &Status,
    online: &Online,
    [gate_port, api_port]: [u16; 2],
    stopping: bool,
) -> wire::Status {
    let maintenance = status.maintenance();
    let state = if stopping {
        State::Stopping
    } else if maintenance.is_some() {
        State::Maintenance
    } else {
        State::Online
    };
    wire::Status {
        state,
        server: cfg.server.name.clone(),
        version: env!("CARGO_PKG_VERSION").into(),
        started_ms: status.started_ms(),
        online: u32::try_from(online.count()).unwrap_or(u32::MAX),
        max: u32::try_from(online.max()).unwrap_or(u32::MAX),
        maintenance: maintenance.map(|m| m.message),
        gate_port,
        api_port,
    }
}

/// The marquee's line as maintenance begins: `Maintenance: <message>`, cut with an ellipsis to the
/// characters the marquee takes, rather than refused (carry-forward 9). The launcher shows the
/// message whole.
fn maintenance_note(message: &str) -> String {
    let room = MAX_LINE_CHARS - MAINTENANCE_SAYS.chars().count();
    if message.chars().count() <= room {
        return format!("{MAINTENANCE_SAYS}{message}");
    }
    let kept: String = message.chars().take(room - 1).collect();
    format!("{MAINTENANCE_SAYS}{}…", kept.trim_end())
}

/// `help`: each command's usage and what it does, or one command's (a name `console_help` has).
fn help(command: Option<&str>) -> Answer {
    let all = commands::console_help();
    let Some(name) = command else {
        let rows: Vec<Vec<String>> = all.into_iter().map(|h| vec![h.usage, h.about]).collect();
        return Answer::done(table(&rows));
    };
    match all.into_iter().find(|h| h.name == name) {
        Some(h) => Answer::done([format!("usage: {}", h.usage), h.about]),
        None => Answer::failed(CommandError::UnknownHere(name.into()).to_string()),
    }
}

/// A list's answer: its header and rows, or `empty` when it has no row.
fn listed(rows: Vec<Vec<String>>, empty: &str) -> Answer {
    if rows.len() <= 1 {
        Answer::line(empty)
    } else {
        Answer::done(table(&rows))
    }
}

fn cells<const N: usize>(cells: [&str; N]) -> Vec<String> {
    cells.map(String::from).to_vec()
}

/// `rows` as the console shows a list: each cell cleaned of hidden characters, and each column as
/// wide as its widest cell, two spaces from the next. The CLI's lists put tabs between cells,
/// which a console would show as escapes.
fn table(rows: &[Vec<String>]) -> Vec<String> {
    let rows: Vec<Vec<Cow<'_, str>>> = rows
        .iter()
        .map(|row| row.iter().map(|cell| clean(cell)).collect())
        .collect();
    let mut widths: Vec<usize> = Vec::new();
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            match widths.get_mut(i) {
                Some(w) => *w = (*w).max(width(cell)),
                None => widths.push(width(cell)),
            }
        }
    }
    rows.iter()
        .map(|row| {
            let mut line = String::new();
            for (i, cell) in row.iter().enumerate() {
                line.push_str(cell);
                if i + 1 < row.len() {
                    let pad = widths[i] - width(cell) + 2;
                    line.extend(std::iter::repeat_n(' ', pad));
                }
            }
            line
        })
        .collect()
}

/// A console in the server's process, as the full-screen console and plain lines take it: the
/// hello, and [`connect`]'s channels.
pub fn session(service: &Arc<ConsoleService>, hub: &LogHub) -> Session {
    let (from_server, to_server) = connect(service, hub);
    Session {
        hello: service.greeting(),
        from_server,
        to_server,
    }
}

/// A console's channels, however it's connected: what the server sends it, and where it sends its
/// command lines. It's sent the log's last lines and the status, then each line as it's logged
/// (or how many it lost by falling behind), each change of the status and each command's answer,
/// in order, then a goodbye once the server has stopped, and the channel closes. A console that
/// goes away leaves the server running.
pub fn connect(
    service: &Arc<ConsoleService>,
    hub: &LogHub,
) -> (mpsc::Receiver<ServerMsg>, mpsc::Sender<ConsoleMsg>) {
    let (to_console, from_server) = mpsc::channel(TO_CONSOLE);
    let (to_server, from_console) = mpsc::channel(TO_SERVER);
    let (answered, answers) = mpsc::channel(TO_SERVER);
    // Subscribed now, so that the backlog is the log as the console connects.
    let logs = hub.subscribe();
    tokio::spawn(forward(Arc::clone(service), logs, answers, to_console));
    tokio::spawn(answer(Arc::clone(service), from_console, answered));
    (from_server, to_server)
}

/// Sends a console the log's backlog and the status, then each line, status and answer, and a
/// goodbye: the one writer to the console, so that it gets them in the order they happen.
async fn forward(
    service: Arc<ConsoleService>,
    mut logs: LogSubscription,
    mut answers: mpsc::Receiver<ServerMsg>,
    out: mpsc::Sender<ServerMsg>,
) {
    for line in std::mem::take(&mut logs.backlog) {
        if out.send(ServerMsg::Log { line }).await.is_err() {
            return;
        }
    }
    let mut status = service.status();
    let now = status.borrow_and_update().clone();
    if out.send(ServerMsg::Status { status: now }).await.is_err() {
        return;
    }
    let mut stopped = service.stopped.subscribe();
    let (mut logging, mut answering) = (true, true);
    loop {
        // No branch waits on the console: a read of a watched value mustn't be held meanwhile.
        let next = tokio::select! {
            item = logs.next(), if logging => match item {
                Some(item) => Next::Send(log_msg(item)),
                None => Next::NoLog,
            },
            changed = status.changed() => match changed {
                Ok(()) => Next::Send(ServerMsg::Status { status: status.borrow_and_update().clone() }),
                Err(_) => Next::End,
            },
            answer = answers.recv(), if answering => match answer {
                Some(answer) => Next::Answer(answer),
                None => Next::NoAnswers,
            },
            seen = stopped.wait_for(|stopped| *stopped) => match seen {
                Ok(_) => Next::Stopped,
                Err(_) => Next::End,
            },
            () = out.closed() => Next::End,
        };
        let sent = match next {
            Next::Send(msg) => out.send(msg).await.is_ok(),
            // What its command logged and changed goes first.
            Next::Answer(answer) => {
                catch_up(&mut logs, &mut status, &out).await && out.send(answer).await.is_ok()
            }
            // The hub is gone with the process's log: the rest still comes.
            Next::NoLog => {
                logging = false;
                true
            }
            Next::NoAnswers => {
                answering = false;
                true
            }
            Next::Stopped => {
                // The answers still due, from a command the server's end caught running: the
                // command task ends once the server has stopped, with a store and a world that
                // answer at once. Then what was logged as it stopped, and goodbye.
                let until = tokio::time::Instant::now() + ANSWERS_WAIT;
                while answering {
                    match tokio::time::timeout_at(until, answers.recv()).await {
                        Ok(Some(answer)) => {
                            let sent = catch_up(&mut logs, &mut status, &out).await
                                && out.send(answer).await.is_ok();
                            if !sent {
                                return;
                            }
                        }
                        Ok(None) | Err(_) => answering = false,
                    }
                }
                if catch_up(&mut logs, &mut status, &out).await {
                    let bye = ServerMsg::Bye {
                        reason: STOPPING.into(),
                    };
                    let _ = out.send(bye).await;
                }
                return;
            }
            Next::End => return,
        };
        if !sent {
            return;
        }
    }
}

/// Sends the lines logged and the status changed that are waiting: what happened before what comes
/// next. False once the console has gone.
async fn catch_up(
    logs: &mut LogSubscription,
    status: &mut watch::Receiver<wire::Status>,
    out: &mpsc::Sender<ServerMsg>,
) -> bool {
    for _ in 0..QUEUE {
        let Some(item) = logs.try_next() else { break };
        if out.send(log_msg(item)).await.is_err() {
            return false;
        }
    }
    if status.has_changed().unwrap_or(false) {
        let now = status.borrow_and_update().clone();
        if out.send(ServerMsg::Status { status: now }).await.is_err() {
            return false;
        }
    }
    true
}

/// What a console's feed does next.
enum Next {
    Send(ServerMsg),
    /// An answer, after what its command logged and changed.
    Answer(ServerMsg),
    /// The log has ended.
    NoLog,
    /// The console sends no more commands.
    NoAnswers,
    /// The server has stopped: say goodbye.
    Stopped,
    /// The console has gone.
    End,
}

fn log_msg(item: LogItem) -> ServerMsg {
    match item {
        LogItem::Line(line) => ServerMsg::Log { line },
        LogItem::Skipped(lines) => ServerMsg::Skipped { lines },
    }
}

/// Runs a console's command lines in the order they came, and hands each answer to the console's
/// feed, until the console goes or the server has stopped: then nothing comes after the goodbye.
async fn answer(
    service: Arc<ConsoleService>,
    mut commands: mpsc::Receiver<ConsoleMsg>,
    out: mpsc::Sender<ServerMsg>,
) {
    let mut stopped = service.stopped.subscribe();
    loop {
        let command = tokio::select! {
            command = commands.recv() => command,
            seen = stopped.wait_for(|stopped| *stopped) => {
                drop(seen);
                None
            }
        };
        let Some(ConsoleMsg::Run { id, line }) = command else {
            return;
        };
        let Answer { ok, lines } = service.run_line(&line).await;
        if out.send(ServerMsg::Answer { id, ok, lines }).await.is_err() {
            return;
        }
    }
}

/// Keeps the status consoles see fresh: every second, for the players online, and at once when
/// maintenance changes. Ends with the service.
pub(crate) async fn keep_fresh(service: Weak<ConsoleService>) {
    let Some(mut maintenance) = service.upgrade().map(|s| s.status.watch_maintenance()) else {
        return;
    };
    let mut every = tokio::time::interval(Duration::from_secs(1));
    every.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = every.tick() => {}
            changed = maintenance.changed() => if changed.is_err() {
                return;
            },
        }
        let Some(service) = service.upgrade() else {
            return;
        };
        service.refresh();
    }
}

#[cfg(test)]
mod tests {
    use std::future;
    use std::path::Path;
    use std::sync::Arc;
    use std::time::Duration;

    use fenji_console::Session;
    use fenji_console::wire::{self, ConsoleMsg, Level, LogLine, Part, ServerMsg};
    use fenji_store::Store;
    use tokio::sync::mpsc;

    use super::*;
    use crate::clock::TestClock;
    use crate::commands::console_help;
    use crate::config::Config;
    use crate::console::log::LogHub;
    use crate::console::{Front, front};
    use crate::online::{Closing, Join, Online, Playing};
    use crate::server::{GateLimits, RunningServer, start};
    use crate::status::{DEFAULT_MESSAGE, Maintenance, Status};
    use crate::world::{ConsoleReply, WorldHandle, WorldIn, logged};

    const NOW: i64 = 1_800_000_000_000;

    fn config(dir: &Path, extra: &str) -> Config {
        Config::parse(
            &format!(
                "[server]\nname = \"Fenji\"\nlogin_bind = \"127.0.0.1:0\"\n\
                 api_bind = \"127.0.0.1:0\"\n{extra}"
            ),
            dir,
        )
        .unwrap()
    }

    /// A server on free loopback ports in a temp folder, its clock at [`NOW`], and the folder.
    async fn started() -> (RunningServer, tempfile::TempDir) {
        let (s, _clock, dir) = started_with("").await;
        (s, dir)
    }

    /// [`started`], with `extra` after `[server]`'s binds, and its clock.
    async fn started_with(extra: &str) -> (RunningServer, TestClock, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let clock = TestClock::at(NOW);
        let cfg = config(dir.path(), extra);
        let s = start(&cfg, Arc::new(clock.clone()), GateLimits::default())
            .await
            .unwrap();
        (s, clock, dir)
    }

    /// A service on a store in `dir` whose world is the test's: the receiver hears what it's sent.
    /// The world's answers are awaited for `world_wait`.
    async fn with_fake_world(
        dir: &Path,
        extra: &str,
        world_wait: Duration,
    ) -> (
        Arc<ConsoleService>,
        mpsc::UnboundedReceiver<WorldIn>,
        Store,
        Arc<Online>,
    ) {
        let cfg = config(dir, extra);
        let store = Store::open(&cfg.store.path).await.unwrap();
        let status = Status::load(&store, NOW, &cfg.server.motd).await.unwrap();
        let online = Arc::new(Online::new(20));
        let (world, heard) = WorldHandle::fake();
        let service = ConsoleService::new(Parts {
            cfg,
            store: store.clone(),
            status,
            online: Arc::clone(&online),
            world,
            clock: Arc::new(TestClock::at(NOW)),
            pin: None,
            gate_port: 7800,
            api_port: 7801,
            world_wait,
        });
        (service, heard, store, online)
    }

    /// A new account `name`, through invite `n`: its id.
    async fn account(store: &Store, n: u8, name: &str) -> i64 {
        store
            .create_invite(&[n; 32], None, NOW, None)
            .await
            .unwrap();
        store
            .redeem_invite(&[n; 32], name, "$phc", NOW)
            .await
            .unwrap()
    }

    fn done<const N: usize>(lines: [&str; N]) -> Answer {
        Answer {
            ok: true,
            lines: lines.map(String::from).to_vec(),
        }
    }

    fn failed<const N: usize>(lines: [&str; N]) -> Answer {
        Answer {
            ok: false,
            lines: lines.map(String::from).to_vec(),
        }
    }

    /// The account and the flag of the `SetAdmin` the world was sent next.
    fn set_admin(world: &mut mpsc::UnboundedReceiver<WorldIn>) -> (i64, bool) {
        match world.try_recv() {
            Ok(WorldIn::SetAdmin { account, admin }) => (account, admin),
            other => panic!("not a SetAdmin: {other:?}"),
        }
    }

    /// The account of the `Ban` the world was sent next.
    fn banned(world: &mut mpsc::UnboundedReceiver<WorldIn>) -> i64 {
        match world.try_recv() {
            Ok(WorldIn::Ban { account }) => account,
            other => panic!("not a Ban: {other:?}"),
        }
    }

    #[tokio::test]
    async fn the_console_runs_its_own_commands() {
        let (s, _dir) = started().await; // a server on port 0 in a temp folder
        let c = s.console();
        assert_eq!(c.run_line("online").await.lines, ["nobody is online"]);
        let made = c.run_line("invite for Lin").await;
        assert!(made.ok && made.lines[0].contains("invite"), "{made:?}");
        assert!(
            c.run_line("invites")
                .await
                .lines
                .iter()
                .any(|l| l.contains("for Lin"))
        );
        assert!(c.run_line("backup").await.lines[0].contains("backups"));
        let on = c.run_line("maintenance on 30m Updating").await;
        assert!(on.ok, "{on:?}");
        assert_eq!(s.status().maintenance().unwrap().message, "Updating");
        assert_eq!(c.status().borrow().state, wire::State::Maintenance);
        assert!(c.run_line("maintenance").await.lines[0].contains("Updating"));
        assert!(c.run_line("maintenance off").await.ok);
        assert!(s.status().maintenance().is_none());
        let unknown = c.run_line("dance").await;
        assert!(!unknown.ok && unknown.lines == ["no command dance; type help"]);
        s.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn stop_from_the_console_stops_the_server_cleanly() {
        let (s, _dir) = started().await;
        let c = s.console();
        let served = tokio::spawn(s.serve_until(std::future::pending()));
        assert!(c.run_line("stop").await.ok);
        let result = tokio::time::timeout(Duration::from_secs(30), served)
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(
            c.run_line("stop").await,
            done(["the server is stopping already"])
        );
    }

    #[test]
    fn the_full_screen_console_needs_two_terminals_and_a_real_term() {
        assert_eq!(
            front(true, true, Some("xterm-256color"), false),
            Front::Full
        );
        assert_eq!(front(true, true, Some("dumb"), false), Front::Plain);
        assert_eq!(front(false, true, Some("xterm"), false), Front::Plain);
        assert_eq!(front(true, false, Some("xterm"), false), Front::Plain);
        assert_eq!(front(true, true, Some("xterm"), true), Front::Plain);
        // Windows' consoles set no TERM.
        assert_eq!(front(true, true, None, false), Front::Full);
    }

    #[tokio::test]
    async fn blank_lines_are_skipped_whichever_front_end_sent_them() {
        // Carry-forward 7: a blank line would otherwise answer "no command ; type help".
        let (s, _dir) = started().await;
        for blank in ["", "   ", " \t "] {
            assert_eq!(s.console().run_line(blank).await, done([]), "{blank:?}");
        }
        s.shutdown().await.unwrap();
    }

    #[test]
    fn every_line_is_logged_as_the_consoles_once() {
        // Each line the host typed, with `by = "console"`; a blank one is no command. The shared
        // commands the world runs are logged there, as Task 9 has it, so once each too.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (s, _dir) = rt.block_on(started());
        let c = s.console();
        let ((), log) = logged(|| {
            rt.block_on(async {
                c.run_line("motd Hello").await;
                c.run_line("dance").await;
                c.run_line("  ").await;
            });
        });
        let runs: Vec<&str> = log
            .lines()
            .filter(|l| l.contains("by=\"console\""))
            .collect();
        assert_eq!(runs.len(), 2, "{log}");
        assert!(runs[0].contains("command=\"motd Hello\""), "{log}");
        assert!(runs[1].contains("command=\"dance\""), "{log}");
        rt.block_on(s.shutdown()).unwrap();
    }

    #[tokio::test]
    async fn help_lists_every_command_and_explains_one() {
        let (s, _dir) = started().await;
        let c = s.console();
        let all = c.run_line("help").await;
        let help = console_help();
        assert!(all.ok && all.lines.len() == help.len(), "{all:?}");
        for (line, h) in all.lines.iter().zip(&help) {
            assert!(
                line.starts_with(&h.usage) && line.ends_with(&h.about),
                "{line}"
            );
        }
        // What each does starts in one column.
        let column: Vec<usize> = all
            .lines
            .iter()
            .zip(&help)
            .map(|(line, h)| line.len() - h.about.len())
            .collect();
        assert!(column.windows(2).all(|w| w[0] == w[1]), "{all:?}");
        assert_eq!(
            c.run_line("help SAY").await,
            done(["usage: announce <text>", "message everyone (or say)"])
        );
        s.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn the_motd_is_set_shown_and_given_back_to_the_configs() {
        // Amendment A, HA1, and carry-forward 6: `motd off` says what's in effect afterwards.
        let (s, _clock, _dir) = started_with("motd = \"From the file\"\n").await;
        let c = s.console();
        assert_eq!(c.run_line("motd").await, done(["the MOTD: From the file"]));
        assert_eq!(
            c.run_line("motd Doors open at 8").await,
            done(["the MOTD is now: Doors open at 8"])
        );
        assert_eq!(s.status().motd(), "Doors open at 8");
        assert_eq!(
            c.run_line("motd off").await,
            done(["the MOTD is the config's again: From the file"])
        );
        assert_eq!(s.status().motd(), "From the file");
        s.shutdown().await.unwrap();
        let (s, _dir) = started().await;
        let c = s.console();
        assert_eq!(
            c.run_line("motd").await,
            done(["no MOTD; motd <text> sets one"])
        );
        assert_eq!(c.run_line("motd off").await, done(["no MOTD now"]));
        s.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn op_and_deop_change_the_store_and_tell_the_world_what_the_account_is_now() {
        // Amendment A, HA5, and carry-forward 10: `SetAdmin` carries `is_admin` after the change.
        let dir = tempfile::tempdir().unwrap();
        let admins = "[access]\nadmins = [\"BOSS\"]\n";
        let (c, mut world, store, _online) = with_fake_world(dir.path(), admins, WORLD_WAIT).await;
        let mai = account(&store, 1, "Mai").await;
        let boss = account(&store, 2, "boss").await;
        assert_eq!(c.run_line("op mai").await, done(["Mai is an op now"]));
        assert_eq!(set_admin(&mut world), (mai, true));
        assert_eq!(c.run_line("op Mai").await, done(["Mai is an op already"]));
        assert_eq!(set_admin(&mut world), (mai, true));
        assert_eq!(
            c.run_line("ops").await,
            done(["BOSS  [access].admins", "Mai   op"])
        );
        assert_eq!(
            c.run_line("deop MAI").await,
            done(["Mai is no longer an op"])
        );
        assert_eq!(set_admin(&mut world), (mai, false));
        assert_eq!(c.run_line("deop Mai").await, done(["Mai isn't an op"]));
        assert_eq!(set_admin(&mut world), (mai, false));
        // A name `[access].admins` gives stays an admin's whatever the store says.
        assert_eq!(c.run_line("op boss").await, done(["boss is an op now"]));
        assert_eq!(set_admin(&mut world), (boss, true));
        assert_eq!(c.run_line("ops").await, done(["boss  [access].admins, op"]));
        assert_eq!(
            c.run_line("deop boss").await,
            failed(["boss is no longer an op, but [access].admins names it, so it stays an admin"])
        );
        assert_eq!(set_admin(&mut world), (boss, true));
        assert_eq!(
            c.run_line("deop boss").await,
            failed(["boss isn't an op; [access].admins names it, so it stays an admin"])
        );
        assert_eq!(set_admin(&mut world), (boss, true));
        // An account that isn't there.
        assert_eq!(c.run_line("op Nobody").await, failed(["no account Nobody"]));
        assert_eq!(
            c.run_line("deop Nobody").await,
            failed(["no account Nobody"])
        );
        assert!(world.try_recv().is_err(), "nothing more for the world");
    }

    #[tokio::test]
    async fn with_no_admins_ops_says_how_to_make_one() {
        let dir = tempfile::tempdir().unwrap();
        let (c, _world, _store, _online) = with_fake_world(dir.path(), "", WORLD_WAIT).await;
        assert_eq!(
            c.run_line("ops").await,
            done(["no admins: op <account> makes one"])
        );
    }

    #[tokio::test]
    async fn ban_lets_the_account_go_and_tells_the_world_and_pardon_lifts_it() {
        // Amendment A, HA5, and carry-forwards 2, 6, 10 and 12.
        let dir = tempfile::tempdir().unwrap();
        let (c, mut world, store, online) = with_fake_world(dir.path(), "", WORLD_WAIT).await;
        let mai = account(&store, 1, "Mai").await;
        let ren = account(&store, 2, "ren").await;
        let Join::Joined(_, mut closing) = online.join(mai, "Mai", NOW) else {
            panic!("Mai is online")
        };
        assert_eq!(
            c.run_line("ban mai").await,
            done(["banned Mai", "Mai was online, and was let go"])
        );
        assert_eq!(closing.try_recv(), Ok(Closing::Banned));
        assert_eq!(online.count(), 0);
        assert_eq!(banned(&mut world), mai);
        let ban = store.ban_of(mai).await.unwrap().unwrap();
        assert_eq!(
            (ban.reason.as_str(), ban.since),
            ("", NOW),
            "no reason is an empty one"
        );
        assert_eq!(
            c.run_line("ban ren Spamming the marquee").await,
            done(["banned ren"])
        );
        assert_eq!(banned(&mut world), ren);
        assert_eq!(
            c.run_line("ban Mai Griefing").await,
            done(["Mai was banned already; the ban's reason is updated"])
        );
        assert_eq!(banned(&mut world), mai);
        let since = "2027-01-15 08:00 UTC";
        assert_eq!(
            c.run_line("bans").await.lines,
            [
                format!("{:<9}{:<22}since", "account", "reason"),
                format!("{:<9}{:<22}{since}", "Mai", "Griefing"),
                format!("{:<9}{:<22}{since}", "ren", "Spamming the marquee"),
            ]
        );
        assert_eq!(c.run_line("pardon MAI").await, done(["pardoned Mai"]));
        assert_eq!(c.run_line("pardon mai").await, done(["Mai isn't banned"]));
        assert_eq!(c.run_line("pardon ren").await, done(["pardoned ren"]));
        assert_eq!(c.run_line("bans").await, done(["nobody is banned"]));
        assert_eq!(
            c.run_line("ban Nobody").await,
            failed(["no account Nobody"])
        );
        assert_eq!(
            c.run_line("pardon Nobody").await,
            failed(["no account Nobody"])
        );
        assert!(world.try_recv().is_err(), "nothing more for the world");
    }

    #[tokio::test]
    async fn online_names_each_account_with_its_character_or_the_lobby() {
        // Carry-forward 10: the account, the character or "lobby", the scene and since when.
        let dir = tempfile::tempdir().unwrap();
        let (c, _world, _store, online) = with_fake_world(dir.path(), "", WORLD_WAIT).await;
        assert_eq!(c.run_line("online").await, done(["nobody is online"]));
        let Join::Joined(lin, _mai) = online.join(2, "mai", NOW + 60_000) else {
            panic!("mai is online")
        };
        let Join::Joined(_, _bob) = online.join(1, "Bob", NOW) else {
            panic!("Bob is online")
        };
        online.playing(
            2,
            lin,
            Playing {
                character: "Lin".into(),
                scene: 5,
            },
        );
        let listed = done([
            "account  character  scene  since",
            "Bob      lobby      -      2027-01-15 08:00 UTC",
            "mai      Lin        5      2027-01-15 08:01 UTC",
        ]);
        assert_eq!(c.run_line("online").await, listed);
        assert_eq!(c.run_line("list").await, listed);
    }

    #[tokio::test]
    async fn invite_gives_a_link_for_each_address_on_the_first_runs_config() {
        // Carry-forward 8: the template listens everywhere with the server's own key and names
        // no address, where the world's `!invite` has no one link to give; the console's gives
        // one for each of the machine's addresses, as `invite create` does, and always this
        // computer's.
        let dir = tempfile::tempdir().unwrap();
        let template = "tls = \"self\"\n";
        let cfg = Config::parse(
            &format!(
                "[server]\nname = \"Fenji\"\nlogin_bind = \"0.0.0.0:7800\"\n\
                 api_bind = \"0.0.0.0:7801\"\n{template}"
            ),
            dir.path(),
        )
        .unwrap();
        assert!(cfg.invite_base(Some(PIN)).is_none(), "the world's has none");
        let store = Store::open(&cfg.store.path).await.unwrap();
        let status = Status::load(&store, NOW, "").await.unwrap();
        let (world, _heard) = WorldHandle::fake();
        let c = ConsoleService::new(Parts {
            cfg,
            store: store.clone(),
            status,
            online: Arc::new(Online::new(20)),
            world,
            clock: Arc::new(TestClock::at(NOW)),
            pin: Some(PIN.into()),
            gate_port: 7800,
            api_port: 7801,
            world_wait: WORLD_WAIT,
        });
        let made = c.run_line("invite").await;
        assert!(made.ok, "{made:?}");
        let mine = made
            .lines
            .iter()
            .find(|l| l.starts_with("link (this computer): https://127.0.0.1:7801/#invite="))
            .unwrap_or_else(|| panic!("{made:?}"));
        assert!(mine.ends_with(&format!("&pin={PIN}")), "{mine}");
        assert!(
            made.lines.iter().any(|l| l.starts_with("code: ")),
            "{made:?}"
        );
        assert_eq!(store.list_invites().await.unwrap().len(), 1);
    }

    const PIN: &str = "q1o9C1M6ufA5UGrUkSYhW-AXXa3S7jdJ9Fxlp0f2cBo";

    #[tokio::test]
    async fn accounts_invites_and_revoke_print_as_the_subcommands_do() {
        let dir = tempfile::tempdir().unwrap();
        let (c, _world, store, _online) = with_fake_world(dir.path(), "", WORLD_WAIT).await;
        assert_eq!(
            c.run_line("accounts").await,
            done(["no accounts yet: type invite for a link to send a friend"])
        );
        assert_eq!(
            c.run_line("invites").await,
            done(["no invites yet: type invite to make one"])
        );
        account(&store, 1, "Mai").await;
        let accounts = c.run_line("accounts").await;
        assert_eq!(
            accounts.lines,
            [
                "id  username  created               last sign-in  characters",
                "1   Mai       2027-01-15 08:00 UTC  -             0",
            ]
        );
        store
            .create_invite(&[2; 32], Some("for Ren"), NOW, None)
            .await
            .unwrap();
        let invites = c.run_line("invites").await;
        assert_eq!(
            invites.lines,
            [
                "id  label    created               status",
                "1   -        2027-01-15 08:00 UTC  redeemed by Mai",
                "2   for Ren  2027-01-15 08:00 UTC  open",
            ]
        );
        assert_eq!(c.run_line("revoke 2").await, done(["revoked invite 2"]));
        assert_eq!(
            c.run_line("revoke 2").await,
            failed(["nothing to revoke: invite 2 is unknown, used or revoked"])
        );
    }

    #[tokio::test]
    async fn maintenance_without_a_message_says_back_soon_and_a_long_duration_saturates() {
        // Carry-forward 6: `On { message: None }` is the default message, and `until` is
        // computed with saturation, as `--expires` is.
        let (s, clock, _dir) = started_with("").await;
        let c = s.console();
        assert_eq!(
            c.run_line("maintenance").await,
            done(["maintenance is off"])
        );
        assert_eq!(
            c.run_line("maintenance on").await,
            done([
                "maintenance is on: Back soon",
                "since 2027-01-15 08:00 UTC",
                "announced, but no one is in the game",
            ])
        );
        assert_eq!(
            s.status().maintenance(),
            Some(Maintenance {
                message: DEFAULT_MESSAGE.into(),
                since_ms: NOW,
                until_ms: None,
            })
        );
        // Turned on again, with a new message and an end as far off as a duration goes: still
        // since it began.
        clock.advance(60_000);
        let longest = c
            .run_line("maintenance on 18446744073709551615m Moving house")
            .await;
        assert!(longest.ok, "{longest:?}");
        assert_eq!(
            s.status().maintenance(),
            Some(Maintenance {
                message: "Moving house".into(),
                since_ms: NOW,
                until_ms: Some(i64::MAX),
            })
        );
        assert_eq!(
            c.run_line("maintenance off").await,
            done(["maintenance is off", "announced, but no one is in the game"])
        );
        assert_eq!(
            c.run_line("maintenance off").await,
            done(["maintenance was off already"])
        );
        s.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn maintenance_turned_on_again_keeps_its_end_unless_given_another() {
        // Review 1, Minor 4: turned on again with a new message, it keeps when it began and when
        // the host expects to be back; a new duration is a new end, from now.
        let (s, clock, _dir) = started_with("").await;
        let c = s.console();
        assert!(c.run_line("maintenance on 30m Updating").await.ok);
        clock.advance(60_000);
        assert!(c.run_line("maintenance on Still updating").await.ok);
        assert_eq!(
            s.status().maintenance(),
            Some(Maintenance {
                message: "Still updating".into(),
                since_ms: NOW,
                until_ms: Some(NOW + 1_800_000),
            })
        );
        assert!(c.run_line("maintenance on 1h Nearly done").await.ok);
        assert_eq!(
            s.status().maintenance(),
            Some(Maintenance {
                message: "Nearly done".into(),
                since_ms: NOW,
                until_ms: Some(NOW + 60_000 + 3_600_000),
            })
        );
        s.shutdown().await.unwrap();
    }

    #[test]
    fn the_maintenance_announcement_is_cut_to_fit_the_marquee() {
        // Carry-forward 9: the marquee takes 140 characters, `Maintenance: ` and the message
        // 213, so a long message is cut, the launcher showing it whole.
        let fits = "é".repeat(127);
        assert_eq!(maintenance_note(&fits), format!("Maintenance: {fits}"));
        let cut = maintenance_note(&"é".repeat(200));
        assert_eq!(cut.chars().count(), 140);
        assert!(
            cut.starts_with("Maintenance: éé") && cut.ends_with("é…"),
            "{cut}"
        );
        // No space before the ellipsis.
        let spaced = format!("{} {}", "a".repeat(125), "b".repeat(50));
        assert_eq!(
            maintenance_note(&spaced),
            format!("Maintenance: {}…", "a".repeat(125))
        );
    }

    #[tokio::test]
    async fn a_long_maintenance_message_is_announced_cut_rather_than_refused() {
        let (s, _dir) = started().await;
        let message = "x".repeat(200);
        let on = s
            .console()
            .run_line(&format!("maintenance on {message}"))
            .await;
        assert!(on.ok, "{on:?}");
        assert_eq!(
            on.lines.last().map(String::as_str),
            Some("announced, but no one is in the game")
        );
        assert_eq!(s.status().maintenance().unwrap().message, message);
        s.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn what_the_world_refused_is_answered_as_not_done() {
        // Review 1, Minor 2: `ok` says whether the command did what was asked, so that the
        // attached console's `--run` exits 1 for "kick Nobody" (M4 hosting spec §3.1).
        let (s, _dir) = started().await;
        let c = s.console();
        assert_eq!(
            c.run_line("kick Nobody").await,
            failed(["no one called Nobody is in the game"])
        );
        assert_eq!(
            c.run_line("tp Mai Lin").await,
            failed(["no one called Mai is in the game"])
        );
        assert_eq!(
            c.run_line(&format!("say {}", "a".repeat(141))).await,
            failed(["not announced: over 140 characters"])
        );
        assert_eq!(
            c.run_line("say Doors open").await,
            done(["announced, but no one is in the game"])
        );
        s.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn the_worlds_answers_come_back_cleaned() {
        // Carry-forward 13: a character's name in the world's answer can't act on the host's
        // terminal.
        let dir = tempfile::tempdir().unwrap();
        let (c, mut world, _store, _online) = with_fake_world(dir.path(), "", WORLD_WAIT).await;
        let answering = tokio::spawn(async move {
            let Some(WorldIn::Console { reply, .. }) = world.recv().await else {
                panic!("a command for the world")
            };
            let text = "Mai\u{1b}[2J was kicked".into();
            reply.send(ConsoleReply { ok: true, text }).unwrap();
        });
        assert_eq!(
            c.run_line("kick Mai").await,
            done([r"Mai\u{1b}[2J was kicked"])
        );
        answering.await.unwrap();
    }

    #[tokio::test]
    async fn a_world_that_doesnt_answer_is_answered_for() {
        // Carry-forward 10: a reply channel dropped without an answer, a world that has stopped,
        // and one that takes too long.
        let dir = tempfile::tempdir().unwrap();
        let wait = Duration::from_millis(200);
        let (c, mut world, _store, _online) = with_fake_world(dir.path(), "", wait).await;
        let dropping = tokio::spawn(async move {
            // The first is dropped unanswered, the second held past the wait.
            let first = world.recv().await;
            drop(first);
            let second = world.recv().await;
            tokio::time::sleep(Duration::from_secs(2)).await;
            drop(second);
        });
        assert_eq!(
            c.run_line("kick Mai").await,
            failed(["the world stopped before it answered"])
        );
        let began = std::time::Instant::now();
        assert_eq!(
            c.run_line("tp Mai Lin").await,
            failed(["the world didn't answer in time"])
        );
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "{:?}",
            began.elapsed()
        );
        dropping.await.unwrap();
        assert_eq!(
            c.run_line("say hello").await,
            failed(["the world has stopped"])
        );
    }

    #[tokio::test]
    async fn the_status_follows_who_is_online_and_is_sent_only_when_it_changes() {
        let (s, _dir) = started().await;
        let c = s.console();
        let mut status = c.status();
        let first = status.borrow_and_update().clone();
        assert_eq!(
            (first.state, first.server.as_str(), first.online, first.max),
            (wire::State::Online, "Fenji", 0, 20)
        );
        assert_eq!(first.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(first.started_ms, NOW);
        assert_eq!(
            (first.gate_port, first.api_port),
            (s.login_addr.port(), s.api_addr.port())
        );
        let Join::Joined(..) = s.api_state().online.join(1, "mai", NOW) else {
            panic!("mai is online")
        };
        tokio::time::timeout(Duration::from_secs(5), status.changed())
            .await
            .expect("within a second or so")
            .unwrap();
        assert_eq!(status.borrow_and_update().online, 1);
        tokio::time::sleep(Duration::from_millis(1_500)).await;
        assert!(!status.has_changed().unwrap(), "nothing changed");
        s.shutdown().await.unwrap();
    }

    fn log_line(text: &str) -> LogLine {
        LogLine {
            at_ms: NOW,
            level: Level::Info,
            parts: vec![Part {
                text: text.into(),
                name: false,
            }],
        }
    }

    async fn next(from_server: &mut mpsc::Receiver<ServerMsg>) -> ServerMsg {
        tokio::time::timeout(Duration::from_secs(30), from_server.recv())
            .await
            .expect("within 30 s")
            .expect("the session is open")
    }

    #[tokio::test]
    async fn the_in_process_console_gets_the_log_the_status_and_its_answers_then_a_goodbye() {
        let (s, _dir) = started().await;
        let hub = LogHub::new(10);
        hub.publish(log_line("before the console"));
        let Session {
            hello,
            mut from_server,
            to_server,
        } = session(&s.console(), &hub);
        assert_eq!((hello.server.as_str(), hello.proto), ("Fenji", wire::PROTO));
        assert_eq!(hello.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(hello.commands, console_help());
        assert_eq!(
            next(&mut from_server).await,
            ServerMsg::Log {
                line: log_line("before the console")
            }
        );
        let ServerMsg::Status { status } = next(&mut from_server).await else {
            panic!("the status, after the backlog")
        };
        assert_eq!(status.state, wire::State::Online);
        hub.publish(log_line("while it's open"));
        assert_eq!(
            next(&mut from_server).await,
            ServerMsg::Log {
                line: log_line("while it's open")
            }
        );
        let run = |id, line: &str| ConsoleMsg::Run {
            id,
            line: line.into(),
        };
        to_server.send(run(7, "online")).await.unwrap();
        assert_eq!(
            next(&mut from_server).await,
            ServerMsg::Answer {
                id: 7,
                ok: true,
                lines: vec!["nobody is online".into()]
            }
        );
        let served = tokio::spawn(s.serve_until(future::pending()));
        to_server.send(run(8, "stop")).await.unwrap();
        // Its answer and the status saying so, then a goodbye once the server has stopped.
        let mut seen = Vec::new();
        let reason = loop {
            match next(&mut from_server).await {
                ServerMsg::Bye { reason } => break reason,
                msg => seen.push(msg),
            }
        };
        assert_eq!(reason, "the server is stopping");
        let stopping = ServerMsg::Answer {
            id: 8,
            ok: true,
            lines: vec!["stopping the server".into()],
        };
        assert!(seen.contains(&stopping), "{seen:?}");
        assert!(
            seen.iter().any(|m| matches!(
                m,
                ServerMsg::Status { status } if status.state == wire::State::Stopping
            )),
            "{seen:?}"
        );
        served.await.unwrap().unwrap();
        let after = tokio::time::timeout(Duration::from_secs(30), from_server.recv()).await;
        assert_eq!(after, Ok(None), "nothing after goodbye: the channel closes");
    }

    #[test]
    fn what_a_command_logs_and_changes_reaches_the_console_before_its_answer() {
        // In the order they happen (M4 hosting plan, Task 11's interface): `stop`, with nothing
        // serving to act on it, logs its line and changes the status at once, then answers.
        use tracing_subscriber::layer::SubscriberExt;
        // While one subscriber alone is registered, tracing-core asks only the subscriber of the
        // thread that first logs at a place whether it's wanted (`Rebuilder::JustOne`), and keeps
        // the answer: another test's thread, with none, could then make this one's lines unwanted
        // everywhere. With a second registered, every subscriber is asked.
        let _asked = tracing::Dispatch::new(tracing_subscriber::registry());
        let hub = LogHub::new(500);
        let subscriber = tracing_subscriber::registry().with(hub.layer());
        let _log = tracing::subscriber::set_default(subscriber);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let (s, _dir) = started().await;
            let Session {
                mut from_server,
                to_server,
                ..
            } = session(&s.console(), &hub);
            while !matches!(next(&mut from_server).await, ServerMsg::Status { .. }) {}
            to_server
                .send(ConsoleMsg::Run {
                    id: 1,
                    line: "stop".into(),
                })
                .await
                .unwrap();
            let mut seen = Vec::new();
            while !matches!(seen.last(), Some(ServerMsg::Answer { .. })) {
                seen.push(next(&mut from_server).await);
            }
            let logged = seen.iter().position(
                |m| matches!(m, ServerMsg::Log { line } if line.message().contains("command=stop")),
            );
            let stopping = seen.iter().position(|m| {
                matches!(m, ServerMsg::Status { status } if status.state == wire::State::Stopping)
            });
            assert!(
                logged.is_some() && stopping.is_some(),
                "before the answer: {seen:?}"
            );
            s.shutdown().await.unwrap();
        });
    }

    #[tokio::test]
    async fn an_answer_due_as_the_server_stops_comes_before_the_goodbye() {
        // Review 1, Minor 3: a command still running when the server has stopped is answered,
        // and then the console is told goodbye.
        let dir = tempfile::tempdir().unwrap();
        let (c, mut world, _store, _online) = with_fake_world(dir.path(), "", WORLD_WAIT).await;
        let hub = LogHub::new(10);
        let Session {
            mut from_server,
            to_server,
            ..
        } = session(&c, &hub);
        while !matches!(next(&mut from_server).await, ServerMsg::Status { .. }) {}
        let kick = ConsoleMsg::Run {
            id: 1,
            line: "kick Mai".into(),
        };
        to_server.send(kick).await.unwrap();
        let Some(WorldIn::Console { reply, .. }) = world.recv().await else {
            panic!("the kick, for the world")
        };
        // The server stops while the world works on it, and the feed sees it first.
        c.end();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let text = "Mai was kicked".into();
        reply.send(ConsoleReply { ok: true, text }).unwrap();
        let mut seen = Vec::new();
        let reason = loop {
            match next(&mut from_server).await {
                ServerMsg::Bye { reason } => break reason,
                msg => seen.push(msg),
            }
        };
        assert_eq!(reason, STOPPING);
        let kicked = ServerMsg::Answer {
            id: 1,
            ok: true,
            lines: vec!["Mai was kicked".into()],
        };
        assert!(seen.contains(&kicked), "before the goodbye: {seen:?}");
    }

    #[tokio::test]
    async fn a_console_that_goes_away_leaves_the_server_running() {
        let (s, _dir) = started().await;
        let hub = LogHub::new(10);
        let Session {
            from_server,
            to_server,
            ..
        } = session(&s.console(), &hub);
        drop((from_server, to_server));
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!*s.console().stopping().borrow());
        assert_eq!(
            s.console().run_line("online").await,
            done(["nobody is online"])
        );
        s.shutdown().await.unwrap();
    }
}
