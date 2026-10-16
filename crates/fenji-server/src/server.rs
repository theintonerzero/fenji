//! Running the server (M3 spec D5; M4 world spec §3.9): one process with the game gate, the world,
//! the account API, and the purge of expired sessions and tokens at start-up and every hour. Its
//! changing facts, maintenance mode among them, are its [`Status`] (M4 hosting spec §3.8, §3.9).
//! With `tls = "self"` the account API speaks TLS with the server's own key (§3.10). The host
//! console's commands run in its [`ConsoleService`], and `fenji-server run` gives the terminal it
//! started in the full-screen console, or plain lines (§3.1).
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use fenji_console::text::clean;
use fenji_console::{Colours, Ending, Mode};
use fenji_store::Store;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::{JoinHandle, JoinSet};

use crate::accounts::Accounts;
use crate::api::{Addrs, AppState, ServerInfo, router};
use crate::clock::{Clock, SystemClock};
use crate::config::{self, Config, Loaded, Tls};
use crate::console::log::LogHub;
use crate::console::service::{self, ConsoleService, Parts, WORLD_WAIT};
use crate::console::{self, Front};
pub use crate::gate::GateLimits;
use crate::gate::{self, Gate, Places, accept_loop};
use crate::listen::{ConnLimits, PlainListener};
use crate::lobby::LobbyLimits;
use crate::online::Online;
use crate::status::Status;
use crate::tls::{self, ServerKey, TlsLimits, TlsListener};
use crate::world::{self, Rules, RunningWorld, TickReport, WorldHandle, WorldIn};

/// The pinned client build this server speaks to (docs/re/pinned-build.md).
pub const PINNED_BUILD: &str = "10015727";

/// A started server: its bound addresses, and the tasks `shutdown` ends.
pub struct RunningServer {
    pub login_addr: SocketAddr,
    pub api_addr: SocketAddr,
    /// The pin of the server's own key, under `tls = "self"`.
    pin: Option<String>,
    accounts: Arc<Accounts>,
    status: Arc<Status>,
    /// The account API's state, as its router serves it.
    api: AppState,
    world: RunningWorld,
    gate: Arc<Gate>,
    /// The host console's commands and status.
    console: Arc<ConsoleService>,
    /// The data folder: the database's directory.
    data: PathBuf,
    /// The gate's listener, which hands back the connections still open when it stops.
    listener: JoinHandle<JoinSet<()>>,
    /// The account API, the purge and the console's status.
    tasks: Vec<JoinHandle<()>>,
}

impl RunningServer {
    pub fn accounts(&self) -> Arc<Accounts> {
        Arc::clone(&self.accounts)
    }

    /// The host console's commands and status, which every console shares.
    pub fn console(&self) -> Arc<ConsoleService> {
        Arc::clone(&self.console)
    }

    /// The data folder, where the database, its backups and the console socket are.
    pub fn data(&self) -> PathBuf {
        self.data.clone()
    }

    /// The pin of the key the account API serves TLS with, under `tls = "self"`, which invite and
    /// server links carry (M4 hosting spec §3.10).
    pub fn pin(&self) -> Option<String> {
        self.pin.clone()
    }

    /// Maintenance, the MOTD and who is in the world.
    pub fn status(&self) -> Arc<Status> {
        Arc::clone(&self.status)
    }

    /// The state the account API serves, for a router over it in process (`api::router`).
    pub fn api_state(&self) -> AppState {
        self.api.clone()
    }

    /// The world's channel, as a connection task holds it.
    pub fn world(&self) -> WorldHandle {
        self.world.handle()
    }

    /// The world's tick times over the last minute.
    pub fn ticks(&self) -> TickReport {
        self.world.report()
    }

    /// Serves until `stop` completes or the console's `stop` asks it to, or until the world fails
    /// (`RunningWorld::failed`): its thread or its services task ends by itself, or its tick hangs.
    /// Then it shuts down: what `fenji-server run` awaits. A world that fails takes the server with
    /// it, so that no login waits for a world that isn't there (Ruling R9) or doesn't answer
    /// (#281), or plays in one that can't save (Ruling R13), and the error makes `fenji-server run`
    /// exit non-zero for systemd to restart it.
    pub async fn serve_until(mut self, stop: impl Future<Output = ()>) -> anyhow::Result<()> {
        let mut stopping = self.console.stopping();
        let failed = tokio::select! {
            () = stop => None,
            seen = stopping.wait_for(|stopping| *stopping) => {
                // Let go of the value at once: the shutdown sets it again.
                drop(seen);
                tracing::info!("shutting down, as the console asked");
                None
            }
            failure = self.world.failed() => {
                tracing::error!("{failure}: stopping the server");
                Some(failure)
            }
        };
        match (self.shutdown().await, failed) {
            (Ok(()), Some(failure)) => Err(anyhow::anyhow!("the server stopped: {failure}")),
            (stopped, _) => stopped,
        }
    }

    /// Stops in an order that lets players in the world read why (#219): the API and the purge
    /// end, the gate stops accepting and lets go of the lobby, the world lets go of every player
    /// with the shutdown reason and saves them, and their connections get the gate's grace to
    /// write their closes before what is left is ended. Then the world stops once its last saves
    /// are written, and the store closes. An error is the world's (`RunningWorld::stop`): its
    /// thread failed, or saves were lost. Consoles see the server stopping throughout, and are told
    /// goodbye at the end.
    pub async fn shutdown(self) -> anyhow::Result<()> {
        self.console.begin_stopping();
        for t in &self.tasks {
            t.abort();
        }
        for t in self.tasks {
            let _ = t.await;
        }
        self.gate.stopping.send_replace(true);
        let conns = match self.listener.await {
            Ok(conns) => conns,
            Err(e) => {
                tracing::error!(error = %e, "the gate's listener failed");
                JoinSet::new()
            }
        };
        self.world.handle().send(WorldIn::Shutdown);
        gate::finish(conns, self.gate.limits.grace()).await;
        let stopped = self.world.stop().await;
        self.accounts.store().close().await;
        self.console.end();
        stopped
    }
}

/// Opens the configured store, making its folder, the data folder, if it's missing (owner-only on
/// Unix): `init` writes a config without that folder, and SQLite makes no folders. The server's
/// start and the admin commands both open it so, whichever runs first.
pub async fn open_store(cfg: &Config) -> anyhow::Result<Store> {
    if let Some(data) = cfg.store.path.parent() {
        config::private_folder(data)
            .with_context(|| format!("can't make the data folder {}", data.display()))?;
    }
    Store::open(&cfg.store.path)
        .await
        .with_context(|| format!("opening {}", cfg.store.path.display()))
}

/// What a start warns about when the account API sits on loopback behind a proxy (`public_api`
/// set) that isn't trusted.
const PROXY_NOT_TRUSTED: &str = "public_api is set and api_bind is a loopback address, so players \
     reach the account API through a proxy on this machine; with trusted_proxies empty, every \
     request seems to come from that proxy and they all share one address's limit: list it in \
     [server] trusted_proxies";

