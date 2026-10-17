//! The launcher's model (launcher spec §3.1, L1): it owns every piece of state and every secret,
//! and the view sees only the [`Snapshot`] published on each change. Intents come in through
//! [`App::intent`], and a loop probes the servers and asks who's online (§3.3, L10). The state
//! sits behind a std mutex that is never held across an `.await`, so a server that never answers
//! holds up nothing but its own request.
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use fenji_auth::invite::InviteCode;
use tokio::sync::watch;
use tokio::time::Instant;
use tracing::warn;

use crate::api::{ApiClient, ApiError, ServerInfo, Session};
use crate::manifest::Manifest;
use crate::pin::Pin;
use crate::probe::{Incompatible, Status};
use crate::servers::SavedServer;
use crate::settings::{Os, Settings};

mod client;
#[cfg(test)]
mod golden;
mod intent;
mod play;
mod servers;
mod snapshot;
pub mod words;

pub use intent::*;
pub use snapshot::*;

/// The repository, whose pages the shell may open (spec §3.9).
pub const REPOSITORY: &str = "https://github.com/theintonerzero/fenji";

/// Where the model keeps its files, what it runs on, and what it checks against.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub home: Option<PathBuf>,
    pub os: Os,
    pub version: String,
    pub manifest: Manifest,
    pub pinned_build: String,
    /// The launcher's own environment, passed on to tools and the game.
    pub env: Vec<(OsString, OsString)>,
    /// Used instead of fetching (tests; players who have their own).
    pub depot_downloader: Option<PathBuf>,
    /// Used before the settings, `PATH` and fetching (tests).
    pub umu_run: Option<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum IntentError {
    /// Refused, in words (spec §3.12); the same words are the snapshot's notice.
    #[error("{0}")]
    Refused(String),
    #[error("the shell handles this intent")]
    ForTheShell,
    #[error("this intent comes with a later task")]
    NotYet,
}

/// The model. Clones share it.
#[derive(Clone)]
pub struct App {
    inner: Arc<Inner>,
}

