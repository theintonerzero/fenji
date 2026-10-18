//! translate: the retranslation's tools (retranslation spec §6.5), on the pinned copy of the game.
//! Everything it writes goes under `re/retranslation/` (ground rule 3).
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use fenji_data::client::GameCopy;
use fenji_translate::{batch, drive, extract, glossary, lines};

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
    /// The glossary's candidates, and its export
    Glossary {
        #[command(subcommand)]
        command: GlossaryCommand,
    },
    /// The lines in batches by context, each with its glossary's terms: OUT/batches/<area>-<n>.json, replacing any plan there
    Batch,
    /// Every batch through its translation, review and settling, in headless Claude Code runs; it resumes where it stopped, and starts no job while OUT/PAUSE is there
    Drive {
        /// The Claude Code CLI
        #[arg(long, default_value = "claude")]
        claude: PathBuf,
        #[arg(long, default_value = drive::DEFAULT_MODEL)]
        model: String,
        #[arg(long, default_value = drive::DEFAULT_EFFORT)]
        effort: String,
        /// Runs at once, two at most
        #[arg(long, default_value_t = drive::MAX_RUNS)]
        runs: usize,
        /// The folder the runs start in, outside any checkout (default: fenji-translate-runs in the system's temporary folder)
        #[arg(long)]
        cwd: Option<PathBuf>,
        /// Tries after the first, before what fails is queued
        #[arg(long, default_value_t = 2)]
        retries: u32,
        /// Only this batch, by its id
        #[arg(long)]
        only: Option<String>,
    },
    /// Where the work stands: batches translated, reviewed, settled and queued, and whether the driver is waiting out a usage limit
    Status,
}

#[derive(Subcommand)]
enum GlossaryCommand {
    /// The names and recurring words of OUT/lines.jsonl, most used first: OUT/candidates.tsv
    Candidates,
    /// OUT/glossary.json as fenji-content's glossary.tsv, which holds no Chinese: OUT/glossary.tsv
    Export,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let repo = fenji_data::re_dir::find_repo_root(&std::env::current_dir()?)?;
    let work = fenji_data::re_dir::resolve_out_dir(&repo, &cli.out)?;
    match cli.command {
        Command::Extract => {
            let client = match cli.client {
                Some(dir) => dir,
                None => PathBuf::from(
                    std::env::var_os("FENJI_CLIENT_PATH")
                        .context("pass --client, or set FENJI_CLIENT_PATH to the pinned copy")?,
                ),
            };
            let exe_path = client.join("bin64").join("gujianol.exe");
            let exe = fenji_luatool::client::load(&exe_path, cli.any_build)?;
            let copy = GameCopy::open(&client).with_context(|| client.display().to_string())?;
            let found = extract::extract(&copy, &exe)?;
            lines::write_lines(&work, &found.lines)?;
            let report = extract::report_text(&found);
            std::fs::write(work.join("report.txt"), &report)?;
            print!("{report}");
        }
        Command::Glossary {
            command: GlossaryCommand::Candidates,
        } => {
            let found = glossary::candidates(&lines::read_lines(&work)?);
            std::fs::write(
                work.join(glossary::CANDIDATES),
                glossary::candidates_tsv(&found),
            )?;
            let names = found
                .iter()
                .filter(|c| c.kind == fenji_translate::check::TermKind::Name)
                .count();
            println!(
                "{} candidates: {names} names, {} words",
                found.len(),
                found.len() - names
            );
        }
        Command::Glossary {
            command: GlossaryCommand::Export,
        } => {
            let out = glossary::export(&glossary::load(&work)?)?;
            std::fs::write(work.join("glossary.tsv"), &out)?;
            println!("{} terms", out.lines().count());
        }
        Command::Batch => {
            let all = lines::read_lines(&work)?;
            let rules = fenji_translate::check::Rules::load(&work, &all)?;
            let planned = batch::plan_batches(&all, &glossary::load(&work)?, &rules);
            batch::write_batches(&work, &planned)?;
            let mut by_area = std::collections::BTreeMap::<&str, usize>::new();
            for b in &planned {
                *by_area
                    .entry(b.id.split('-').next().unwrap_or(""))
                    .or_default() += 1;
            }
            println!("{} batches: {by_area:?}", planned.len());
        }
        Command::Drive {
            claude,
            model,
            effort,
            runs,
            cwd,
            retries,
            only,
        } => {
            let cwd = cwd.unwrap_or_else(|| std::env::temp_dir().join("fenji-translate-runs"));
            let driver = drive::Driver {
                claude,
                model,
                effort,
                runs,
                retries,
                only,
                ..drive::Driver::new(work, cwd)
            };
            println!("{:?}", driver.run()?);
        }
        Command::Status => println!("{:?}", drive::status(&work)?),
    }
    Ok(())
}