/// What a start warns the host about in `cfg`'s addresses, `pin` being the server's own key's:
/// - no invite link that reaches beyond this computer (M4 hosting spec §3.11), which needs
///   `public_api`, or `tls = "self"`, which gives one for each of the machine's addresses (HA4);
/// - a proxy in front of a loopback API (`public_api` set) that `trusted_proxies` doesn't list,
///   unless the API serves TLS itself: what's in front of it then passes the players' TLS through
///   and can't add `X-Forwarded-For`, so trusting it would let each player name their own address
///   (the hosting plan's final review, Minor 3);
/// - for that reason, `trusted_proxies` set under `tls = "self"`, which can only be a mistake
///   (#359).
///
/// A loopback API without `public_api` is a server for this computer, and warns of nothing.
fn address_warnings(cfg: &Config, pin: Option<&str>) -> Vec<String> {
    let s = &cfg.server;
    let mut warnings = Vec::new();
    if s.tls.is_none() && cfg.invite_base(pin).is_none() {
        warnings.push(crate::invite::no_links());
    }
    let loopback = s.api_bind.ip().to_canonical().is_loopback();
    if s.tls.is_none() && s.public_api.is_some() && loopback && s.trusted_proxies.is_empty() {
        warnings.push(PROXY_NOT_TRUSTED.to_owned());
    }
    if s.tls == Some(Tls::OwnKey) && !s.trusted_proxies.is_empty() {
        warnings.push(own_key_proxies());
    }
    warnings
}

/// What a start warns about when `trusted_proxies` is set under `tls = "self"`.
fn own_key_proxies() -> String {
    format!(
        "trusted_proxies is set, but under tls = \"self\" the account API ends TLS itself, so no \
         proxy in front of it can be trusted: one that ended TLS would break the pin in your links, \
         and one that passes TLS through can't add X-Forwarded-For, so trusting it only lets each \
         player name their own address. Empty trusted_proxies; with Caddy in front, put a # before \
         tls = \"self\" instead: see {}",
        crate::config::guide()
    )
}

/// The server link a start shows beside "Fenji is running" under `tls = "self"` (M4 hosting spec
/// §3.10), for players who already have an account: the first of `bases` (`invite::bases`), the
/// one address set for links or the machine's first, and that `link` shows the rest. None without
/// the server's own key, whose host has an address for players already, or without a base.
fn running_link(cfg: &Config, bases: &[(String, crate::invite::InviteBase)]) -> Option<String> {
    if cfg.server.tls != Some(Tls::OwnKey) {
        return None;
    }
    let (label, base) = bases.first()?;
    let link = crate::invite::server_link(base);
    let label = crate::text::escape_hidden(label);
    Some(match bases.len() {
        1 => format!("server link ({label}): {link}"),
        _ => format!("server link ({label}): {link} (type link for the others)"),
    })
}

/// Why the server can't listen on `addr`, and, for a port another program uses, what to do (the
/// hosting plan's final review B, Minor 4).
fn cant_listen(addr: SocketAddr, e: std::io::Error) -> anyhow::Error {
    let todo = match e.kind() {
        std::io::ErrorKind::AddrInUse => format!(
            ": another program uses port {}; stop it, or give the server another port in fenji.toml",
            addr.port()
        ),
        _ => String::new(),
    };
    anyhow::Error::new(e).context(format!("can't listen on {addr}{todo}"))
}

/// The account API's listener: plain, or TLS with the server's own key. Either keeps the open
/// connections to their places and ends idle ones (Ruling HP9).
enum ApiListener {
    Plain(PlainListener),
    OwnKey(TlsListener),
}

/// Opens the store ([`open_store`]), loads the server's [`Status`] from it, and under
/// `tls = "self"` its key (made on the first start, in `data/tls/`), binds both listeners (port 0
/// takes a free port) and starts serving.
pub async fn start(
    cfg: &Config,
    clock: Arc<dyn Clock>,
    limits: GateLimits,
) -> anyhow::Result<RunningServer> {
    let store = open_store(cfg).await?;
    let status = Status::load(&store, clock.now_ms(), &cfg.server.motd).await?;
    if status.maintenance().is_some() {
        tracing::warn!(
            "maintenance is on, as it was: only admins can sign in until it's turned off"
        );
    }
    let key = match cfg.server.tls {
        // The certificate's names don't matter to launchers, which pin the key.
        Some(Tls::OwnKey) => Some(ServerKey::load_or_make(
            &tls::folder(&cfg.store.path),
            cfg.server.public_host.as_deref().unwrap_or("localhost"),
        )?),
        None => None,
    };
    let pin = key.as_ref().map(|k| k.pin.clone());
    let world = world::start(
        Rules::from_config(cfg),
        store.clone(),
        cfg.invite_base(pin.as_deref()),
        Arc::clone(&clock),
        status.names_sender(),
        status.watch_motd(),
    )?;
    // `[access].admins`, lower-cased: names are compared without regard to ASCII case.
    let admins: Vec<String> = cfg
        .access
        .admins
        .iter()
        .map(|a| a.to_ascii_lowercase())
        .collect();
    let accounts = Arc::new(
        Accounts::new(store, cfg.auth.clone(), Arc::clone(&clock))
            .with_door(Arc::clone(&status), admins.clone()),
    );
    // The fixed hash for unknown usernames, made now rather than at the first one (#132's review).
    tokio::task::spawn_blocking(fenji_auth::password::warm_dummy);
    let login = TcpListener::bind(cfg.server.login_bind)
        .await
        .map_err(|e| cant_listen(cfg.server.login_bind, e))?;
    let login_addr = login.local_addr()?;
    let api = TcpListener::bind(cfg.server.api_bind)
        .await
        .map_err(|e| cant_listen(cfg.server.api_bind, e))?;
    let api_addr = api.local_addr()?;
    let (conns, proxies) = (ConnLimits::default(), &cfg.server.trusted_proxies);
    let api = match &key {
        Some(key) => ApiListener::OwnKey(TlsListener::new(
            api,
            key,
            TlsLimits::default(),
            conns,
            proxies,
        )?),
        None => ApiListener::Plain(PlainListener::new(api, conns, proxies)?),
    };
    for warning in address_warnings(cfg, pin.as_deref()) {
        tracing::warn!("{warning}");
    }
    let info = ServerInfo {
        name: cfg.server.name.clone(),
        version: env!("CARGO_PKG_VERSION").into(),
        pinned_build: PINNED_BUILD.into(),
        gate_host: cfg.server.public_host.clone(),
        gate_port: login_addr.port(),
        server_id: cfg.server.id,
        source: cfg.server.source_url.clone(),
        host: cfg.server.host.clone(),
    };
    let online = Arc::new(Online::new(limits.max_players));
    let console = ConsoleService::new(Parts {
        cfg: cfg.clone(),
        store: accounts.store().clone(),
        status: Arc::clone(&status),
        online: Arc::clone(&online),
        world: world.handle(),
        clock: Arc::clone(&clock),
        pin: pin.clone(),
        gate_port: login_addr.port(),
        api_port: api_addr.port(),
        world_wait: WORLD_WAIT,
    });
    let gate = Arc::new(Gate {
        accounts: Arc::clone(&accounts),
        online: Arc::clone(&online),
        lobby: LobbyLimits {
            max_characters: cfg.lobby.max_characters,
        },
        limits,
        waiting: Places::new(
            limits.max_unauthenticated,
            limits.max_unauthenticated_per_network,
        ),
        clock,
        world: world.handle(),
        start: world::start_location(cfg),
        admins,
        stopping: watch::Sender::new(false),
    });
    let state = AppState {
        accounts: Arc::clone(&accounts),
        info,
        trusted_proxies: cfg.server.trusted_proxies.clone(),
        status: Arc::clone(&status),
        online,
    };
    let app = router(state.clone());
    let purger = Arc::clone(&accounts);
    let listener = tokio::spawn(accept_loop(login, Arc::clone(&gate)));
    let tasks = vec![
        tokio::spawn(async move {
            let service = app.into_make_service_with_connect_info::<Addrs>();
            let served = match api {
                ApiListener::Plain(tcp) => axum::serve(tcp, service).await,
                ApiListener::OwnKey(tls) => axum::serve(tls, service).await,
            };
            if let Err(e) = served {
                tracing::error!(error = %e, "the account API stopped");
            }
        }),
        tokio::spawn(async move {
            let mut every = tokio::time::interval(Duration::from_secs(3600));
            loop {
                every.tick().await;
                match purger.purge().await {
                    Ok(n) => tracing::debug!(n, "purged expired sessions and tokens"),
                    Err(e) => tracing::warn!(error = ?e, "purge"),
                }
            }
        }),
        tokio::spawn(service::keep_fresh(Arc::downgrade(&console))),
    ];
    Ok(RunningServer {
        login_addr,
        api_addr,
        pin,
        accounts,
        status,
        api: state,
        world,
        gate,
        console,
        data: config::data_folder(&cfg.store.path),
        listener,
        tasks,
    })
}