struct Inner {
    cfg: AppConfig,
    /// Steam's own folders, where its library list is (Amendment A, LA1).
    steam_roots: Vec<PathBuf>,
    rt: tokio::runtime::Handle,
    state: Mutex<State>,
    tx: watch::Sender<Snapshot>,
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

struct State {
    settings: Settings,
    /// The servers signed in to without "Remember this server": in the list until the launcher
    /// closes, and never in settings.json, whenever they were added (spec §3.4; #348).
    transient: HashSet<u32>,
    live: HashMap<u32, Live>,
    tab: Tab,
    need_sign_in: bool,
    client: client::Client,
    game: GameView,
    /// Cancel start came while the game was starting (#348): the start sees it and ends.
    start_cancelled: bool,
    /// Each server's invite, waiting to be redeemed there: one per server, so a second server's
    /// link never drops the first's (#376).
    pending_invites: HashMap<u32, InviteCode>,
    key_change: Option<KeyChange>,
    notice: Option<Notice>,
    update: UpdateView,
    swept_at: Option<Instant>,
    /// The copy Steam installed, once looked for (Amendment A, LA1).
    steam: Option<crate::steam::SteamCopy>,
    /// The DepotDownloader Fenji would run: the configured one, else the one in the tools folder,
    /// looked up at start and set again by a fetch, never read on each change (#367).
    depot_downloader: Option<PathBuf>,
}

/// What this run knows of a server beyond its saved entry.
#[derive(Default)]
struct Live {
    /// Kept between requests, so its connection stays open (L10).
    api: Option<Arc<ApiClient>>,
    status: Option<Status>,
    /// What the server said last; kept while it doesn't answer, so its name and host stay known.
    info: Option<ServerInfo>,
    ping_ms: Option<u32>,
    probing: bool,
    probed_at: Option<Instant>,
    seen_saved_at: Option<Instant>,
    session: Option<Session>,
    username: Option<String>,
    lobby: Option<Vec<String>>,
    listing: bool,
    listed_at: Option<Instant>,
}

impl Live {
    /// The server's client, made the first time it's needed.
    fn client(&mut self, saved: &SavedServer) -> Result<Arc<ApiClient>, ApiError> {
        if let Some(api) = &self.api {
            return Ok(api.clone());
        }
        let api = Arc::new(ApiClient::new(&saved.addr()?)?);
        self.api = Some(api.clone());
        Ok(api)
    }
}

/// A link offered another key for a saved server (or a key, when it had none): nothing changes
/// until the player chooses (the launcher plan's Task 4 ruling).
struct KeyChange {
    id: u32,
    offered: Pin,
    had_key: bool,
    /// The invite the link carried; it waits for the player's choice too.
    invite: Option<InviteCode>,
}

impl App {
    /// Loads the settings (a damaged file is moved aside, with a notice), and starts the loop on
    /// `rt`.
    pub fn start(cfg: AppConfig, rt: tokio::runtime::Handle) -> App {
        let (settings, problem) = Settings::load_or_reset(&cfg.config_dir);
        // A download an earlier launcher left running, crashed or killed, ends: hidden, it had no
        // window to close (the final review of #429, I3).
        crate::depot::end_left(&cfg.data_dir.join("depot"));
        // A game an earlier launcher started may still run (#348).
        let left = play::GameMark::left_running(&cfg.data_dir);
        let state = State {
            update: UpdateView {
                current: cfg.version.clone(),
                auto: settings.auto_update,
                checking: false,
                available: None,
                ready: false,
                notes: None,
                show_dialog: false,
                up_to_date: false,
            },
            settings,
            transient: HashSet::new(),
            live: HashMap::new(),
            tab: Tab::Play,
            need_sign_in: false,
            client: client::Client::default(),
            game: left
                .as_ref()
                .map_or(GameView::Idle, |mark| GameView::Running {
                    since_ms: mark.since_ms,
                }),
            start_cancelled: false,
            pending_invites: HashMap::new(),
            key_change: None,
            notice: problem.map(|text| Notice { text, error: true }),
            swept_at: None,
            steam: None,
            depot_downloader: cfg.depot_downloader.clone().or_else(|| {
                crate::tools::installed(
                    &crate::tools::depot_downloader(),
                    &cfg.data_dir.join("tools"),
                )
            }),
        };
        let (tx, _) = watch::channel(state.snapshot(&cfg));
        let steam_roots =
            crate::steam::roots(cfg.os, cfg.home.as_deref(), crate::steam::registry_path());
        let app = App {
            inner: Arc::new(Inner {
                cfg,
                steam_roots,
                rt,
                state: Mutex::new(state),
                tx,
            }),
        };
        app.inner.rt.spawn(servers::run(Arc::downgrade(&app.inner)));
        app.check_at_start();
        app.find_steam();
        if let Some(mark) = left {
            app.follow_left_game(mark);
        }
        app
    }

    pub fn snapshot(&self) -> Snapshot {
        self.inner.tx.borrow().clone()
    }

    /// Every snapshot from now on, starting with the current one.
    pub fn subscribe(&self) -> watch::Receiver<Snapshot> {
        self.inner.tx.subscribe()
    }

