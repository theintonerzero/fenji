//! The command line (M3 spec §4 `cli`): `run`, the default, and the admin's subcommands for invites
//! and accounts. Admin commands open the same database as a running server; SQLite's WAL and busy
//! timeout let both write.
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand};
use fenji_auth::invite::InviteCode;
use fenji_store::Store;

use crate::clock::Clock;
use crate::config::Config;

/// Fenji's server: the game gate, the account API and the admin commands.
#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    /// The configuration file (fenji.example.toml shows every key)
    #[arg(long, default_value = "fenji.toml")]
    pub config: PathBuf,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    /// Run the server (the default)
    Run,
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

/// `30m`, `12h` or `7d`.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let bad = || format!("{s:?}: a number above 0, then m, h or d");
    let (n, secs) = if let Some(n) = s.strip_suffix('m') {
        (n, 60)
    } else if let Some(n) = s.strip_suffix('h') {
        (n, 3600)
    } else if let Some(n) = s.strip_suffix('d') {
        (n, 86_400)
    } else {
        return Err(bad());
    };
    let n: u64 = n
        .bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| n.parse().ok())
        .flatten()
        .filter(|&n| n > 0)
        .ok_or_else(bad)?;
    Ok(Duration::from_secs(n.saturating_mul(secs)))
}

fn time(ms: i64) -> String {
    let secs = ms / 1000;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02} UTC",
        rem / 3600,
        rem % 3600 / 60
    )
}

/// Runs one admin command against the configured database, writing plain lines to `out`.
pub async fn admin(
    cfg: &Config,
    cmd: &Command,
    out: &mut dyn Write,
    clock: &dyn Clock,
) -> anyhow::Result<()> {
    let store = Store::open(&cfg.store.path).await?;
    let now = clock.now_ms();
    match cmd {
        Command::Run => anyhow::bail!("`run` isn't an admin command"),
        Command::Invite {
            cmd: InviteCmd::Create { label, expires },
        } => {
            let code = InviteCode::generate()?;
            let expires_at = expires
                .map(|d| now.saturating_add(i64::try_from(d.as_millis()).unwrap_or(i64::MAX)));
            store
                .create_invite(&code.hash(), label.as_deref(), now, expires_at)
                .await?;
            writeln!(out, "code: {}", code.display())?;
            writeln!(
                out,
                "link: https://{}/#invite={}",
                cfg.server.public_host,
                code.display()
            )?;
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
                writeln!(
                    out,
                    "{}\t{}\t{}\t{status}",
                    i.id,
                    i.label.as_deref().unwrap_or("-"),
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
            } else {
                writeln!(out, "no account named {username}")?;
            }
        }
    }
    store.close().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{parse_duration, time};

    #[test]
    fn durations_take_minutes_hours_and_days_and_refuse_the_rest() {
        assert_eq!(parse_duration("90m"), Ok(Duration::from_secs(5400)));
        assert_eq!(parse_duration("2h"), Ok(Duration::from_secs(7200)));
        assert_eq!(parse_duration("7d"), Ok(Duration::from_secs(604_800)));
        for bad in ["7é", "é", "", "m", "0m", "0d", "-1h", "1w", "1.5h", "h1"] {
            assert!(parse_duration(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn times_print_as_utc_dates() {
        assert_eq!(time(0), "1970-01-01 00:00 UTC");
        assert_eq!(time(1_800_000_000_000), "2027-01-15 08:00 UTC");
    }
}
