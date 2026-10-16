//! Self-updates (launcher spec §3.8; L5, L6; plan LP11). Tauri's updater looks for a newer
//! launcher in Fenji's latest release, checks its signature with the release key built into the
//! launcher, downloads it in the background, and installs it when the player says so, then
//! restarts. The view sees only the model's `UpdateView`, which [`view`] makes from what the shell
//! knows.
use std::sync::{Mutex, MutexGuard};

use fenji_launcher::app::{App, UpdateView};
use tauri::AppHandle;
use tauri_plugin_updater::{Update, UpdaterExt};

/// A check the player asked for, by "Check now", that failed (spec §3.8).
pub const CHECK_FAILED: &str = "Fenji couldn't check for updates. Try again later.";
/// "Restart and update" when the downloaded update couldn't be installed.
pub const INSTALL_FAILED: &str = "Fenji couldn't install the update. Try again later.";

/// An update the updater found: its version, its notes, and whether it's downloaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub version: String,
    pub notes: Option<String>,
    pub ready: bool,
}

/// What the view shows. The dialog shows only for a downloaded version whose dialog the player
/// hasn't seen (spec §3.8: once a version), and "up to date" only when the last check found nothing
/// newer (`up_to_date`), never beside an update.
pub fn view(
    current: &str,
    auto: bool,
    checking: bool,
    found: Option<&Found>,
    seen: Option<&str>,
    up_to_date: bool,
) -> UpdateView {
    UpdateView {
        current: current.into(),
        auto,
        checking,
        available: found.map(|f| f.version.clone()),
        ready: found.is_some_and(|f| f.ready),
        notes: found.and_then(|f| f.notes.clone()),
        show_dialog: found.is_some_and(|f| f.ready && seen != Some(f.version.as_str())),
        up_to_date: up_to_date && found.is_none(),
    }
}

/// What the updater knows apart from the update and its bytes: the moves a check, a download,
/// "Later" and closing make, kept pure so their sequences are tested (the final review's
/// Important 1, which the view alone didn't catch).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct State {
    checking: bool,
    found: Option<Found>,
    up_to_date: bool,
    /// The player asked for this check ("Check now", or Play's "Check for updates"): its update's
    /// dialog shows even for a version they said "Later" to.
    asked: bool,
}

impl State {
    /// A check begins, unless one is running.
    fn begin(&mut self) -> bool {
        !std::mem::replace(&mut self.checking, true)
    }

    /// The check found nothing newer.
    fn current(&mut self) {
        self.checking = false;
        self.up_to_date = true;
    }

    /// The check failed: what was found before stays.
    fn failed(&mut self) {
        self.checking = false;
    }

    /// The check found `version`. Whether it must download: not when the bytes this run holds are
    /// that version's already.
    fn found(&mut self, version: &str, notes: Option<String>, asked: bool) -> bool {
        self.checking = false;
        self.up_to_date = false;
        self.asked = asked;
        let held = self
            .found
            .as_ref()
            .is_some_and(|f| f.ready && f.version == version);
        if !held {
            self.found = Some(Found {
                version: version.into(),
                notes,
                ready: false,
            });
        }
        !held
    }

    /// The found version's bytes are here.
    fn downloaded(&mut self) {
        if let Some(found) = &mut self.found {
            found.ready = true;
        }
    }

    /// The download, or the install, failed: nothing is held.
    fn lost(&mut self) {
        self.found = None;
    }

    /// "Later" means not now: the version stays ready, for Settings' "Restart and update" and for
    /// closing; its dialog shows again only when the player asks. Returns the version to mark seen.
    fn dismissed(&mut self) -> Option<String> {
        self.asked = false;
        self.found.as_ref().map(|f| f.version.clone())
    }

    /// What the view shows, given the version whose dialog the player last saw.
    fn view(&self, current: &str, auto: bool, seen: Option<&str>) -> UpdateView {
        let seen = if self.asked { None } else { seen };
        view(
            current,
            auto,
            self.checking,
            self.found.as_ref(),
            seen,
            self.up_to_date,
        )
    }

    /// Closing installs a ready update when "Update automatically" is on: "Signed updates install
    /// when you restart" (the Settings board).
    fn installs_on_close(&self, auto: bool) -> bool {
        auto && self.found.as_ref().is_some_and(|f| f.ready)
    }
}