/// `fenji-server run`: serves until Ctrl-C, SIGTERM, its terminal closing (SIGHUP, unless it came
/// ignored, as under `nohup`; on Windows, its window closing, logging off or shutting down) or the
/// console's `stop`, each stopping it cleanly (`signalled`), with `front` in the terminal it
/// started in (M4 hosting spec §3.1), `hub` being the log's. A first run (§3.6) says
/// what it wrote before it starts, and what's next once it runs. Under `tls = "self"` the start
/// shows a server link beside "Fenji is running" (§3.10).
/// - First it claims its data folder (§3.5): another server using it stops this one before it
///   opens the store or listens (Review Focus 5), saying what to do with the `--config` it was
///   `given`, if any. On Linux and macOS the claim is also the console
///   socket, which consoles attach to while it serves, and are told goodbye on as it stops;
///   elsewhere it's the folder's lock alone ([`crate::lock`]).
/// - [`Front::Full`]: the full-screen console holds the terminal while the server runs on a task
///   of its own; Ctrl-C there is `stop`. However the console ends, a panic included, it gives the
///   terminal back first (Review Focus 2). One that ends while the server runs leaves it running,
///   and says so, until a signal stops it. A terminal that hangs up ends the console, and its
///   SIGHUP stops the server, unless SIGHUP came ignored: the log then says the server runs on
///   without a console (#359). Then `run` waits for the server's end. Once the server has stopped,
///   it waits [`CONSOLE_GRACE`] at most for a console that hasn't ended: one whose terminal has
///   stopped reading may never end.
/// - [`Front::Plain`]: commands from stdin and answers on stdout, the log being on stderr. The end
///   of stdin leaves the server running. Once the server has stopped, `run` waits
///   [`CONSOLE_GRACE`] at most for these too: a stdout that takes no more answers, such as a pipe
///   nobody reads, would hold a write for ever (#359).
pub async fn run(
    loaded: Loaded,
    given: Option<&Path>,
    front: Front,
    hub: LogHub,
) -> anyhow::Result<()> {
    let (cfg, first_run) = match loaded {
        Loaded::Existing(cfg) => (cfg, None),
        Loaded::FirstRun(cfg) => {
            let here = std::env::current_dir().unwrap_or_else(|_| ".".into());
            (cfg, Some(config::first_run_lines(&here)))
        }
    };
    let data = config::data_folder(&cfg.store.path);
    #[cfg(unix)]
    let claim = console::socket::claim(&data, given).await?;
    #[cfg(unix)]
    if let Some(why) = claim.unattachable() {
        tracing::warn!("{why}");
    }
    // Held until the server has stopped, at the end of `run`.
    #[cfg(not(unix))]
    let _lock = crate::lock::take(&data, given)?;
    if let Some([wrote, ..]) = &first_run {
        tracing::info!("{wrote}");
    }
    let server = match start(&cfg, Arc::new(SystemClock), GateLimits::from_config(&cfg)).await {
        Ok(server) => server,
        Err(e) => {
            // The full-screen console's log went to its file alone: what a first run wrote, and
            // any warning, come with the error.
            if front == Front::Full {
                for line in hub.subscribe().backlog {
                    say(&line.to_string());
                }
            }
            return Err(e);
        }
    };
    let stop = signalled();
    tracing::info!(gate = %server.login_addr, api = %server.api_addr, "Fenji is running");
    let bases = crate::invite::bases(&cfg, server.pin().as_deref(), server.api_addr.port());
    if let Some(link) = running_link(&cfg, &bases) {
        tracing::info!("{link}");
    }
    match &first_run {
        Some([_, next @ ..]) => {
            for line in next {
                tracing::info!("{line}");
            }
        }
        None => no_one_yet(&server).await,
    }
    let console = server.console();
    #[cfg(unix)]
    let attaching = tokio::spawn(console::socket::serve(
        claim,
        Arc::clone(&console),
        hub.clone(),
    ));
    let served = tokio::spawn(server.serve_until(stop));
    let session = service::session(&console, &hub);
    let logs = console::log::folder(&cfg.store.path);
    let served = match front {
        // Signals stop the server here (`signalled`), and the console ends on its goodbye.
        Front::Full => {
            // A panic on another of the server's threads goes to the log, and the console runs on.
            let tui = fenji_console::run_tui(
                session,
                Mode::InProcess,
                Colours::from_env(),
                std::future::pending(),
                Some(console::log::log_panic),
            );
            let ended = held_until_stopped(tui, console.stopped(), CONSOLE_GRACE).await;
            if matches!(ended, Ok(Ending::HungUp)) {
                // No terminal to tell: the log says it, unless the hang-up's SIGHUP is stopping
                // the server.
                if runs_on(console.stopping(), HANGUP_WAIT).await {
                    tracing::warn!("{TERMINAL_GONE}");
                }
            } else {
                // The terminal is the host's again, whatever ended the console, unless the
                // console was left to its thread, its terminal stalled.
                let serving = !served.is_finished() && !*console.stopping().borrow();
                if let Some(note) = closed_note(&ended, serving, &logs) {
                    say(&note);
                }
            }
            joined(served).await
        }
        Front::Plain => {
            let input = tokio::io::BufReader::new(tokio::io::stdin());
            let (out, err) = (std::io::stdout(), std::io::stderr());
            let leave = std::future::pending();
            let lines =
                fenji_console::run_plain(session, input, out, err, false, Mode::InProcess, leave);
            // Bounded as the full-screen console is: a write of an answer to a stdout that takes
            // no more, such as a pipe nobody reads, never ends (#359).
            let ended = held_until_stopped(lines, console.stopped(), CONSOLE_GRACE).await;
            let serving = !served.is_finished() && !*console.stopping().borrow();
            if let Err(e) = ended
                && serving
            {
                tracing::warn!(error = %e, "the console's lines have stopped: the server runs on");
            }
            joined(served).await
        }
    };
    // The server has stopped, however it ended: attached consoles are told goodbye, then the
    // socket goes, and the claim on the data folder with it.
    console.end();
    #[cfg(unix)]
    let _ = attaching.await;
    if front == Front::Full {
        say(&format!(
            "Fenji has stopped. Its log is in {}.",
            logs.display()
        ));
    }
    served
}

