//! luatool: the Lua scripts embedded in the pinned SoLO client (M1 spec §3). Writes only under re/.
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
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
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    // Check the output directory before the 50 MB exe is read.
    let mut out = match &cli.command {
        Command::List { .. } => None,
        Command::Extract { out, .. } | Command::Disasm { out, .. } => Some(Out::new(out)?),
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
        _ => unreachable!("every command that writes has an output directory"),
    };
    if let Some(o) = &out {
        summary.write_failures(o, name)?;
        eprintln!("output: {}", o.root().display());
    }
    eprintln!("{}", summary.line(name));
    Ok(())
}
