//! Running the server (M3 spec D5; M4 world spec §3.9): one process with the game gate, the world,
//! the account API, and the purge of expired sessions and tokens at start-up and every hour.
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use fenji_store::Store;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::{JoinHandle, JoinSet};

use crate::accounts::Accounts;
use crate::api::{AppState, ServerInfo, router};
use crate::clock::{Clock, SystemClock};
use crate::config::Config;
pub use crate::gate::GateLimits;
use crate::gate::{self, Gate, Places, accept_loop};
use crate::lobby::LobbyLimits;
use crate::online::Online;
use crate::world::{self, Rules, RunningWorld, TickReport, WorldHandle, WorldIn};

/// The pinned client build this server speaks to (docs/re/pinned-build.md).
pub const PINNED_BUILD: &str = "10015727";

/// A started server: its bound addresses, and the tasks `shutdown` ends.
pub struct RunningServer {
    pub login_addr: SocketAddr,
    pub api_addr: SocketAddr,
    accounts: Arc<Accounts>,
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

    /// The world's channel, as a connection task holds it.
    pub fn world(&self) -> WorldHandle {
        self.world.handle()
    }

    /// The world's tick times over the last minute.
    pub fn ticks(&self) -> TickReport {
        self.world.report()
    }

    /// Serves until `stop` completes, or until the world ends by itself, which only a tick that
    /// panicked does, then shuts down: what `fenji-server run` awaits. A world that ended takes
    /// the server with it, so that no login waits for a world that isn't there, and the error
    /// makes `fenji-server run` exit non-zero for systemd to restart it (Ruling R9).
    pub async fn serve_until(mut self, stop: impl Future<Output = ()>) -> anyhow::Result<()> {
        let failed = tokio::select! {
            () = stop => false,
            () = self.world.ended() => {
                tracing::error!("the world has stopped: stopping the server");
                true
            }
        };
        match self.shutdown().await {
            Ok(()) if failed => Err(anyhow::anyhow!("the world stopped by itself")),
            stopped => stopped,
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

/// Opens the store, binds both listeners (port 0 takes a free port) and starts serving.
pub async fn start(
    cfg: &Config,
    clock: Arc<dyn Clock>,
    limits: GateLimits,
) -> anyhow::Result<RunningServer> {
    let store = Store::open(&cfg.store.path)
        .await
        .with_context(|| format!("opening {}", cfg.store.path.display()))?;
    let world = world::start(
        Rules::from_config(cfg),
        store.clone(),
        cfg.invite_base(),
        Arc::clone(&clock),
    )?;
    let accounts = Arc::new(Accounts::new(store, cfg.auth.clone(), Arc::clone(&clock)));
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
    if cfg.server.api_bind.ip().is_loopback() && cfg.server.trusted_proxies.is_empty() {
        tracing::warn!(
            "api_bind is a loopback address and trusted_proxies is empty: behind a proxy on this \
             machine, every request comes from 127.0.0.1 and shares one address limit; list the \
             proxy in [server] trusted_proxies"
        );
    }
    let info = ServerInfo {
        name: cfg.server.name.clone(),
        version: env!("CARGO_PKG_VERSION").into(),
        pinned_build: PINNED_BUILD.into(),
        gate_host: cfg.server.public_host.clone(),
        gate_port: login_addr.port(),
        server_id: cfg.server.id,
    };
    let gate = Arc::new(Gate {
        accounts: Arc::clone(&accounts),
        online: Online::new(limits.max_players),
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
        admins: cfg
            .access
            .admins
            .iter()
            .map(|a| a.to_ascii_lowercase())
            .collect(),
        stopping: watch::Sender::new(false),
    });
    let app = router(AppState {
        accounts: Arc::clone(&accounts),
        info,
        trusted_proxies: cfg.server.trusted_proxies.clone(),
    });
    let purger = Arc::clone(&accounts);
    let listener = tokio::spawn(accept_loop(login, Arc::clone(&gate)));
    let tasks = vec![
        tokio::spawn(async move {
            let service = app.into_make_service_with_connect_info::<SocketAddr>();
            if let Err(e) = axum::serve(api, service).await {
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
        accounts,
        world,
        gate,
        listener,
        tasks,
    })
}

/// `fenji-server run`: serves until Ctrl-C or SIGTERM.
pub async fn run(cfg: Config) -> anyhow::Result<()> {
    let server = start(&cfg, Arc::new(SystemClock), GateLimits::from_config(&cfg)).await?;
    tracing::info!(gate = %server.login_addr, api = %server.api_addr, "Fenji is running");
    server.serve_until(signalled()).await
}

/// Ctrl-C, or SIGTERM where there is one.
async fn signalled() {
    #[cfg(unix)]
    {
        let mut term =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(t) => t,
                Err(_) => {
                    let _ = tokio::signal::ctrl_c().await;
                    return;
                }
            };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
    tracing::info!("shutting down");
}

#[cfg(test)]
mod tests {
    use std::future;

    use fenji_store::{NewCharacter, Place};
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

    #[tokio::test]
    async fn a_panicking_tick_saves_and_kicks_everyone_then_stops_the_server_with_an_error() {
        // Ruling R9 (#222): a bug that panics a tick mustn't leave a server that takes logins with
        // no world behind them. The world saves everyone it can and lets them go with the
        // shutdown's reason, and the server stops with an error, so `fenji-server run` exits
        // non-zero and systemd restarts it.
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::parse(
            "[server]\nname = \"Fenji\"\nlogin_bind = \"127.0.0.1:0\"\n\
             api_bind = \"127.0.0.1:0\"\n",
            dir.path(),
        )
        .unwrap();
        let clock = Arc::new(TestClock::at(NOW));
        let server = start(&cfg, clock, GateLimits::default()).await.unwrap();
        let store = server.accounts().store().clone();
        let mut players = Vec::new();
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
            players.push(store.create_character(account, &new, 9, NOW).await.unwrap());
        }
        let (login, world) = (server.login_addr, server.world());
        let served = tokio::spawn(server.serve_until(future::pending()));
        let mut outs = Vec::new();
        for (conn, c) in (1..).zip(&players) {
            let (out, mut rx) = mpsc::channel(8);
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
            assert!(matches!(next(&mut rx).await, WorldOut::LoadScene { .. }));
            outs.push(rx);
        }
        world.send(WorldIn::Panic("a bug in a tick"));
        for rx in &mut outs {
            assert_eq!(next(rx).await, WorldOut::Kicked(KickReason::Shutdown));
        }
        let stopped = tokio::time::timeout(Duration::from_secs(30), served)
            .await
            .expect("the server stops by itself")
            .unwrap();
        let e = stopped.expect_err("with an error");
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
        for c in &players {
            let place = store.character(c.account_id, c.pid).await.unwrap();
            assert_eq!(place.unwrap().place, Some(saved), "{}", c.name);
        }
    }
}
