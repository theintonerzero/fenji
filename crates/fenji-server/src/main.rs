use std::io::{IsTerminal, Write};
use std::path::Path;
use std::process::ExitCode;

use clap::Parser;
use fenji_server::cli::{Cli, Command, admin};
use fenji_server::clock::SystemClock;
use fenji_server::config::{self, Config};
use fenji_server::console;
use fenji_server::console::log::Sink;

fn main() -> anyhow::Result<ExitCode> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let ran = runtime.block_on(fenji_server());
    // Without waiting for what's left: plain lines may leave a read of stdin that nothing can
    // cancel (`tokio::io::stdin`), and the server has stopped by now.
    runtime.shutdown_background();
    ran
}

async fn fenji_server() -> anyhow::Result<ExitCode> {
    let cli = Cli::parse();
    let given = cli.config.as_deref();
    let path = given.unwrap_or(Path::new(config::DEFAULT_PATH));
    match cli.command.unwrap_or(Command::Run) {
        // Only `run` makes a first run (M4 hosting spec §3.6); the admin commands need a config
        // that's there.
        Command::Run => {
            let loaded = config::load_or_first_run(given)?;
            // The terminal it started in decides the console, and the console where the log goes
            // (§3.1, §3.4): the process's log is set once, so only now.
            let term = std::env::var("TERM").ok();
            let front = console::front(
                std::io::stdin().is_terminal(),
                std::io::stdout().is_terminal(),
                term.as_deref(),
                cli.plain,
            );
            let hub = console::log::init(&console::sinks(front, &loaded.config().store.path))?;
            fenji_server::server::run(loaded, front, hub).await?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Init => {
            config::init(path)?;
            writeln!(std::io::stdout(), "wrote {}", path.display())?;
            Ok(ExitCode::SUCCESS)
        }
        // Attaches to the server that runs with this config (§3.5), which must be there; the
        // server's log is what it shows.
        Command::Console { run } => {
            let cfg = Config::load(path)?;
            console::attach::console(&cfg, run.as_deref(), cli.plain).await
        }
        cmd => {
            // The log goes to stderr, as systemd's journal keeps it.
            console::log::init(&[Sink::Stderr])?;
            let cfg = Config::load(path)?;
            let mut out = std::io::stdout().lock();
            admin(&cfg, &cmd, &mut out, &SystemClock).await?;
            Ok(ExitCode::SUCCESS)
        }
    }
}
