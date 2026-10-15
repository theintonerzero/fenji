//! `fenji-launch`: the launcher's library from a terminal (M3 spec D14). It redeems invites,
//! verifies a client folder, writes manifests, starts the client on Linux (through umu-run), and
//! gives the sandboxed runner a ticket's login environment. A password comes from a file or
//! `FENJI_PASSWORD`, never from an argument.
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use fenji_launcher::api::{ApiClient, ApiError};
use fenji_launcher::manifest::{HEADER, Manifest};
use fenji_launcher::settings::Os;
use fenji_launcher::start::{self, Login, UmuOptions};
use fenji_launcher::verify::{Level, verify};
use fenji_launcher::{invite_link, manifest, settings};

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
    /// Starts the game on Linux: checks the client folder against the pinned build, gets the Wine
    /// prefix ready, signs in, takes a game token, signs out and starts the client with the token
    /// in its environment. With --print, prints the login path's environment as KEY=VALUE lines
    /// instead.
    Play {
        #[arg(long)]
        server: String,
        #[arg(long)]
        username: String,
        #[arg(long)]
        password_file: Option<PathBuf>,
        #[arg(long)]
        print: bool,
        /// The client folder, which holds bin64/ and data/.
        #[arg(long)]
        client: Option<PathBuf>,
        /// Linux: umu-run (by default the one on PATH).
        #[arg(long)]
        umu_run: Option<PathBuf>,
        /// Linux: the launcher's Wine prefix (by default fenji-launcher/prefix in the data folder).
        #[arg(long)]
        prefix: Option<PathBuf>,
        /// Linux: a Proton build for PROTONPATH (by default $PROTONPATH, else umu-run's choice).
        #[arg(long)]
        proton: Option<PathBuf>,
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
            runtime()?
                .block_on(api.register(&code.display(), &username, &pw))
                .map_err(refused_redeem)?;
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
            client,
            umu_run,
            prefix,
            proton,
        } => {
            // Before anything else, --print included (plan Amendment B; ground rule 6).
            let os = Os::this();
            start::check_os(os)?;
            let mut note = |m: &str| eprintln!("fenji-launch: {m}");
            if print {
                let pw = password(password_file.as_deref())?;
                let login = Login {
                    server: &server,
                    username: &username,
                    password: &pw,
                };
                for (k, v) in start::take_ticket(os, &login, || Ok(()), &mut note)? {
                    println!("{k}={v}");
                }
                return Ok(ExitCode::SUCCESS);
            }
            let Some(client) = client else {
                eprintln!("fenji-launch: give the client folder with --client (or use --print)");
                return Ok(ExitCode::from(2));
            };
            let cwd = std::env::current_dir().context("reading the current folder")?;
            let platform = start::linux_platform(
                UmuOptions {
                    umu_run,
                    prefix,
                    proton,
                },
                &cwd,
                std::env::var_os("PATH").as_deref(),
                settings::data_dir(),
            )?;
            let pw = password(password_file.as_deref())?;
            let login = Login {
                server: &server,
                username: &username,
                password: &pw,
            };
            // Only the pinned build starts (ground rule 1).
            let status = start::play(
                os,
                &client,
                &cwd,
                &Manifest::pinned(),
                &platform,
                &login,
                &mut note,
            )?;
            if !status.success() {
                note(&format!("the client ended: {status}"));
            }
            Ok(ExitCode::from(start::exit_byte(status)))
        }
    }
}

/// What `redeem` prints when the server refuses: the error, which carries the host's words for
/// maintenance and a ban. Maintenance refuses an invite before spending it (M4 hosting spec §3.9),
/// so the player also hears that it still works.
fn refused_redeem(e: ApiError) -> anyhow::Error {
    match e {
        ApiError::Maintenance { .. } => {
            anyhow::anyhow!("{e}; the invite wasn't used, so try it again once the server is back")
        }
        e => e.into(),
    }
}

fn runtime() -> anyhow::Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_redeem_held_by_maintenance_says_the_invite_still_works() {
        let held = ApiError::Maintenance {
            message: "Updating".into(),
            until: None,
        };
        assert_eq!(
            refused_redeem(held).to_string(),
            "the server is in maintenance: Updating; the invite wasn't used, so try it again \
             once the server is back"
        );
        let other = refused_redeem(ApiError::Insecure).to_string();
        assert_eq!(other, ApiError::Insecure.to_string());
    }
}
