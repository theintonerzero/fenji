//! What a server and a console say to each other (M4 hosting spec §3.5): JSON lines over the
//! console socket, or these values over channels when the console runs in the server's process.
//! - The server speaks first, with [`ServerMsg::Hello`]: its version, the protocol's and its
//!   commands.
//! - Then it sends log lines, status changes and answers, each answer with its command's number.
//! - A console sends numbered command lines, [`ConsoleMsg::Run`].
//!
//! A line is at most [`LINE_MAX`] bytes, either way.
use std::fmt;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::text::clean;

/// The protocol's version. A console of another version is told so and closed.
pub const PROTO: u32 = 1;
/// The longest line, either way, in bytes, without its line ending.
pub const LINE_MAX: usize = 4096;
/// The longest command line a console sends, in bytes. Its front ends take no hidden character in
/// one, so a [`ConsoleMsg::Run`], JSON's escapes and all, stays well within [`LINE_MAX`].
pub const INPUT_MAX: usize = 1024;

/// What a server sends a console.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    /// The first message: the server's version, the protocol's, the server's name and the
    /// commands it takes.
    Hello {
        version: String,
        proto: u32,
        server: String,
        commands: Vec<CommandHelp>,
    },
    /// A line of the server's log.
    Log { line: LogLine },
    /// How many log lines this console lost by falling behind: `… 37 lines skipped`.
    Skipped { lines: u64 },
    /// The server's state and facts, when they change.
    Status { status: Status },
    /// The answer to the console's command `id`: whether it ran, and what it says.
    Answer {
        id: u64,
        ok: bool,
        lines: Vec<String>,
    },
    /// The last message: why the server closes this console.
    Bye { reason: String },
}

/// What a console sends a server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConsoleMsg {
    /// Runs a command line. The server answers with [`ServerMsg::Answer`] and the same `id`.
    Run { id: u64, line: String },
}

/// A line of the server's log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogLine {
    /// When, in Unix milliseconds.
    pub at_ms: i64,
    pub level: Level,
    /// The message in pieces. Its text is the parts' texts joined as they are, so a part carries
    /// its own spaces: `entered `, then the name `Mai`.
    pub parts: Vec<Part>,
}

/// A piece of a log line's message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Part {
    pub text: String,
    /// Whether it names someone, a character or an account: names show in gold.
    #[serde(default, skip_serializing_if = "is_false")]
    pub name: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// How much a log line matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

/// The server's state and facts, as the console's status strip shows them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub state: State,
    /// The server's name.
    pub server: String,
    pub version: String,
    /// When it started, in Unix milliseconds.
    pub started_ms: i64,
    /// The players connected, of the most it takes.
    pub online: u32,
    pub max: u32,
    /// While it's in maintenance: the host's message.
    pub maintenance: Option<String>,
    pub gate_port: u16,
    pub api_port: u16,
}

/// Whether the server takes players.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Online,
    /// Only the host's admins can sign in.
    Maintenance,
    Stopping,
}

/// A command the console takes, for its palette.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandHelp {
    /// The word that runs it: `invite`.
    pub name: String,
    /// How to write it: `invite [label]`.
    pub usage: String,
    /// What it does, in a few words.
    pub about: String,
}

/// [`ServerMsg::Hello`]'s fields: what a console knows of its server from the start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    pub version: String,
    pub proto: u32,
    pub server: String,
    pub commands: Vec<CommandHelp>,
}

impl From<Hello> for ServerMsg {
    fn from(hello: Hello) -> Self {
        let Hello {
            version,
            proto,
            server,
            commands,
        } = hello;
        ServerMsg::Hello {
            version,
            proto,
            server,
            commands,
        }
    }
}

impl TryFrom<ServerMsg> for Hello {
    /// Any other message, given back.
    type Error = ServerMsg;

    fn try_from(msg: ServerMsg) -> Result<Self, Self::Error> {
        match msg {
            ServerMsg::Hello {
                version,
                proto,
                server,
                commands,
            } => Ok(Hello {
                version,
                proto,
                server,
                commands,
            }),
            other => Err(other),
        }
    }
}

impl Level {
    /// The level as the console shows it: `ERROR`, `WARN`, `INFO`, `DEBUG` or `TRACE`.
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        }
    }
}

impl LogLine {
    /// The time of day it was logged, `HH:MM:SS`, in UTC (Ruling HP6).
    pub fn time(&self) -> String {
        let secs = self.at_ms.div_euclid(1000).rem_euclid(86_400);
        format!(
            "{:02}:{:02}:{:02}",
            secs / 3600,
            secs % 3600 / 60,
            secs % 60
        )
    }

    /// The message: the parts' texts in order, with hidden characters as escapes.
    pub fn message(&self) -> String {
        self.parts.iter().map(|p| clean(&p.text)).collect()
    }
}

/// `HH:MM:SS LEVEL message`, the console's columns as one line: the time of day in UTC, the level
/// padded to five and the message, with hidden characters as escapes. Plain mode prints it.
impl fmt::Display for LogLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {:<5} {}",
            self.time(),
            self.level.as_str(),
            self.message()
        )
    }
}

