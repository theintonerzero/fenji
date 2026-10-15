//! What the view asks of the model (plan LP1): one tagged enum, read from the shell's one command.
//! `apps/launcher/fixtures/intents.json` holds an example of each, which Rust reads here and the
//! view's TypeScript type-checks.
use std::fmt;

use serde::{Deserialize, Serialize};

use super::snapshot::LevelView;

/// A password: shown as "…" by `Debug`, read from JSON as a plain string. It lives in its intent
/// and is dropped after the request it's for.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct Secret(pub String);

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(…)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum Intent {
    AddServer {
        text: String,
    },
    RemoveServer {
        id: u32,
    },
    SelectServer {
        id: u32,
    },
    Refresh,
    ViewTab {
        tab: Tab,
    },
    Focus {
        focused: bool,
    },
    Redeem {
        code: Option<String>,
        username: String,
        password: Secret,
    },
    SignIn {
        username: String,
        password: Secret,
        remember: bool,
    },
    SignOut,
    CancelSignIn,
    /// Trust the key the last link offered for this server (its `keyChange`).
    UseNewKey {
        id: u32,
    },
    /// Keep the saved key, and drop the link that offered another.
    KeepKey,
    ChooseFolder {
        path: String,
    },
    Check {
        level: LevelView,
    },
    StopCheck,
    Download {
        folder: String,
    },
    CancelDownload,
    Repair,
    Play,
    PlayOn {
        id: u32,
    },
    DismissNotice,
    SetAutoUpdate {
        on: bool,
    },
    SetUmuRun {
        path: Option<String>,
    },
    // The shell's own (plan LP10, LP11): the model answers them with `IntentError::ForTheShell`.
    PickFolder {
        purpose: FolderPurpose,
    },
    OpenUrl {
        url: String,
    },
    OpenLogFolder,
    Minimize,
    Close,
    CheckUpdate,
    InstallUpdate,
    DismissUpdate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum Tab {
    Play,
    Servers,
    Account,
    Client,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum FolderPurpose {
    Game,
    Download,
}

/// An intent's name, as its JSON `type` spells it.
pub fn name(i: &Intent) -> &'static str {
    match i {
        Intent::AddServer { .. } => "addServer",
        Intent::RemoveServer { .. } => "removeServer",
        Intent::SelectServer { .. } => "selectServer",
        Intent::Refresh => "refresh",
        Intent::ViewTab { .. } => "viewTab",
        Intent::Focus { .. } => "focus",
        Intent::Redeem { .. } => "redeem",
        Intent::SignIn { .. } => "signIn",
        Intent::SignOut => "signOut",
        Intent::CancelSignIn => "cancelSignIn",
        Intent::UseNewKey { .. } => "useNewKey",
        Intent::KeepKey => "keepKey",
        Intent::ChooseFolder { .. } => "chooseFolder",
        Intent::Check { .. } => "check",
        Intent::StopCheck => "stopCheck",
        Intent::Download { .. } => "download",
        Intent::CancelDownload => "cancelDownload",
        Intent::Repair => "repair",
        Intent::Play => "play",
        Intent::PlayOn { .. } => "playOn",
        Intent::DismissNotice => "dismissNotice",
        Intent::SetAutoUpdate { .. } => "setAutoUpdate",
        Intent::SetUmuRun { .. } => "setUmuRun",
        Intent::PickFolder { .. } => "pickFolder",
        Intent::OpenUrl { .. } => "openUrl",
        Intent::OpenLogFolder => "openLogFolder",
        Intent::Minimize => "minimize",
        Intent::Close => "close",
        Intent::CheckUpdate => "checkUpdate",
        Intent::InstallUpdate => "installUpdate",
        Intent::DismissUpdate => "dismissUpdate",
    }
}

/// Every intent's name, in the enum's order.
pub const ALL: &[&str] = &[
    "addServer",
    "removeServer",
    "selectServer",
    "refresh",
    "viewTab",
    "focus",
    "redeem",
    "signIn",
    "signOut",
    "cancelSignIn",
    "useNewKey",
    "keepKey",
    "chooseFolder",
    "check",
    "stopCheck",
    "download",
    "cancelDownload",
    "repair",
    "play",
    "playOn",
    "dismissNotice",
    "setAutoUpdate",
    "setUmuRun",
    "pickFolder",
    "openUrl",
    "openLogFolder",
    "minimize",
    "close",
    "checkUpdate",
    "installUpdate",
    "dismissUpdate",
];
