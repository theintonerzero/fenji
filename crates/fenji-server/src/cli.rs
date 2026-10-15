//! The command line (M3 spec §4 `cli`): `run`, the default; `init`, which writes a config to edit
//! (M4 hosting spec §3.6); and the admin's subcommands for invites, accounts and backups (§3.12).
//! Admin commands open the same database as a running server; SQLite's WAL and busy timeout let
//! both write.
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand};

use crate::clock::{Clock, Utc};
use crate::config::{Config, Tls};
use crate::text::escape_hidden;

/// `--expires`'s durations, which the console's `maintenance on` takes too.
pub use crate::commands::parse_duration;

/// Fenji's server: the game gate, the account API and the admin commands.
#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    /// The configuration file (fenji.example.toml shows every key); without it, fenji.toml in the
    /// working directory, which the server writes on its first run
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    /// Run the server (the default)
    Run,
    /// Write a configuration file to edit, at --config's path or as fenji.toml here; never over one
    Init,
    /// Create, list and revoke invites
    Invite {
        #[command(subcommand)]
        cmd: InviteCmd,
    },
    /// List and delete accounts
    Account {
        #[command(subcommand)]
        cmd: AccountCmd,
    },
    /// Copy the database into the backups folder beside it; safe while the server runs
    Backup {
        /// How many backups to keep, this one among them; the oldest beyond go
        #[arg(long, default_value_t = crate::backup::DEFAULT_KEEP, value_parser = parse_keep)]
        keep: usize,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub enum InviteCmd {
    /// Create a one-time invite; its code is shown once
    Create {
        /// Who it's for (shown in `invite list`)
        #[arg(long)]
        label: Option<String>,
        /// How long it stays valid: 30m, 12h or 7d
        #[arg(long, value_parser = parse_duration)]
        expires: Option<Duration>,
    },
    /// List invites (never their codes)
    List,
    /// Revoke an unused invite by its id from `invite list`
    Revoke { id: i64 },
}

#[derive(Debug, Clone, Subcommand)]
pub enum AccountCmd {
    /// List accounts
    List,
    /// Delete an account with its characters (PRIVACY.md, "Deleting your data")
    Delete { username: String },
}

/// `--keep`: a number above 0. Some tools take 0 to mean "keep them all"; here it would delete
/// every older backup.
fn parse_keep(s: &str) -> Result<usize, String> {
    s.bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| s.parse().ok())
        .flatten()
        .filter(|&n| n > 0)
        .ok_or_else(|| format!("{s:?}: a number above 0"))
}

fn time(ms: i64) -> String {
    let t = Utc::from_ms(ms);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02} UTC",
        t.year, t.month, t.day, t.hour, t.minute
    )
}

