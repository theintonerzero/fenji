//! Running the server (M3 spec D5; M4 world spec §3.9): one process with the game gate, the world,
//! the account API, and the purge of expired sessions and tokens at start-up and every hour. Its
//! changing facts, maintenance mode among them, are its [`Status`] (M4 hosting spec §3.8, §3.9).
//! With `tls = "self"` the account API speaks TLS with the server's own key (§3.10).
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use fenji_store::Store;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::{JoinHandle, JoinSet};

use crate::accounts::Accounts;
use crate::api::{Addrs, AppState, ServerInfo, router};
use crate::clock::{Clock, SystemClock};
use crate::config::{self, Config, Loaded, Tls};
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
    /// The gate's listener, which hands back the connections still open when it stops.
    listener: JoinHandle<JoinSet<()>>,
    /// The account API and the purge.
    tasks: Vec<JoinHandle<()>>,
}

impl RunningServer {
    pub fn accounts(&self) -> Arc<Accounts> {
        Arc::clone(&self.accounts)
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

    /// Serves until `stop` completes, or until the world's thread or its services task ends by
    /// itself, then shuts down: what `fenji-server run` awaits. Either ending early takes the
    /// server with it, so that no login waits for a world that isn't there (Ruling R9) or plays
    /// in one that can't save (Ruling R13), and the error makes `fenji-server run` exit non-zero
    /// for systemd to restart it.
    pub async fn serve_until(mut self, stop: impl Future<Output = ()>) -> anyhow::Result<()> {
        let failed = tokio::select! {
            () = stop => None,
            what = self.world.ended() => {
                tracing::error!("{what} has ended: stopping the server");
                Some(what)
            }
        };
        match (self.shutdown().await, failed) {
            (Ok(()), Some(what)) => Err(anyhow::anyhow!("{what} ended before the server")),
            (stopped, _) => stopped,
        }
    }

    /// Stops in an order that lets players in the world read why (#219): the API and the purge
    /// end, the gate stops accepting and lets go of the lobby, the world lets go of every player
    /// with the shutdown reason and saves them, and their connections get the gate's grace to
    /// write their closes before what is left is ended. Then the world stops once its last saves
    /// are written, and the store closes. An error is the world's (`RunningWorld::stop`): its
    /// thread failed, or saves were lost.
    pub async fn shutdown(self) -> anyhow::Result<()> {
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
/// - a proxy in front of a loopback API (`public_api` set) that `trusted_proxies` doesn't list.
///
/// A loopback API without `public_api` is a server for this computer, and warns of nothing.
fn address_warnings(cfg: &Config, pin: Option<&str>) -> Vec<&'static str> {
    let s = &cfg.server;
    let mut warnings = Vec::new();
    if s.tls.is_none() && cfg.invite_base(pin).is_none() {
        warnings.push(crate::invite::NO_LINKS);
    }
    let loopback = s.api_bind.ip().to_canonical().is_loopback();
    if s.public_api.is_some() && loopback && s.trusted_proxies.is_empty() {
        warnings.push(PROXY_NOT_TRUSTED);
    }
    warnings
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
        .with_context(|| format!("can't listen on {}", cfg.server.login_bind))?;
    let login_addr = login.local_addr()?;
    let api = TcpListener::bind(cfg.server.api_bind)
        .await
        .with_context(|| format!("can't listen on {}", cfg.server.api_bind))?;
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
        listener,
        tasks,
    })
}

/// `fenji-server run`: serves until Ctrl-C or SIGTERM. A first run (M4 hosting spec §3.6) says what
/// it wrote before it starts, and what's next once it runs.
pub async fn run(loaded: Loaded) -> anyhow::Result<()> {
    let (cfg, first_run) = match loaded {
        Loaded::Existing(cfg) => (cfg, None),
        Loaded::FirstRun(cfg) => {
            let here = std::env::current_dir().unwrap_or_else(|_| ".".into());
            (cfg, Some(config::first_run_lines(&here)))
        }
    };
    if let Some([wrote, ..]) = &first_run {
        tracing::info!("{wrote}");
    }
    let server = start(&cfg, Arc::new(SystemClock), GateLimits::from_config(&cfg)).await?;
    let stop = signalled();
    tracing::info!(gate = %server.login_addr, api = %server.api_addr, "Fenji is running");
    if let Some([_, next @ ..]) = &first_run {
        for line in next {
            tracing::info!("{line}");
        }
    }
    server.serve_until(stop).await
}

/// Ctrl-C or SIGTERM. SIGTERM is caught from this call on, not from the future's first poll, so
/// that one sent as soon as the server says it runs stops it cleanly.
#[cfg(unix)]
fn signalled() -> impl Future<Output = ()> {
    let term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate());
    async move {
        match term {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
        tracing::info!("shutting down");
    }
}

/// Ctrl-C.
#[cfg(not(unix))]
async fn signalled() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutting down");
}

#[cfg(test)]
mod tests {
    use std::future;
    use std::path::Path;

    use fenji_store::{Character, NewCharacter, Place};
    use tokio::net::TcpStream;
    use tokio::sync::mpsc;

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
                [crate::invite::NO_LINKS],
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
