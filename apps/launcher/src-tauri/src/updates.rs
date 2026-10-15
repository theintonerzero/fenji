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

/// The updater's state in the shell: whether a check is under way, what it found, the found
/// update's bytes once they're here, and whether the last check found nothing newer. A check that
/// fails leaves that as it was.
#[derive(Default)]
pub struct Updates {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    checking: bool,
    found: Option<Found>,
    downloaded: Option<(Update, Vec<u8>)>,
    up_to_date: bool,
}

impl Updates {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Hands the model what the view should show now.
    fn publish(&self, model: &App) {
        let snapshot = model.snapshot();
        let seen = model.update_seen();
        let inner = self.lock();
        model.set_update(view(
            &snapshot.update.current,
            snapshot.settings.auto_update,
            inner.checking,
            inner.found.as_ref(),
            seen.as_deref(),
            inner.up_to_date,
        ));
    }

    /// Looks for a newer launcher and downloads it. Only the check the player asked for (`asked`)
    /// says when it fails; at start, off-line, it's only logged (spec §3.8). One check at a time.
    pub async fn check(&self, app: &AppHandle, model: &App, asked: bool) {
        {
            let mut inner = self.lock();
            if inner.checking {
                return;
            }
            inner.checking = true;
        }
        self.publish(model);
        let checked = match app.updater() {
            Ok(updater) => updater.check().await,
            Err(e) => Err(e),
        };
        let update = match checked {
            Ok(Some(update)) => update,
            Ok(None) => {
                tracing::info!("the launcher is up to date");
                let mut inner = self.lock();
                inner.checking = false;
                inner.up_to_date = true;
                drop(inner);
                return self.publish(model);
            }
            Err(e) => return self.failed(model, asked, &e),
        };
        tracing::info!(version = %update.version, "a launcher update downloads");
        {
            let mut inner = self.lock();
            inner.checking = false;
            inner.up_to_date = false;
            inner.found = Some(Found {
                version: update.version.clone(),
                notes: update.body.clone(),
                ready: false,
            });
        }
        self.publish(model);
        match update.download(|_, _| {}, || {}).await {
            Ok(bytes) => {
                tracing::info!(version = %update.version, "the launcher update is ready");
                let mut inner = self.lock();
                if let Some(found) = &mut inner.found {
                    found.ready = true;
                }
                inner.downloaded = Some((update, bytes));
            }
            Err(e) => {
                self.lock().found = None;
                return self.failed(model, asked, &e);
            }
        }
        self.publish(model);
    }

    fn failed(&self, model: &App, asked: bool, e: &tauri_plugin_updater::Error) {
        tracing::warn!(error = %e, "the update check failed");
        self.lock().checking = false;
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
            self.lock().found = None;
            model.note(INSTALL_FAILED.into(), true);
            self.publish(model);
            return Err(INSTALL_FAILED.into());
        }
        tracing::info!(version = %update.version, "the launcher update is installed; restarting");
        app.restart()
    }

    /// "Later": the dialog doesn't show again for this version.
    pub fn dismiss(&self, model: &App) {
        let version = self.lock().found.as_ref().map(|f| f.version.clone());
        if let Some(version) = version {
            model.mark_update_seen(&version);
        }
        self.publish(model);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(view("0.5.0", false, false, None, None, false).available, None);
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