/// The updater in the shell: its [`State`], and the found update with its bytes once they're here.
#[derive(Default)]
pub struct Updates {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    state: State,
    downloaded: Option<(Update, Vec<u8>)>,
}

impl Updates {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Hands the model what the view should show now.
    fn publish(&self, model: &App) {
        let snapshot = model.snapshot();
        let seen = model.update_seen();
        let view = self.lock().state.view(
            &snapshot.update.current,
            snapshot.settings.auto_update,
            seen.as_deref(),
        );
        model.set_update(view);
    }

    /// Looks for a newer launcher and downloads it, unless this run holds it already. Only the
    /// check the player asked for (`asked`) says when it fails; at start, off-line, it's only
    /// logged (spec §3.8). One check at a time.
    pub async fn check(&self, app: &AppHandle, model: &App, asked: bool) {
        if !self.lock().state.begin() {
            return;
        }
        self.publish(model);
        let checked = match app.updater() {
            Ok(updater) => updater.check().await,
            Err(e) => Err(e),
        };
        let update = match checked {
            Ok(Some(update)) if newer(&update.current_version, &update.version) => update,
            Ok(Some(update)) => {
                tracing::warn!(
                    offered = %update.version,
                    current = %update.current_version,
                    "the release offers no newer launcher: not installed"
                );
                self.lock().state.current();
                return self.publish(model);
            }
            Ok(None) => {
                tracing::info!("the launcher is up to date");
                self.lock().state.current();
                return self.publish(model);
            }
            Err(e) => return self.failed(model, asked, &e),
        };
        let download = self
            .lock()
            .state
            .found(&update.version, update.body.clone(), asked);
        self.publish(model);
        if !download {
            tracing::info!(version = %update.version, "the launcher update is ready already");
            return;
        }
        tracing::info!(version = %update.version, "a launcher update downloads");
        match update.download(|_, _| {}, || {}).await {
            Ok(bytes) => {
                tracing::info!(version = %update.version, "the launcher update is ready");
                let mut inner = self.lock();
                inner.state.downloaded();
                inner.downloaded = Some((update, bytes));
            }
            Err(e) => {
                self.lock().state.lost();
                return self.failed(model, asked, &e);
            }
        }
        self.publish(model);
    }

    fn failed(&self, model: &App, asked: bool, e: &tauri_plugin_updater::Error) {
        tracing::warn!(error = %e, "the update check failed");
        self.lock().state.failed();
        if asked {
            model.note(CHECK_FAILED.into(), true);
        }
        self.publish(model);
    }

    /// "Restart and update": installs the downloaded update and restarts the launcher.
    pub fn install(&self, app: &AppHandle, model: &App) -> Result<(), String> {
        let downloaded = self.lock().downloaded.take();
        let Some((update, bytes)) = downloaded else {
            return Err(INSTALL_FAILED.into());
        };
        if let Err(e) = update.install(&bytes) {
            tracing::warn!(error = %e, "the launcher update didn't install");
            self.lock().state.lost();
            model.note(INSTALL_FAILED.into(), true);
            self.publish(model);
            return Err(INSTALL_FAILED.into());
        }
        tracing::info!(version = %update.version, "the launcher update is installed; restarting");
        app.restart()
    }

    /// "Later": not now (see [`State::dismissed`]).
    pub fn dismiss(&self, model: &App) {
        let version = self.lock().state.dismissed();
        if let Some(version) = version {
            model.mark_update_seen(&version);
        }
        self.publish(model);
    }

    /// Closing the launcher: a ready update installs first when "Update automatically" is on,
    /// without starting the launcher again (Windows's installer included), then the launcher ends.
    pub fn close(&self, app: &AppHandle, model: &App) {
        let auto = model.snapshot().settings.auto_update;
        let held = {
            let mut inner = self.lock();
            if inner.state.installs_on_close(auto) {
                inner.downloaded.take()
            } else {
                None
            }
        };
        if let Some((update, bytes)) = held {
            let update = update.restart_after_install(false);
            match update.install(&bytes) {
                Ok(()) => tracing::info!(
                    version = %update.version,
                    "the launcher update is installed; the next start runs it"
                ),
                Err(e) => tracing::warn!(error = %e, "the launcher update didn't install on close"),
            }
        }
        app.exit(0);
    }
}

