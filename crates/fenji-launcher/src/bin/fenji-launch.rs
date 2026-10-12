//! `fenji-launch`: the launcher's library from a terminal (M3 spec D14). It redeems invites,
//! verifies a client folder, writes manifests, and gives the sandboxed runner a ticket's login
//! environment. A password comes from a file or `FENJI_PASSWORD`, never from an argument.
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use fenji_launcher::api::ApiClient;
use fenji_launcher::launch::{LaunchSpec, resolve_ipv4};
use fenji_launcher::login_path::available;
use fenji_launcher::manifest::{HEADER, Manifest};
use fenji_launcher::verify::{Level, verify};
use fenji_launcher::{invite_link, manifest};

#[derive(Parser)]
#[command(
    name = "fenji-launch",
    version,
    about = "Fenji's launcher, from a terminal"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Creates an account with an invite code or link.
    Redeem {
        /// The server's address; optional when the invite is a link.
        #[arg(long)]
        server: Option<String>,
        /// The invite code, or the link that carries it.
        #[arg(long)]
        invite: String,
        #[arg(long)]
        username: String,
        /// A file holding the password (otherwise FENJI_PASSWORD).
        #[arg(long)]
        password_file: Option<PathBuf>,
    },
    /// Checks a client folder against the pinned build (or another manifest).
    Verify {
        #[arg(long)]
        client: PathBuf,
        /// Hash every file, not only the executables.
        #[arg(long)]
        full: bool,
        #[arg(long)]
        manifest: Option<PathBuf>,
    },
    /// Writes the manifest of a client folder.
    Manifest {
        #[arg(long)]
        client: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Signs in, takes a game token and signs out; with --print, prints the login path's
    /// environment as KEY=VALUE lines.
    Play {
        #[arg(long)]
        server: String,
        #[arg(long)]
        username: String,
        #[arg(long)]
        password_file: Option<PathBuf>,
        #[arg(long)]
        print: bool,
    },
}

fn password(file: Option<&Path>) -> anyhow::Result<String> {
    let p = match file {
        Some(f) => {
            std::fs::read_to_string(f).with_context(|| format!("reading {}", f.display()))?
        }
        None => std::env::var("FENJI_PASSWORD").map_err(|_| {
            anyhow::anyhow!("give the password with --password-file FILE or FENJI_PASSWORD")
        })?,
    };
    let p = p.trim_end_matches(['\r', '\n']).to_string();
    if p.is_empty() {
        bail!("the password is empty");
    }
    Ok(p)
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("fenji-launch: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    match cli.command {
        Command::Redeem {
            server,
            invite,
            username,
            password_file,
        } => {
            let (from_link, code) = invite_link::parse(&invite)
                .context("that isn't an invite code or link (XXXX-XXXX-XXXX-XXXX)")?;
            let server = server
                .or(from_link)
                .context("give the server's address with --server")?;
            let pw = password(password_file.as_deref())?;
            let api = ApiClient::new(&server)?;
            runtime()?.block_on(api.register(&code.display(), &username, &pw))?;
            println!("registered {username} on {server}");
            Ok(ExitCode::SUCCESS)
        }
        Command::Verify {
            client,
            full,
            manifest,
        } => {
            let m = match manifest {
                Some(f) => manifest::Manifest::parse(
                    &std::fs::read_to_string(&f)
                        .with_context(|| format!("reading {}", f.display()))?,
                )?,
                None => Manifest::pinned(),
            };
            let level = if full { Level::Full } else { Level::Quick };
            let r = verify(&client, &m, level)?;
            println!(
                "{} files checked ({}): {} missing, {} of the wrong size, {} with the wrong content",
                m.entries.len(),
                if full { "full" } else { "quick" },
                r.missing.len(),
                r.wrong_size.len(),
                r.wrong_hash.len()
            );
            for (what, list) in [
                ("missing", &r.missing),
                ("wrong size", &r.wrong_size),
                ("wrong content", &r.wrong_hash),
            ] {
                for p in list {
                    println!("{what}: {p}");
                }
            }
            Ok(if r.ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
        Command::Manifest { client, out } => {
            let m = Manifest::generate(&client)?;
            std::fs::write(&out, format!("# {HEADER}\n{}", m.to_tsv()))
                .with_context(|| format!("writing {}", out.display()))?;
            eprintln!("{} files", m.entries.len());
            Ok(ExitCode::SUCCESS)
        }
        Command::Play {
            server,
            username,
            password_file,
            print,
        } => {
            let paths = available();
            let Some(path) = paths.first() else {
                bail!("this build has no login path");
            };
            if !print {
                eprintln!(
                    "M3 doesn't start the client from here (the sandboxed runner does); use --print \
                     to get the login environment"
                );
                return Ok(ExitCode::from(2));
            }
            let pw = password(password_file.as_deref())?;
            let api = ApiClient::new(&server)?;
            let rt = runtime()?;
            // The gate must be usable before any password is sent (M3 spec §4).
            let info = rt.block_on(api.server_info())?;
            let gate = resolve_ipv4(&info.gate_host, info.gate_port)?;
            let session = rt.block_on(api.sign_in(&username, &pw))?;
            let grant = rt.block_on(api.game_token(&session));
            // The ticket is all the client needs; the session ends here either way.
            if let Err(e) = rt.block_on(api.sign_out(session)) {
                eprintln!("fenji-launch: signing out: {e}");
            }
            let grant = grant?;
            if (grant.gate_host.as_str(), grant.gate_port)
                != (info.gate_host.as_str(), info.gate_port)
            {
                bail!("the server named one gate in its info and another in the game token");
            }
            let mut spec = LaunchSpec {
                program: PathBuf::new(),
                args: Vec::new(),
                env: Vec::new(),
                cwd: PathBuf::new(),
            };
            path.apply(&grant, gate, &mut spec);
            for (k, v) in spec.env.iter().filter(|(k, _)| k.starts_with("gj_")) {
                println!("{k}={v}");
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn runtime() -> anyhow::Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?)
}
