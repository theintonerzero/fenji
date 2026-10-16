//! The admin commands, in one module for the game's chat and the host console. In the chat, admins
//! type one of four after `!` (M4 spec §4.3; [`parse`]). The console takes the same four, its `tp`
//! with two names, and commands of its own, which the chat doesn't know (M4 hosting spec §3.3;
//! §8, HA1 and HA5; [`parse_console`]).
use std::fmt;
use std::time::Duration;

use fenji_console::wire::CommandHelp;

/// An admin command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `tp <name>` takes the admin to a player; `tp <name> <name>` takes the first to the second.
    /// Within one scene only, for now: a player in another scene is refused (#280).
    Tp { who: Option<String>, to: String },
    /// `kick <name>`.
    Kick { name: String },
    /// `announce <text>`: the marquee, for everyone in the game.
    Announce { text: String },
    /// `invite [label]`: a new invite link.
    Invite { label: Option<String> },
}

/// A command the host console takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsoleCommand {
    /// One of the chat's four, which the world runs as it does an admin's. Its `tp` has two names,
    /// and `say <text>` is `announce`.
    Shared(Command),
    /// `online`, or `list`: who is connected.
    Online,
    /// `link`: the server's own links, for players who have an account, without an invite.
    Link,
    /// `accounts`: the accounts, as `account list` shows them.
    Accounts,
    /// `invites`: the invites, as `invite list` shows them, never their codes.
    Invites,
    /// `revoke <id>`: revokes an unused invite, as `invite revoke` does.
    Revoke { id: i64 },
    /// `maintenance [on [<duration>] [<message>] | off]` (§3.9).
    Maintenance(MaintenanceArg),
    /// `backup`: a copy of the database in the backups folder (§3.12).
    Backup,
    /// `stop`: stops the server gracefully, as SIGTERM does.
    Stop,
    /// `help [<command>]`: the commands, or one's usage. A command is named as [`console_help`]
    /// names it, so `help SAY` asks about `announce`.
    Help { command: Option<String> },
    /// `motd <text>` sets the MOTD (`Some`), kept in the database over the file's `[server] motd`.
    /// `motd off` drops the database's (`None`, for
    /// [`Status::set_motd`](crate::status::Status::set_motd)), so the file's is the MOTD again:
    /// empty, unless the host wrote one there.
    Motd(Option<String>),
    /// `motd` alone: the MOTD in effect.
    MotdShow,
    /// `op <account>`: makes the account an admin, kept in the database.
    Op { account: String },
    /// `deop <account>`: takes the account's op away; a name in `[access].admins` stays an
    /// admin's.
    Deop { account: String },
    /// `ops`: the admins.
    Ops,
    /// `ban <account> [<reason>]`: refuses the account's sign-ins and game tickets, with the
    /// reason, and lets it go at once if it's online.
    Ban {
        account: String,
        reason: Option<String>,
    },
    /// `pardon <account>`: lifts the account's ban.
    Pardon { account: String },
    /// `bans`: the bans, with their reasons.
    Bans,
}

/// What `maintenance` asks for (M4 hosting spec §3.9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaintenanceArg {
    /// `maintenance` alone: the state, the message and the times.
    Show,
    /// `maintenance on [<duration>] [<message>]`. The duration is how long the host expects it to
    /// last, which players are shown; it doesn't end maintenance by itself. Without a message,
    /// players read [`crate::status::DEFAULT_MESSAGE`].
    On {
        lasts: Option<Duration>,
        message: Option<String>,
    },
    /// `maintenance off`.
    Off,
}

/// Why a line isn't a command that can run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    /// A word the chat doesn't know.
    Unknown(String),
    /// A word the console doesn't know.
    UnknownHere(String),
    Usage(&'static str),
    /// Controls or invisible formatting ([`crate::text::hidden`]): a label or an announcement
    /// shows them to everyone, and `invite list` to the host's terminal.
    NotPlain,
}