/// Whether `offered` is a newer launcher than `current`, both SemVer. An older one, or the same,
/// is never installed, whatever a release offers: a signed older version can't roll a player back
/// to its flaws (#348). The updater compares versions too; this holds whatever its settings.
fn newer(current: &str, offered: &str) -> bool {
    match (
        semver::Version::parse(current),
        semver::Version::parse(offered),
    ) {
        (Ok(current), Ok(offered)) => offered > current,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_newer_launcher_is_ever_installed() {
        // #348: a release that offers an older launcher, or the same one, isn't installed, so
        // a signed older version can't roll a player back to its flaws.
        assert!(newer("0.5.0", "0.5.1"));
        assert!(newer("0.5.0", "0.6.0-rc.1"));
        for offered in ["0.5.0", "0.4.1", "0.4.9", "", "not a version", "v0.5.1"] {
            assert!(!newer("0.5.0", offered), "{offered:?}");
        }
        assert!(!newer("not a version", "0.5.1"));
    }

    #[test]
    fn the_dialog_shows_once_per_ready_version() {
        let found = Found {
            version: "0.5.1".into(),
            notes: Some("Notes".into()),
            ready: true,
        };
        let v = view("0.5.0", true, false, Some(&found), None, false);
        assert!(v.ready && v.show_dialog && v.available.as_deref() == Some("0.5.1"));
        assert!(!view("0.5.0", true, false, Some(&found), Some("0.5.1"), false).show_dialog);
        let downloading = Found {
            ready: false,
            ..found.clone()
        };
        assert!(!view("0.5.0", true, false, Some(&downloading), None, false).show_dialog);
        assert_eq!(
            view("0.5.0", false, false, None, None, false).available,
            None
        );
    }

    /// The final review's Important 1: "Later" meant never. Now it means not now: the update stays
    /// ready for Settings' "Restart and update", and installs on close with automatic updates on.
    #[test]
    fn later_is_not_now_the_update_stays_ready_and_installs_on_close() {
        let mut s = State::default();
        assert!(s.begin());
        assert!(s.found("0.5.1", None, false), "a new version downloads");
        s.downloaded();
        assert!(s.view("0.5.0", true, None).show_dialog);
        assert_eq!(s.dismissed().as_deref(), Some("0.5.1"));
        let v = s.view("0.5.0", true, Some("0.5.1"));
        assert!(
            !v.show_dialog && v.ready,
            "dismissed, yet still ready to install"
        );
        assert!(s.installs_on_close(true));
        assert!(
            !s.installs_on_close(false),
            "with automatic updates off, only when asked"
        );
    }

    #[test]
    fn a_check_the_player_asks_for_shows_a_dismissed_update_without_downloading_it_again() {
        let mut s = State::default();
        s.begin();
        s.found("0.5.1", None, false);
        s.downloaded();
        s.dismissed();
        // "Check now", or Play's "Check for updates" for a server that runs a newer Fenji.
        assert!(s.begin());
        assert!(
            !s.found("0.5.1", None, true),
            "the bytes this run holds are that version's"
        );
        assert!(s.view("0.5.0", true, Some("0.5.1")).show_dialog);
        // A check nobody asked for keeps a dismissed dialog away.
        s.dismissed();
        assert!(s.begin());
        assert!(!s.found("0.5.1", None, false));
        assert!(!s.view("0.5.0", true, Some("0.5.1")).show_dialog);
    }

    #[test]
    fn a_newer_version_than_the_one_held_downloads_and_one_check_runs_at_a_time() {
        let mut s = State::default();
        assert!(s.begin());
        assert!(!s.begin(), "a second check while one runs");
        s.found("0.5.1", None, false);
        s.downloaded();
        assert!(s.begin());
        assert!(s.found("0.5.2", Some("Notes".into()), false));
        assert!(!s.view("0.5.0", true, None).ready, "0.5.2 isn't here yet");
        assert!(!s.installs_on_close(true));
        s.lost();
        assert_eq!(s.view("0.5.0", true, None).available, None);
    }

    /// The finish review's fix 6: the board's "Launcher 0.5.0 · up to date", after a check found
    /// nothing newer, and never beside an update.
    #[test]
    fn up_to_date_only_after_a_check_found_nothing_newer() {
        assert!(view("0.5.0", true, false, None, None, true).up_to_date);
        assert!(!view("0.5.0", true, false, None, None, false).up_to_date);
        let found = Found {
            version: "0.5.1".into(),
            notes: None,
            ready: false,
        };
        assert!(!view("0.5.0", true, false, Some(&found), None, true).up_to_date);
    }
}