    /// Does what the view asked. A refusal's words also become the snapshot's notice.
    pub async fn intent(&self, intent: Intent) -> Result<(), IntentError> {
        let done = match intent {
            Intent::AddServer { text } => self.add_server(&text),
            Intent::RemoveServer { id } => {
                self.remove_server(id);
                Ok(())
            }
            Intent::SelectServer { id } => {
                self.select_server(id);
                Ok(())
            }
            Intent::Refresh => {
                self.refresh();
                Ok(())
            }
            Intent::ViewTab { tab } => {
                self.view_tab(tab);
                Ok(())
            }
            Intent::Focus { focused } => {
                self.focus(focused);
                Ok(())
            }
            Intent::Redeem {
                code,
                username,
                password,
            } => self.redeem(code, username, password).await,
            Intent::SignIn {
                username,
                password,
                remember,
            } => self.sign_in(username, password, remember).await,
            Intent::SignOut => {
                self.sign_out().await;
                Ok(())
            }
            Intent::CancelSignIn => {
                self.change(|st| st.need_sign_in = false);
                Ok(())
            }
            Intent::UseNewKey { id } => {
                self.use_new_key(id);
                Ok(())
            }
            Intent::KeepKey => {
                self.change(|st| st.key_change = None);
                Ok(())
            }
            Intent::ChooseFolder { path } => self.choose_folder(path),
            Intent::Check { level } => self.check(level),
            Intent::StopCheck => {
                self.stop_check();
                Ok(())
            }
            Intent::Download { folder } => self.download(folder),
            Intent::CancelDownload => {
                self.cancel_download();
                Ok(())
            }
            Intent::Repair => self.repair(),
            Intent::Play => self.play().await,
            Intent::PlayOn { id } => {
                self.select_server(id);
                self.play().await
            }
            Intent::CancelStart => {
                self.cancel_start();
                Ok(())
            }
            Intent::DismissNotice { text } => {
                // Only the notice the view showed: a newer one came for a reason (#348).
                self.change(|st| {
                    if st.notice.as_ref().is_some_and(|n| n.text == text) {
                        st.notice = None;
                    }
                });
                Ok(())
            }
            Intent::SetAutoUpdate { on } => {
                self.change(|st| {
                    st.settings.auto_update = on;
                    self.save(st);
                });
                Ok(())
            }
            Intent::SetUmuRun { path } => {
                self.change(|st| {
                    st.settings.umu_run = path.filter(|p| !p.trim().is_empty()).map(PathBuf::from);
                    self.save(st);
                });
                Ok(())
            }
            // Each refusal is the arm's value, never a `return`, so it's noted below: Settings'
            // "Change" passes on no answer (#376).
            Intent::SetDownloadFolder { path } => {
                if path.trim().is_empty() {
                    Err(refused(words::NO_FOLDER))
                } else if crate::steam::in_library(
                    std::path::Path::new(&path),
                    &self.inner.steam_roots,
                ) {
                    // Steam looks after its libraries (Amendment A, LA2): refused where the
                    // folder is chosen, not only once a download starts (#376).
                    Err(refused(words::STEAM_FOLDER_IN_SETTINGS))
                } else {
                    self.change(|st| {
                        st.settings.download_folder = Some(PathBuf::from(path));
                        self.save(st);
                    });
                    Ok(())
                }
            }
            Intent::PickFolder { .. }
            | Intent::OpenUrl { .. }
            | Intent::OpenLogFolder
            | Intent::OpenNotices
            | Intent::Minimize
            | Intent::Close
            | Intent::CheckUpdate
            | Intent::InstallUpdate
            | Intent::DismissUpdate => Err(IntentError::ForTheShell),
        };
        if let Err(IntentError::Refused(text)) = &done {
            self.note(text.clone(), true);
        }
        done
    }

    /// The shell's updater's state (Task 15).
    pub fn set_update(&self, update: UpdateView) {
        self.change(|st| st.update = update);
    }

    /// A notice from the shell.
    pub fn note(&self, text: String, error: bool) {
        self.change(|st| st.notice = Some(Notice { text, error }));
    }

    /// The version whose update dialog was last shown (spec §3.8: once a version).
    pub fn update_seen(&self) -> Option<String> {
        self.inner.lock().settings.update_seen.clone()
    }

    pub fn mark_update_seen(&self, version: &str) {
        self.change(|st| {
            st.settings.update_seen = Some(version.to_string());
            self.save(st);
        });
    }

    /// Whether the shell may open `url` in the browser (spec §3.9): the repository's pages, and
    /// the source address of a server in the list.
    pub fn allowed_url(&self, url: &str) -> bool {
        repository_page(url)
            || self
                .inner
                .lock()
                .live
                .values()
                .filter_map(|l| l.info.as_ref()?.source.as_deref())
                .any(|source| source == url)
    }

    /// Changes the state and publishes the snapshot, under the lock; `f` must not lock again.
    fn change<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        let mut st = self.inner.lock();
        let r = f(&mut st);
        let next = st.snapshot(&self.inner.cfg);
        self.inner.tx.send_if_modified(|current| {
            let changed = *current != next;
            if changed {
                *current = next;
            }
            changed
        });
        r
    }

    /// Writes the settings, without the servers that aren't to be remembered. A failure becomes a
    /// notice: the change still holds until the launcher closes.
    fn save(&self, st: &mut State) {
        let mut kept = st.settings.clone();
        kept.servers.retain(|s| !st.transient.contains(&s.id));
        if kept.active.is_some_and(|id| st.transient.contains(&id)) {
            kept.active = kept.servers.first().map(|s| s.id);
        }
        let dir = &self.inner.cfg.config_dir;
        if let Err(e) = kept.save(dir) {
            warn!(error = %e, "couldn't save the launcher's settings");
            st.notice = Some(Notice {
                text: words::cant_save(dir),
                error: true,
            });
        }
    }
}

