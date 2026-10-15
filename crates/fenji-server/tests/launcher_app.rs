//! The launcher's model against a running server (launcher plan Task 8, LP2, LP3; spec §3.3, §3.4,
//! §3.12 and the plan's Review Focus 1 and 3): a new player's way from an invite link to signed in,
//! with no secret in any snapshot, the settings or the log; a silent server that holds up nothing
//! else; maintenance and a ban in words; and what removing a server forgets. Then Play (launcher
//! plan Task 10; spec §3.7, LP8): the ticket reaches only the client, a sign-in carries Play on,
//! one game at a time, and every refusal in words.
mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::*;
use fenji_auth::invite::InviteCode;
use fenji_launcher::app::{
    App, AppConfig, CheckView, GameView, Intent, LevelView, Secret, Snapshot, StatusView, Tab,
};
use fenji_launcher::manifest::Manifest;
use fenji_launcher::settings::Os;
use fenji_server::clock::TestClock;
use fenji_server::server::GateLimits;
use fenji_server::server::RunningServer;
use fenji_server::status::Maintenance;

fn config(dir: &Path) -> AppConfig {
    AppConfig {
        config_dir: dir.join("config"),
        data_dir: dir.join("data"),
        home: Some(dir.join("home")),
        os: Os::Linux,
        version: "0.5.0".into(),
        manifest: Manifest::default(),
        pinned_build: "10015727".into(),
        env: vec![("PATH".into(), std::env::var_os("PATH").unwrap_or_default())],
        depot_downloader: None,
        terminal: None,
        umu_run: None,
    }
}

async fn until(app: &App, what: impl Fn(&Snapshot) -> bool) -> Snapshot {
    let mut rx = app.subscribe();
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if what(&rx.borrow_and_update()) {
                return rx.borrow().clone();
            }
            rx.changed().await.unwrap();
        }
    })
    .await
    .expect("the state never came")
}

fn password() -> Secret {
    Secret("a long password".into())
}

