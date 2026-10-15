//! Chat's script messages (docs/protocol/packets/chat.md): the line a player says (115), the line a
//! client shows (378), and a refusal (270) with the `errors` sheet's chat codes, as the client's
//! own scripts send and show them (`CHAT.SendMessageToServer` in `chat.luac`, `on_chat_message`
//! in `chat_receive.luac`). This module builds and parses; the world decides who hears what, and
//! checks every line's text.
use crate::lobby;
use crate::packet::script_body;
use crate::pickle::{Table, Value};

/// `PROTOC2S_CHAT_TRANSMIT (channel, text, extra)`: a line a player says (chat.md §1). The client
/// sends it as a type-0 script message.
pub const CHAT_TRANSMIT: i64 = 115;
/// `PROTOS2C_SYS_CHAT_MESSAGE (channel, sender, content, receiver, options, template)`: a line for
/// the client to show (chat.md §2).
pub const SYS_CHAT_MESSAGE: i64 = 378;
/// `PROTOC2S_CHATBOX_SETTINGS_CHANGE (settings)`: the chat box's settings, which the client sends
/// while property 389 is unset (chat.md §4). Fenji keeps none of it in M4.
pub const CHATBOX_SETTINGS_CHANGE: i64 = 422;
/// The server group every player link names (chat.md 2.2): Fenji is one server, and a player's own
/// `prop_base_group_id` (31), which Fenji never sets, reads 0, so names show without the "※" that
/// marks a player from another server.
pub const GROUP_ID: i64 = 0;

/// The client's channel numbers (`enum_base.luac`; chat.md §3).
pub mod channel {
    /// `CHANNEL_NEARBY`: the players who see the speaker.
    pub const NEARBY: i64 = 0;
    /// `CHANNEL_PERSONAL`: a whisper.
    pub const WHISPER: i64 = 1;
    /// `CHANNEL_WORLD`: everyone in the game.
    pub const WORLD: i64 = 5;
    /// `CHANNEL_SYSTEM_FRAME_YELLOW`: a chat-box line labelled System, with no sender, as the
    /// original server's scripts addressed one player (`Say(64, …)`).
    pub const SYSTEM: i64 = 64;
    /// `CHANNEL_SYSTEM_GOROUND`: the marquee across the top of the screen, and a line in every chat
    /// tab.
    pub const MARQUEE: i64 = 66;
}

/// Rows of the client's `errors` sheet, by `literal` without its `ERR_`, as [`lobby::code`] names
/// its own (chat.md §5). The English texts are the client's.
pub mod code {
    /// `ERR_CHAT_INVALID_CONTENT`: "The message cannot be sent".
    pub const CHAT_INVALID_CONTENT: i64 = 8_639_093_037_559_110_139;
    /// `ERR_CHAT_MAX_CHAR`: "The number of words in the chat exceeds the limit".
    pub const CHAT_MAX_CHAR: i64 = -6_664_778_746_412_662_761;
    /// `ERR_CHAT_TOO_FAST`: "You're typing too fast!".
    pub const CHAT_TOO_FAST: i64 = -7_815_789_390_081_211_457;
    /// `ERR_CHAT_RECEIVER_OFFLINE`: "The person you are whispering to is not online".
    pub const CHAT_RECEIVER_OFFLINE: i64 = 2_418_279_538_352_872_837;
    /// `ERR_CHAT_TALK_TOSELF`: "You cannot whisper to yourself".
    pub const CHAT_TALK_TOSELF: i64 = 437_753_046_416_001_599;
    /// `ERR_CHAT_INVALID_CHANNEL`: "Invalid chat channel".
    pub const CHAT_INVALID_CHANNEL: i64 = 1_246_893_794_701_349_639;
}

/// The channels M4 carries (chat.md §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Nearby,
    Whisper,
    World,
}

impl Channel {
    /// The client's number for it.
    pub fn number(self) -> i64 {
        match self {
            Self::Nearby => channel::NEARBY,
            Self::Whisper => channel::WHISPER,
            Self::World => channel::WORLD,
        }
    }

