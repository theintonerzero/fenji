//! The world's events and values (M4 world spec §3.9). Connection tasks turn the client's messages
//! into [`WorldIn`], and [`WorldOut`] back into messages, so the world never sees the wire. The
//! host's console speaks to it in the same events (M4 hosting spec §3.3; §8, HA5).
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};

use crate::commands::Command;

/// A connection's id: the gate's online id (`Online::join`).
pub type ConnId = u64;

/// An actor's id. A player's is its character's pid (world spec §2.1).
pub type ActorId = i64;

/// The largest id a player's actor may have: the client takes an id with any of its top 16 bits
/// set for an NPC's (world spec §2.1).
pub const MAX_PLAYER_ID: ActorId = (1 << 48) - 1;

/// Every player's speed rate in M4, as a percentage of a normal run (world spec §3.4).
pub const SPEED_RATE: i64 = 100;

/// A point: x, y and z (up) in the client's units (centimetres, Inferred), and a yaw in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pos {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f64,
}

/// A scene and a point in it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Location {
    pub scene: i64,
    pub pos: Pos,
}

/// What others see of a character. The world reads only the name and the vocation, for chat; the
/// connection side encodes the rest.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    pub name: String,
    pub vocation: i64,
    pub skeleton: i64,
    pub level: i64,
    /// The appearance as the store keeps it: the lobby's pickle (M3 spec D8).
    pub appearance: Arc<[u8]>,
}

/// A character entering the world, after its client's 10.
#[derive(Debug, Clone)]
pub struct Player {
    pub conn: ConnId,
    pub account: i64,
    /// Listed in `[access].admins`.
    pub admin: bool,
    pub pid: ActorId,
    pub look: Look,
    /// Where the store says it stands, or `[world]`'s start for a character that never entered.
    pub at: Location,
}

/// One move from a client. The world checks `pos`, and relays it with `detail`: the rest of the
/// move as the connection side decoded and bounded it (its mode or direction, say), never a
/// position. Viewers' connections encode each relay from these two, never from the client's
/// bytes, so a move can't carry anything the world didn't check.
#[derive(Debug, Clone, PartialEq)]
pub struct MoveStep {
    pub pos: Pos,
    pub detail: Arc<[u8]>,
}

/// The chat channels M4 carries (world spec §3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatChannel {
    Nearby,
    World,
    Whisper,
}

/// Whom a whisper is for: a pid, or a character name, to which the client may add `@<server>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhisperTarget {
    Pid(ActorId),
    Name(String),
}

/// A chat line from a player (115).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatIn {
    pub channel: ChatChannel,
    pub text: String,
    /// A whisper's target.
    pub to: Option<WhisperTarget>,
}

/// A player as a chat line names them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Speaker {
    pub pid: ActorId,
    pub name: String,
    pub vocation: i64,
}

/// A chat line for a client to show (378).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatOut {
    /// A player's line. A whisper names its target, and both ends get it.
    Said {
        channel: ChatChannel,
        from: Speaker,
        to: Option<Speaker>,
        text: String,
    },
    /// An admin's announcement, for everyone: the marquee.
    Announce { text: String },
    /// A line from the server to this player only, such as a command's answer.
    System { text: String },
}

/// Why a chat line was refused (270, with an `ERR_CHAT_*` code the connection side picks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRefusal {
    /// Empty, or with control characters or markup that the client's own input never sends.
    Invalid,
    /// Over 140 characters or 2,048 bytes.
    TooLong,
    /// Over the rate.
    TooFast,
    /// The whisper's target isn't in the game.
    NotOnline,
    /// A whisper to oneself.
    ToSelf,
    /// A channel M4 doesn't carry. The connection side refuses these itself.
    Channel,
}

/// Why the world let go of a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KickReason {
    /// The account entered again, on a newer connection.
    Replaced,
    /// An admin's `!kick`, the console's `kick`, or a ban.
    Kicked,
    /// The client stopped reading: its outbound channel filled.
    SlowReader,
    /// The connection sent more in one tick than the world takes.
    Flood,
    /// The server is stopping.
    Shutdown,
}

