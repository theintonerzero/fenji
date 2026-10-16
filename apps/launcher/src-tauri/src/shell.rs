//! The shell (launcher spec §3.1, §3.9; plan LP1, LP10, LP11). One command takes every intent and
//! one gives the current snapshot; each new snapshot reaches the view as the `snapshot` event. The
//! few things only the shell can do (the folder dialog, links, the window) happen here when the
//! view asks; the model does the rest.
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use fenji_launcher::app::{App, AppConfig, FolderPurpose, Intent, Snapshot};
use fenji_launcher::appimage;
use fenji_launcher::manifest::{Manifest, PINNED_BUILD};
use fenji_launcher::settings::{self, Os};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::EnvFilter;

use crate::updates::Updates;

const NOT_OURS: &str = "Fenji only opens its own pages and your servers' source code.";
/// Open the third-party notices in a build that doesn't carry them: a local build (#341).
const NO_NOTICES: &str = "This build has no third-party notices file. Fenji's releases include it.";
/// The launcher's log keeps this many days (the plan's Global Constraints).
const LOG_DAYS: usize = 7;

/// Which side handles an intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// The shell's own (plan LP10, LP11).
    Shell,
    Model,
}

/// Where each intent goes. Every intent is named, so a new one needs a decision here.
pub fn route(intent: &Intent) -> Route {
    match intent {
        Intent::PickFolder { .. }
        | Intent::OpenUrl { .. }
        | Intent::OpenLogFolder
        | Intent::OpenNotices
        | Intent::Minimize
        | Intent::Close
        | Intent::CheckUpdate
        | Intent::InstallUpdate
        | Intent::DismissUpdate => Route::Shell,
        Intent::AddServer { .. }
        | Intent::RemoveServer { .. }
        | Intent::SelectServer { .. }
        | Intent::Refresh
        | Intent::ViewTab { .. }
        | Intent::Focus { .. }
        | Intent::Redeem { .. }
        | Intent::SignIn { .. }
        | Intent::SignOut
        | Intent::CancelSignIn
        | Intent::UseNewKey { .. }
        | Intent::KeepKey
        | Intent::ChooseFolder { .. }
        | Intent::Check { .. }
        | Intent::StopCheck
        | Intent::Download { .. }
        | Intent::CancelDownload
        | Intent::Repair
        | Intent::Play
        | Intent::PlayOn { .. }
        | Intent::CancelStart
        | Intent::DismissNotice { .. }
        | Intent::SetAutoUpdate { .. }
        | Intent::SetUmuRun { .. }
        | Intent::SetDownloadFolder { .. } => Route::Model,
    }
}

/// What the shell keeps: the model, where its log is, and the updater's state.
struct Shell {
    model: App,
    logs: PathBuf,
    updates: Updates,
}

