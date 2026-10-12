//! Running the server (M3 spec D5): one process with the account API, and the purge of expired
//! sessions and tokens at start-up and every hour. The gate joins it in M3 Task 6.
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use fenji_store::Store;

use crate::accounts::Accounts;
use crate::api::{AppState, ServerInfo, router};
use crate::clock::SystemClock;
use crate::config::Config;

/// The pinned client build this server speaks to (docs/re/pinned-build.md).
pub const PINNED_BUILD: &str = "10015727";

pub async fn run(cfg: Config) -> anyhow::Result<()> {
    let store = Store::open(&cfg.store.path)
        .await
        .with_context(|| format!("opening {}", cfg.store.path.display()))?;
    let accounts = Arc::new(Accounts::new(
        store,
        cfg.auth.clone(),
        Arc::new(SystemClock),
    ));
    let info = ServerInfo {
        name: cfg.server.name.clone(),
        version: env!("CARGO_PKG_VERSION").into(),
        pinned_build: PINNED_BUILD.into(),
        gate_host: cfg.server.public_host.clone(),
        gate_port: cfg.server.login_bind.port(),
        server_id: cfg.server.id,
    };
    if cfg.server.api_bind.ip().is_loopback() && cfg.server.trusted_proxies.is_empty() {
        tracing::warn!(
            "api_bind is a loopback address and trusted_proxies is empty: behind a proxy on this \
             machine, every request comes from 127.0.0.1 and shares one address limit; list the \
             proxy in [server] trusted_proxies"
        );
    }
    let purger = Arc::clone(&accounts);
    tokio::spawn(async move {
        let mut every = tokio::time::interval(Duration::from_secs(3600));
        loop {
            every.tick().await;
            match purger.purge().await {
                Ok(n) => tracing::debug!(n, "purged expired sessions and tokens"),
                Err(e) => tracing::warn!(error = %e, "purge"),
            }
        }
    });
    let app = router(AppState {
        accounts,
        info,
        trusted_proxies: cfg.server.trusted_proxies.clone(),
    });
    let listener = tokio::net::TcpListener::bind(cfg.server.api_bind)
        .await
        .with_context(|| format!("can't listen on {}", cfg.server.api_bind))?;
    tracing::info!(api = %listener.local_addr()?, "account API listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown())
    .await?;
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
