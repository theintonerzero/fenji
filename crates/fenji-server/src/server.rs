//! Running the server (M3 spec D5): one process with the game gate, the account API, and the
//! purge of expired sessions and tokens at start-up and every hour.
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use fenji_store::Store;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use crate::accounts::Accounts;
use crate::api::{AppState, ServerInfo, router};
use crate::clock::{Clock, SystemClock};
use crate::config::Config;
pub use crate::gate::GateLimits;
use crate::gate::{Gate, Places, accept_loop};
use crate::lobby::LobbyLimits;
use crate::online::Online;

/// The pinned client build this server speaks to (docs/re/pinned-build.md).
pub const PINNED_BUILD: &str = "10015727";

/// A started server: its bound addresses, and the tasks `shutdown` ends.
pub struct RunningServer {
    pub login_addr: SocketAddr,
    pub api_addr: SocketAddr,
    accounts: Arc<Accounts>,
    tasks: Vec<JoinHandle<()>>,
}

impl RunningServer {
    pub fn accounts(&self) -> Arc<Accounts> {
        Arc::clone(&self.accounts)
    }

    /// Ends the gate (and every connection), the API and the purge, then closes the store.
    pub async fn shutdown(self) {
        for t in &self.tasks {
            t.abort();
        }
        for t in self.tasks {
            let _ = t.await;
        }
        self.accounts.store().close().await;
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
    let accounts = Arc::new(Accounts::new(store, cfg.auth.clone(), Arc::clone(&clock)));
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
    });
    let app = router(AppState {
        accounts: Arc::clone(&accounts),
        info,
        trusted_proxies: cfg.server.trusted_proxies.clone(),
    });
    let purger = Arc::clone(&accounts);
    let tasks = vec![
        tokio::spawn(accept_loop(login, gate)),
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
        tasks,
    })
}

/// `fenji-server run`: serves until Ctrl-C or SIGTERM.
pub async fn run(cfg: Config) -> anyhow::Result<()> {
    let server = start(&cfg, Arc::new(SystemClock), GateLimits::from_config(&cfg)).await?;
    tracing::info!(gate = %server.login_addr, api = %server.api_addr, "Fenji is running");
    shutdown().await;
    server.shutdown().await;
    Ok(())
}

/// Ctrl-C, or SIGTERM where there is one.
async fn shutdown() {
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