/// Why the full-screen console ended when its own code panicked: the panic hook has given the
/// terminal back and printed the message.
const CONSOLE_PANICKED: &str = "a panic ended the console; its message is above";

/// A console, full-screen or plain lines, run until it ends on a thread of its own, off the
/// runtime's workers: its writes to the terminal block, and a terminal that stops reading (a
/// stalled SSH link) would otherwise hold a worker, all of them on a one-CPU host, and the server
/// with them (re-review, NB2). One whose own code panics ends as a console that failed, so `run`
/// goes on serving until a signal stops the server gracefully, rather than unwinding through it
/// and out of the process (review 1, Important 1). `main` doesn't wait for this thread as it
/// exits.
pub(crate) async fn held(
    console: impl Future<Output = std::io::Result<Ending>> + Send + 'static,
) -> std::io::Result<Ending> {
    let runtime = tokio::runtime::Handle::current();
    match tokio::task::spawn_blocking(move || runtime.block_on(console)).await {
        Ok(ended) => ended,
        Err(e) if e.is_panic() => Err(std::io::Error::other(CONSOLE_PANICKED)),
        Err(e) => Err(std::io::Error::other(e.to_string())),
    }
}

/// How long `run` waits for its console, full-screen or plain lines, once the server has stopped,
/// and the process for the log's stderr as it exits ([`crate::console::log::finish`]). The console
/// ends on the server's goodbye, unless it's caught in a write to a terminal that has stopped
/// reading, as over a stalled SSH link (the fix pass's re-review, NB1), or to a stdout nobody reads
/// (#359). The goodbye itself may wait `ANSWERS_WAIT` for an answer still due, from the same
/// moment, so this is a second longer: a console is never left as its goodbye comes.
pub const CONSOLE_GRACE: Duration =
    crate::console::service::ANSWERS_WAIT.saturating_add(Duration::from_secs(1));

// The order the grace's doc promises.
const _: () =
    assert!(CONSOLE_GRACE.as_millis() > crate::console::service::ANSWERS_WAIT.as_millis());

/// How long `run` gives the server to begin stopping once its full-screen console's terminal has
/// gone: the hang-up's SIGHUP stops it, unless SIGHUP came ignored.
const HANGUP_WAIT: Duration = Duration::from_millis(500);

/// What `run` logs once its full-screen console's terminal has gone while the server runs on, SIGHUP
/// having come ignored (`trap '' HUP`, or `nohup`): the server has no console now, and this says how
/// a host reaches it, and stops it (#359).
const TERMINAL_GONE: &str = "the console's terminal has gone (its window closed, or its SSH \
     connection was lost), and SIGHUP didn't stop the server, so it runs on without a console: \
     `fenji-server console` attaches to it, and `fenji-server console --run stop`, SIGINT or \
     SIGTERM stops it";

/// Whether the server runs on `wait` after its console's terminal has gone, `stopping` being the
/// console service's ([`ConsoleService::stopping`]): not when it begins stopping meanwhile.
async fn runs_on(mut stopping: watch::Receiver<bool>, wait: Duration) -> bool {
    let stops = tokio::time::timeout(wait, stopping.wait_for(|stopping| *stopping)).await;
    !matches!(stops, Ok(Ok(_)))
}

/// Why `run` stops waiting for its console: the server has stopped, and the console didn't end
/// within [`CONSOLE_GRACE`].
const CONSOLE_LEFT: &str = "the server has stopped, but the console hasn't ended, its terminal \
     or its output likely stalled: it's left behind";

/// [`held`], waited for no longer than `grace` once the server has `stopped`
/// ([`ConsoleService::stopped`]). A console still running then, such as one caught in a write to a
/// terminal that has stopped reading, or to a stdout nobody reads, is left to its thread, which
/// `main` doesn't wait for, so that the process ends. The terminal gets its mode back first
/// ([`fenji_console::restore_mode`], which takes no write, and does nothing for plain lines), so
/// that the shell after the server reads lines once the terminal reads again (#359).
async fn held_until_stopped(
    console: impl Future<Output = std::io::Result<Ending>> + Send + 'static,
    mut stopped: watch::Receiver<bool>,
    grace: Duration,
) -> std::io::Result<Ending> {
    let left = async move {
        let _ = stopped.wait_for(|stopped| *stopped).await;
        tokio::time::sleep(grace).await;
    };
    tokio::select! {
        ended = held(console) => ended,
        () = left => {
            fenji_console::restore_mode();
            tracing::info!("{CONSOLE_LEFT}");
            Err(std::io::Error::other(CONSOLE_LEFT))
        }
    }
}

/// Why the full-screen console ended while the server may run on; `None` once the server said
/// goodbye, having stopped.
fn still_serving(ended: &std::io::Result<Ending>) -> Option<String> {
    match ended {
        Ok(Ending::ServerStopped(_)) => None,
        Ok(Ending::InputClosed) => Some("its keys ended".into()),
        Ok(Ending::Lost | Ending::Detached) => Some("it lost the server".into()),
        Ok(Ending::HungUp) => Some("its terminal has gone".into()),
        Err(e) => Some(e.to_string()),
    }
}

/// What `run` tells the host once the full-screen console has ended while the server still serves
/// (`serving`): why, how the server stops now, and where its log is. Nothing once the server has
/// said goodbye, or stopped meanwhile.
fn closed_note(ended: &std::io::Result<Ending>, serving: bool, logs: &Path) -> Option<String> {
    let why = still_serving(ended)?;
    serving.then(|| {
        format!(
            "The console has closed ({why}). The server runs on until Ctrl-C, SIGTERM or closing \
             this terminal stops it; its log is in {}. `fenji-server --plain` gives the console \
             as plain lines, for a terminal the full-screen one doesn't suit.",
            logs.display()
        )
    })
}

