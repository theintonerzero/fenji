//! translate: the retranslation's tools (retranslation spec §6.5), on the pinned copy of the game.
//! Everything it writes goes under `re/retranslation/` (ground rule 3).
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use fenji_data::client::GameCopy;
use fenji_translate::{extract, lines};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// The client copy (default: $FENJI_CLIENT_PATH)
    #[arg(long, global = true)]
    client: Option<PathBuf>,
    /// Read an exe that isn't the pinned build
    #[arg(long, global = true)]
    any_build: bool,
    /// The work folder, inside re/
    #[arg(long, global = true, default_value = "re/retranslation")]
    out: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Every source line of the copy, with its English today and where the game uses it: OUT/lines.jsonl and OUT/report.txt
    Extract,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let repo = fenji_data::re_dir::find_repo_root(&std::env::current_dir()?)?;
    let work = fenji_data::re_dir::resolve_out_dir(&repo, &cli.out)?;
    let client = match cli.client {
        Some(dir) => dir,
        None => PathBuf::from(
            std::env::var_os("FENJI_CLIENT_PATH")
                .context("pass --client, or set FENJI_CLIENT_PATH to the pinned copy")?,
        ),
    };
    match cli.command {
        Command::Extract => {
            let exe_path = client.join("bin64").join("gujianol.exe");
            let exe = fenji_luatool::client::load(&exe_path, cli.any_build)?;
            let copy = GameCopy::open(&client).with_context(|| client.display().to_string())?;
            let found = extract::extract(&copy, &exe)?;
            lines::write_lines(&work, &found.lines)?;
            let report = extract::report_text(&found);
            std::fs::write(work.join("report.txt"), &report)?;
            print!("{report}");
        }
    }
    Ok(())
}
