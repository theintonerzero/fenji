use std::io::{IsTerminal, Write};
use std::path::Path;
use std::process::ExitCode;

use clap::Parser;
use fenji_server::cli::{Cli, Command, admin};
use fenji_server::clock::SystemClock;
use fenji_server::config;
use fenji_server::console;
use fenji_server::console::log::Sink;

fn main() -> ExitCode {
    let ran = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(anyhow::Error::from)
        .and_then(|runtime| {
            let ran = runtime.block_on(fenji_server());
            // Without waiting for what's left: plain lines may leave a read of stdin that nothing
            // can cancel (`tokio::io::stdin`), and the server has stopped by now.
            runtime.shutdown_background();
            ran
        });
    let code = ran.unwrap_or_else(|e| {
        // Not as Rust's `main` would print it, through std's stderr, which the log's writer may
        // hold in a write nobody reads (#359).
        console::log::failed(&e);
        ExitCode::FAILURE
    });
    // The log's last lines on stderr, which a thread of its own writes: a while for them, and no
    // longer, as a stderr nobody reads would hold the exit for ever (#359).
    console::log::finish(fenji_server::server::CONSOLE_GRACE);
    code
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
            // Before anything is written in the data folder, the full-screen console's log file
            // first (the hosting plan's final review, Minor 1).
            fenji_server::folder::check(&config::data_folder(&loaded.config().store.path))?;
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
            fenji_server::server::run(loaded, given, front, hub).await?;
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
            let cfg = config::load_existing(given)?;
            console::attach::console(&cfg, run.as_deref(), cli.plain).await
        }
        cmd => {
            // The log goes to stderr, as systemd's journal keeps it.
            console::log::init(&[Sink::Stderr])?;
            let cfg = config::load_existing(given)?;
            let mut out = std::io::stdout().lock();
            admin(&cfg, given, &cmd, &mut out, &SystemClock).await?;
            Ok(ExitCode::SUCCESS)
        }
    }
}
