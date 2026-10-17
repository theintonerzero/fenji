//! What the view is shown (launcher spec §3.1): the whole state as one `Snapshot`, published on
//! every change. It never holds a password, a session token or a ticket. Every type here has a
//! TypeScript twin in `apps/launcher/src/gen/`, written by ts-rs when the tests run (plan LP13).
use serde::{Deserialize, Serialize};

use crate::verify::Problem;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct Snapshot {
    pub launcher_version: String,
    pub platform: PlatformView,
    pub servers: Vec<ServerView>,
    pub active: Option<u32>,
    /// Who's online on the active server; `None` unless signed in there.
    pub lobby: Option<Vec<String>>,
    /// Play is waiting for a sign-in (plan LP8).
    pub need_sign_in: bool,
    pub pending_invite: Option<PendingInvite>,
    /// A link offered another key for a saved server: nothing changes until the player chooses.
    pub key_change: Option<KeyChangeView>,
    pub client: ClientView,
    pub job: Option<JobView>,
    pub game: GameView,
    pub update: UpdateView,
    pub settings: SettingsView,
    pub notice: Option<Notice>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct PlatformView {
    /// `linux`, `windows`, `macos` or `other`.
    pub os: String,
    pub can_play: bool,
    pub can_download: bool,
    /// The player's home folder, which the view shows paths under as `~`, for display only:
    /// intents carry whole paths. None on Windows, where `~` means nothing to a player.
    pub home: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct ServerView {
    pub id: u32,
    /// The server's own name, or its shown address until it has answered once.
    pub name: String,
    /// `moonward.example.net`, `203.0.113.5:7801`: the origin without its scheme.
    pub address: String,
    /// Whether its own key is pinned (hosting spec §3.10).
    pub pinned: bool,
    pub host: Option<String>,
    /// Where the server is, when its host set it (hosting spec Amendment C, HC5).
    pub region: Option<String>,
    pub status: StatusView,
    pub players: Option<PlayersView>,
    pub motd: String,
    pub ping_ms: Option<u32>,
    pub last_seen_ms: Option<i64>,
    pub version: Option<String>,
    pub source: Option<String>,
    pub username: Option<String>,
    pub signed_in: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum StatusView {
    /// Not probed yet.
    Unknown,
    Online,
    Maintenance {
        message: String,
        until: Option<i64>,
    },
    Offline,
    KeyChanged,
    OwnKey,
    /// Another build of the game, or (`newer`) a newer Fenji than this launcher knows.
    Incompatible {
        newer: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct PlayersView {
    pub online: u32,
    pub max: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct PendingInvite {
    pub server_id: u32,
    /// `InviteCode::display()`: `K7QM-3XRD-VN2P-HT8B`.
    pub code: String,
}

/// A saved server and a link that offers another key for it (or a key, when it had none).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct KeyChangeView {
    pub server_id: u32,
    pub name: String,
    pub had_key: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct ClientView {
    pub folder: Option<String>,
    pub default_folder: String,
    pub build: String,
    pub files: usize,
    pub check: CheckView,
    /// DepotDownloader for the player's own terminal, wherever a download works (spec §3.12).
    pub manual: Option<ManualView>,
    /// The copy Steam installed, if Steam's files name one (Amendment A, LA1).
    pub steam: Option<SteamCopyView>,
    /// Whether the chosen folder is in a Steam library, which Steam looks after (LA2).
    pub in_steam: bool,
}

/// A copy Steam installed: where, at which build, and whether that's the pinned one.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct SteamCopyView {
    pub folder: String,
    pub build: String,
    pub pinned: bool,
}

/// DepotDownloader's commands for the player's own terminal, each on one line, their own Steam
/// login left for them to type (#348).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct ManualView {
    /// The download into the folder.
    pub download: String,
    /// The repair of the files the last check found wrong, which Fenji listed in a file of its
    /// own, with `-validate`: none unless the copy needs repair.
    pub repair: Option<String>,
    /// Where to get DepotDownloader first, when Fenji hasn't fetched it.
    pub get: Option<ToolLinkView>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct ToolLinkView {
    pub version: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum CheckView {
    None,
    FolderMissing,
    Verified {
        level: LevelView,
        at_ms: i64,
    },
    NeedsRepair {
        fine: usize,
        files: Vec<FileProblem>,
        at_ms: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct FileProblem {
    pub path: String,
    pub problem: Problem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum LevelView {
    Quick,
    Full,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum JobView {
    Checking {
        level: LevelView,
        files_done: usize,
        files_total: usize,
        percent: f32,
    },
    Fetching {
        tool: String,
        percent: Option<f32>,
    },
    Downloading {
        repair: bool,
        percent: Option<f32>,
        file: Option<String>,
        waiting_for_steam: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum GameView {
    Idle,
    /// `first_start`: Proton sets the Wine prefix up first, on the first start, or the first with
    /// a new Proton (the final review of 0.5.5's Amendment B work, I4).
    Starting {
        first_start: bool,
    },
    Running {
        since_ms: i64,
    },
    Ended {
        code: i32,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct UpdateView {
    pub current: String,
    pub auto: bool,
    pub checking: bool,
    pub available: Option<String>,
    pub ready: bool,
    pub notes: Option<String>,
    pub show_dialog: bool,
    /// The last check found nothing newer: the column's foot says "up to date" (the boards).
    pub up_to_date: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct SettingsView {
    pub auto_update: bool,
    /// GameMode's switch, where GameMode is installed, and none where it isn't (launcher spec
    /// Amendment B, LB2).
    pub gamemode: Option<bool>,
    pub launch_options: String,
    pub umu_run: Option<String>,
    pub log_folder: String,
}

/// The last thing that went wrong (or a word from the shell), in words (spec §3.12).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct Notice {
    pub text: String,
    pub error: bool,
}