/// Runs one admin command against the configured database, writing plain lines to `out`.
pub async fn admin(
    cfg: &Config,
    cmd: &Command,
    out: &mut dyn Write,
    clock: &dyn Clock,
) -> anyhow::Result<()> {
    let store = crate::server::open_store(cfg).await?;
    let now = clock.now_ms();
    match cmd {
        Command::Run => anyhow::bail!("`run` isn't an admin command"),
        Command::Init => anyhow::bail!("`init` isn't an admin command"),
        Command::Invite {
            cmd: InviteCmd::Create { label, expires },
        } => {
            let expires_at = expires
                .map(|d| now.saturating_add(i64::try_from(d.as_millis()).unwrap_or(i64::MAX)));
            // The pin from the certificate alone, and no key made here: this may run as another
            // user than the server, which couldn't then read a key made here.
            let tls = crate::tls::folder(&cfg.store.path);
            let own_key = cfg.server.tls == Some(Tls::OwnKey);
            let pin = if own_key {
                crate::tls::pin_in(&tls)?
            } else {
                None
            };
            let api_port = cfg.server.api_bind.port();
            let bases = crate::invite::bases(cfg, pin.as_deref(), api_port);
            let made =
                crate::invite::create(&store, None, label.as_deref(), now, expires_at).await?;
            for (label, base) in &bases {
                let link = base.invite_link(&made.code);
                writeln!(out, "link ({}): {link}", escape_hidden(label))?;
            }
            writeln!(out, "code: {}", made.code)?;
            let s = &cfg.server;
            if own_key && pin.is_none() {
                writeln!(
                    out,
                    "link: none yet; the server makes its key in {} when it first starts with \
                     tls = \"self\", and links carry the key's pin",
                    tls.display()
                )?;
            } else if bases.is_empty() {
                writeln!(out, "link: none; {}", crate::invite::NO_LINKS)?;
            } else if !own_key && cfg.invite_base(None).is_none() {
                // Links for this computer alone, from an API that listens beyond it.
                writeln!(out, "other computers: {}", crate::invite::NO_LINKS)?;
            } else if own_key
                && s.public_api.is_none()
                && s.public_host.is_none()
                && !s.api_bind.ip().to_canonical().is_loopback()
            {
                // A link for each of the machine's addresses (HA4), and how to go beyond them.
                writeln!(
                    out,
                    "friends over the internet: open ports {} and {api_port} on your router, or \
                     use a VPN: see docs/hosting.md",
                    s.login_bind.port()
                )?;
            }
            if let Some(at) = expires_at {
                writeln!(out, "expires: {}", time(at))?;
            }
        }
        Command::Invite {
            cmd: InviteCmd::List,
        } => {
            writeln!(out, "id\tlabel\tcreated\tstatus")?;
            for i in store.list_invites().await? {
                let status = match (&i.redeemed_at, &i.revoked_at, i.expires_at) {
                    (Some(_), _, _) => match &i.redeemed_by {
                        Some(name) => format!("redeemed by {name}"),
                        None => "redeemed (account deleted)".into(),
                    },
                    (None, Some(_), _) => "revoked".into(),
                    (None, None, Some(at)) if at <= now => "expired".into(),
                    _ => "open".into(),
                };
                // Labels from M3 were never checked, so one may hold hidden characters.
                writeln!(
                    out,
                    "{}\t{}\t{}\t{status}",
                    i.id,
                    i.label.as_deref().map_or_else(|| "-".into(), escape_hidden),
                    time(i.created_at)
                )?;
            }
        }
        Command::Invite {
            cmd: InviteCmd::Revoke { id },
        } => {
            if store.revoke_invite(*id, now).await? {
                writeln!(out, "revoked invite {id}")?;
            } else {
                writeln!(
                    out,
                    "nothing to revoke: invite {id} is unknown, used or revoked"
                )?;
            }
        }
        Command::Account {
            cmd: AccountCmd::List,
        } => {
            writeln!(out, "id\tusername\tcreated\tlast sign-in\tcharacters")?;
            for a in store.list_accounts().await? {
                writeln!(
                    out,
                    "{}\t{}\t{}\t{}\t{}",
                    a.id,
                    a.username,
                    time(a.created_at),
                    a.last_sign_in_at.map_or_else(|| "-".into(), time),
                    a.characters
                )?;
            }
        }
        Command::Account {
            cmd: AccountCmd::Delete { username },
        } => {
            if store.delete_account(username).await? {
                writeln!(out, "deleted {username}, with its characters")?;
                if !store.checkpoint().await? {
                    writeln!(
                        out,
                        "the database's write-ahead log still holds older copies, as the server had \
                         it in use; they go when SQLite next restarts the log, or when the server stops"
                    )?;
                }
            } else {
                writeln!(out, "no account named {username}")?;
            }
        }
        Command::Backup { keep } => {
            let dir = crate::backup::folder(&cfg.store.path);
            let made = crate::backup::run(&store, &dir, *keep, now).await?;
            writeln!(out, "{}", made.display())?;
        }
    }
    store.close().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Command, time};

    #[test]
    fn config_is_optional_and_goes_before_or_after_the_command() {
        // The spec writes `init [--config <path>]`; the missing-config hint, `--config <path> init`.
        for args in [
            ["fenji-server", "init", "--config", "x.toml"],
            ["fenji-server", "--config", "x.toml", "init"],
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            assert_eq!(
                cli.config.as_deref(),
                Some(std::path::Path::new("x.toml")),
                "{args:?}"
            );
            assert!(matches!(cli.command, Some(Command::Init)), "{args:?}");
        }
        let cli = Cli::try_parse_from(["fenji-server"]).unwrap();
        assert!(cli.config.is_none() && cli.command.is_none());
    }

    #[test]
    fn backup_keeps_14_unless_told_and_never_none() {
        let keep = |args: &[&str]| match Cli::try_parse_from(args).map(|c| c.command) {
            Ok(Some(Command::Backup { keep })) => Some(keep),
            _ => None,
        };
        assert_eq!(keep(&["fenji-server", "backup"]), Some(14));
        assert_eq!(keep(&["fenji-server", "backup", "--keep", "3"]), Some(3));
        for bad in ["0", "-1", "+3", "x", ""] {
            assert_eq!(
                keep(&["fenji-server", "backup", "--keep", bad]),
                None,
                "{bad:?}"
            );
        }
    }

    #[test]
    fn times_print_as_utc_dates() {
        assert_eq!(time(0), "1970-01-01 00:00 UTC");
        assert_eq!(time(1_800_000_000_000), "2027-01-15 08:00 UTC");
    }
}