/// The server's result, once its task has ended.
async fn joined(served: JoinHandle<anyhow::Result<()>>) -> anyhow::Result<()> {
    served
        .await
        .unwrap_or_else(|e| Err(anyhow::anyhow!("the server's task failed: {e}")))
}

/// A line for the host's terminal, outside the log: on stderr, and never a panic when it's gone.
pub(crate) fn say(text: &str) {
    say_to(&mut std::io::stderr(), text);
}

/// `text` as a line on `out`, cleaned of hidden characters as everything a console prints is
/// (Review Focus 1): an error's text, or a path, may hold one.
fn say_to(out: &mut impl Write, text: &str) {
    let _ = writeln!(out, "{}", clean(text));
}

/// With no account in the store, says how to invite the first player: a server whose first run
/// couldn't start runs as an ordinary one after, and never said so (carry-forward 3).
async fn no_one_yet(server: &RunningServer) {
    match server.accounts().store().list_accounts().await {
        Ok(accounts) if accounts.is_empty() => {
            tracing::info!("No one has an account yet: type `invite` for a link to send a friend.");
        }
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, "can't list the accounts"),
    }
}

/// Ctrl-C, SIGTERM, or SIGHUP, which the server gets when the terminal or window it runs in closes,
/// or its SSH session is lost (the hosting plan's final review, Important 1), unless SIGHUP came
/// ignored, as `nohup` starts a server that is to outlive its terminal. SIGTERM and SIGHUP are
/// caught from this call on, not from the future's first poll, so that one sent as soon as the
/// server says it runs stops it cleanly.
#[cfg(unix)]
fn signalled() -> impl Future<Output = ()> {
    use tokio::signal::unix::{Signal, SignalKind, signal};
    let term = signal(SignalKind::terminate()).ok();
    // Read first: catching SIGHUP replaces what was there.
    let hangup = if crate::unix::hangup_ignored() {
        None
    } else {
        signal(SignalKind::hangup()).ok()
    };
    // A signal that isn't caught never comes here.
    async fn caught(signal: Option<Signal>) {
        match signal {
            Some(mut signal) => {
                signal.recv().await;
            }
            None => std::future::pending().await,
        }
    }
    async move {
        let signal = tokio::select! {
            _ = tokio::signal::ctrl_c() => "SIGINT",
            () = caught(term) => "SIGTERM",
            () = caught(hangup) => "SIGHUP",
        };
        tracing::info!(signal, "shutting down");
    }
}

/// Ctrl-C, or the window closing, the user logging off or the system shutting down (the hosting
/// plan's final review, Important 1), each caught from this call on. Windows ends the process a few
/// seconds after the last three, or once it exits: the stop lets the world save everyone first,
/// before its grace for the players' connections.
#[cfg(windows)]
fn signalled() -> impl Future<Output = ()> {
    use tokio::signal::windows::{ctrl_close, ctrl_logoff, ctrl_shutdown};
    let close = ctrl_close().ok();
    let logoff = ctrl_logoff().ok();
    let shutdown = ctrl_shutdown().ok();
    async move {
        let event = tokio::select! {
            _ = tokio::signal::ctrl_c() => "Ctrl-C",
            Some(()) = async { close?.recv().await } => "the window closed",
            Some(()) = async { logoff?.recv().await } => "logging off",
            Some(()) = async { shutdown?.recv().await } => "the system shutting down",
        };
        tracing::info!(event, "shutting down");
    }
}

#[cfg(test)]
mod tests {
    use std::future;
    use std::path::Path;

    use fenji_store::{Character, NewCharacter, Place};
    use tokio::net::TcpStream;
    use tokio::sync::{mpsc, oneshot};

    use super::*;
    use crate::clock::TestClock;
    use crate::world::{KickReason, Location, Look, Player, Pos, WorldOut};

    const NOW: i64 = 1_800_000_000_000;

    const AT: Location = Location {
        scene: 5,
        pos: Pos {
            x: 1.0,
            y: 2.0,
            z: 3.0,
            yaw: 90.0,
        },
    };

    async fn next(rx: &mut mpsc::Receiver<WorldOut>) -> WorldOut {
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("within 5 s")
            .expect("the channel is open")
    }

    /// A server in `dir`, with two characters that have never stood in the world.
    async fn server_with_characters(dir: &Path) -> (RunningServer, Vec<Character>) {
        let cfg = Config::parse(
            "[server]\nname = \"Fenji\"\nlogin_bind = \"127.0.0.1:0\"\n\
             api_bind = \"127.0.0.1:0\"\n",
            dir,
        )
        .unwrap();
        let clock = Arc::new(TestClock::at(NOW));
        let server = start(&cfg, clock, GateLimits::default()).await.unwrap();
        let store = server.accounts().store().clone();
        let mut characters = Vec::new();
        for (n, name) in [(1u8, "aaa"), (2, "bbb")] {
            store
                .create_invite(&[n; 32], None, NOW, None)
                .await
                .unwrap();
            let account = store
                .redeem_invite(&[n; 32], name, "$phc", NOW)
                .await
                .unwrap();
            let new = NewCharacter {
                name: format!("{name}c"),
                vocation: 0,
                skeleton: 2,
                appearance: vec![1],
            };
            let c = store.create_character(account, &new, 9, NOW).await.unwrap();
            characters.push(c);
        }
        (server, characters)
    }

    /// Each character enters the world at [`AT`], as its link would have it: their channels, once
    /// each has had its LoadScene.
    async fn enter(world: &WorldHandle, characters: &[Character]) -> Vec<mpsc::Receiver<WorldOut>> {
        let mut links = Vec::new();
        for (conn, c) in (1..).zip(characters) {
            let (out, mut link) = mpsc::channel(8);
            let look = Look {
                name: c.name.clone(),
                vocation: c.vocation,
                skeleton: c.skeleton,
                level: c.level,
                appearance: Arc::from(c.appearance.as_slice()),
            };
            let player = Player {
                conn,
                account: c.account_id,
                admin: false,
                pid: c.pid,
                look,
                at: AT,
            };
            assert!(world.send(WorldIn::Join { player, out }));
            assert!(matches!(next(&mut link).await, WorldOut::LoadScene { .. }));
            links.push(link);
        }
        links
    }

    /// A console whose own code panics, as a bug in the view or the terminal's library would.
    async fn panicking_console() -> std::io::Result<Ending> {
        panic!("in the console's own code")
    }

    #[tokio::test]
    async fn a_console_that_panics_ends_as_one_that_failed_and_the_server_runs_on() {
        // Review 1, Important 1 (carry-forward 5): a panic in the full-screen console's own code
        // mustn't unwind through `run` and take the server down without a graceful stop. It ends
        // the console as an error; the panic hook has given the terminal back by then.
        let ended = held(panicking_console()).await;
        let e = ended.expect_err("the console failed");
        assert_eq!(
            e.to_string(),
            "a panic ended the console; its message is above"
        );
        assert_eq!(
            still_serving(&Err(e)),
            Some("a panic ended the console; its message is above".into())
        );
        // A console that ends ends so.
        let closed = held(async { Ok(Ending::InputClosed) }).await;
        assert_eq!(closed.unwrap(), Ending::InputClosed);
    }