/// Why a line isn't a message.
#[derive(Debug, thiserror::Error)]
pub enum WireError {
    #[error("a console line is at most {LINE_MAX} bytes; this one is {len}")]
    TooLong { len: usize },
    #[error("that isn't a console message: {0}")]
    Bad(#[from] serde_json::Error),
}

/// `msg` as one line of JSON, without a line ending: JSON writes a line break inside a string as
/// `\n`.
///
/// # Panics
/// If `msg` can't be JSON, which none of this module's messages is.
pub fn encode<T: Serialize>(msg: &T) -> String {
    serde_json::to_string(msg).expect("the console's messages are JSON")
}

/// A server's line as its message. Refused over [`LINE_MAX`] bytes; a line ending doesn't count.
pub fn decode_server(line: &str) -> Result<ServerMsg, WireError> {
    decode(line)
}

/// A console's line as its message. Refused over [`LINE_MAX`] bytes; a line ending doesn't count.
pub fn decode_console(line: &str) -> Result<ConsoleMsg, WireError> {
    decode(line)
}

fn decode<T: DeserializeOwned>(line: &str) -> Result<T, WireError> {
    let line = line.strip_suffix('\n').unwrap_or(line);
    let line = line.strip_suffix('\r').unwrap_or(line);
    if line.len() > LINE_MAX {
        return Err(WireError::TooLong { len: line.len() });
    }
    Ok(serde_json::from_str(line)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_single_lines_that_round_trip_and_long_lines_are_refused() {
        let msg = ServerMsg::Answer {
            id: 7,
            ok: true,
            lines: vec!["a\nb".into()],
        };
        let line = encode(&msg);
        assert!(!line.contains('\n'));
        assert_eq!(decode_server(&line).unwrap(), msg);
        assert!(decode_server(&"x".repeat(LINE_MAX + 1)).is_err());
    }

    #[test]
    fn the_messages_are_json_tagged_by_type() {
        let run = ConsoleMsg::Run {
            id: 3,
            line: "online".into(),
        };
        assert_eq!(encode(&run), r#"{"type":"run","id":3,"line":"online"}"#);
        assert_eq!(decode_console(&encode(&run)).unwrap(), run);
        let log = ServerMsg::Log {
            line: LogLine {
                at_ms: 5,
                level: Level::Warn,
                parts: vec![
                    Part {
                        text: "chat from ".into(),
                        name: false,
                    },
                    Part {
                        text: "Mai".into(),
                        name: true,
                    },
                ],
            },
        };
        assert_eq!(
            encode(&log),
            r#"{"type":"log","line":{"at_ms":5,"level":"warn","parts":[{"text":"chat from "},{"text":"Mai","name":true}]}}"#
        );
        assert_eq!(decode_server(&encode(&log)).unwrap(), log);
        let status = ServerMsg::Status {
            status: Status {
                state: State::Maintenance,
                server: "Moonward".into(),
                version: "0.5.0".into(),
                started_ms: 1,
                online: 3,
                max: 20,
                maintenance: Some("Updating".into()),
                gate_port: 7800,
                api_port: 7801,
            },
        };
        assert!(encode(&status).contains(r#""state":"maintenance""#));
        assert_eq!(decode_server(&encode(&status)).unwrap(), status);
        for msg in [
            ServerMsg::Skipped { lines: 37 },
            ServerMsg::Bye {
                reason: "the server is stopping".into(),
            },
        ] {
            assert_eq!(decode_server(&encode(&msg)).unwrap(), msg);
        }
    }

    #[test]
    fn a_console_line_at_the_limit_is_read_and_one_over_it_refused() {
        let run = |n: usize| {
            encode(&ConsoleMsg::Run {
                id: 1,
                line: "x".repeat(n),
            })
        };
        let fits = LINE_MAX - run(0).len();
        assert!(decode_console(&run(fits)).is_ok());
        assert!(matches!(
            decode_console(&run(fits + 1)),
            Err(WireError::TooLong { len }) if len == LINE_MAX + 1
        ));
        // The line's own ending doesn't count.
        assert!(decode_console(&format!("{}\r\n", run(fits))).is_ok());
        assert!(matches!(
            decode_console(r#"{"type":"dance"}"#),
            Err(WireError::Bad(_))
        ));
    }

    #[test]
    fn hello_is_a_server_message_and_back() {
        let hello = Hello {
            version: "0.5.0".into(),
            proto: PROTO,
            server: "Moonward".into(),
            commands: vec![CommandHelp {
                name: "stop".into(),
                usage: "stop".into(),
                about: "stop the server".into(),
            }],
        };
        let msg = ServerMsg::from(hello.clone());
        assert!(matches!(msg, ServerMsg::Hello { proto: 1, .. }));
        assert_eq!(
            Hello::try_from(decode_server(&encode(&msg)).unwrap()),
            Ok(hello)
        );
        let bye = ServerMsg::Bye { reason: "x".into() };
        assert_eq!(Hello::try_from(bye.clone()), Err(bye));
    }

    #[test]
    fn a_log_line_prints_as_the_console_shows_it() {
        let line = LogLine {
            at_ms: 3_723_000,
            level: Level::Warn,
            parts: vec![
                Part {
                    text: "chat from ".into(),
                    name: false,
                },
                Part {
                    text: "Mai\u{1b}[2J".into(),
                    name: true,
                },
            ],
        };
        assert_eq!(line.time(), "01:02:03");
        assert_eq!(line.message(), r"chat from Mai\u{1b}[2J");
        assert_eq!(line.to_string(), r"01:02:03 WARN  chat from Mai\u{1b}[2J");
        // Times of day in UTC, before 1970 and at any distance too.
        let at = |at_ms| LogLine {
            at_ms,
            level: Level::Error,
            parts: vec![],
        };
        assert_eq!(at(86_399_999).to_string(), "23:59:59 ERROR ");
        assert_eq!(at(-1_000).time(), "23:59:59");
        for at_ms in [i64::MIN, i64::MAX] {
            assert_eq!(at(at_ms).time().len(), 8);
        }
    }
}
