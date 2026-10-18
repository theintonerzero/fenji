//! translate: the retranslation's tools (retranslation spec §6.5), on the pinned copy of the game.
//! Everything it writes goes under `re/retranslation/` (ground rule 3).
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use fenji_data::client::GameCopy;
use fenji_translate::{assemble, batch, drive, early, extract, glossary, lines};

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
    /// The new English into a fenji-content checkout, by area, with the glossary and the style guide; what it covers goes in OUT/coverage.txt
    Assemble {
        /// The fenji-content checkout, outside this one
        #[arg(long)]
        to: PathBuf,
    },
    /// The early check's loader, for a sandboxed run only: new English into a run workspace's copy, and back out (docs/re/retranslation-locale.md §4)
    Early {
        #[command(subcommand)]
        command: EarlyCommand,
    },
}

#[derive(Subcommand)]
enum EarlyCommand {
    /// Loads a file's lines (fingerprint, tab, English) into the copy: a patch part, and one more _index line; the copy's own _index waits in OUT/early/ until revert
    Apply {
        /// The run workspace's copy, inside re/
        #[arg(long, default_value = "re/runs/client")]
        copy: PathBuf,
        /// The lines' new English, as fenji-content's files hold it
        #[arg(long)]
        english: PathBuf,
    },
    /// Puts the copy's own _index back, and takes the patch part out
    Revert {
        /// The run workspace's copy, inside re/
        #[arg(long, default_value = "re/runs/client")]
        copy: PathBuf,
    },
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
        Command::Status => {
            println!("{:?}", drive::status(&work)?);
            let used = drive::usage(&work)?;
            println!(
                "{} runs: {} tokens in, {} out, ${:.2} by the CLI's reckoning",
                used.runs, used.input_tokens, used.output_tokens, used.cost_usd
            );
        }
        Command::Assemble { to } => {
            let all = lines::read_lines(&work)?;
            let coverage = assemble::assemble(&work, &all, &to)?;
            println!(
                "{} lines, {} translated, {} missing: {}",
                coverage.lines,
                coverage.translated,
                coverage.missing.len(),
                work.join(assemble::COVERAGE).display()
            );
        }
        Command::Early { command } => {
            let pinned = cli
                .client
                .or_else(|| std::env::var_os("FENJI_CLIENT_PATH").map(PathBuf::from));
            match command {
                EarlyCommand::Apply { copy, english } => {
                    let copy = early::run_copy(&repo, &copy, pinned.as_deref())?;
                    let text = std::fs::read_to_string(&english)
                        .with_context(|| english.display().to_string())?;
                    let english: std::collections::HashMap<_, _> = fenji_pack::read_tsv(&text)?
                        .into_iter()
                        .map(|e| (e.fingerprint, e.english))
                        .collect();
                    let applied = early::apply(&copy, &work, &english, &[])?;
                    let key: String = applied.key.iter().map(|b| format!("{b:02x}")).collect();
                    println!(
                        "{} lines given new English, of {} asked for; the cache's new key {key}",
                        applied.replaced,
                        english.len()
                    );
                }
                EarlyCommand::Revert { copy } => {
                    let copy = early::run_copy(&repo, &copy, pinned.as_deref())?;
                    if early::revert(&copy, &work)? {
                        println!("the copy's own _index put back, and the patch part taken out");
                    } else {
                        println!("nothing to put back");
                    }
                }
            }
        }
    }
    Ok(())
}
