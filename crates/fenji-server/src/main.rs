use std::io::Write;
use std::path::Path;

use clap::Parser;
use fenji_server::cli::{Cli, Command, admin};
use fenji_server::clock::SystemClock;
use fenji_server::config::{self, Config};
use fenji_server::console;
use fenji_server::console::log::Sink;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // The log goes to stderr, which systemd's journal keeps (M4 hosting spec §3.4).
    console::log::init(&[Sink::Stderr])?;
    let cli = Cli::parse();
    let given = cli.config.as_deref();
    let path = given.unwrap_or(Path::new(config::DEFAULT_PATH));
    match cli.command.unwrap_or(Command::Run) {
        // Only `run` makes a first run (M4 hosting spec §3.6); the admin commands need a config
        // that's there.
        Command::Run => fenji_server::server::run(config::load_or_first_run(given)?).await,
        Command::Init => {
            config::init(path)?;
            writeln!(std::io::stdout(), "wrote {}", path.display())?;
            Ok(())
        }
        cmd => {
            let cfg = Config::load(path)?;
            let mut out = std::io::stdout().lock();
            admin(&cfg, &cmd, &mut out, &SystemClock).await
        }
    }
}