/// Holds the log writer's thread until the launcher ends, so the last lines are written.
struct LogGuard(#[allow(dead_code)] WorkerGuard);

/// Every intent the view sends: `Ok` when it was done, otherwise its sentence.
#[tauri::command]
async fn intent(app: AppHandle, shell: State<'_, Shell>, intent: Intent) -> Result<(), String> {
    match route(&intent) {
        Route::Model => shell.model.intent(intent).await.map_err(|e| e.to_string()),
        Route::Shell => shell.handle(&app, intent).await,
    }
}

/// The current snapshot, for the view's first draw.
#[tauri::command]
fn snapshot(shell: State<'_, Shell>) -> Snapshot {
    shell.model.snapshot()
}

impl Shell {
    async fn handle(&self, app: &AppHandle, intent: Intent) -> Result<(), String> {
        match intent {
            Intent::PickFolder { purpose } => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                app.dialog().file().pick_folder(move |picked| {
                    let _ = tx.send(picked);
                });
                // Closed without a choice: nothing to do.
                let Some(picked) = rx.await.ok().flatten() else {
                    return Ok(());
                };
                let path = picked
                    .into_path()
                    .map_err(|e| e.to_string())?
                    .display()
                    .to_string();
                let chosen = match purpose {
                    FolderPurpose::Game => Intent::ChooseFolder { path },
                    // "Change" beside the download folder: where Start download saves, not a
                    // download (the final review).
                    FolderPurpose::Download => Intent::SetDownloadFolder { path },
                };
                self.model.intent(chosen).await.map_err(|e| e.to_string())
            }
            Intent::OpenUrl { url } => {
                if !self.model.allowed_url(&url) {
                    self.model.note(NOT_OURS.into(), true);
                    return Err(NOT_OURS.into());
                }
                open(app, &url, false)
            }
            Intent::OpenLogFolder => open(app, &self.logs.display().to_string(), true),
            Intent::OpenNotices => {
                // A release bundle carries them at its resources' root (#341); a local build
                // doesn't.
                let notices = app
                    .path()
                    .resource_dir()
                    .map(|dir| dir.join("THIRD-PARTY-NOTICES.txt"))
                    .ok()
                    .filter(|path| path.is_file());
                match notices {
                    Some(path) => open(app, &path.display().to_string(), true),
                    None => {
                        self.model.note(NO_NOTICES.into(), false);
                        Ok(())
                    }
                }
            }
            Intent::Minimize => app
                .get_webview_window("main")
                .ok_or("the launcher's window isn't open")?
                .minimize()
                .map_err(|e| e.to_string()),
            Intent::Close => {
                self.updates.close(app, &self.model);
                Ok(())
            }
            Intent::CheckUpdate => {
                check_update(app.clone(), true);
                Ok(())
            }
            Intent::InstallUpdate => self.updates.install(app, &self.model),
            Intent::DismissUpdate => {
                self.updates.dismiss(&self.model);
                Ok(())
            }
            other => self.model.intent(other).await.map_err(|e| e.to_string()),
        }
    }
}

/// Opens a link, or a folder, in the desktop's own program. Inside an AppImage, `xdg-open` starts
/// with the environment from outside it (#329), which the opener plugin would pass on.
fn open(app: &AppHandle, target: &str, folder: bool) -> Result<(), String> {
    if let Some(opened) = appimage::open(OsStr::new(target), std::env::vars_os()) {
        return opened.map_err(|e| e.to_string());
    }
    let opener = app.opener();
    if folder {
        opener.open_path(target, None::<&str>)
    } else {
        opener.open_url(target, None::<&str>)
    }
    .map_err(|e| e.to_string())
}

/// The model's configuration for this machine. A missing config or data folder is fatal.
fn config() -> Result<AppConfig, String> {
    let config_dir = settings::config_dir().ok_or(
        "Fenji couldn't find a folder for its settings. Set HOME (APPDATA on Windows) and start it again.",
    )?;
    let data_dir = settings::data_dir().ok_or(
        "Fenji couldn't find a folder for its data. Set HOME (APPDATA on Windows) and start it again.",
    )?;
    Ok(AppConfig {
        config_dir,
        data_dir,
        home: std::env::var_os("HOME").map(PathBuf::from),
        os: Os::this(),
        version: env!("CARGO_PKG_VERSION").into(),
        manifest: Manifest::pinned(),
        pinned_build: PINNED_BUILD.into(),
        env: std::env::vars_os().collect(),
        depot_downloader: None,
        terminal: None,
        umu_run: None,
    })
}

/// The launcher's log: `<data dir>/logs/launcher.<date>.log`, a file a day, 7 kept. Nothing secret
/// reaches it: the model's types never show a password, token or ticket.
fn logging(logs: &Path) -> Result<WorkerGuard, Box<dyn std::error::Error>> {
    std::fs::create_dir_all(logs)?;
    let file = Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix("launcher")
        .filename_suffix("log")
        .max_log_files(LOG_DAYS)
        .build(logs)?;
    let (writer, guard) = tracing_appender::non_blocking(file);
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .try_init()
        .map_err(|e| e.to_string())?;
    Ok(guard)
}

