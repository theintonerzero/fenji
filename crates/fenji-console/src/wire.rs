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
    /// The answer to the console's command `id`: whether it ran, and what it says. An answer too
    /// long for one line comes in parts, in order, each with the same `id` and `ok`: every part but
    /// the last says `more` ([`encode_server`]).
    Answer {
        id: u64,
        ok: bool,
        lines: Vec<String>,
        #[serde(default, skip_serializing_if = "is_false")]
        more: bool,
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

/// What ends a line cut to fit.
const CUT: &str = "…";

/// `msg` as the lines a server sends on the console socket, without their line endings, each at
/// most [`LINE_MAX`] bytes:
/// - a log line too long, such as a packet's hex or an error's chain, is cut, `…` where it was;
/// - an answer too long comes in parts, as many whole lines in each as fit, and every part but
///   the last says `more`; a line too long for a part alone is cut so;
/// - the other messages go whole: the server keeps its name, its maintenance message and its
///   commands short. A console refuses a line over the limit, so one of those too long, a hello
///   above all, is a bug, which a debug build panics on (#359).
pub fn encode_server(msg: ServerMsg) -> Vec<String> {
    let whole = encode(&msg);
    debug_assert!(
        whole.len() <= LINE_MAX || matches!(msg, ServerMsg::Log { .. } | ServerMsg::Answer { .. }),
        "a message that goes whole is {} bytes, over the wire's {LINE_MAX}: {whole:.80}",
        whole.len()
    );
    if whole.len() <= LINE_MAX {
        return vec![whole];
    }
    match msg {
        ServerMsg::Log { line } => vec![cut_log(&line)],
        ServerMsg::Answer { id, ok, lines, .. } => answer_parts(id, ok, lines),
        _ => vec![whole],
    }
}

/// `line` as a log message within a line: as many of its first characters as fit, in their parts,
/// then `…`.
fn cut_log(line: &LogLine) -> String {
    let cut = |kept| {
        encode(&ServerMsg::Log {
            line: first_chars(line, kept),
        })
    };
    // `fits` fits, and `over` doesn't: no character takes less than a byte, and the whole line
    // didn't fit.
    let total: usize = line.parts.iter().map(|p| p.text.chars().count()).sum();
    let (mut fits, mut over) = (0, total.min(LINE_MAX));
    while over - fits > 1 {
        let mid = fits + (over - fits) / 2;
        if cut(mid).len() <= LINE_MAX {
            fits = mid;
        } else {
            over = mid;
        }
    }
    cut(fits)
}

/// `line` with the first `kept` characters of its text, in its parts as they were, then `…`, which
/// names no one.
fn first_chars(line: &LogLine, kept: usize) -> LogLine {
    let mut left = kept;
    let mut parts = Vec::new();
    for part in &line.parts {
        if left == 0 {
            break;
        }
        let text: String = part.text.chars().take(left).collect();
        left -= text.chars().count();
        parts.push(Part {
            text,
            name: part.name,
        });
    }
    match parts.last_mut() {
        Some(last) if !last.name => last.text.push_str(CUT),
        _ => parts.push(Part {
            text: CUT.into(),
            name: false,
        }),
    }
    LogLine {
        at_ms: line.at_ms,
        level: line.level,
        parts,
    }
}

/// An answer too long for a line, as parts that each fit, in order: as many whole lines in each
/// as fit, the last part alone without `more`.
fn answer_parts(id: u64, ok: bool, lines: Vec<String>) -> Vec<String> {
    let part = |lines, more| {
        encode(&ServerMsg::Answer {
            id,
            ok,
            lines,
            more,
        })
    };
    // What a part's lines may take: the line, less the answer around them, `more` and all. JSON
    // puts a comma between two lines, and nothing else.
    let room = LINE_MAX - part(Vec::new(), true).len();
    let mut parts = Vec::new();
    let (mut kept, mut used) = (Vec::new(), 0);
    for line in lines {
        let line = fit(line, room);
        let size = json_len(&line);
        if !kept.is_empty() && used + 1 + size > room {
            parts.push(part(std::mem::take(&mut kept), true));
            used = 0;
        }
        used += size + usize::from(!kept.is_empty());
        kept.push(line);
    }
    parts.push(part(kept, false));
    parts
}

/// `line`, or as many of its first characters as fit in `room` bytes of JSON, then `…`.
fn fit(line: String, room: usize) -> String {
    if json_len(&line) <= room {
        return line;
    }
    let cut = |kept| {
        let mut text: String = line.chars().take(kept).collect();
        text.push_str(CUT);
        text
    };
    // `fits` fits, and `over` doesn't: no character takes less than a byte, and the whole line
    // didn't fit.
    let (mut fits, mut over) = (0, line.chars().count().min(room));
    while over - fits > 1 {
        let mid = fits + (over - fits) / 2;
        if json_len(&cut(mid)) <= room {
            fits = mid;
        } else {
            over = mid;
        }
    }
    cut(fits)
}

/// The bytes `text` takes as a JSON string, its quotes and escapes included.
fn json_len(text: &str) -> usize {
    encode(&text).len()
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
            more: false,
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

    /// The answers `lines` decode to, one per line, each checked against the limit.
    fn answers(lines: &[String]) -> Vec<(u64, bool, Vec<String>, bool)> {
        lines
            .iter()
            .map(|line| {
                assert!(line.len() <= LINE_MAX, "{} bytes", line.len());
                match decode_server(line).unwrap() {
                    ServerMsg::Answer {
                        id,
                        ok,
                        lines,
                        more,
                    } => (id, ok, lines, more),
                    other => panic!("not an answer: {other:?}"),
                }
            })
            .collect()
    }

    #[test]
    fn a_log_line_too_long_for_a_line_is_cut_and_says_so() {
        // M4 hosting plan, Task 11's carry-forward 1: a packet's hex or an error's chain can pass
        // the limit. The console gets the line cut, rather than one it must refuse. Quotes and
        // backslashes take two bytes each in JSON.
        let line = LogLine {
            at_ms: 5,
            level: Level::Trace,
            parts: vec![
                Part {
                    text: "sent ".into(),
                    name: false,
                },
                Part {
                    text: "Mai".into(),
                    name: true,
                },
                Part {
                    text: format!(" {}", "\"ab\\".repeat(5_000)),
                    name: false,
                },
            ],
        };
        let encoded = encode_server(ServerMsg::Log { line: line.clone() });
        let [only] = encoded.as_slice() else {
            panic!("one line, not {}", encoded.len())
        };
        assert!(only.len() <= LINE_MAX, "{} bytes", only.len());
        // Cut no more than it must: one character's bytes and the ellipsis at most.
        assert!(only.len() > LINE_MAX - 8, "{} bytes", only.len());
        let ServerMsg::Log { line: cut } = decode_server(only).unwrap() else {
            panic!("a log line")
        };
        assert_eq!((cut.at_ms, cut.level), (5, Level::Trace));
        assert_eq!(cut.parts[..2], line.parts[..2], "the name stays a name");
        let text = cut.message();
        assert!(text.starts_with("sent Mai \"ab\\\"ab\\"), "{text:.40}");
        assert!(text.ends_with('…'), "{}", &text[text.len() - 10..]);
    }

    #[test]
    fn an_answer_too_long_for_a_line_comes_in_parts_each_but_the_last_saying_more() {
        // Carry-forward 1: `invites` lists every invite ever made.
        let lines: Vec<String> = (0..300)
            .map(|n| format!("{n:03} {}", "z".repeat(60)))
            .collect();
        let encoded = encode_server(ServerMsg::Answer {
            id: 7,
            ok: false,
            lines: lines.clone(),
            more: false,
        });
        let parts = answers(&encoded);
        assert!(parts.len() > 1, "{} part", parts.len());
        let mut got = Vec::new();
        for (i, (id, ok, part, more)) in parts.into_iter().enumerate() {
            assert_eq!((id, ok), (7, false), "part {i}");
            assert_eq!(more, i + 1 < encoded.len(), "part {i}");
            got.extend(part);
        }
        assert_eq!(got, lines);
    }

    #[test]
    fn a_line_too_long_for_a_part_alone_is_cut_and_the_rest_kept() {
        let encoded = encode_server(ServerMsg::Answer {
            id: 1,
            ok: true,
            lines: vec!["first".into(), "w".repeat(10_000), "last".into()],
            more: false,
        });
        let got: Vec<String> = answers(&encoded)
            .into_iter()
            .flat_map(|(.., lines, _)| lines)
            .collect();
        assert_eq!(got.len(), 3, "{got:?}");
        assert_eq!((got[0].as_str(), got[2].as_str()), ("first", "last"));
        assert!(
            got[1].starts_with("www") && got[1].ends_with('…'),
            "{:.20}",
            got[1]
        );
        assert!(got[1].len() > 3_000, "cut no more than it must");
    }

    #[test]
    fn a_message_that_fits_goes_as_one_line_as_encode_makes_it() {
        for msg in [
            ServerMsg::Answer {
                id: 3,
                ok: true,
                lines: vec!["nobody is online".into()],
                more: false,
            },
            ServerMsg::Answer {
                id: 4,
                ok: false,
                lines: vec![],
                more: false,
            },
            ServerMsg::Skipped { lines: 37 },
            ServerMsg::Bye {
                reason: "the server is stopping".into(),
            },
        ] {
            assert_eq!(encode_server(msg.clone()), [encode(&msg)]);
        }
        // A whole answer says nothing of parts.
        let whole = ServerMsg::Answer {
            id: 3,
            ok: true,
            lines: vec![],
            more: false,
        };
        assert_eq!(
            encode(&whole),
            r#"{"type":"answer","id":3,"ok":true,"lines":[]}"#
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "a message that goes whole is")]
    fn a_hello_too_long_for_a_line_is_caught_in_a_debug_build() {
        // The hosting plan's Task 11 (#359): a hello goes whole, as every message but a log line
        // and an answer does, and a console refuses a line over the limit, so a server whose
        // hello didn't fit would turn every console away. The config keeps it short
        // (`tests/attach.rs`'s `the_longest_hello_and_status_a_config_allows_fit_one_line`); a
        // debug build says so if it ever isn't.
        let hello = ServerMsg::Hello {
            version: "0.5.0".into(),
            proto: PROTO,
            server: "x".repeat(LINE_MAX),
            commands: vec![],
        };
        encode_server(hello);
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