    #[test]
    fn systemd_waits_out_a_stops_worst_case_with_room_to_spare() {
        // #281: a stop takes at most the connections' grace, the world thread's wait, the last
        // saves' and the consoles' goodbye, one after another: 30 s. The reference unit gives it
        // more than that before systemd ends it, so nothing is cut short even then.
        let worst = GateLimits::default().grace()
            + world::THREAD_WAIT
            + world::SAVE_WAIT
            + crate::console::socket::GOODBYE_WAIT;
        assert_eq!(worst, Duration::from_secs(30));
        let unit = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../deploy/systemd/fenji.service"
        ))
        .unwrap();
        let secs: u64 = unit
            .lines()
            .find_map(|l| l.strip_prefix("TimeoutStopSec="))
            .expect("the unit sets TimeoutStopSec")
            .parse()
            .unwrap();
        assert!(
            Duration::from_secs(secs) >= worst + Duration::from_secs(5),
            "TimeoutStopSec={secs}, for a worst case of {worst:?}"
        );
    }

    #[test]
    fn a_console_that_blocks_its_thread_leaves_the_runtimes_workers_free() {
        // Re-review, NB2: a terminal that stops reading (a stalled SSH link) blocks the
        // console's writes. On a host with one CPU, tokio has one worker: a console on it would
        // stop the whole server.
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let mut console = Box::pin(held(async {
                // A write to a terminal that has stopped reading.
                std::thread::sleep(Duration::from_millis(1_000));
                Ok(Ending::InputClosed)
            }));
            // Started: `held`'s first poll hands the console to the thread it runs on.
            tokio::select! {
                biased;
                _ = &mut console => panic!("the console ended at once"),
                () = std::future::ready(()) => {}
            }
            std::thread::sleep(Duration::from_millis(100));
            // A task of the server's, while the console blocks: its one worker must be free.
            let probing = std::time::Instant::now();
            tokio::spawn(async {}).await.unwrap();
            let took = probing.elapsed();
            assert!(
                took < Duration::from_millis(200),
                "the runtime's one worker waited {took:?} on the console"
            );
            assert_eq!(console.await.unwrap(), Ending::InputClosed);
        });
    }

    #[test]
    fn run_waits_for_its_console_longer_than_a_goodbye_waits_for_an_answer() {
        // #359 (Task 7's review, Minor 3): a console's goodbye may wait ANSWERS_WAIT for an
        // answer still due as the server stops, and both waits count from its stop. Equal, `run`
        // could leave a console at the instant its goodbye came: the last answer lost, and a
        // full-screen console's screen never given back.
        let answers = crate::console::service::ANSWERS_WAIT;
        assert!(
            CONSOLE_GRACE >= answers + Duration::from_secs(1),
            "CONSOLE_GRACE {CONSOLE_GRACE:?}, ANSWERS_WAIT {answers:?}"
        );
    }

    #[test]
    fn a_console_that_hasnt_ended_once_the_server_has_stopped_is_left_behind() {
        // The fix pass's re-review, NB1: a console caught in a read of a terminal that has gone
        // never ends, and `run` waited for it without end, the process with it.
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let grace = Duration::from_millis(100);
            let (stop, stopped) = watch::channel(false);
            let stuck = future::pending::<std::io::Result<Ending>>();
            let mut waiting = Box::pin(held_until_stopped(stuck, stopped.clone(), grace));
            // While the server serves, a console is waited for, however long it takes.
            let serving = tokio::time::timeout(Duration::from_millis(500), &mut waiting).await;
            assert!(serving.is_err(), "left while the server served");
            // Once it has stopped, `grace` at most.
            stop.send_replace(true);
            let left = tokio::time::timeout(Duration::from_secs(10), waiting).await;
            let left = left.expect("waited for without end");
            assert_eq!(left.unwrap_err().to_string(), CONSOLE_LEFT);
            // A console that ends on the server's goodbye ends so, well within its grace.
            let bye = Ending::ServerStopped(service::STOPPING.into());
            let ending = bye.clone();
            let ample = Duration::from_secs(10);
            let ended = held_until_stopped(async move { Ok(ending) }, stopped, ample).await;
            assert_eq!(ended.unwrap(), bye);
        });
        // As `main` does: the console left behind keeps its thread, which isn't waited for.
        rt.shutdown_background();
    }

    #[tokio::test]
    async fn a_server_whose_console_hung_up_runs_on_unless_it_begins_stopping() {
        // #359: the hang-up's SIGHUP stops the server unless it came ignored, so `run` logs that
        // the server runs on without a console only once it hasn't begun stopping.
        let (stop, stopping) = watch::channel(false);
        assert!(runs_on(stopping.clone(), Duration::from_millis(50)).await);
        let stops = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            stop.send_replace(true);
            stop
        });
        assert!(!runs_on(stopping, Duration::from_secs(10)).await);
        let _stop = stops.await.unwrap();
    }

    #[test]
    fn the_console_says_the_server_runs_on_only_while_it_does() {
        // Review 1, Minor 3: a console that ends as the server stops (a key pressed just then, or
        // the server's own task failing) mustn't be told the server runs on.
        let logs = Path::new("/srv/fenji/data/logs");
        let panicked = || Err(std::io::Error::other(CONSOLE_PANICKED));
        let note = closed_note(&panicked(), true, logs).expect("a note while it serves");
        // Closing the terminal stops it cleanly too (the hosting plan's final review, Important 1).
        assert_eq!(
            note,
            "The console has closed (a panic ended the console; its message is above). The \
             server runs on until Ctrl-C, SIGTERM or closing this terminal stops it; its log is \
             in /srv/fenji/data/logs. `fenji-server --plain` gives the console as plain lines, \
             for a terminal the full-screen one doesn't suit."
        );
        assert_eq!(closed_note(&panicked(), false, logs), None);
        assert_eq!(closed_note(&Ok(Ending::Lost), false, logs), None);
        let stopped = Ok(Ending::ServerStopped(service::STOPPING.into()));
        assert_eq!(closed_note(&stopped, true, logs), None);
    }

    #[test]
    fn runs_own_lines_reach_the_terminal_cleaned() {
        // Review 1, Minor 8 (Review Focus 1): an error's text or a path with a hidden character
        // can't act on the host's terminal.
        let mut out = Vec::new();
        say_to(&mut out, "its log is in /srv/\u{1b}[2Jfenji/logs");
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "its log is in /srv/\\u{1b}[2Jfenji/logs\n"
        );
    }

    #[tokio::test]
    async fn a_config_from_init_starts_and_makes_its_data_folder_and_key() {
        // `init` writes the template, whose database is in data/, but not that folder, and SQLite
        // makes no folders: the start makes it, and the key the template's tls = "self" asks for.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fenji.toml");
        config::init(&path).unwrap();
        // Free loopback ports rather than the template's, as FENJI_LOGIN_BIND and FENJI_API_BIND
        // would make them (Ruling HP7).
        let cfg = Config::load(&path)
            .unwrap()
            .override_binds(|_| Some("127.0.0.1:0".into()))
            .unwrap();
        let ports = (cfg.server.login_bind.port(), cfg.server.api_bind.port());
        assert_eq!(ports, (0, 0), "the binds are free ports");
        let clock = Arc::new(TestClock::at(NOW));
        let server = start(&cfg, clock, GateLimits::default()).await.unwrap();
        let data = dir.path().join("data");
        assert!(data.join("fenji.db").is_file());
        assert!(data.join("tls/key.pem").is_file() && data.join("tls/cert.pem").is_file());
        assert_eq!(server.pin().map(|p| p.len()), Some(43));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&data).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700, "data/ is owner-only");
        }
        TcpStream::connect(server.login_addr).await.unwrap();
        TcpStream::connect(server.api_addr).await.unwrap();
        server.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn a_port_in_use_says_what_to_do() {
        // The hosting plan's final review B, Minor 4: "can't listen on …" said what was wrong, not
        // what to do.
        let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let taken = held.local_addr().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::parse(
            &format!(
                "[server]\nname = \"Fenji\"\nlogin_bind = \"{taken}\"\napi_bind = \"127.0.0.1:0\"\n"
            ),
            dir.path(),
        )
        .unwrap();
        let clock = Arc::new(TestClock::at(NOW));
        let e = match start(&cfg, clock, GateLimits::default()).await {
            Ok(_) => panic!("it listened on a port in use"),
            Err(e) => format!("{e:#}"),
        };
        assert!(
            e.starts_with(&format!(
                "can't listen on {taken}: another program uses port {}; stop it, or give the \
                 server another port in fenji.toml",
                taken.port()
            )),
            "{e}"
        );
    }

    #[tokio::test]
    async fn a_key_without_its_certificate_stops_the_start_before_anything_listens() {
        let dir = tempfile::tempdir().unwrap();
        let tls = dir.path().join("tls");
        crate::tls::ServerKey::load_or_make(&tls, "127.0.0.1").unwrap();
        std::fs::remove_file(tls.join("cert.pem")).unwrap();
        let cfg = Config::parse(
            "[server]\nname = \"Fenji\"\nlogin_bind = \"127.0.0.1:0\"\napi_bind = \"127.0.0.1:0\"\n\
             tls = \"self\"\n",
            dir.path(),
        )
        .unwrap();
        let clock = Arc::new(TestClock::at(NOW));
        let e = match start(&cfg, clock, GateLimits::default()).await {
            Ok(_) => panic!("a server started without its certificate"),
            Err(e) => format!("{e:#}"),
        };
        assert!(e.contains("cert.pem"), "{e}");
        assert!(!tls.join("cert.pem").exists(), "never a silent new key");
    }

    #[test]
    fn a_start_warns_only_where_the_addresses_need_the_host() {
        let cfg = |extra: &str| {
            Config::parse(&format!("[server]\nname = \"F\"\n{extra}"), Path::new("/")).unwrap()
        };
        let pin = "q1o9C1M6ufA5UGrUkSYhW-AXXa3S7jdJ9Fxlp0f2cBo";
        // This computer only, by default, and HA3's template: nothing to warn about.
        assert_eq!(address_warnings(&cfg(""), None), Vec::<String>::new());
        let template =
            cfg("tls = \"self\"\nlogin_bind = \"0.0.0.0:7800\"\napi_bind = \"0.0.0.0:7801\"\n");
        assert_eq!(address_warnings(&template, Some(pin)), Vec::<String>::new());
        // No invite link that reaches beyond this computer (#174, §3.11).
        for extra in [
            "public_host = \"203.0.113.5\"\n",
            "api_bind = \"0.0.0.0:7801\"\n",
        ] {
            assert_eq!(
                address_warnings(&cfg(extra), None),
                [crate::invite::no_links()],
                "{extra}"
            );
        }
        // A proxy on this machine forwards to a loopback API: one address limit for everyone,
        // unless it's trusted.
        let proxied = cfg("public_api = \"https://play.example.net\"\n");
        let warned = address_warnings(&proxied, None);
        assert_eq!(warned.len(), 1, "{warned:?}");
        assert!(
            warned[0].contains("trusted_proxies") && warned[0].contains("public_api"),
            "says why it matters here: {}",
            warned[0]
        );
        let trusted =
            cfg("public_api = \"https://play.example.net\"\ntrusted_proxies = [\"127.0.0.1\"]\n");
        assert_eq!(address_warnings(&trusted, None), Vec::<String>::new());
        let direct =
            cfg("public_api = \"https://play.example.net\"\napi_bind = \"0.0.0.0:7801\"\n");
        assert_eq!(address_warnings(&direct, None), Vec::<String>::new());
        // The hosting plan's final review, Minor 3: under tls = "self" the API ends TLS itself,
        // so what forwards players to a loopback API (an SSH tunnel, socat, a stream proxy) passes
        // their TLS through and can't add X-Forwarded-For. Listed as trusted, it would let each
        // client name its own address: no advice to list it.
        let tunnelled = cfg("tls = \"self\"\npublic_api = \"https://play.example.net:7801\"\n");
        assert_eq!(
            address_warnings(&tunnelled, Some(pin)),
            Vec::<String>::new()
        );
    }

    #[test]
    fn under_its_own_key_a_trusted_proxy_is_warned_of() {
        // #359: under tls = "self" the account API ends TLS itself. A proxy in front that ended it
        // would break the pin in players' links, and one that passes it through can't add
        // X-Forwarded-For: trusting one can only let each player name their own address.
        let cfg = |extra: &str| {
            Config::parse(&format!("[server]\nname = \"F\"\n{extra}"), Path::new("/")).unwrap()
        };
        let pin = "q1o9C1M6ufA5UGrUkSYhW-AXXa3S7jdJ9Fxlp0f2cBo";
        let own = "tls = \"self\"\nlogin_bind = \"0.0.0.0:7800\"\napi_bind = \"0.0.0.0:7801\"\n";
        let trusted = cfg(&format!("{own}trusted_proxies = [\"127.0.0.1\"]\n"));
        let warned = address_warnings(&trusted, Some(pin));
        assert_eq!(warned.len(), 1, "{warned:?}");
        assert!(
            warned[0].starts_with("trusted_proxies is set, but under tls = \"self\"")
                && warned[0].contains(&crate::config::guide()),
            "says why it's a mistake, and where to read more: {}",
            warned[0]
        );
        // A tunnel to a loopback API, which the warnings let be (the final review, Minor 3), is
        // one too, listed as trusted.
        let tunnelled = cfg(
            "tls = \"self\"\npublic_api = \"https://play.example.net:7801\"\n\
             trusted_proxies = [\"127.0.0.1\"]\n",
        );
        assert_eq!(address_warnings(&tunnelled, Some(pin)), warned);
        // Without its own key, a proxy that adds X-Forwarded-For is what Caddy is: trusted, as the
        // write-up's section 4 lists it.
        let caddy =
            cfg("public_api = \"https://play.example.net\"\ntrusted_proxies = [\"127.0.0.1\"]\n");
        assert_eq!(address_warnings(&caddy, None), Vec::<String>::new());
    }

    #[test]
    fn under_its_own_key_the_start_shows_one_server_link() {
        // The hosting plan's final review, Important 2 (spec §3.10): beside "Fenji is running",
        // the server's own link for players who have an account, and that `link` shows the rest.
        let cfg = |extra: &str| {
            Config::parse(&format!("[server]\nname = \"F\"\n{extra}"), Path::new("/")).unwrap()
        };
        let pin = "q1o9C1M6ufA5UGrUkSYhW-AXXa3S7jdJ9Fxlp0f2cBo";
        let base = |label: &str, origin: &str| {
            let pin = Some(pin.to_owned());
            (
                label.to_owned(),
                crate::invite::InviteBase {
                    origin: origin.into(),
                    pin,
                },
            )
        };
        let own = cfg("tls = \"self\"\napi_bind = \"0.0.0.0:7801\"\n");
        let one = [base("public_host", "https://203.0.113.5:7801")];
        assert_eq!(
            running_link(&own, &one),
            Some(format!(
                "server link (public_host): https://203.0.113.5:7801/#pin={pin}"
            ))
        );
        let several = [
            base("eth0", "https://192.168.1.10:7801"),
            base("this computer", "https://127.0.0.1:7801"),
        ];
        assert_eq!(
            running_link(&own, &several),
            Some(format!(
                "server link (eth0): https://192.168.1.10:7801/#pin={pin} (type link for the \
                 others)"
            ))
        );
        // An interface's name is shown as the console shows text.
        let odd = [base("eth\u{1b}[2J0", "https://192.168.1.10:7801")];
        let shown = running_link(&own, &odd).unwrap();
        assert!(
            shown.starts_with("server link (eth\\u{1b}[2J0): "),
            "{shown}"
        );
        // Without its own key there's no pin to give, and no base, no link.
        assert_eq!(running_link(&cfg(""), &one), None);
        assert_eq!(running_link(&own, &[]), None);
    }

    /// The error a server serving in `served` stops with by itself (30 s at most).
    async fn failure(served: JoinHandle<anyhow::Result<()>>) -> anyhow::Error {
        tokio::time::timeout(Duration::from_secs(30), served)
            .await
            .expect("the server stops by itself")
            .unwrap()
            .expect_err("with an error")
    }

    #[tokio::test]
    async fn a_panicking_tick_saves_and_kicks_everyone_then_stops_the_server_with_an_error() {
        // Ruling R9 (#222): a bug that panics a tick mustn't leave a server that takes logins with
        // no world behind them. The world saves everyone it can and lets them go with the
        // shutdown's reason, and the server stops with an error, so `fenji-server run` exits
        // non-zero and systemd restarts it.
        let dir = tempfile::tempdir().unwrap();
        let (server, characters) = server_with_characters(dir.path()).await;
        let (login, world) = (server.login_addr, server.world());
        let served = tokio::spawn(server.serve_until(future::pending()));
        let mut links = enter(&world, &characters).await;
        world.send(WorldIn::Panic("a bug in a tick"));
        for link in &mut links {
            assert_eq!(next(link).await, WorldOut::Kicked(KickReason::Shutdown));
        }
        let e = failure(served).await;
        assert!(format!("{e:#}").contains("a bug in a tick"), "{e:#}");
        assert!(TcpStream::connect(login).await.is_err(), "no more logins");
        let store = Store::open(&dir.path().join("fenji.db")).await.unwrap();
        let saved = Place {
            scene: 5,
            x: 1.0,
            y: 2.0,
            z: 3.0,
            yaw: 90.0,
        };
        for c in &characters {
            let place = store.character(c.account_id, c.pid).await.unwrap();
            assert_eq!(place.unwrap().place, Some(saved), "{}", c.name);
        }
    }

    #[test]
    fn a_tick_that_hangs_stops_the_server_with_an_error_that_says_so() {
        // #281: a tick that never returned left a server that took logins into a world that
        // never answered, without a word. The watch takes a tick count that stands still for its
        // time, here a second, for a hang. The server logs it as an error and stops as it always
        // does: the gate stops, and the world's shutdown tick, once the tick returns, lets
        // everyone go with its reason and saves them. It ends with an error, so `fenji-server run`
        // exits non-zero and systemd restarts it. The log is caught by the process's own
        // subscriber (`world::logged`), on the one thread of this test's runtime.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (stopped, log) = crate::world::logged(|| {
            rt.block_on(async {
                let dir = tempfile::tempdir().unwrap();
                let (mut server, characters) = server_with_characters(dir.path()).await;
                server.world.hang_after(Duration::from_secs(1));
                let (login, world) = (server.login_addr, server.world());
                let served = tokio::spawn(server.serve_until(future::pending()));
                let mut links = enter(&world, &characters).await;
                // A tick that hangs for 3 s: past the watch's second, and over within the stop's
                // wait for the world's thread.
                let (begun, working) = oneshot::channel();
                let work = Duration::from_secs(3);
                world.send(WorldIn::Stall { work, begun });
                working.await.unwrap();
                for link in &mut links {
                    assert_eq!(next(link).await, WorldOut::Kicked(KickReason::Shutdown));
                }
                let e = failure(served).await;
                assert!(TcpStream::connect(login).await.is_err(), "no more logins");
                let store = Store::open(&dir.path().join("fenji.db")).await.unwrap();
                for c in &characters {
                    let place = store.character(c.account_id, c.pid).await.unwrap();
                    assert!(place.unwrap().place.is_some(), "{} was saved", c.name);
                }
                format!("{e:#}")
            })
        });
        assert!(stopped.contains("no progress for 1s"), "{stopped}");
        let said = log
            .lines()
            .any(|l| l.contains("ERROR") && l.contains("made no progress for 1s"));
        assert!(said, "{log}");
    }

    #[tokio::test]
    async fn a_services_task_that_ends_early_stops_the_server_with_an_error() {
        // Ruling R13: without its services task the world can't save, so a server that went on
        // taking logins and play would lose what everyone does. As when the world itself fails
        // (R9), everyone is let go with the shutdown's reason, and the server stops with an error.
        let dir = tempfile::tempdir().unwrap();
        let (server, characters) = server_with_characters(dir.path()).await;
        let (login, world) = (server.login_addr, server.world());
        let services = server.world.services_task();
        let served = tokio::spawn(server.serve_until(future::pending()));
        let mut links = enter(&world, &characters).await;
        services.abort();
        for link in &mut links {
            assert_eq!(next(link).await, WorldOut::Kicked(KickReason::Shutdown));
        }
        let e = failure(served).await;
        assert!(format!("{e:#}").contains("services task"), "{e:#}");
        assert!(TcpStream::connect(login).await.is_err(), "no more logins");
    }
}