impl State {
    fn snapshot(&self, cfg: &AppConfig) -> Snapshot {
        let active = self.settings.active.and_then(|id| self.live.get(&id));
        let platform = platform(cfg.os, cfg.home.as_deref());
        // Where Start download saves: the folder the player chose, or ~/Games/SoLO.
        let default_folder = self.settings.download_folder.clone().unwrap_or_else(|| {
            cfg.home
                .as_ref()
                .map(|h| h.join("Games").join("SoLO"))
                .unwrap_or_default()
        });
        let folder = self.settings.client.as_ref();
        // For a player without a terminal Fenji can open (spec §3.12), wherever a download works:
        // the DepotDownloader Fenji would run, or else where to get one (#348).
        let manual = platform.can_download.then(|| {
            let tool = crate::tools::depot_downloader();
            let dd = self.depot_downloader.clone();
            let folder = folder.unwrap_or(&default_folder);
            let command = |list| crate::depot::manual_command(dd.as_deref(), folder, list);
            ManualView {
                download: command(None),
                // Never into a Steam library: Steam repairs what's there (LA2).
                repair: (self.client.repair_list())
                    .filter(|_| !self.client.in_steam)
                    .map(|list| command(Some(list))),
                get: dd.is_none().then(|| ToolLinkView {
                    version: tool.version.clone(),
                    url: tool.url.clone(),
                }),
            }
        });
        Snapshot {
            launcher_version: cfg.version.clone(),
            platform,
            servers: self
                .settings
                .servers
                .iter()
                .map(|s| self.server_view(s))
                .collect(),
            active: self.settings.active,
            lobby: active
                .filter(|l| l.session.is_some())
                .and_then(|l| l.lobby.clone()),
            need_sign_in: self.need_sign_in,
            // The active server's: the one Join is for.
            pending_invite: self.settings.active.and_then(|id| {
                self.pending_invites.get(&id).map(|code| PendingInvite {
                    server_id: id,
                    code: code.display(),
                })
            }),
            key_change: self.key_change.as_ref().map(|k| KeyChangeView {
                server_id: k.id,
                name: self.name(k.id),
                had_key: k.had_key,
            }),
            client: ClientView {
                folder: folder.map(|p| p.display().to_string()),
                default_folder: default_folder.display().to_string(),
                build: cfg.pinned_build.clone(),
                files: cfg.manifest.entries.len(),
                check: self.client.check.clone(),
                manual,
                steam: self.steam.as_ref().map(|c| SteamCopyView {
                    folder: c.folder.display().to_string(),
                    build: c.build.clone(),
                    pinned: c.build == cfg.pinned_build,
                }),
                in_steam: self.client.in_steam,
            },
            job: self.client.job.clone(),
            game: self.game.clone(),
            update: UpdateView {
                auto: self.settings.auto_update,
                ..self.update.clone()
            },
            settings: SettingsView {
                auto_update: self.settings.auto_update,
                umu_run: self
                    .settings
                    .umu_run
                    .as_ref()
                    .map(|p| p.display().to_string()),
                log_folder: cfg.data_dir.join("logs").display().to_string(),
            },
            notice: self.notice.clone(),
        }
    }

    fn server_view(&self, saved: &SavedServer) -> ServerView {
        let live = self.live.get(&saved.id);
        let info = live.and_then(|l| l.info.as_ref());
        let status = live.and_then(|l| l.status.as_ref());
        // The players count is the server's answer of now, not of when it last answered.
        let answering = matches!(status, Some(Status::Online | Status::Maintenance { .. }));
        ServerView {
            id: saved.id,
            name: self.name(saved.id),
            address: saved.shown_address(),
            pinned: saved.pin.is_some(),
            host: info.and_then(|i| i.host.clone()),
            region: info.and_then(|i| i.region.clone()),
            status: status.map_or(StatusView::Unknown, status_view),
            players: info
                .filter(|_| answering)
                .and_then(|i| i.players)
                .map(|p| PlayersView {
                    online: p.online,
                    max: p.max,
                }),
            motd: info.map(|i| i.motd.clone()).unwrap_or_default(),
            ping_ms: live.and_then(|l| l.ping_ms),
            last_seen_ms: saved.last_seen_ms,
            version: info.map(|i| i.version.clone()).filter(|v| !v.is_empty()),
            source: info.and_then(|i| i.source.clone()),
            username: live
                .and_then(|l| l.username.clone())
                .or_else(|| saved.username.clone()),
            signed_in: live.is_some_and(|l| l.session.is_some()),
        }
    }

