//! The command line (M3 spec §4 `cli`): `run`, the default; `init`, which writes a config to edit
//! (M4 hosting spec §3.6); `console`, which attaches to a running server (§3.5); and the admin's
//! subcommands for invites, accounts and backups (§3.12).
//! Admin commands open the same database as a running server; SQLite's WAL and busy timeout let
//! both write. The host console's `invite`, `invites`, `accounts` and `revoke` share their code
//! here (§3.3).
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand};
use fenji_store::Store;

use crate::clock::{Clock, Utc};
use crate::config::{Config, Tls};
use crate::invite::{InviteBase, server_link};
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
    /// The console as plain lines, in a terminal too: commands from stdin, answers on stdout and
    /// the log on stderr, for screen readers and simple terminals
    #[arg(long, global = true)]
    pub plain: bool,
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
    /// Attach to the running server's console (Linux and macOS): Ctrl-C, exit or quit leaves it,
    /// and the server runs on
    Console {
        /// Run one command, print its answer and exit: 0 if it ran, 1 if not
        #[arg(long, value_name = "COMMAND")]
        run: Option<String>,
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

/// A moment as the CLI and the console show it: `YYYY-MM-DD HH:MM UTC`.
pub(crate) fn time(ms: i64) -> String {
    let t = Utc::from_ms(ms);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02} UTC",
        t.year, t.month, t.day, t.hour, t.minute
    )
}

/// `lasts` after `now` (Unix ms), as `--expires` and `maintenance on` compute it: a duration too
/// long for the clock ends at its last moment.
pub(crate) fn ms_after(now: i64, lasts: Duration) -> i64 {
    now.saturating_add(i64::try_from(lasts.as_millis()).unwrap_or(i64::MAX))
}

/// What `invite create` and the console's `invite` say of a new invite with `code`: a link for each
/// of `bases`, each with the server's own link beside it, the code, and what links need where
/// there are none, or how they go further ([`link_hint`]). `pin` is the server's own key's, and
/// `ports` the gate's and the API's.
pub(crate) fn invite_lines(
    cfg: &Config,
    pin: Option<&str>,
    ports: [u16; 2],
    bases: &[(String, InviteBase)],
    code: &str,
    expires_at: Option<i64>,
) -> Vec<String> {
    let mut lines = Vec::new();
    for (label, base) in bases {
        let link = base.invite_link(code);
        lines.push(format!("link ({}): {link}", escape_hidden(label)));
        lines.push(server_link_line(label, base));
    }
    lines.push(format!("code: {code}"));
    lines.extend(link_hint(cfg, pin, ports, bases));
    if let Some(at) = expires_at {
        lines.push(format!("expires: {}", time(at)));
    }
    lines
}

/// What the console's `link` says (M4 hosting spec §3.10): the server's own link for each of
/// `bases`, for players who already have an account, and what links need where there are none, or
/// how they go further ([`link_hint`]). False when there's no link at all.
pub(crate) fn link_lines(
    cfg: &Config,
    pin: Option<&str>,
    ports: [u16; 2],
    bases: &[(String, InviteBase)],
) -> (bool, Vec<String>) {
    let mut lines: Vec<String> = bases
        .iter()
        .map(|(label, base)| server_link_line(label, base))
        .collect();
    lines.extend(link_hint(cfg, pin, ports, bases));
    (!bases.is_empty(), lines)
}

/// `server link (<label>): <link>`: the server's own link from `base` ([`server_link`]).
fn server_link_line(label: &str, base: &InviteBase) -> String {
    format!(
        "server link ({}): {}",
        escape_hidden(label),
        server_link(base)
    )
}

/// What links need where `bases` has none, or how they go further, `pin` being the server's own
/// key's and `ports` the gate's and the API's: a line, if there's anything to say.
fn link_hint(
    cfg: &Config,
    pin: Option<&str>,
    [gate_port, api_port]: [u16; 2],
    bases: &[(String, InviteBase)],
) -> Option<String> {
    let s = &cfg.server;
    let own_key = s.tls == Some(Tls::OwnKey);
    if own_key && pin.is_none() {
        Some(format!(
            "link: none yet; the server makes its key in {} when it first starts with \
             tls = \"self\", and links carry the key's pin",
            crate::tls::folder(&cfg.store.path).display()
        ))
    } else if bases.is_empty() {
        Some(format!("link: none; {}", crate::invite::NO_LINKS))
    } else if !own_key && cfg.invite_base(None).is_none() {
        // Links for this computer alone, from an API that listens beyond it.
        Some(format!("other computers: {}", crate::invite::NO_LINKS))
    } else if own_key
        && s.public_api.is_none()
        && s.public_host.is_none()
        && !s.api_bind.ip().to_canonical().is_loopback()
    {
        // A link for each of the machine's addresses (HA4), and how to go beyond them.
        Some(format!(
            "friends over the internet: open ports {gate_port} and {api_port} on your router, or \
             use a VPN: see docs/hosting.md"
        ))
    } else {
        None
    }
}

/// `invite list`'s rows at `now`, never the codes: its header, then each invite's.
pub(crate) async fn invite_rows(store: &Store, now: i64) -> anyhow::Result<Vec<Vec<String>>> {
    let mut rows = vec![row(["id", "label", "created", "status"])];
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
        let label = i.label.as_deref().map_or_else(|| "-".into(), escape_hidden);
        rows.push(vec![i.id.to_string(), label, time(i.created_at), status]);
    }
    Ok(rows)
}

