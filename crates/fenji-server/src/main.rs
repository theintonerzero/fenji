use clap::Parser;
use fenji_server::cli::{Cli, Command, admin};
use fenji_server::clock::SystemClock;
use fenji_server::config::Config;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    let cfg = Config::load(&cli.config)?;
    match cli.command.unwrap_or(Command::Run) {
        Command::Run => fenji_server::server::run(cfg).await,
        cmd => {
            let mut out = std::io::stdout().lock();
            admin(&cfg, &cmd, &mut out, &SystemClock).await
        }
    }
}