    /// A server's name: as it last said, or as saved, or its shown address.
    fn name(&self, id: u32) -> String {
        let saved = self.settings.server(id);
        self.live
            .get(&id)
            .and_then(|l| l.info.as_ref())
            .map(|i| i.name.clone())
            .or_else(|| saved.and_then(|s| s.name.clone()))
            .or_else(|| saved.map(SavedServer::shown_address))
            .unwrap_or_default()
    }

    /// Who hosts it, if the server said.
    fn host(&self, id: u32) -> Option<String> {
        self.live.get(&id)?.info.as_ref()?.host.clone()
    }

    /// A server's client, if the server is in the list.
    fn api(&mut self, id: u32) -> Option<Result<Arc<ApiClient>, ApiError>> {
        let saved = self.settings.server(id)?;
        Some(self.live.entry(id).or_default().client(saved))
    }

    /// An answer's words, as a refusal.
    fn refusal(&self, id: u32, e: &ApiError, username: Option<&str>) -> IntentError {
        IntentError::Refused(words::api_error(
            e,
            &self.name(id),
            self.host(id).as_deref(),
            username,
        ))
    }
}

fn refused(text: &str) -> IntentError {
    IntentError::Refused(text.to_string())
}

/// Now, in Unix milliseconds.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

fn status_view(status: &Status) -> StatusView {
    match status {
        Status::Online => StatusView::Online,
        Status::Maintenance { message, until } => StatusView::Maintenance {
            message: message.clone(),
            until: *until,
        },
        Status::Offline => StatusView::Offline,
        Status::KeyChanged => StatusView::KeyChanged,
        Status::OwnKey => StatusView::OwnKey,
        Status::Incompatible(why) => StatusView::Incompatible {
            newer: *why == Incompatible::Newer,
        },
    }
}

fn platform(os: Os, home: Option<&std::path::Path>) -> PlatformView {
    let (name, linux) = match os {
        Os::Linux => ("linux", true),
        Os::Windows => ("windows", false),
        Os::MacOs => ("macos", false),
        Os::Other => ("other", false),
    };
    PlatformView {
        os: name.into(),
        // Play and Steam downloads are Linux's alone in M4 (spec L2).
        can_play: linux,
        can_download: linux,
        home: home
            .filter(|_| os != Os::Windows)
            .map(|h| h.display().to_string()),
    }
}