    fn from_number(n: i64) -> Option<Self> {
        match n {
            channel::NEARBY => Some(Self::Nearby),
            channel::WHISPER => Some(Self::Whisper),
            channel::WORLD => Some(Self::World),
            _ => None,
        }
    }
}

/// Whom a whisper is for, as its `extra` names them (chat.md 1.4): a pid, or a name, to which the
/// client adds `@` and a server name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Pid(i64),
    Name(String),
}

/// 115 as the server takes it: the channel, the text as the client's own input wrote it (chat.md
/// 1.5), unchecked, and a whisper's target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transmit {
    pub channel: Channel,
    pub text: String,
    pub to: Option<Target>,
}

/// Why a line is refused, and the client's code for it (chat.md §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Not text, or text the server doesn't pass on: `ERR_CHAT_INVALID_CONTENT`.
    Invalid,
    /// Over the client's limits: `ERR_CHAT_MAX_CHAR`.
    TooLong,
    /// Over the server's rate: `ERR_CHAT_TOO_FAST`.
    TooFast,
    /// A whisper to no one in the game: `ERR_CHAT_RECEIVER_OFFLINE`.
    NotOnline,
    /// A whisper to oneself: `ERR_CHAT_TALK_TOSELF`.
    ToSelf,
    /// A channel M4 doesn't carry: `ERR_CHAT_INVALID_CHANNEL`.
    Channel,
}

impl Refusal {
    pub fn code(self) -> i64 {
        match self {
            Self::Invalid => code::CHAT_INVALID_CONTENT,
            Self::TooLong => code::CHAT_MAX_CHAR,
            Self::TooFast => code::CHAT_TOO_FAST,
            Self::NotOnline => code::CHAT_RECEIVER_OFFLINE,
            Self::ToSelf => code::CHAT_TALK_TOSELF,
            Self::Channel => code::CHAT_INVALID_CHANNEL,
        }
    }
}

/// An integer, or a float with no fraction, as the client's `tonumber` can give either.
fn whole(v: &Value) -> Option<i64> {
    match v {
        Value::Int(i) => Some(*i),
        Value::Float(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => Some(*f as i64),
        _ => None,
    }
}

fn get<'a>(t: &'a Table, key: &str) -> Option<&'a Value> {
    t.hash
        .iter()
        .find(|(k, _)| matches!(k, Value::Str(b) if b == key.as_bytes()))
        .map(|(_, v)| v)
}

/// A whisper's target from its `extra` (chat.md 1.4): the pid when it is a whole number, which the
/// client has when the target was clicked, else the name. `None` when neither is usable: the world
/// refuses that as not online.
fn target(extra: Option<&Value>) -> Option<Target> {
    let Some(Value::Table(t)) = extra else {
        return None;
    };
    if let Some(pid) = get(t, "pid").and_then(whole) {
        return Some(Target::Pid(pid));
    }
    match get(t, "name") {
        Some(Value::Str(b)) => String::from_utf8(b.clone()).ok().map(Target::Name),
        _ => None,
    }
}

/// 115's arguments, after its id (chat.md §1): the channel, which must be the integer 0, 1 or 5,
/// else [`Refusal::Channel`]; the text, which must be a string of UTF-8, else
/// [`Refusal::Invalid`]; and for a whisper, `extra`'s target. A trailing nil, or nothing, after the
/// text are the same, and values after `extra` are ignored. The text is passed on as it came: the
/// world checks it.
pub fn parse_transmit(args: &[Value]) -> Result<Transmit, Refusal> {
    let channel = match args.first() {
        Some(Value::Int(n)) => Channel::from_number(*n).ok_or(Refusal::Channel)?,
        _ => return Err(Refusal::Channel),
    };
    let text = match args.get(1) {
        Some(Value::Str(b)) => String::from_utf8(b.clone()).map_err(|_| Refusal::Invalid)?,
        _ => return Err(Refusal::Invalid),
    };
    let to = match channel {
        Channel::Whisper => target(args.get(2)),
        Channel::Nearby | Channel::World => None,
    };
    Ok(Transmit { channel, text, to })
}