/// `account list`'s rows: its header, then each account's.
pub(crate) async fn account_rows(store: &Store) -> anyhow::Result<Vec<Vec<String>>> {
    let mut rows = vec![row([
        "id",
        "username",
        "created",
        "last sign-in",
        "characters",
    ])];
    for a in store.list_accounts().await? {
        rows.push(vec![
            a.id.to_string(),
            a.username,
            time(a.created_at),
            a.last_sign_in_at.map_or_else(|| "-".into(), time),
            a.characters.to_string(),
        ]);
    }
    Ok(rows)
}

/// Revokes unused invite `id` at `now`: whether it did, and what `invite revoke` says.
pub(crate) async fn revoke(store: &Store, id: i64, now: i64) -> anyhow::Result<(bool, String)> {
    Ok(if store.revoke_invite(id, now).await? {
        (true, format!("revoked invite {id}"))
    } else {
        (
            false,
            format!("nothing to revoke: invite {id} is unknown, used or revoked"),
        )
    })
}

fn row<const N: usize>(cells: [&str; N]) -> Vec<String> {
    cells.map(String::from).to_vec()
}

/// Writes `rows` as the CLI prints a list: one line each, the cells apart by tabs.
fn write_rows(out: &mut dyn Write, rows: &[Vec<String>]) -> std::io::Result<()> {
    for row in rows {
        writeln!(out, "{}", row.join("\t"))?;
    }
    Ok(())
}

/// Runs one admin command against the configured database, writing plain lines to `out`. A data
/// folder that isn't this user's is refused first ([`crate::folder::check`]).
pub async fn admin(
    cfg: &Config,
    cmd: &Command,
    out: &mut dyn Write,
    clock: &dyn Clock,
) -> anyhow::Result<()> {
    crate::folder::check(&crate::config::data_folder(&cfg.store.path))?;
    let store = crate::server::open_store(cfg).await?;
    let now = clock.now_ms();
    match cmd {
        Command::Run => anyhow::bail!("`run` isn't an admin command"),
        Command::Init => anyhow::bail!("`init` isn't an admin command"),
        Command::Console { .. } => anyhow::bail!("`console` isn't an admin command"),
        Command::Invite {
            cmd: InviteCmd::Create { label, expires },
        } => {
            let expires_at = expires.map(|d| ms_after(now, d));
            // The pin from the certificate alone, and no key made here: this may run as another
            // user than the server, which couldn't then read a key made here.
            let pin = if cfg.server.tls == Some(Tls::OwnKey) {
                crate::tls::pin_in(&crate::tls::folder(&cfg.store.path))?
            } else {
                None
            };
            let ports = [cfg.server.login_bind.port(), cfg.server.api_bind.port()];
            let bases = crate::invite::bases(cfg, pin.as_deref(), ports[1]);
            let made =
                crate::invite::create(&store, None, label.as_deref(), now, expires_at).await?;
            let lines = invite_lines(cfg, pin.as_deref(), ports, &bases, &made.code, expires_at);
            for line in lines {
                writeln!(out, "{line}")?;
            }
        }
        Command::Invite {
            cmd: InviteCmd::List,
        } => write_rows(out, &invite_rows(&store, now).await?)?,
        Command::Invite {
            cmd: InviteCmd::Revoke { id },
        } => writeln!(out, "{}", revoke(&store, *id, now).await?.1)?,
        Command::Account {
            cmd: AccountCmd::List,
        } => write_rows(out, &account_rows(&store).await?)?,
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
    fn plain_goes_before_or_after_the_command_and_is_off_by_default() {
        // M4 hosting spec §3.1: the start scripts pass their arguments on, so `--plain` works
        // without a command too.
        for args in [
            &["fenji-server", "--plain"][..],
            &["fenji-server", "run", "--plain"],
            &["fenji-server", "--plain", "run"],
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            assert!(cli.plain, "{args:?}");
            assert!(matches!(cli.command, None | Some(Command::Run)), "{args:?}");
        }
        assert!(!Cli::try_parse_from(["fenji-server"]).unwrap().plain);
    }

    #[test]
    fn console_takes_one_command_to_run_and_the_global_plain() {
        // M4 hosting spec §3.1; Task 11's carry-forward 5: `--plain` is the global flag, before or
        // after `console`.
        let console = |args: &[&str]| match Cli::try_parse_from(args) {
            Ok(Cli {
                command: Some(Command::Console { run }),
                plain,
                ..
            }) => Some((run, plain)),
            _ => None,
        };
        assert_eq!(console(&["fenji-server", "console"]), Some((None, false)));
        assert_eq!(
            console(&["fenji-server", "console", "--plain"]),
            Some((None, true))
        );
        assert_eq!(
            console(&["fenji-server", "--plain", "console"]),
            Some((None, true))
        );
        assert_eq!(
            console(&["fenji-server", "console", "--run", "kick Nobody"]),
            Some((Some("kick Nobody".into()), false))
        );
        assert_eq!(
            console(&["fenji-server", "console", "--run", ""]),
            Some((Some(String::new()), false))
        );
        assert_eq!(console(&["fenji-server", "console", "--run"]), None);
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