pub const TP: &str = "tp <name> [<name>]";
/// The console's `tp`: the console has no character of its own to take anywhere.
pub const TP_CONSOLE: &str = "tp <name> <name>";
pub const KICK: &str = "kick <name>";
pub const ANNOUNCE: &str = "announce <text>";
/// The console's `announce` by the name a Minecraft server's console gives it.
pub const SAY: &str = "say <text>";
pub const INVITE: &str = "invite [label of at most 64 characters]";
pub const REVOKE: &str = "revoke <id>";
/// The host console's `maintenance` (M4 hosting spec §3.9), whose message
/// [`crate::status::check_message`] checks.
pub const MAINTENANCE: &str = "maintenance [on [<duration>] [<message>] | off]";
pub const HELP: &str = "help [<command>]";
pub const MOTD: &str = "motd [<text> | off]";
pub const OP: &str = "op <account>";
pub const DEOP: &str = "deop <account>";
pub const BAN: &str = "ban <account> [<reason>]";
pub const PARDON: &str = "pardon <account>";

/// The longest invite label.
pub const LABEL_MAX: usize = 64;

/// The console's commands as `help` lists them, §3.3's then Amendment A's: each one's word, its
/// usage, which for a command without arguments is its word, and what it does. The palette shows
/// that beside the usages: on an 80-column terminal, beside `maintenance`'s and before the list's
/// padding column at the right, it has 26 columns.
const CONSOLE: [(&str, &str, &str); 20] = [
    ("tp", TP_CONSOLE, "take a player to another"),
    ("kick", KICK, "disconnect a player"),
    ("announce", ANNOUNCE, "message everyone (or say)"),
    ("invite", INVITE, "make an invite link"),
    ("link", "link", "show the server's links"),
    ("online", "online", "who is connected (or list)"),
    ("accounts", "accounts", "list the accounts"),
    ("invites", "invites", "list the invites"),
    ("revoke", REVOKE, "revoke an unused invite"),
    ("maintenance", MAINTENANCE, "show or change maintenance"),
    ("backup", "backup", "back up the database"),
    ("stop", "stop", "stop the server"),
    ("help", HELP, "list or explain commands"),
    ("motd", MOTD, "show or set the MOTD"),
    ("op", OP, "make an account an admin"),
    ("deop", DEOP, "take an account's op away"),
    ("ops", "ops", "list the admins"),
    ("ban", BAN, "ban an account"),
    ("pardon", PARDON, "lift an account's ban"),
    ("bans", "bans", "list the bans"),
];

/// The command as an admin types it, without the chat's `!`: what the log says was run (the hosting
/// plan's final review B, Minor 3). [`parse`] reads it back as the same command.
impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Command::Tp { who: Some(who), to } => write!(f, "tp {who} {to}"),
            Command::Tp { who: None, to } => write!(f, "tp {to}"),
            Command::Kick { name } => write!(f, "kick {name}"),
            Command::Announce { text } => write!(f, "announce {text}"),
            Command::Invite { label: Some(label) } => write!(f, "invite {label}"),
            Command::Invite { label: None } => f.write_str("invite"),
        }
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(word) => {
                write!(
                    f,
                    "no command {word}; the commands are tp, kick, announce and invite"
                )
            }
            Self::UnknownHere(word) => write!(f, "no command {word}; type help"),
            Self::Usage(usage) => write!(f, "usage: {usage}"),
            Self::NotPlain => write!(f, "commands take plain text only"),
        }
    }
}

impl std::error::Error for CommandError {}

/// Checks that `label` can be an invite's: plain text of at most [`LABEL_MAX`] characters.
/// `!invite` checks it here, and [`crate::invite::create`] again for every caller.
pub fn check_label(label: &str) -> Result<(), CommandError> {
    if !crate::text::plain(label) {
        return Err(CommandError::NotPlain);
    }
    if label.chars().count() > LABEL_MAX {
        return Err(CommandError::Usage(INVITE));
    }
    Ok(())
}

/// `text`'s first word, and the rest, each without the spaces around it.
fn split_word(text: &str) -> (&str, &str) {
    let text = text.trim();
    match text.split_once(char::is_whitespace) {
        Some((word, rest)) => (word, rest.trim()),
        None => (text, ""),
    }
}

