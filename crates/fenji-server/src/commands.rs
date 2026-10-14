//! The admin command set (M4 spec §4.3): one module for the game's chat, where admins
//! (`[access].admins`) type a command after `!`, and for the host console, which hosting adds,
//! with the same names and arguments.
use std::fmt;

/// An admin command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `tp <name>` takes the admin to a player; `tp <name> <name>` takes the first to the second.
    Tp { who: Option<String>, to: String },
    /// `kick <name>`.
    Kick { name: String },
    /// `announce <text>`: the marquee, for everyone in the game.
    Announce { text: String },
    /// `invite [label]`: a new invite link.
    Invite { label: Option<String> },
}

/// Why a line isn't a command that can run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    Unknown(String),
    Usage(&'static str),
    /// Controls or invisible formatting ([`crate::text::hidden`]): a label or an announcement
    /// shows them to everyone, and `invite list` to the host's terminal.
    NotPlain,
}

pub const TP: &str = "tp <name> [<name>]";
pub const KICK: &str = "kick <name>";
pub const ANNOUNCE: &str = "announce <text>";
pub const INVITE: &str = "invite [label of at most 64 characters]";

/// The longest invite label.
pub const LABEL_MAX: usize = 64;

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(word) => {
                write!(
                    f,
                    "no command {word}; the commands are tp, kick, announce and invite"
                )
            }
            Self::Usage(usage) => write!(f, "usage: {usage}"),
            Self::NotPlain => write!(f, "commands take plain text only"),
        }
    }
}

/// Parses a command, without its `!`.
pub fn parse(line: &str) -> Result<Command, CommandError> {
    if !crate::text::plain(line) {
        return Err(CommandError::NotPlain);
    }
    let line = line.trim();
    let (word, rest) = match line.split_once(char::is_whitespace) {
        Some((word, rest)) => (word, rest.trim()),
        None => (line, ""),
    };
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
        "invite" if rest.chars().count() > LABEL_MAX => Err(CommandError::Usage(INVITE)),
        "invite" => Ok(Command::Invite {
            label: (!rest.is_empty()).then(|| rest.into()),
        }),
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
}
