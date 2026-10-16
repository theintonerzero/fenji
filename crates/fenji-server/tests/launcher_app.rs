//! The launcher's model against a running server (launcher plan Task 8, LP2, LP3; spec §3.3, §3.4,
//! §3.12 and the plan's Review Focus 1 and 3): a new player's way from an invite link to signed in,
//! with no secret in any snapshot, the settings or the log; a silent server that holds up nothing
//! else; maintenance and a ban in words; and what removing a server forgets. Then Play (launcher
//! plan Task 10; spec §3.7, LP8): the ticket reaches only the client, a sign-in carries Play on,
//! one game at a time, and every refusal in words.
mod common;

#[cfg(unix)]
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

#[tokio::test(flavor = "current_thread")]
async fn a_new_player_goes_from_a_link_to_signed_in_and_no_secret_leaks() {
    // The log is caught by the process's own subscriber, which no other test's thread can make
    // blind to a line (#359).
    let log = caught::as_text(tracing::Level::TRACE);
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
    let logs = log.text();
    assert!(
        logs.contains("made an account with an invite"),
        "the log wasn't caught: {logs}"
    );
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
async fn every_server_in_the_rail_is_probed_with_no_servers_tab() {
    // The redesign's R8: the rail always shows every server, so the model probes them all every
    // 30 s, not only while a Servers tab shows. A launcher opened again on saved servers knew
    // only the active one's state.
    let (one, two) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, _ca) = server(one.path(), "", GateLimits::default()).await;
    let (b, _cb) = server(two.path(), "", GateLimits::default()).await;
    let cfg = config(one.path());
    let first = App::start(cfg.clone(), tokio::runtime::Handle::current());
    for s in [&a, &b] {
        first
            .intent(Intent::AddServer {
                text: format!("http://{}", s.api_addr),
            })
            .await
            .unwrap();
    }
    until(&first, |s| {
        s.servers.len() == 2
            && s.servers
                .iter()
                .all(|v| matches!(v.status, StatusView::Online))
    })
    .await;
    let active = first.snapshot().servers[0].id;
    first
        .intent(Intent::SelectServer { id: active })
        .await
        .unwrap();
    drop(first);
    // Opened again, as a player does: no tab is ever shown.
    let next = App::start(cfg, tokio::runtime::Handle::current());
    let s = until(&next, |s| {
        s.servers.len() == 2
            && s.servers
                .iter()
                .all(|v| matches!(v.status, StatusView::Online))
    })
    .await;
    assert_eq!(s.active, Some(active));
    a.shutdown().await.unwrap();
    b.shutdown().await.unwrap();
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

#[tokio::test]
async fn unticking_remember_forgets_a_server_saved_in_an_earlier_run_too() {
    // #348: unticked, "Remember this server" forgot only a server added in this run; launcher spec
    // §3.4 says the server is forgotten when the launcher closes, whenever it was added.
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    account(&s, "tester").await;
    let first = App::start(config(dir.path()), tokio::runtime::Handle::current());
    first
        .intent(Intent::AddServer {
            text: format!("http://{}", s.api_addr),
        })
        .await
        .unwrap();
    first
        .intent(Intent::SignIn {
            username: "tester".into(),
            password: password(),
            remember: true,
        })
        .await
        .unwrap();
    let file = dir.path().join("config/settings.json");
    assert!(std::fs::read_to_string(&file).unwrap().contains("tester"));
    // A later run signs in on the saved server without remembering it.
    let later = App::start(config(dir.path()), tokio::runtime::Handle::current());
    later
        .intent(Intent::SignIn {
            username: "tester".into(),
            password: password(),
            remember: false,
        })
        .await
        .unwrap();
    assert_eq!(
        later.snapshot().servers.len(),
        1,
        "it stays while the launcher runs"
    );
    let saved = std::fs::read_to_string(&file).unwrap();
    assert!(
        !saved.contains(&s.api_addr.to_string()) && !saved.contains("tester"),
        "{saved}"
    );
    s.shutdown().await.unwrap();
}

/// umu-run as Play uses it: `createprefix` makes the prefix's two marks, then, if the test says,
/// waits on "Proton" as umu-run 1.4.4 does: Proton in a session of its own, which here ignores
/// SIGTERM and has a child of its own, SIGTERM forwarded to it, and a wait while it lives. The game
/// records its environment, waits, and ends as the test says.
const FAKE_UMU_RUN: &str = r#"#!/bin/sh
if [ "$1" = createprefix ]; then
  mkdir -p "$WINEPREFIX/drive_c/windows" && : > "$WINEPREFIX/system.reg"
  if [ "${FENJI_TEST_PREFIX_SLEEP:-0}" != 0 ]; then
    setsid sh -c 'trap "" TERM; sleep "$0" & echo $! > "$1"; wait' \
      "$FENJI_TEST_PREFIX_SLEEP" "$FENJI_TEST_RECORD.prefix" &
    proton=$!
    trap 'kill -TERM "$proton" 2>/dev/null' TERM
    while kill -0 "$proton" 2>/dev/null; do wait "$proton"; done
  fi
  exit 0
fi
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
    cfg: AppConfig,
    code: InviteCode,
    client: PathBuf,
    record: PathBuf,
}