/// Parses a command, without its `!`.
pub fn parse(line: &str) -> Result<Command, CommandError> {
    if !crate::text::plain(line) {
        return Err(CommandError::NotPlain);
    }
    let (word, rest) = split_word(line);
    let args: Vec<&str> = rest.split_whitespace().collect();
    match word.to_ascii_lowercase().as_str() {
        "tp" => match args.as_slice() {
            [to] => Ok(Command::Tp {
                who: None,
                to: (*to).into(),
            }),
            [who, to] => Ok(Command::Tp {
                who: Some((*who).into()),
                to: (*to).into(),
            }),
            _ => Err(CommandError::Usage(TP)),
        },
        "kick" => match args.as_slice() {
            [name] => Ok(Command::Kick {
                name: (*name).into(),
            }),
            _ => Err(CommandError::Usage(KICK)),
        },
        "announce" if !rest.is_empty() => Ok(Command::Announce { text: rest.into() }),
        "announce" => Err(CommandError::Usage(ANNOUNCE)),
        "invite" => {
            check_label(rest)?;
            Ok(Command::Invite {
                label: (!rest.is_empty()).then(|| rest.into()),
            })
        }
        _ => Err(CommandError::Unknown(word.into())),
    }
}

/// A chat line as a command: `Some` when it starts with `!` and a letter, `None` for any other
/// line, which is plain chat even from an admin.
pub fn from_chat(text: &str) -> Option<Result<Command, CommandError>> {
    let rest = text.strip_prefix('!')?;
    rest.starts_with(|c: char| c.is_ascii_alphabetic())
        .then(|| parse(rest))
}

/// Parses a line the host typed at the console. The chat's four read as [`parse`] reads them,
/// except `tp`, which takes two names here; `say` is `announce`, and `list` is `online`. Words are
/// in any case.
pub fn parse_console(line: &str) -> Result<ConsoleCommand, CommandError> {
    use ConsoleCommand as C;
    if !crate::text::plain(line) {
        return Err(CommandError::NotPlain);
    }
    let (word, rest) = split_word(line);
    let args: Vec<&str> = rest.split_whitespace().collect();
    // A command that takes nothing: anything after its word is answered with its usage.
    let alone = |command: ConsoleCommand, usage: &'static str| {
        if args.is_empty() {
            Ok(command)
        } else {
            Err(CommandError::Usage(usage))
        }
    };
    // The account a command takes, its one argument.
    let account = |usage: &'static str| match args.as_slice() {
        [name] => Ok(String::from(*name)),
        _ => Err(CommandError::Usage(usage)),
    };
    match word.to_ascii_lowercase().as_str() {
        // Two names, as the console has no character of its own. The line is plain, so whatever
        // `parse` refuses is the number of names.
        "tp" => match parse(line) {
            Ok(tp @ Command::Tp { who: Some(_), .. }) => Ok(C::Shared(tp)),
            _ => Err(CommandError::Usage(TP_CONSOLE)),
        },
        "kick" | "announce" | "invite" => parse(line).map(C::Shared),
        "say" if rest.is_empty() => Err(CommandError::Usage(SAY)),
        "say" => Ok(C::Shared(Command::Announce { text: rest.into() })),
        "online" => alone(C::Online, "online"),
        "link" => alone(C::Link, "link"),
        "list" => alone(C::Online, "list"),
        "accounts" => alone(C::Accounts, "accounts"),
        "invites" => alone(C::Invites, "invites"),
        "revoke" => match args.as_slice() {
            [id] => id
                .parse()
                .map(|id| C::Revoke { id })
                .map_err(|_| CommandError::Usage(REVOKE)),
            _ => Err(CommandError::Usage(REVOKE)),
        },
        "maintenance" => maintenance(rest).map(C::Maintenance),
        "backup" => alone(C::Backup, "backup"),
        "stop" => alone(C::Stop, "stop"),
        "help" => match args.as_slice() {
            [] => Ok(C::Help { command: None }),
            [name] => help_topic(name).map(|name| C::Help {
                command: Some(name.into()),
            }),
            _ => Err(CommandError::Usage(HELP)),
        },
        "motd" if rest.is_empty() => Ok(C::MotdShow),
        "motd" if rest.eq_ignore_ascii_case("off") => Ok(C::Motd(None)),
        "motd" => check_text(rest, MOTD).map(|()| C::Motd(Some(rest.into()))),
        "op" => account(OP).map(|account| C::Op { account }),
        "deop" => account(DEOP).map(|account| C::Deop { account }),
        "ops" => alone(C::Ops, "ops"),
        "ban" => {
            let (account, reason) = split_word(rest);
            if account.is_empty() {
                return Err(CommandError::Usage(BAN));
            }
            check_text(reason, BAN)?;
            Ok(C::Ban {
                account: account.into(),
                reason: (!reason.is_empty()).then(|| reason.into()),
            })
        }
        "pardon" => account(PARDON).map(|account| C::Pardon { account }),
        "bans" => alone(C::Bans, "bans"),
        _ => Err(CommandError::UnknownHere(word.into())),
    }
}

