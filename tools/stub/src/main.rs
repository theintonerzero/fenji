use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use tokio::net::TcpListener;
use tokio::task::JoinSet;
use tracing_subscriber::EnvFilter;

/// Accepts game-client connections on one or more addresses and hex-dumps everything received.
#[derive(Parser)]
#[command(version)]
struct Args {
    /// Address to listen on; repeat for several ports (e.g. --listen 127.0.0.1:8888 --listen 127.0.0.1:8889).
    #[arg(long = "listen", required = true)]
    listen: Vec<SocketAddr>,
    /// Directory for per-connection logs (keep it under the git-ignored re/).
    #[arg(long, default_value = "re/stub-logs")]
    out: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let args = Args::parse();

    let mut servers = JoinSet::new();
    for addr in args.listen {
        let listener = TcpListener::bind(addr)
            .await
            .with_context(|| format!("cannot listen on {addr}"))?;
        tracing::info!(addr = %listener.local_addr()?, out = %args.out.display(), "listening");
        servers.spawn(fenji_stub::server::serve(listener, args.out.clone()));
    }

    tokio::select! {
        _ = tokio::signal::ctrl_c() => tracing::info!("shutting down"),
        Some(result) = servers.join_next() => result??,
    }
    Ok(())
}