impl PlayBed {
    async fn new() -> PlayBed {
        Self::build(Os::Linux, 1, 0, 0).await
    }

    async fn with_sleep(secs: u32) -> PlayBed {
        Self::build(Os::Linux, secs, 0, 0).await
    }

    async fn with_exit(code: i32) -> PlayBed {
        Self::build(Os::Linux, 1, code, 0).await
    }

    async fn on(os: Os) -> PlayBed {
        Self::build(os, 1, 0, 0).await
    }

    /// A first start whose `umu-run createprefix` never ends by itself.
    #[cfg(target_os = "linux")]
    async fn with_stuck_prefix() -> PlayBed {
        Self::build(Os::Linux, 1, 0, 600).await
    }

    async fn build(os: Os, sleep: u32, exit: i32, prefix_sleep: u32) -> PlayBed {
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
        // A shell script, run on Linux alone, as Play is (launcher spec L2).
        #[cfg(unix)]
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
            (
                "FENJI_TEST_PREFIX_SLEEP".into(),
                prefix_sleep.to_string().into(),
            ),
        ]);
        let app = App::start(cfg.clone(), tokio::runtime::Handle::current());
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
            cfg,
            code,
            client,
            record,
        }
    }

    /// The launcher opened again on the same folders, as a player does after closing it.
    #[cfg(target_os = "linux")]
    fn reopened(&self) -> App {
        App::start(self.cfg.clone(), tokio::runtime::Handle::current())
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
    // Review Focus 1 to its end (#348): redeem, sign in and Play leave the password, the session
    // token and the ticket in no snapshot, log line or settings file. The log is caught by the
    // process's own subscriber, which no other test's thread can make blind to a line (#359).
    let log = caught::as_text(tracing::Level::TRACE);
    let t = PlayBed::new().await;
    let seen = t.collect_snapshots();
    t.signed_in().await;
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
    let settings = std::fs::read_to_string(t.cfg.config_dir.join("settings.json")).unwrap();
    let logs = log.text();
    assert!(
        logs.contains("the game is running"),
        "the log wasn't caught"
    );
    let seen = seen.lock().unwrap().clone();
    assert!(seen.iter().any(|json| json.contains(r#""signedIn":true"#)));
    for text in seen.iter().chain([&settings, &logs]) {
        assert!(
            !text.contains("a long password") && !text.contains("fenji1-"),
            "a secret leaked: {text}"
        );
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
        "Playing on this system isn't supported yet"
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

/// Linux only: the game's mark reads `/proc`, as Play runs only there (launcher spec L2).
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_game_a_closed_launcher_left_running_holds_play_in_the_next() {
    // #348: one game at a time across a restart. Closing the launcher leaves the game running
    // (spec L12), and the next launcher saw no game: a second Play started a second client on the
    // same prefix, over the first's game.log.
    let t = PlayBed::with_sleep(4).await;
    t.signed_in().await;
    t.app.intent(Intent::Play).await.unwrap();
    let GameView::Running { since_ms } =
        until(&t.app, |s| matches!(s.game, GameView::Running { .. }))
            .await
            .game
    else {
        unreachable!()
    };
    let next = t.reopened();
    assert_eq!(next.snapshot().game, GameView::Running { since_ms });
    assert_eq!(
        next.intent(Intent::Play).await.unwrap_err().to_string(),
        "The game is already running."
    );
    // Once the game ends, the next launcher plays again.
    until(&next, |s| matches!(s.game, GameView::Idle)).await;
    t.s.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_folder_and_downloads_wait_while_the_game_starts_or_runs() {
    // #348: Play reads the folder again after the prefix and the ticket, so a folder changed during
    // a first start launched one that was never checked; a download or repair would write into a
    // copy the game was running from.
    let t = PlayBed::with_sleep(5).await;
    t.signed_in().await;
    t.app.intent(Intent::Play).await.unwrap();
    until(&t.app, |s| {
        matches!(s.game, GameView::Starting { .. } | GameView::Running { .. })
    })
    .await;
    let other = t.client.with_file_name("elsewhere").display().to_string();
    for intent in [
        Intent::ChooseFolder {
            path: other.clone(),
        },
        Intent::Download {
            folder: other.clone(),
        },
    ] {
        assert_eq!(
            t.app.intent(intent).await.unwrap_err().to_string(),
            "Not while the game is running."
        );
    }
    assert_eq!(
        t.app.snapshot().client.folder.as_deref(),
        Some(t.client.display().to_string().as_str())
    );
    t.s.shutdown().await.unwrap();
}

/// Linux only: what the cancel ends is found, and looked for here, in `/proc`.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_start_stuck_making_the_prefix_can_be_cancelled() {
    // #348: Starting had no way out, so a first start whose prefix never finished held Play until
    // the launcher closed.
    let t = PlayBed::with_stuck_prefix().await;
    t.signed_in().await;
    t.app.intent(Intent::Play).await.unwrap();
    let child = t.record.with_extension("prefix");
    let pid = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(pid) = std::fs::read_to_string(&child)
                .ok()
                .and_then(|text| text.trim().parse::<u32>().ok())
            {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("createprefix never started");
    assert!(matches!(
        t.app.snapshot().game,
        GameView::Starting { first_start: true }
    ));
    t.app.intent(Intent::CancelStart).await.unwrap();
    let s = until(&t.app, |s| matches!(s.game, GameView::Idle)).await;
    let notice = s.notice.unwrap();
    assert_eq!(notice.text, "Start cancelled.");
    assert!(!notice.error);
    // Everything umu-run started ends, though Proton has a session of its own and ignores SIGTERM
    // (#348's final review).
    let gone = Instant::now();
    while alive(pid) {
        assert!(
            gone.elapsed() < Duration::from_secs(10),
            "createprefix's child outlived the cancel"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // Its two marks are there, but the prefix was never finished: the next start makes it again.
    let prefix = t.cfg.data_dir.join("prefix");
    assert!(prefix.join("system.reg").is_file());
    assert!(!fenji_launcher::start::prefix_ready(&prefix));
    assert!(!t.record.exists(), "the client was started");
    t.s.shutdown().await.unwrap();
}

/// Whether `pid` runs: a process that has ended but isn't reaped yet doesn't.
#[cfg(target_os = "linux")]
fn alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
        stat.rsplit_once(')')
            .is_some_and(|(_, rest)| !rest.trim_start().starts_with(['Z', 'X']))
    })
}

#[tokio::test]
async fn a_client_that_fails_its_check_never_starts() {
    let t = PlayBed::new().await;
    t.signed_in().await;
    std::fs::write(t.client.join("bin64/a.dll"), b"tampered").unwrap();
    assert_eq!(
        t.app.intent(Intent::Play).await.unwrap_err().to_string(),
        "Some game files need repair. Open Game."
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
        "The game couldn't start (code 7). See the logs."
    );
    t.s.shutdown().await.unwrap();
}