/// `maintenance`'s arguments: none, to show it; `off`; or `on`, then a duration if the next word
/// is one, its unit in any case, then the message. A word written as a duration that isn't one,
/// such as `0m`, is refused, so that players aren't shown it as the message.
fn maintenance(args: &str) -> Result<MaintenanceArg, CommandError> {
    let (switch, rest) = split_word(args);
    match switch.to_ascii_lowercase().as_str() {
        "" => Ok(MaintenanceArg::Show),
        "off" if rest.is_empty() => Ok(MaintenanceArg::Off),
        "on" => {
            let (first, after) = split_word(rest);
            let first = first.to_ascii_lowercase();
            let (lasts, message) = match parse_duration(&first) {
                Ok(lasts) => (Some(lasts), after),
                Err(_) if written_as_duration(&first) => {
                    return Err(CommandError::Usage(MAINTENANCE));
                }
                Err(_) => (None, rest),
            };
            check_text(message, MAINTENANCE)?;
            Ok(MaintenanceArg::On {
                lasts,
                message: (!message.is_empty()).then(|| message.into()),
            })
        }
        _ => Err(CommandError::Usage(MAINTENANCE)),
    }
}

/// Whether `word`, in lower case, is written as a duration: digits, then `m`, `h` or `d`.
/// [`parse_duration`] still refuses a zero, or more digits than it can hold.
fn written_as_duration(word: &str) -> bool {
    word.strip_suffix(['m', 'h', 'd'])
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The command `help <word>` asks about, as [`console_help`] names it: `word` in lower case, or
/// for an alias, its command.
fn help_topic(word: &str) -> Result<&'static str, CommandError> {
    let lower = word.to_ascii_lowercase();
    let name = match lower.as_str() {
        "say" => "announce",
        "list" => "online",
        name => name,
    };
    CONSOLE
        .iter()
        .map(|&(command, ..)| command)
        .find(|&command| command == name)
        .ok_or_else(|| CommandError::UnknownHere(word.into()))
}

/// Checks text the console sets for players to read, a maintenance message, an MOTD or a ban's
/// reason, as [`crate::status::check_message`] does; text too long is answered with `usage`, so
/// that the answer names the command the host typed.
fn check_text(text: &str, usage: &'static str) -> Result<(), CommandError> {
    crate::status::check_message(text).map_err(|e| match e {
        CommandError::Usage(_) => CommandError::Usage(usage),
        e => e,
    })
}

/// The console's commands, for its palette and `help`: each one's word, usage and what it does,
/// in the M4 hosting spec's order (§3.3, then §8's HA1 and HA5). An alias is named in its
/// command's description.
pub fn console_help() -> Vec<CommandHelp> {
    CONSOLE
        .iter()
        .map(|&(name, usage, about)| CommandHelp {
            name: name.into(),
            usage: usage.into(),
            about: about.into(),
        })
        .collect()
}