/// What connection tasks tell the world.
#[derive(Debug)]
pub enum WorldIn {
    /// A character enters. The world answers on `out`.
    Join {
        player: Player,
        out: mpsc::Sender<WorldOut>,
    },
    /// The client has loaded the scene (type 3).
    Loaded { conn: ConnId },
    /// The client asks for an actor it was told about (type 5).
    Pull { conn: ConnId, pid: ActorId },
    /// The player moved.
    Move { conn: ConnId, step: MoveStep },
    /// The player said something (115).
    Chat { conn: ConnId, line: ChatIn },
    /// A line for one player from outside the tick, such as `!invite`'s link.
    Tell { conn: ConnId, text: String },
    /// One of the shared commands from the host's console, which has no character (M4 hosting
    /// spec §3.3): the tick's admin phase runs it as an admin's, with its text as the host typed
    /// it, and answers on `reply`. Its `tp` takes two names.
    Console {
        order: Command,
        reply: oneshot::Sender<ConsoleReply>,
    },
    /// The account is an admin now, or no longer is (`op` and `deop`; M4 hosting spec §8, HA5):
    /// its player in the world, if any, runs commands from this tick, or no longer does, and so
    /// does its next join, whose link may have read the account before the change. `admin` is
    /// what `accounts::is_admin` says after the change, so a `deop` leaves an account that
    /// `[access].admins` names an admin.
    SetAdmin { account: i64, admin: bool },
    /// The host banned the account (HA5): its player in the world, if any, is let go as an
    /// admin's kick lets one go.
    Ban { account: i64 },
    /// The connection has ended.
    Leave { conn: ConnId },
    /// Save everyone and stop.
    Shutdown,
    /// A bug: the tick that takes it panics with this message, as a tick with a bug would, for
    /// the tests of Ruling R9. Built for tests only, so no connection can send it.
    #[cfg(test)]
    Panic(&'static str),
    /// A slow tick: the tick that takes it says so on `begun`, then works for `work`, so that a
    /// test knows what the tick after it takes (Ruling R10), or holds a tick that hangs. Built for
    /// tests only, so no connection can send it.
    #[cfg(test)]
    Stall {
        work: std::time::Duration,
        begun: tokio::sync::oneshot::Sender<()>,
    },
}

/// The world's answer to a command from the host's console: what it says, and whether the command
/// did what was asked. A refusal, such as "no one called Mai is in the game", isn't, so an attached
/// console's `--run` exits 1 for it (M4 hosting spec §3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleReply {
    pub ok: bool,
    pub text: String,
}

/// What the world tells a connection task.
#[derive(Debug, Clone, PartialEq)]
pub enum WorldOut {
    /// Load this scene and stand here (772): the first message after a join. A teleport to
    /// another scene would send it too, but the world refuses one for now (#280).
    LoadScene { pid: ActorId, at: Location },
    /// The player's own actor, once its client has loaded (type 7, then its speed, then 8).
    SelfEnter {
        pid: ActorId,
        look: Look,
        pos: Pos,
        speed: i64,
    },
    /// Another player came into view (6).
    Announce { pid: ActorId, pos: Pos },
    /// Another player, after the client pulled it (7).
    Enter {
        pid: ActorId,
        look: Look,
        pos: Pos,
        speed: i64,
    },
    /// Players that left the view or the game (9).
    Leave { pids: Vec<ActorId> },
    /// Another player moved.
    Moved { pid: ActorId, step: MoveStep },
    /// The player's move was refused: it stands here (20).
    Correct { pos: Pos },
    /// An admin moved the player within its scene.
    Teleport { pos: Pos },
    /// A chat line (378).
    Chat(ChatOut),
    /// The player's chat line was refused (270).
    ChatRefused(ChatRefusal),
    /// The world has let go of the connection, which the connection task then closes.
    Kicked(KickReason),
}

/// Work the world hands to the services task, off the tick. The tests compare the work queued.
#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq))]
pub enum Service {
    /// Save where a character stands.
    SavePlace { pid: ActorId, at: Location },
    /// Make an invite for `conn`'s `!invite`, and tell it the link.
    CreateInvite { conn: ConnId, label: Option<String> },
}