/// `https://github.com/theintonerzero/fenji` and the pages under it, and nothing that only starts
/// with the same text.
fn repository_page(url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(url) else {
        return false;
    };
    let mut path = url.path_segments().into_iter().flatten();
    url.scheme() == "https"
        && url.host_str() == Some("github.com")
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && path.next() == Some("theintonerzero")
        && path.next() == Some("fenji")
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};

    use super::*;
    use crate::test_support::until;

    fn config(dir: &Path) -> AppConfig {
        AppConfig {
            config_dir: dir.join("config"),
            data_dir: dir.join("data"),
            home: Some(dir.join("home")),
            os: Os::Linux,
            version: "0.5.0".into(),
            manifest: Manifest::default(),
            pinned_build: "10015727".into(),
            env: Vec::new(),
            depot_downloader: None,
            umu_run: None,
        }
    }

    fn add(text: &str) -> Intent {
        Intent::AddServer { text: text.into() }
    }

    const OLD: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const NEW: &str = "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBA";

    const INFO: &str = r#"{"name":"Moonward","version":"0.5.0","pinned_build":"10015727","gate_host":"127.0.0.1","gate_port":7800,"server_id":1,"api_version":1}"#;
    const SESSION: &str = r#"{"token":"fenji1-0000000000000000000000000000000000000000000000000000000000000000","expires_at":1792059600000}"#;

    /// A server with its own key, whose sign-ins and redemptions take half a second: time for the
    /// player to do something else meanwhile. Its port and its pin.
    async fn slow_server() -> (u16, Pin) {
        let made = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let cert = made.cert.der().clone();
        let pin = Pin::of_cert(&cert).unwrap();
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(made.signing_key.serialize_der()));
        let port = crate::test_support::tls_server(cert, key, |line| {
            let slow = Duration::from_millis(500);
            if line.starts_with("POST /api/v1/sessions ") {
                (slow, SESSION)
            } else if line.starts_with("POST /api/v1/accounts ") {
                (slow, "{}")
            } else {
                (Duration::ZERO, INFO)
            }
        })
        .await;
        (port, pin)
    }

    fn sign_in() -> Intent {
        Intent::SignIn {
            username: "lin".into(),
            password: Secret("a long password".into()),
            remember: true,
        }
    }

    #[tokio::test]
    async fn dismissing_a_notice_leaves_a_newer_one_standing() {
        // #348: the view dismisses the notice it showed; one that came meanwhile stays.
        let dir = tempfile::tempdir().unwrap();
        let app = App::start(config(dir.path()), tokio::runtime::Handle::current());
        app.note("The first.".into(), true);
        app.note("The second.".into(), false);
        app.intent(Intent::DismissNotice {
            text: "The first.".into(),
        })
        .await
        .unwrap();
        let notice = app.snapshot().notice.map(|n| n.text);
        assert_eq!(notice.as_deref(), Some("The second."));
        app.intent(Intent::DismissNotice {
            text: "The second.".into(),
        })
        .await
        .unwrap();
        assert_eq!(app.snapshot().notice, None);
    }

    #[tokio::test]
    async fn a_session_signed_in_under_a_replaced_key_is_never_kept() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::start(config(dir.path()), tokio::runtime::Handle::current());
        let (port, pin) = slow_server().await;
        app.intent(add(&format!("https://127.0.0.1:{port}/#pin={pin}")))
            .await
            .unwrap();
        until(&app, |s| matches!(s.servers[0].status, StatusView::Online)).await;
        let id = app.snapshot().servers[0].id;
        let signing = tokio::spawn({
            let app = app.clone();
            async move { app.intent(sign_in()).await }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        // While the sign-in waits, the player takes another key for the server.
        app.intent(add(&format!("https://127.0.0.1:{port}/#pin={NEW}")))
            .await
            .unwrap();
        app.intent(Intent::UseNewKey { id }).await.unwrap();
        let refused = signing.await.unwrap().unwrap_err();
        assert_eq!(
            refused.to_string(),
            words::key_changed_meanwhile("Moonward")
        );
        assert!(!app.snapshot().servers[0].signed_in);
    }

    #[tokio::test]
    async fn an_invite_pasted_while_a_request_runs_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::start(config(dir.path()), tokio::runtime::Handle::current());
        let (port, pin) = slow_server().await;
        let link = |code: &str| {
            add(&format!(
                "https://127.0.0.1:{port}/#invite={code}&pin={pin}"
            ))
        };
        let pending = |app: &App| app.snapshot().pending_invite.map(|p| p.code);
        app.intent(link("ABCD-EFGH-JKMN-PQRS")).await.unwrap();
        until(&app, |s| matches!(s.servers[0].status, StatusView::Online)).await;
        // A redemption spends its own invite, not one pasted while it ran.
        let redeeming = tokio::spawn({
            let app = app.clone();
            async move {
                app.intent(Intent::Redeem {
                    code: None,
                    username: "lin".into(),
                    password: Secret("a long password".into()),
                })
                .await
            }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        app.intent(link("K7QM-3XRD-VN2P-HT8B")).await.unwrap();
        redeeming.await.unwrap().unwrap();
        assert!(app.snapshot().servers[0].signed_in);
        assert_eq!(pending(&app).as_deref(), Some("K7QM-3XRD-VN2P-HT8B"));
        // Nor does a sign-in drop an invite pasted while it ran.
        app.intent(Intent::SignOut).await.unwrap();
        app.intent(link("ABCD-EFGH-JKMN-PQRS")).await.unwrap();
        let signing = tokio::spawn({
            let app = app.clone();
            async move { app.intent(sign_in()).await }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        app.intent(link("K7QM-3XRD-VN2P-HT8B")).await.unwrap();
        signing.await.unwrap().unwrap();
        assert_eq!(pending(&app).as_deref(), Some("K7QM-3XRD-VN2P-HT8B"));
        // A sign-in with nothing pasted meanwhile drops the invite waiting there.
        app.intent(Intent::SignOut).await.unwrap();
        app.intent(sign_in()).await.unwrap();
        assert_eq!(pending(&app), None);
    }

    #[tokio::test]
    async fn each_server_keeps_its_own_invite() {
        // #376 (the redesign's final review, Minor 13): the model kept one invite, so a second
        // server's link dropped the first server's without a word, and the rail lets a player
        // leave Join to paste one. Each server's invite now waits on it.
        let dir = tempfile::tempdir().unwrap();
        let app = App::start(config(dir.path()), tokio::runtime::Handle::current());
        let pending = |app: &App| app.snapshot().pending_invite.map(|p| (p.server_id, p.code));
        app.intent(add(&format!(
            "https://127.0.0.1:9/#invite=ABCD-EFGH-JKMN-PQRS&pin={OLD}"
        )))
        .await
        .unwrap();
        app.intent(add(&format!(
            "https://127.0.0.1:10/#invite=K7QM-3XRD-VN2P-HT8B&pin={NEW}"
        )))
        .await
        .unwrap();
        let ids: Vec<u32> = app.snapshot().servers.iter().map(|s| s.id).collect();
        let (first, second) = (ids[0], ids[1]);
        // The snapshot has the active server's: the second's, as adding it made it active.
        assert_eq!(pending(&app), Some((second, "K7QM-3XRD-VN2P-HT8B".into())));
        app.intent(Intent::SelectServer { id: first })
            .await
            .unwrap();
        assert_eq!(pending(&app), Some((first, "ABCD-EFGH-JKMN-PQRS".into())));
        // Removing a server drops its invite alone.
        app.intent(Intent::RemoveServer { id: first })
            .await
            .unwrap();
        app.intent(Intent::SelectServer { id: second })
            .await
            .unwrap();
        assert_eq!(pending(&app), Some((second, "K7QM-3XRD-VN2P-HT8B".into())));
    }

    #[tokio::test]
    async fn a_code_alone_waits_on_the_active_server_and_needs_one() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::start(config(dir.path()), tokio::runtime::Handle::current());
        let refused = app.intent(add("ABCD-EFGH-JKMN-PQRS")).await.unwrap_err();
        assert_eq!(refused.to_string(), words::NEED_LINK);
        assert_eq!(
            app.snapshot().notice,
            Some(Notice {
                text: words::NEED_LINK.into(),
                error: true
            })
        );
        app.intent(add("http://127.0.0.1:9")).await.unwrap();
        app.intent(add("abcd efgh jkmn pqrs")).await.unwrap();
        let snap = app.snapshot();
        assert_eq!(
            snap.pending_invite,
            Some(PendingInvite {
                server_id: snap.servers[0].id,
                code: "ABCD-EFGH-JKMN-PQRS".into()
            })
        );
    }

    #[tokio::test]
    async fn a_link_with_another_key_changes_nothing_until_the_player_says() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::start(config(dir.path()), tokio::runtime::Handle::current());
        let file = dir.path().join("config/settings.json");
        app.intent(add(&format!("https://127.0.0.1:9/#pin={OLD}")))
            .await
            .unwrap();
        let id = app.snapshot().servers[0].id;
        let link = format!("https://127.0.0.1:9/#invite=ABCD-EFGH-JKMN-PQRS&pin={NEW}");
        app.intent(add(&link)).await.unwrap();
        let snap = app.snapshot();
        assert_eq!(
            snap.key_change,
            Some(KeyChangeView {
                server_id: id,
                name: "127.0.0.1:9".into(),
                had_key: true
            })
        );
        assert_eq!(snap.pending_invite, None, "the link's invite waits too");
        let saved = std::fs::read_to_string(&file).unwrap();
        assert!(saved.contains(OLD) && !saved.contains(NEW), "{saved}");
        let refused = app
            .intent(Intent::Redeem {
                code: Some("ABCD-EFGH-JKMN-PQRS".into()),
                username: "lin".into(),
                password: Secret("a long password".into()),
            })
            .await
            .unwrap_err();
        assert_eq!(refused.to_string(), words::key_first("127.0.0.1:9"));

        app.intent(Intent::KeepKey).await.unwrap();
        let snap = app.snapshot();
        assert_eq!((snap.key_change, snap.pending_invite), (None, None));
        assert!(std::fs::read_to_string(&file).unwrap().contains(OLD));

        app.intent(add(&link)).await.unwrap();
        app.intent(Intent::UseNewKey { id }).await.unwrap();
        let snap = app.snapshot();
        assert_eq!(snap.key_change, None);
        assert_eq!(
            snap.pending_invite.map(|p| (p.server_id, p.code)),
            Some((id, "ABCD-EFGH-JKMN-PQRS".to_string()))
        );
        let saved = std::fs::read_to_string(&file).unwrap();
        assert!(saved.contains(NEW) && !saved.contains(OLD), "{saved}");
    }

    #[tokio::test]
    async fn a_damaged_settings_file_starts_fresh_with_a_notice() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(dir.path().join("config/settings.json"), "{ not json").unwrap();
        let app = App::start(config(dir.path()), tokio::runtime::Handle::current());
        let snap = app.snapshot();
        assert!(snap.servers.is_empty());
        let notice = snap.notice.unwrap();
        assert!(
            notice.error && notice.text.contains("settings.json.bad"),
            "{notice:?}"
        );
        assert!(dir.path().join("config/settings.json.bad").exists());
    }

    /// The final review: "Change" beside the download folder started the download at once. It
    /// now only changes where "Start download" saves, and the choice is kept.
    #[tokio::test]
    async fn the_download_folder_the_player_picks_is_where_start_download_saves() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::start(config(dir.path()), tokio::runtime::Handle::current());
        let before = app.snapshot().client.default_folder;
        assert!(before.ends_with("Games/SoLO"), "{before}");
        app.intent(Intent::SetDownloadFolder {
            path: "/mnt/games/SoLO".into(),
        })
        .await
        .unwrap();
        let snap = app.snapshot();
        assert_eq!(snap.client.default_folder, "/mnt/games/SoLO");
        assert_eq!(snap.job, None, "nothing downloads until the player says so");
        assert_eq!(snap.notice, None);
        drop(app);
        let again = App::start(config(dir.path()), tokio::runtime::Handle::current());
        assert_eq!(again.snapshot().client.default_folder, "/mnt/games/SoLO");
    }

    #[tokio::test]
    async fn the_shell_s_intents_are_answered_without_a_notice() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::start(config(dir.path()), tokio::runtime::Handle::current());
        assert!(matches!(
            app.intent(Intent::Minimize).await,
            Err(IntentError::ForTheShell)
        ));
        assert_eq!(app.snapshot().notice, None);
        app.intent(Intent::SetAutoUpdate { on: false })
            .await
            .unwrap();
        app.intent(Intent::SetUmuRun {
            path: Some("/usr/bin/umu-run".into()),
        })
        .await
        .unwrap();
        let snap = app.snapshot();
        assert_eq!(
            (
                snap.update.auto,
                snap.settings.auto_update,
                snap.settings.umu_run
            ),
            (false, false, Some("/usr/bin/umu-run".to_string()))
        );
        let saved = std::fs::read_to_string(dir.path().join("config/settings.json")).unwrap();
        assert!(
            saved.contains("\"auto_update\": false") && saved.contains("umu-run"),
            "{saved}"
        );
    }

    #[test]
    fn an_intent_s_future_can_move_between_threads() {
        // The shell's command needs it, and it holds only while no lock is kept across an
        // `.await`: a compile-time check.
        fn sendable<T: Send>(_: T) {}
        fn check(app: &App) {
            sendable(app.intent(Intent::Refresh));
        }
        let _ = check;
    }

    #[tokio::test]
    async fn only_the_repository_and_listed_sources_open() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::start(config(dir.path()), tokio::runtime::Handle::current());
        assert!(app.allowed_url(REPOSITORY));
        assert!(app.allowed_url("https://github.com/theintonerzero/fenji/blob/dev/PRIVACY.md"));
        for url in [
            "https://github.com/theintonerzero/fenjix",
            "https://github.com/theintonerzero",
            "http://github.com/theintonerzero/fenji",
            "https://github.com.example.net/theintonerzero/fenji",
            "https://someone@github.com/theintonerzero/fenji",
            "https://github.com:8443/theintonerzero/fenji",
            "https://example.net/source",
            "file:///etc/passwd",
            "not a link",
        ] {
            assert!(!app.allowed_url(url), "{url}");
        }
    }
}