/// A duration as `invite create --expires` and the console's `maintenance on` take it: `30m`,
/// `12h` or `7d`.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_four_commands_parse_with_their_arguments() {
        assert_eq!(
            parse("tp Mai"),
            Ok(Command::Tp {
                who: None,
                to: "Mai".into()
            })
        );
        assert_eq!(
            parse("TP  Lin   Mai "),
            Ok(Command::Tp {
                who: Some("Lin".into()),
                to: "Mai".into()
            })
        );
        assert_eq!(parse("kick Ren"), Ok(Command::Kick { name: "Ren".into() }));
        assert_eq!(
            parse("announce  Server restart in 5 minutes "),
            Ok(Command::Announce {
                text: "Server restart in 5 minutes".into()
            })
        );
        assert_eq!(parse("invite"), Ok(Command::Invite { label: None }));
        assert_eq!(
            parse("invite for Bob"),
            Ok(Command::Invite {
                label: Some("for Bob".into())
            })
        );
    }

    #[test]
    fn a_command_shows_as_it_was_typed() {
        // The log's record of what admins and the console ran (the hosting plan's final review B,
        // Minor 3): read back, each is the command it was.
        for (typed, shown) in [
            ("tp Mai", "tp Mai"),
            ("TP  Lin   Mai ", "tp Lin Mai"),
            ("kick Nobody", "kick Nobody"),
            ("announce  Restart at 20:00 ", "announce Restart at 20:00"),
            ("invite", "invite"),
            ("invite for Bob", "invite for Bob"),
        ] {
            let command = parse(typed).unwrap();
            assert_eq!(command.to_string(), shown, "{typed:?}");
            assert_eq!(parse(shown), Ok(command), "{typed:?}");
        }
    }

    #[test]
    fn wrong_arguments_give_the_usage_and_unknown_words_say_so() {
        for (line, usage) in [
            ("tp", TP),
            ("tp a b c", TP),
            ("kick", KICK),
            ("kick a b", KICK),
            ("announce", ANNOUNCE),
            ("announce   ", ANNOUNCE),
        ] {
            assert_eq!(parse(line), Err(CommandError::Usage(usage)), "{line}");
        }
        assert_eq!(
            parse(&format!("invite {}", "x".repeat(65))),
            Err(CommandError::Usage(INVITE))
        );
        assert_eq!(
            parse("give sword"),
            Err(CommandError::Unknown("give".into()))
        );
        for hidden in [
            "invite \u{1b}]0;owned\u{7}",
            "announce \u{202E}evil",
            "kick Mai\u{200B}",
        ] {
            assert_eq!(parse(hidden), Err(CommandError::NotPlain), "{hidden:?}");
        }
        assert_eq!(
            CommandError::NotPlain.to_string(),
            "commands take plain text only"
        );
        assert_eq!(
            CommandError::Unknown("give".into()).to_string(),
            "no command give; the commands are tp, kick, announce and invite"
        );
    }

    #[test]
    fn an_invite_label_may_have_64_characters_and_no_more() {
        // Characters, not bytes: each é is two.
        let longest = "é".repeat(64);
        assert_eq!(
            parse(&format!("invite {longest}")),
            Ok(Command::Invite {
                label: Some(longest)
            })
        );
        assert_eq!(
            parse(&format!("invite {}", "é".repeat(65))),
            Err(CommandError::Usage(INVITE))
        );
    }

    #[test]
    fn only_a_bang_and_a_letter_make_a_chat_line_a_command() {
        assert_eq!(
            from_chat("!kick Ren"),
            Some(Ok(Command::Kick { name: "Ren".into() }))
        );
        assert_eq!(
            from_chat("!what"),
            Some(Err(CommandError::Unknown("what".into())))
        );
        for plain in ["hello", "!", "! kick Ren", "!!", "!1", " !kick Ren"] {
            assert_eq!(from_chat(plain), None, "{plain:?}");
        }
    }

    #[test]
    fn the_console_parses_its_own_commands_and_the_shared_ones() {
        use ConsoleCommand as C;
        assert_eq!(parse_console("online"), Ok(C::Online));
        assert_eq!(parse_console("LINK"), Ok(C::Link));
        assert_eq!(parse_console("revoke 12"), Ok(C::Revoke { id: 12 }));
        assert_eq!(
            parse_console("maintenance"),
            Ok(C::Maintenance(MaintenanceArg::Show))
        );
        assert_eq!(
            parse_console("maintenance off"),
            Ok(C::Maintenance(MaintenanceArg::Off))
        );
        assert_eq!(
            parse_console("maintenance on"),
            Ok(C::Maintenance(MaintenanceArg::On {
                lasts: None,
                message: None
            }))
        );
        assert_eq!(
            parse_console("maintenance on 30m Back soon"),
            Ok(C::Maintenance(MaintenanceArg::On {
                lasts: Some(Duration::from_secs(1800)),
                message: Some("Back soon".into())
            }))
        );
        assert_eq!(
            parse_console("maintenance on Back soon"),
            Ok(C::Maintenance(MaintenanceArg::On {
                lasts: None,
                message: Some("Back soon".into())
            }))
        );
        assert_eq!(
            parse_console("tp Mai Lin"),
            Ok(C::Shared(Command::Tp {
                who: Some("Mai".into()),
                to: "Lin".into()
            }))
        );
        assert_eq!(
            parse_console("tp Lin"),
            Err(CommandError::Usage(TP_CONSOLE))
        );
        assert_eq!(
            parse_console("help invite"),
            Ok(C::Help {
                command: Some("invite".into())
            })
        );
        assert_eq!(
            parse_console("dance"),
            Err(CommandError::UnknownHere("dance".into()))
        );
        assert_eq!(
            parse_console(&format!("maintenance on {}", "x".repeat(201))),
            Err(CommandError::Usage(MAINTENANCE))
        );
    }

    #[test]
    fn console_only_words_stay_unknown_in_chat() {
        for word in [
            "online",
            "link",
            "accounts",
            "invites",
            "revoke 1",
            "maintenance on",
            "backup",
            "stop",
            "help",
            // Amendment A's (HA1, HA5): the chat still knows only the shared four.
            "motd Hello",
            "op Mai",
            "deop Mai",
            "ops",
            "ban Mai",
            "pardon Mai",
            "bans",
            "say hi",
            "list",
        ] {
            assert!(
                matches!(parse(word), Err(CommandError::Unknown(_))),
                "{word}"
            );
        }
    }

    #[test]
    fn every_console_command_has_help() {
        let names: Vec<_> = console_help().into_iter().map(|h| h.name).collect();
        assert_eq!(
            names,
            [
                "tp",
                "kick",
                "announce",
                "invite",
                // The server's links without an invite (the hosting plan's final review).
                "link",
                "online",
                "accounts",
                "invites",
                "revoke",
                "maintenance",
                "backup",
                "stop",
                "help",
                // Amendment A's, after §3.3's.
                "motd",
                "op",
                "deop",
                "ops",
                "ban",
                "pardon",
                "bans"
            ]
        );
    }

    #[test]
    fn the_console_takes_ops_bans_the_motd_and_minecrafts_say_and_list() {
        use ConsoleCommand as C;
        assert_eq!(parse_console("motd"), Ok(C::MotdShow));
        assert_eq!(
            parse_console("motd  Welcome to  Moonward! "),
            Ok(C::Motd(Some("Welcome to  Moonward!".into())))
        );
        assert_eq!(
            parse_console("op Mai"),
            Ok(C::Op {
                account: "Mai".into()
            })
        );
        assert_eq!(
            parse_console("DEOP Mai"),
            Ok(C::Deop {
                account: "Mai".into()
            })
        );
        assert_eq!(parse_console("ops"), Ok(C::Ops));
        assert_eq!(
            parse_console("ban Mai"),
            Ok(C::Ban {
                account: "Mai".into(),
                reason: None
            })
        );
        assert_eq!(
            parse_console("ban Mai  spam and  griefing "),
            Ok(C::Ban {
                account: "Mai".into(),
                reason: Some("spam and  griefing".into())
            })
        );
        assert_eq!(
            parse_console("pardon Mai"),
            Ok(C::Pardon {
                account: "Mai".into()
            })
        );
        assert_eq!(parse_console("bans"), Ok(C::Bans));
        assert_eq!(
            parse_console("say Restart at 20:00"),
            Ok(C::Shared(Command::Announce {
                text: "Restart at 20:00".into()
            }))
        );
        assert_eq!(parse_console("LIST"), Ok(C::Online));
    }

    #[test]
    fn the_shared_commands_read_as_in_chat_but_tp_takes_two_names() {
        for line in [
            "kick Ren",
            "KICK Ren",
            "announce  Restart at 20:00 ",
            "invite",
            "invite for Bob",
        ] {
            let shared = parse(line).unwrap();
            assert_eq!(
                parse_console(line),
                Ok(ConsoleCommand::Shared(shared)),
                "{line}"
            );
        }
        // The console has no character to take anywhere.
        for line in ["tp", "tp Lin", "TP a b c"] {
            assert_eq!(
                parse_console(line),
                Err(CommandError::Usage(TP_CONSOLE)),
                "{line}"
            );
        }
        assert_eq!(parse_console("kick"), Err(CommandError::Usage(KICK)));
        assert_eq!(
            parse_console(&format!("invite {}", "x".repeat(65))),
            Err(CommandError::Usage(INVITE))
        );
    }

    #[test]
    fn wrong_arguments_give_the_usage_of_the_word_typed() {
        for (line, usage) in [
            ("online now", "online"),
            ("link me", "link"),
            ("list all", "list"),
            ("accounts x", "accounts"),
            ("invites x", "invites"),
            ("backup now", "backup"),
            ("stop now", "stop"),
            ("ops x", "ops"),
            ("bans x", "bans"),
            ("revoke", REVOKE),
            ("revoke x", REVOKE),
            ("revoke 1 2", REVOKE),
            ("maintenance off now", MAINTENANCE),
            ("maintenance soon", MAINTENANCE),
            ("help tp kick", HELP),
            ("say", SAY),
            ("say   ", SAY),
            ("op", OP),
            ("op Mai Lin", OP),
            ("deop", DEOP),
            ("pardon", PARDON),
            ("pardon Mai Lin", PARDON),
            ("ban", BAN),
        ] {
            assert_eq!(
                parse_console(line),
                Err(CommandError::Usage(usage)),
                "{line}"
            );
        }
    }

    #[test]
    fn a_message_an_motd_or_a_reason_is_plain_and_200_characters_at_most() {
        use ConsoleCommand as C;
        // Characters, not bytes: each é is two.
        let longest = "é".repeat(200);
        assert_eq!(
            parse_console(&format!("maintenance on 1h {longest}")),
            Ok(C::Maintenance(MaintenanceArg::On {
                lasts: Some(Duration::from_secs(3600)),
                message: Some(longest.clone())
            }))
        );
        assert_eq!(
            parse_console(&format!("motd {longest}")),
            Ok(C::Motd(Some(longest.clone())))
        );
        assert_eq!(
            parse_console(&format!("ban Mai {longest}")),
            Ok(C::Ban {
                account: "Mai".into(),
                reason: Some(longest)
            })
        );
        // One more, and the answer is the usage of the command the host typed.
        let over = "é".repeat(201);
        for (line, usage) in [
            (format!("maintenance on 1h {over}"), MAINTENANCE),
            (format!("motd {over}"), MOTD),
            (format!("ban Mai {over}"), BAN),
        ] {
            assert_eq!(
                parse_console(&line),
                Err(CommandError::Usage(usage)),
                "{usage}"
            );
            let typed = line.split(' ').next().unwrap();
            assert!(
                CommandError::Usage(usage)
                    .to_string()
                    .starts_with(&format!("usage: {typed} ")),
                "{usage}"
            );
        }
        for hidden in [
            "motd \u{1b}[2Jgone",
            "ban Mai back \u{202E}soon",
            "maintenance on zero\u{200B}width",
            "say \u{1b}]0;owned\u{7}",
            "online\u{200B}",
            "motd two\nlines",
        ] {
            assert_eq!(
                parse_console(hidden),
                Err(CommandError::NotPlain),
                "{hidden:?}"
            );
        }
    }

    #[test]
    fn motd_off_gives_the_files_motd_back_and_other_words_are_the_text() {
        use ConsoleCommand as C;
        // `Motd(None)` is `Status::set_motd(None)`: the console's MOTD goes, and the file's
        // `[server] motd`, empty unless the host wrote one there, is the MOTD again.
        assert_eq!(parse_console("motd off"), Ok(C::Motd(None)));
        assert_eq!(parse_console("MOTD Off "), Ok(C::Motd(None)));
        assert_eq!(
            parse_console("motd Off to the races!"),
            Ok(C::Motd(Some("Off to the races!".into())))
        );
    }

    #[test]
    fn maintenance_reads_a_duration_first_and_on_or_off_in_any_case() {
        let on = |lasts: Option<u64>, message: Option<&str>| {
            Ok(ConsoleCommand::Maintenance(MaintenanceArg::On {
                lasts: lasts.map(Duration::from_secs),
                message: message.map(Into::into),
            }))
        };
        let refused = Err(CommandError::Usage(MAINTENANCE));
        for (line, parsed) in [
            ("MAINTENANCE ON 2h", on(Some(7200), None)),
            (
                "maintenance on 1d  Moving to a new  machine ",
                on(Some(86_400), Some("Moving to a new  machine")),
            ),
            // A duration's unit is in any case too.
            ("maintenance on 2H", on(Some(7200), None)),
            (
                "maintenance on 30M Back soon",
                on(Some(1800), Some("Back soon")),
            ),
            // Written as a duration, but not one: a slip, which players mustn't read as the message.
            ("maintenance on 0m", refused.clone()),
            ("maintenance on 0H Back soon", refused.clone()),
            ("maintenance on 99999999999999999999d", refused.clone()),
            // A duration is the first word, in `invite create`'s syntax; anything else is the
            // message.
            ("maintenance on Back in 30m", on(None, Some("Back in 30m"))),
            ("maintenance on 2 hours", on(None, Some("2 hours"))),
            ("maintenance on 2nd", on(None, Some("2nd"))),
            ("maintenance on 30min", on(None, Some("30min"))),
            ("maintenance on Back at 9", on(None, Some("Back at 9"))),
            (
                "maintenance Off",
                Ok(ConsoleCommand::Maintenance(MaintenanceArg::Off)),
            ),
        ] {
            assert_eq!(parse_console(line), parsed, "{line}");
        }
    }

    #[test]
    fn help_names_a_command_in_any_case_or_by_its_alias() {
        let help = |name: &str| {
            Ok(ConsoleCommand::Help {
                command: Some(name.into()),
            })
        };
        assert_eq!(
            parse_console("help"),
            Ok(ConsoleCommand::Help { command: None })
        );
        assert_eq!(parse_console("HELP Maintenance"), help("maintenance"));
        assert_eq!(parse_console("help say"), help("announce"));
        assert_eq!(parse_console("help LIST"), help("online"));
        assert_eq!(
            parse_console("help dance"),
            Err(CommandError::UnknownHere("dance".into()))
        );
        let all = console_help();
        for h in &all {
            // Each command listed is one the console takes and explains.
            assert_eq!(
                parse_console(&format!("help {}", h.name)),
                help(&h.name),
                "{h:?}"
            );
            assert!(
                !matches!(parse_console(&h.name), Err(CommandError::UnknownHere(_))),
                "{h:?}"
            );
            assert!(h.usage.starts_with(&h.name) && !h.about.is_empty(), "{h:?}");
            // The palette's room for it on an 80-column terminal, beside `maintenance`'s usage and
            // before the list's padding column.
            assert!(h.about.chars().count() <= 26, "{h:?}");
        }
        let about = |name: &str| &all.iter().find(|h| h.name == name).unwrap().about;
        assert!(about("announce").contains("say"), "the aliases are named");
        assert!(about("online").contains("list"), "the aliases are named");
    }

    #[test]
    fn an_unknown_word_at_the_console_points_to_help() {
        assert_eq!(
            CommandError::UnknownHere("dance".into()).to_string(),
            "no command dance; type help"
        );
        assert_eq!(
            parse_console("give sword"),
            Err(CommandError::UnknownHere("give".into()))
        );
    }

    #[test]
    fn durations_take_minutes_hours_and_days_and_refuse_the_rest() {
        assert_eq!(parse_duration("90m"), Ok(Duration::from_secs(5400)));
        assert_eq!(parse_duration("2h"), Ok(Duration::from_secs(7200)));
        assert_eq!(parse_duration("7d"), Ok(Duration::from_secs(604_800)));
        for bad in ["7é", "é", "", "m", "0m", "0d", "-1h", "1w", "1.5h", "h1"] {
            assert!(parse_duration(bad).is_err(), "{bad:?}");
        }
    }
}