/// A `std::io::Write` over a shared buffer, for the log.
struct Writer(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_new_player_goes_from_a_link_to_signed_in_and_no_secret_leaks() {
    let logs = Arc::new(Mutex::new(Vec::<u8>::new()));
    let sink = logs.clone();
    let _guard = tracing::subscriber::set_default(
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_writer(move || Writer(sink.clone()))
            .finish(),
    );
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    let code = fenji_auth::invite::InviteCode::generate().unwrap();
    s.accounts()
        .store()
        .create_invite(&code.hash(), None, NOW, None)
        .await
        .unwrap();
    let app = App::start(config(dir.path()), tokio::runtime::Handle::current());
    // Every snapshot published, as the shell would forward it.
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let (keep, mut rx) = (seen.clone(), app.subscribe());
    tokio::spawn(async move {
        loop {
            let json = serde_json::to_string(&*rx.borrow_and_update()).unwrap();
            keep.lock().unwrap().push(json);
            if rx.changed().await.is_err() {
                break;
            }
        }
    });
    app.intent(Intent::AddServer {
        text: format!("http://{}/#invite={}", s.api_addr, code.display()),
    })
    .await
    .unwrap();
    assert!(app.snapshot().pending_invite.is_some());
    app.intent(Intent::Redeem {
        code: None,
        username: "tester".into(),
        password: password(),
    })
    .await
    .unwrap();
    app.intent(Intent::ViewTab { tab: Tab::Play })
        .await
        .unwrap();
    let snap = until(&app, |s| {
        s.servers[0].signed_in
            && s.lobby.is_some()
            && matches!(s.servers[0].status, StatusView::Online)
    })
    .await;
    assert_eq!(snap.servers[0].username.as_deref(), Some("tester"));
    assert!(snap.pending_invite.is_none());
    app.intent(Intent::SignOut).await.unwrap();
    tokio::task::yield_now().await;
    let seen = seen.lock().unwrap().clone();
    assert!(
        seen.len() >= 3,
        "the collector saw too few snapshots: {}",
        seen.len()
    );
    let settings = std::fs::read_to_string(dir.path().join("config/settings.json")).unwrap();
    let logs = String::from_utf8(logs.lock().unwrap().clone()).unwrap();
    for text in seen.iter().chain([&settings, &logs]) {
        assert!(
            !text.contains("a long password") && !text.contains("fenji1-"),
            "a secret leaked: {text}"
        );
    }
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_silent_server_never_holds_up_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    let silent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let silent_addr = silent.local_addr().unwrap();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((c, _)) = silent.accept().await {
            held.push(c);
        }
    });
    let app = App::start(config(dir.path()), tokio::runtime::Handle::current());
    app.intent(Intent::AddServer {
        text: format!("http://{}", s.api_addr),
    })
    .await
    .unwrap();
    // From here, nothing waits for the silent server: not its own Add, not the tab, not the rest.
    let started = Instant::now();
    app.intent(Intent::AddServer {
        text: format!("http://{silent_addr}"),
    })
    .await
    .unwrap();
    app.intent(Intent::ViewTab { tab: Tab::Servers })
        .await
        .unwrap();
    let real = app.snapshot().servers[0].id;
    app.intent(Intent::SelectServer { id: real }).await.unwrap();
    until(&app, |s| matches!(s.servers[0].status, StatusView::Online)).await;
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "the silent server held things up"
    );
    let snap = until(&app, |s| matches!(s.servers[1].status, StatusView::Offline)).await;
    assert_eq!(snap.servers[1].ping_ms, None);
    // No answer within a probe's 5 s is offline (launcher spec L10), with room for a slow machine.
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "the silent server stayed unknown for {:?}",
        started.elapsed()
    );
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn maintenance_and_a_ban_reach_the_player_in_words() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    account(&s, "tester").await;
    let app = App::start(config(dir.path()), tokio::runtime::Handle::current());
    app.intent(Intent::AddServer {
        text: format!("http://{}", s.api_addr),
    })
    .await
    .unwrap();
    until(&app, |s| matches!(s.servers[0].status, StatusView::Online)).await;
    let back = NOW + 1_800_000;
    s.status()
        .set_maintenance(
            s.accounts().store(),
            Some(Maintenance {
                message: "Updating to Fenji 0.4.1. Back around 21:00.".into(),
                since_ms: NOW,
                until_ms: Some(back),
            }),
        )
        .await
        .unwrap();
    app.intent(Intent::Refresh).await.unwrap();
    let snap = until(&app, |s| {
        matches!(s.servers[0].status, StatusView::Maintenance { .. })
    })
    .await;
    assert_eq!(
        snap.servers[0].status,
        StatusView::Maintenance {
            message: "Updating to Fenji 0.4.1. Back around 21:00.".into(),
            until: Some(back)
        }
    );
    let sign_in = || Intent::SignIn {
        username: "tester".into(),
        password: password(),
        remember: true,
    };
    let refused = app.intent(sign_in()).await.unwrap_err();
    let words = "Fenji is in maintenance: Updating to Fenji 0.4.1. Back around 21:00.";
    assert_eq!(refused.to_string(), words);
    let notice = app.snapshot().notice.unwrap();
    assert_eq!((notice.text.as_str(), notice.error), (words, true));

    s.status()
        .set_maintenance(s.accounts().store(), None)
        .await
        .unwrap();
    let store = s.accounts().store().clone();
    let id = store
        .account_by_username("tester")
        .await
        .unwrap()
        .unwrap()
        .id;
    assert!(store.ban(id, "Griefing", NOW).await.unwrap());
    let refused = app.intent(sign_in()).await.unwrap_err();
    assert_eq!(
        refused.to_string(),
        "Your host has banned this account from Fenji: Griefing."
    );
    let notice = app.snapshot().notice.unwrap();
    assert!(
        notice.text.contains("has banned this account from") && notice.error,
        "{notice:?}"
    );
    assert!(!app.snapshot().servers[0].signed_in);
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn removing_a_server_forgets_its_session_and_username() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    account(&s, "tester").await;
    let app = App::start(config(dir.path()), tokio::runtime::Handle::current());
    let address = format!("http://{}", s.api_addr);
    app.intent(Intent::AddServer {
        text: address.clone(),
    })
    .await
    .unwrap();
    app.intent(Intent::SignIn {
        username: "tester".into(),
        password: password(),
        remember: true,
    })
    .await
    .unwrap();
    let snap = app.snapshot();
    assert!(snap.servers[0].signed_in);
    let file = dir.path().join("config/settings.json");
    assert!(std::fs::read_to_string(&file).unwrap().contains("tester"));
    app.intent(Intent::RemoveServer {
        id: snap.servers[0].id,
    })
    .await
    .unwrap();
    let snap = app.snapshot();
    assert!(snap.servers.is_empty() && snap.active.is_none() && snap.lobby.is_none());
    let saved = std::fs::read_to_string(&file).unwrap();
    assert!(
        !saved.contains("tester") && !saved.contains(&s.api_addr.to_string()),
        "{saved}"
    );
    app.intent(Intent::AddServer { text: address })
        .await
        .unwrap();
    let again = app.snapshot().servers[0].clone();
    assert_eq!((again.signed_in, again.username), (false, None));
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_server_signed_in_without_remember_stays_off_the_disk() {
    // Launcher spec §3.4: unticked, "Remember this server" forgets a server added in this run when
    // the launcher closes; it never reaches settings.json.
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    account(&s, "tester").await;
    let app = App::start(config(dir.path()), tokio::runtime::Handle::current());
    app.intent(Intent::AddServer {
        text: format!("http://{}", s.api_addr),
    })
    .await
    .unwrap();
    let file = dir.path().join("config/settings.json");
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .contains(&s.api_addr.to_string())
    );
    app.intent(Intent::SignIn {
        username: "tester".into(),
        password: password(),
        remember: false,
    })
    .await
    .unwrap();
    let snap = app.snapshot();
    assert_eq!(
        (snap.servers.len(), snap.servers[0].signed_in),
        (1, true),
        "it stays in the list while the launcher runs"
    );
    let saved = std::fs::read_to_string(&file).unwrap();
    assert!(
        !saved.contains(&s.api_addr.to_string()) && !saved.contains("tester"),
        "{saved}"
    );
    s.shutdown().await.unwrap();
}

