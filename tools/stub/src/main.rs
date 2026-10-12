use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use tokio::net::TcpListener;
use tokio::task::JoinSet;
use tracing_subscriber::EnvFilter;

/// Accepts game-client connections and logs everything received; with --speak, also speaks the transport.
#[derive(Parser)]
#[command(version)]
struct Args {
    /// Address to listen on; repeat for several ports (e.g. --listen 127.0.0.1:8888 --listen 127.0.0.1:8889).
    #[arg(long = "listen", required = true)]
    listen: Vec<SocketAddr>,
    /// Directory for per-connection logs (default: re/stub-logs in the git checkout above the
    /// current directory; keep it under the git-ignored re/).
    #[arg(long)]
    out: Option<PathBuf>,
    /// Speak the client's transport: send the hello, read the auth, reply (M2 spec §5)
    #[arg(long)]
    speak: bool,
    /// Variant for server-to-client frames (--speak); only 3, AES-128, is implemented
    #[arg(long, default_value_t = fenji_proto::variant::AES128)]
    down_variant: u32,
    /// Variant for client-to-server frames (--speak)
    #[arg(long, default_value_t = fenji_proto::variant::AES128)]
    up_variant: u32,
    /// Reply to the auth message: ok or reject (each in a stored frame), or none
    #[arg(long, default_value = "ok", value_parser = ["ok", "reject", "none"])]
    auth_reply: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let args = Args::parse();
    let out = match args.out {
        Some(out) => out,
        None => fenji_stub::default_out_dir(&std::env::current_dir()?)
            .context("no git checkout above the current directory: pass --out")?,
    };

    let mode = if args.speak {
        fenji_stub::server::Mode::Speak(fenji_stub::speak::SpeakOpts {
            variants: fenji_proto::hello::Variants {
                down: args.down_variant,
                up: args.up_variant,
            },
            auth_reply: match args.auth_reply.as_str() {
                "ok" => fenji_stub::speak::AuthReply::Ok,
                "reject" => fenji_stub::speak::AuthReply::Reject,
                _ => fenji_stub::speak::AuthReply::None,
            },
        })
    } else {
        fenji_stub::server::Mode::Listen
    };

    let mut servers = JoinSet::new();
    for addr in args.listen {
        let listener = TcpListener::bind(addr)
            .await
            .with_context(|| format!("cannot listen on {addr}"))?;
        tracing::info!(addr = %listener.local_addr()?, out = %out.display(), "listening");
        servers.spawn(fenji_stub::server::serve(
            listener,
            out.clone(),
            mode.clone(),
        ));
    }

    tokio::select! {
        _ = tokio::signal::ctrl_c() => tracing::info!("shutting down"),
        Some(result) = servers.join_next() => result??,
    }
    Ok(())
}
