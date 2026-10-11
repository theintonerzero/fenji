//! luatool: the Lua scripts embedded in the pinned SoLO client (M1 spec §3). Writes only under re/.
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use clap::{Parser, Subcommand};
use fenji_luatool::decompile::Unluac;
use fenji_luatool::{client, commands, output::Out};

/// The Lua scripts embedded in the pinned SoLO client. Writes only under re/.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// The client's gujianol.exe (default: $FENJI_CLIENT_PATH/bin64/gujianol.exe)
    #[arg(long, global = true)]
    exe: Option<PathBuf>,
    /// Read an exe that isn't the pinned build (docs/re/pinned-build.md)
    #[arg(long, global = true)]
    any_build: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List the embedded files as TSV on stdout: path, stored size, raw size, kind, status
    List {
        /// Only engine paths starting with this, such as data/interface/addons/Login/
        #[arg(long, default_value = "")]
        prefix: String,
    },
    /// Write the decoded files, as the client stores them, under OUT (inside re/)
    Extract {
        /// Only engine paths starting with this
        #[arg(long, default_value = "")]
        prefix: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Write a disassembly (<path>.dis) of each Lua chunk under OUT (inside re/)
    Disasm {
        /// Only engine paths starting with this
        #[arg(long, default_value = "")]
        prefix: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Write each Lua chunk normalised to stock Lua 5.3 under OUT (inside re/)
    Normalize {
        /// Only engine paths starting with this
        #[arg(long, default_value = "")]
        prefix: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Measure server-side logic: census.tsv and census-server-chunks.tsv under OUT (inside re/)
    Census {
        /// Only engine paths starting with this
        #[arg(long, default_value = "")]
        prefix: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Decompile each Lua chunk with unluac into OUT (inside re/)
    Decompile {
        /// Only engine paths starting with this
        #[arg(long, default_value = "")]
        prefix: String,
        #[arg(long)]
        out: PathBuf,
        /// The unluac jar (default: the pinned one that scripts/bootstrap.sh installs)
        #[arg(long)]
        jar: Option<PathBuf>,
        /// The java binary (default: $JAVA_HOME/bin/java, else java on PATH)
        #[arg(long)]
        java: Option<PathBuf>,
        /// Parallel unluac processes (default: the number of CPUs)
        #[arg(long)]
        jobs: Option<usize>,
        /// Seconds per chunk before unluac is stopped
        #[arg(long, default_value_t = 120)]
        timeout: u64,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    // Check the output directory before the 50 MB exe is read.
    let mut out = match &cli.command {
        Command::List { .. } => None,
        Command::Extract { out, .. }
        | Command::Disasm { out, .. }
        | Command::Normalize { out, .. }
        | Command::Decompile { out, .. }
        | Command::Census { out, .. } => Some(Out::new(out)?),
    };
    let image = client::load(&client::exe_path(cli.exe.clone())?, cli.any_build)?;
    let fs = fenji_data::ExeFs::scan(&image)?;
    let (name, summary) = match (&cli.command, out.as_mut()) {
        (Command::List { prefix }, _) => (
            "list",
            commands::list(&fs, prefix, &mut std::io::stdout().lock())?,
        ),
        (Command::Extract { prefix, .. }, Some(o)) => {
            ("extract", commands::extract(&fs, prefix, o)?)
        }
        (Command::Disasm { prefix, .. }, Some(o)) => ("disasm", commands::disasm(&fs, prefix, o)?),
        (Command::Normalize { prefix, .. }, Some(o)) => {
            ("normalize", commands::normalize(&fs, prefix, o)?)
        }
        (
            Command::Decompile {
                prefix,
                jar,
                java,
                jobs,
                timeout,
                ..
            },
            Some(o),
        ) => {
            let repo = fenji_data::re_dir::find_repo_root(&std::env::current_dir()?)?;
            let unluac = Unluac::locate(
                java.clone(),
                jar.clone(),
                &repo,
                Duration::from_secs(*timeout),
            )?;
            let workers =
                jobs.unwrap_or_else(|| std::thread::available_parallelism().map_or(4, |n| n.get()));
            (
                "decompile",
                commands::decompile(&fs, prefix, o, &unluac, workers)?,
            )
        }
        (Command::Census { prefix, .. }, Some(o)) => ("census", commands::census(&fs, prefix, o)?),
        _ => unreachable!("every command that writes has an output directory"),
    };
    if let Some(o) = &out {
        summary.write_failures(o, name)?;
        eprintln!("output: {}", o.root().display());
    }
    eprintln!("{}", summary.line(name));
    Ok(())
}