/// umu-run as Play uses it: `createprefix` makes the prefix's two marks; the game records its
/// environment, waits, and ends as the test says.
const FAKE_UMU_RUN: &str = r#"#!/bin/sh
if [ "$1" = createprefix ]; then mkdir -p "$WINEPREFIX/drive_c/windows" && : > "$WINEPREFIX/system.reg"; exit 0; fi
env > "$FENJI_TEST_RECORD"
sleep "${FENJI_TEST_SLEEP:-1}"
exit "${FENJI_TEST_EXIT:-0}"
"#;

/// A server with an invite, a client folder (`bin64/` with two small files) and its manifest, the
/// fake umu-run, and an App that plays with them, its folder chosen and checked.
struct PlayBed {
    _dir: tempfile::TempDir,
    s: RunningServer,
    _c: TestClock,
    app: App,
    code: InviteCode,
    client: PathBuf,
    record: PathBuf,
}

impl PlayBed {
    async fn new() -> PlayBed {
        Self::build(Os::Linux, 1, 0).await
    }

    async fn with_sleep(secs: u32) -> PlayBed {
        Self::build(Os::Linux, secs, 0).await
    }

    async fn with_exit(code: i32) -> PlayBed {
        Self::build(Os::Linux, 1, code).await
    }

    async fn on(os: Os) -> PlayBed {
        Self::build(os, 1, 0).await
    }

    async fn build(os: Os, sleep: u32, exit: i32) -> PlayBed {
        let dir = tempfile::tempdir().unwrap();
        let (s, c) = server(dir.path(), "", GateLimits::default()).await;
        let code = InviteCode::generate().unwrap();
        s.accounts()
            .store()
            .create_invite(&code.hash(), None, NOW, None)
            .await
            .unwrap();
        let client = dir.path().join("game");
        std::fs::create_dir_all(client.join("bin64")).unwrap();
        std::fs::write(client.join("bin64/a.dll"), b"a small library").unwrap();
        std::fs::write(client.join("bin64/gujianol.exe"), b"a small program").unwrap();
        let umu_run = dir.path().join("umu-run");
        std::fs::write(&umu_run, FAKE_UMU_RUN).unwrap();
        std::fs::set_permissions(&umu_run, std::fs::Permissions::from_mode(0o755)).unwrap();
        let record = dir.path().join("record");
        let mut cfg = config(dir.path());
        cfg.os = os;
        cfg.manifest = Manifest::generate(&client).unwrap();
        cfg.umu_run = Some(umu_run);
        cfg.env.extend([
            ("FENJI_TEST_RECORD".into(), record.clone().into()),
            ("FENJI_TEST_SLEEP".into(), sleep.to_string().into()),
            ("FENJI_TEST_EXIT".into(), exit.to_string().into()),
        ]);
        let app = App::start(cfg, tokio::runtime::Handle::current());
        app.intent(Intent::ChooseFolder {
            path: client.display().to_string(),
        })
        .await
        .unwrap();
        until(&app, |s| {
            matches!(
                s.client.check,
                CheckView::Verified {
                    level: LevelView::Quick,
                    ..
                }
            )
        })
        .await;
        PlayBed {
            _dir: dir,
            s,
            _c: c,
            app,
            code,
            client,
            record,
        }
    }

    /// The invite link added and redeemed, as a new player does.
    async fn signed_in(&self) {
        self.app
            .intent(Intent::AddServer {
                text: format!("http://{}/#invite={}", self.s.api_addr, self.code.display()),
            })
            .await
            .unwrap();
        self.app
            .intent(Intent::Redeem {
                code: None,
                username: "tester".into(),
                password: password(),
            })
            .await
            .unwrap();
        until(&self.app, |s| {
            s.servers[0].signed_in && matches!(s.servers[0].status, StatusView::Online)
        })
        .await;
    }

    /// The account exists, and the server is in the list, but the launcher holds no session.
    async fn added_without_signing_in(&self) {
        account(&self.s, "tester").await;
        self.app
            .intent(Intent::AddServer {
                text: format!("http://{}", self.s.api_addr),
            })
            .await
            .unwrap();
        until(&self.app, |s| {
            matches!(s.servers[0].status, StatusView::Online)
        })
        .await;
    }