/// A player as a line names them: the player link's fields (chat.md 2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Player<'a> {
    pub pid: i64,
    pub name: &'a str,
    pub vocation: i64,
}

/// `name` as `EscapeLinkParam` writes it (chat.md 2.2): a space, `'`, `"` and `\` as `\` and the
/// byte in three decimal digits, which `ReverseLinkParam` reads back.
fn link_param(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            ' ' | '\'' | '"' | '\\' => out.push_str(&format!("\\{:03}", c as u32)),
            _ => out.push(c),
        }
    }
    out
}

/// The player link `GetPlayerLink(pid, name, vocation, gid)` builds (chat.md 2.2), with
/// [`GROUP_ID`] and nothing optional: no guild, faction or GM mark. The client reads the pid, the
/// name and the group from it to show a line, to answer a whisper and to put a bubble over the
/// speaker, and takes the colour from the vocation.
pub fn player_link(p: &Player) -> String {
    format!(
        " [link type=\"player\" pid=\"{}\" name=\"{}\" vocation=\"{}\" gid=\"{GROUP_ID}\"] ",
        p.pid,
        link_param(p.name),
        p.vocation
    )
}

/// A line the server writes itself, made to show as written (chat.md 2.5): `&`, `<` and `>` as the
/// entities the client's own input writes for them (`EscapeHtmlChar`), since the line becomes
/// HTML, and a `\` before each bracket, which the client's tag parser reads as the bracket itself.
fn server_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '[' => out.push_str("\\["),
            ']' => out.push_str("\\]"),
            _ => out.push(c),
        }
    }
    out
}

/// A line for a client to show (chat.md §2, §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line<'a> {
    /// A player's line, on its channel. A whisper names its target, and both ends get the same
    /// line: the client shows "You tell …" when the sender is its own player (chat.md 2.4).
    Said {
        channel: Channel,
        from: Player<'a>,
        to: Option<Player<'a>>,
        text: &'a str,
    },
    /// An admin's announcement, for everyone: the marquee (66).
    Announce { text: &'a str },
    /// A line from the server to one player, such as a command's answer (64).
    System { text: &'a str },
}

impl Line<'_> {
    /// The channel the line goes on (chat.md §3).
    pub fn channel(&self) -> i64 {
        match self {
            Self::Said { channel, .. } => channel.number(),
            Self::Announce { .. } => channel::MARQUEE,
            Self::System { .. } => channel::SYSTEM,
        }
    }
}

/// 378 for `line` (chat.md §2): the channel, the sender's link, the content, the receiver's link,
/// and no options or template. A line from the server names no sender and no receiver (`""`), as
/// the client's own error lines do. A player's text and an announcement go as they came, already
/// in the client's form and checked by the world; the server's own text is made to show as written
/// first.
pub fn say(line: &Line) -> Vec<u8> {
    let (sender, content, receiver) = match *line {
        Line::Said { from, to, text, .. } => (
            player_link(&from),
            text.to_string(),
            to.map(|p| player_link(&p)).unwrap_or_default(),
        ),
        Line::Announce { text } => (String::new(), text.to_string(), String::new()),
        Line::System { text } => (String::new(), server_text(text), String::new()),
    };
    script_body(
        SYS_CHAT_MESSAGE,
        &[
            Value::Int(line.channel()),
            Value::Str(sender.into_bytes()),
            Value::Str(content.into_bytes()),
            Value::Str(receiver.into_bytes()),
            Value::Nil,
            Value::Nil,
        ],
    )
    .expect("an integer, three strings and two nils")
}

/// 270 with the refusal's code (chat.md §5): the client shows its text as red header text.
pub fn refusal(r: Refusal) -> Vec<u8> {
    lobby::error_code(r.code())
}