/// Starts the model, forwards its snapshots to the view, and opens the window.
fn start(app: &mut tauri::App, cfg: AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    let logs = cfg.data_dir.join("logs");
    let guard = logging(&logs)?;
    tracing::info!(version = %cfg.version, "the launcher starts");
    let model = App::start(cfg, tauri::async_runtime::handle().inner().clone());
    let mut snapshots = model.subscribe();
    let handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        while snapshots.changed().await.is_ok() {
            let snapshot = snapshots.borrow_and_update().clone();
            if let Err(e) = handle.emit("snapshot", snapshot) {
                tracing::warn!(error = %e, "a snapshot didn't reach the view");
            }
        }
    });
    let auto_update = model.snapshot().settings.auto_update;
    app.manage(Shell {
        model,
        logs,
        updates: Updates::default(),
    });
    app.manage(LogGuard(guard));
    if auto_update {
        check_update(app.handle().clone(), false);
    }
    let window = app
        .config()
        .app
        .windows
        .first()
        .ok_or("tauri.conf.json names no window")?
        .clone();
    tauri::WebviewWindowBuilder::from_config(app.handle(), &window)?.build()?;
    Ok(())
}

/// Checks for a launcher update in the background (spec §3.8): at start with "Update
/// automatically" on, or when the player asks.
fn check_update(app: AppHandle, asked: bool) {
    tauri::async_runtime::spawn(async move {
        let shell = app.state::<Shell>();
        shell.updates.check(&app, &shell.model, asked).await;
    });
}

/// What the player is told when the launcher can't start, if it can't: its settings and folders
/// couldn't be read (`config`'s words), or `start` failed, such as its log folder that can't be made
/// or a window that won't open. Shown in a native dialog, never as a silent panic (#348).
fn setup_failure<C>(
    config: Result<C, String>,
    start: impl FnOnce(C) -> Result<(), Box<dyn std::error::Error>>,
) -> Option<String> {
    match config {
        Ok(cfg) => start(cfg)
            .err()
            .map(|e| format!("Fenji's launcher couldn't start: {e}")),
        Err(why) => Some(why),
    }
}

pub fn run() {
    tauri::Builder::default()
        // First, so a second start hands over to the running one before it does anything else.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![intent, snapshot])
        .setup(|app| {
            // Before the window opens, or as it fails to: a native dialog, then the launcher ends.
            if let Some(why) = setup_failure(config(), |cfg| start(app, cfg)) {
                tracing::error!(%why, "the launcher couldn't start");
                let handle = app.handle().clone();
                app.dialog()
                    .message(why)
                    .title("Fenji")
                    .kind(MessageDialogKind::Error)
                    .show(move |_| handle.exit(1));
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Fenji's launcher couldn't run");
}

#[cfg(test)]
mod tests {
    use fenji_launcher::app::{ALL, name};

    use super::*;

    /// The shell's own intents (plan LP10, LP11); the model takes the rest.
    const SHELL: &[&str] = &[
        "pickFolder",
        "openUrl",
        "openLogFolder",
        "openNotices",
        "minimize",
        "close",
        "checkUpdate",
        "installUpdate",
        "dismissUpdate",
    ];

    #[test]
    fn a_start_that_fails_ends_in_a_dialog_not_a_silent_panic() {
        // #348: an error from start() left setup and ended the run in a panic, with no window and,
        // on Windows, no console: the player saw nothing.
        let failed = |_: u8| -> Result<(), Box<dyn std::error::Error>> {
            Err("the log folder can't be made".into())
        };
        assert_eq!(
            setup_failure(Ok(1), failed).as_deref(),
            Some("Fenji's launcher couldn't start: the log folder can't be made")
        );
        assert_eq!(
            setup_failure(Err::<u8, _>("no home folder".into()), failed).as_deref(),
            Some("no home folder")
        );
        assert_eq!(setup_failure(Ok(1), |_| Ok(())), None);
    }

    #[test]
    fn every_intent_goes_to_one_side() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/intents.json");
        let examples: Vec<Intent> =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mut routed = std::collections::BTreeMap::new();
        for example in &examples {
            routed.insert(name(example), route(example));
        }
        assert_eq!(routed.len(), ALL.len(), "an intent with no example");
        for (intent, side) in routed {
            let want = if SHELL.contains(&intent) {
                Route::Shell
            } else {
                Route::Model
            };
            assert_eq!(side, want, "{intent}");
        }
    }
}