    /// Every snapshot published from now on, as JSON.
    fn collect_snapshots(&self) -> Arc<Mutex<Vec<String>>> {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (keep, mut rx) = (seen.clone(), self.app.subscribe());
        tokio::spawn(async move {
            loop {
                let json = serde_json::to_string(&*rx.borrow_and_update()).unwrap();
                keep.lock().unwrap().push(json);
                if rx.changed().await.is_err() {
                    break;
                }
            }
        });
        seen
    }

    /// Maintenance on, through the server's status, and the launcher seeing it.
    async fn maintenance_on(&self, message: &str) {
        self.s
            .status()
            .set_maintenance(
                self.s.accounts().store(),
                Some(Maintenance {
                    message: message.into(),
                    since_ms: NOW,
                    until_ms: None,
                }),
            )
            .await
            .unwrap();
        self.app.intent(Intent::Refresh).await.unwrap();
        until(&self.app, |s| {
            matches!(s.servers[0].status, StatusView::Maintenance { .. })
        })
        .await;
    }
}

#[tokio::test]
async fn play_takes_a_ticket_and_the_ticket_reaches_only_the_client() {
    let t = PlayBed::new().await;
    t.signed_in().await;
    let seen = t.collect_snapshots();
    t.app.intent(Intent::Play).await.unwrap();
    until(&t.app, |s| {
        matches!(s.game, GameView::Starting { first_start: true })
    })
    .await;
    until(&t.app, |s| matches!(s.game, GameView::Running { .. })).await;
    until(&t.app, |s| matches!(s.game, GameView::Ended { code: 0 })).await;
    let record = std::fs::read_to_string(&t.record).unwrap();
    assert!(
        record.lines().any(|l| l.starts_with("gj_ticket=fenji1-")),
        "{record}"
    );
    assert!(record.contains("gj_uinfo=") && record.contains("gj_gates="));
    for json in seen.lock().unwrap().iter() {
        assert!(!json.contains("fenji1-"), "the ticket reached a snapshot");
    }
    t.s.shutdown().await.unwrap();
}

#[tokio::test]
async fn play_without_a_session_asks_for_one_then_carries_on() {
    let t = PlayBed::new().await;
    t.added_without_signing_in().await;
    t.app.intent(Intent::Play).await.unwrap();
    assert!(t.app.snapshot().need_sign_in);
    t.app
        .intent(Intent::SignIn {
            username: "tester".into(),
            password: password(),
            remember: true,
        })
        .await
        .unwrap();
    until(&t.app, |s| {
        !s.need_sign_in && matches!(s.game, GameView::Running { .. } | GameView::Ended { .. })
    })
    .await;
    t.s.shutdown().await.unwrap();
}

#[tokio::test]
async fn one_game_at_a_time_and_play_refuses_in_words() {
    let t = PlayBed::with_sleep(5).await;
    t.signed_in().await;
    t.app.intent(Intent::Play).await.unwrap();
    until(&t.app, |s| matches!(s.game, GameView::Running { .. })).await;
    assert_eq!(
        t.app.intent(Intent::Play).await.unwrap_err().to_string(),
        "The game is already running."
    );
    let w = PlayBed::on(Os::Windows).await;
    assert_eq!(
        w.app.intent(Intent::Play).await.unwrap_err().to_string(),
        "Fenji can't start the client on this system yet"
    );
    let m = PlayBed::new().await;
    m.signed_in().await;
    m.maintenance_on("Back at nine").await;
    let err = m.app.intent(Intent::Play).await.unwrap_err().to_string();
    assert!(
        err.contains("in maintenance") && err.contains("Back at nine"),
        "{err}"
    );
    for bed in [t, w, m] {
        bed.s.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn a_client_that_fails_its_check_never_starts() {
    let t = PlayBed::new().await;
    t.signed_in().await;
    std::fs::write(t.client.join("bin64/a.dll"), b"tampered").unwrap();
    assert_eq!(
        t.app.intent(Intent::Play).await.unwrap_err().to_string(),
        "Some game files need repair. The Client tab shows which."
    );
    assert!(!t.record.exists(), "the client was started");
    t.s.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_client_that_dies_at_once_says_it_couldnt_start() {
    let t = PlayBed::with_exit(7).await;
    t.signed_in().await;
    t.app.intent(Intent::Play).await.unwrap();
    let s = until(&t.app, |s| matches!(s.game, GameView::Ended { .. })).await;
    assert_eq!(
        s.notice.unwrap().text,
        "The game couldn't start: umu-run ended with 7. The log folder has the details."
    );
    t.s.shutdown().await.unwrap();
}
