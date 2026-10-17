//! The golden snapshots (plan LP2): one `Snapshot` for each state a board shows, built by hand with
//! the boards' values and checked against `apps/launcher/fixtures/snapshots/<state>.json`. The
//! view's tests render the same files, so a change on either side fails the other's tests.
//! `FENJI_BLESS=1 cargo test -p fenji-launcher golden` writes them again.
use std::path::Path;

use super::intent::Intent;
use super::snapshot::*;
use crate::verify::Problem;

// The boards' clock: 2026-10-15, from 09:14 UTC, in Unix milliseconds.
/// 20:14, when the boards were drawn: the last answer, and "last checked".
const NOW: i64 = 1_792_055_640_000;
/// 20:10: the last check of the files.
const CHECKED: i64 = 1_792_055_400_000;
/// 20:05: when the game started.
const STARTED: i64 = 1_792_055_100_000;
/// 18:42: when the unreachable server last answered.
const SEEN_TODAY: i64 = 1_792_050_120_000;
/// 21:00: when the host expects maintenance to end.
const BACK: i64 = 1_792_058_400_000;
/// 23:10 the evening before: Ember Peak's last answer.
const SEEN_YESTERDAY: i64 = 1_791_979_800_000;

const FOLDER: &str = "/home/player/Games/SoLO";
/// Where the model lists the files to repair, for the player's own terminal.
const REPAIR_LIST: &str = "/home/player/.local/share/fenji-launcher/repair-files.txt";
const MOTD: &str = "the lobby is open. Make your characters now; the world opens with Fenji 0.5.";
const SOURCE: &str = "https://github.com/theintonerzero/fenji";
const MAINTENANCE: &str = "Updating to Fenji 0.4.1. Back around 21:00.";
const NOTES: &str = "The Play tab shows the host's maintenance message and when the server is back.\n\
                     Checking files shows which file it's on and how many are left.\n\
                     Plainer words when the server can't be reached.";

fn names(list: &[&str]) -> Option<Vec<String>> {
    Some(list.iter().map(|n| n.to_string()).collect())
}

fn moonward() -> ServerView {
    ServerView {
        id: 1,
        name: "Moonward".into(),
        address: "moonward.example.net".into(),
        pinned: false,
        host: Some("Wen".into()),
        region: Some("Germany".into()),
        status: StatusView::Online,
        players: Some(PlayersView { online: 3, max: 10 }),
        motd: MOTD.into(),
        ping_ms: Some(24),
        last_seen_ms: Some(NOW),
        version: Some("0.5.0".into()),
        source: Some(SOURCE.into()),
        username: Some("lin_yue".into()),
        signed_in: true,
    }
}

fn jade_hall() -> ServerView {
    ServerView {
        id: 2,
        name: "Jade Hall".into(),
        address: "jadehall.example.org".into(),
        pinned: false,
        host: Some("Mei".into()),
        region: None,
        status: StatusView::Maintenance {
            message: "moving the server to a new machine. Back around 21:00.".into(),
            until: Some(BACK),
        },
        players: Some(PlayersView { online: 0, max: 8 }),
        motd: String::new(),
        ping_ms: Some(31),
        last_seen_ms: Some(NOW),
        version: Some("0.5.0".into()),
        source: Some(SOURCE.into()),
        username: None,
        signed_in: false,
    }
}

/// Never answered in this run: its name is the one saved, and nothing else is known.
fn ember_peak() -> ServerView {
    ServerView {
        id: 3,
        name: "Ember Peak".into(),
        address: "203.0.113.24:7801".into(),
        pinned: true,
        host: None,
        region: None,
        status: StatusView::Offline,
        players: None,
        motd: String::new(),
        ping_ms: None,
        last_seen_ms: Some(SEEN_YESTERDAY),
        version: None,
        source: None,
        username: None,
        signed_in: false,
    }
}

/// Play · ready: signed in to Moonward, three friends online, the copy verified.
fn base() -> Snapshot {
    Snapshot {
        launcher_version: "0.5.0".into(),
        platform: PlatformView {
            os: "linux".into(),
            can_play: true,
            can_download: true,
            home: Some("/home/player".into()),
        },
        servers: vec![moonward()],
        active: Some(1),
        lobby: names(&["Wen", "Shu", "Kai"]),
        need_sign_in: false,
        pending_invite: None,
        key_change: None,
        client: ClientView {
            folder: Some(FOLDER.into()),
            default_folder: FOLDER.into(),
            build: "10015727".into(),
            files: 210,
            check: CheckView::Verified {
                level: LevelView::Quick,
                at_ms: CHECKED,
            },
            manual: None,
            steam: None,
            in_steam: false,
        },
        job: None,
        game: GameView::Idle,
        update: UpdateView {
            current: "0.5.0".into(),
            auto: true,
            checking: false,
            available: None,
            ready: false,
            notes: None,
            show_dialog: false,
            // As the boards show it: a launcher whose last check found nothing newer.
            up_to_date: true,
        },
        settings: SettingsView {
            auto_update: true,
            // GameMode installed, as on most gaming Linux desktops, and on, as by default.
            gamemode: Some(true),
            umu_run: None,
            log_folder: "/home/player/.local/share/fenji-launcher/logs".into(),
        },
        notice: None,
    }
}

fn with(change: impl FnOnce(&mut Snapshot)) -> Snapshot {
    let mut s = base();
    change(&mut s);
    s
}

/// DepotDownloader for the player's own terminal, which Fenji hasn't fetched: the download, and the
/// repair of `list`'s files when there's one.
fn manual(list: Option<&Path>) -> ManualView {
    let tool = crate::tools::depot_downloader();
    ManualView {
        download: crate::depot::manual_command(None, Path::new(FOLDER), None),
        repair: list.map(|list| crate::depot::manual_command(None, Path::new(FOLDER), Some(list))),
        get: Some(ToolLinkView {
            version: tool.version,
            url: tool.url,
        }),
    }
}

/// No folder chosen yet, and nothing checked.
fn no_client(s: &mut Snapshot) {
    s.client.folder = None;
    s.client.check = CheckView::None;
}

fn signed_out(s: &mut Snapshot, username: Option<&str>) {
    s.servers[0].signed_in = false;
    s.servers[0].username = username.map(String::from);
    s.lobby = None;
}

/// Every state a board shows, by its file's name.
pub fn all() -> Vec<(&'static str, Snapshot)> {
    vec![
        (
            "welcome",
            with(|s| {
                s.servers.clear();
                s.active = None;
                s.lobby = None;
                no_client(s);
            }),
        ),
        (
            "redeem",
            with(|s| {
                signed_out(s, None);
                no_client(s);
                s.pending_invite = Some(PendingInvite {
                    server_id: 1,
                    code: "K7QM-3XRD-VN2P-HT8B".into(),
                });
            }),
        ),
        (
            "sign-in",
            with(|s| {
                signed_out(s, None);
                s.need_sign_in = true;
            }),
        ),
        ("account", base()),
        ("play-ready", base()),
        ("play-signed-out", with(|s| signed_out(s, Some("lin_yue")))),
        (
            "play-checking",
            with(|s| {
                s.client.check = CheckView::None;
                s.job = Some(JobView::Checking {
                    level: LevelView::Quick,
                    files_done: 164,
                    files_total: 210,
                    percent: 78.0,
                });
            }),
        ),
        (
            "play-starting",
            with(|s| s.game = GameView::Starting { first_start: true }),
        ),
        (
            "play-getting-proton",
            with(|s| {
                s.game = GameView::Starting { first_start: false };
                s.job = Some(JobView::Fetching {
                    tool: "GE-Proton".into(),
                    percent: Some(42.0),
                });
            }),
        ),
        (
            "play-running",
            with(|s| {
                s.game = GameView::Running { since_ms: STARTED };
                s.lobby = names(&["Wen", "Shu", "Kai", "lin_yue"]);
                s.servers[0].players = Some(PlayersView { online: 4, max: 10 });
            }),
        ),
        (
            "play-ended",
            with(|s| {
                s.game = GameView::Ended { code: 3 };
                s.notice = Some(Notice {
                    text: "The game couldn't start (code 3). See the logs.".into(),
                    error: true,
                });
            }),
        ),
        (
            "play-maintenance",
            with(|s| {
                s.servers[0].status = StatusView::Maintenance {
                    message: MAINTENANCE.into(),
                    until: Some(BACK),
                };
                s.servers[0].players = Some(PlayersView { online: 0, max: 10 });
                s.lobby = names(&[]);
            }),
        ),
        (
            "play-offline",
            with(|s| {
                // What it said last stays known (its host, version and notice); its players and
                // ping don't.
                let m = &mut s.servers[0];
                m.status = StatusView::Offline;
                m.players = None;
                m.ping_ms = None;
                m.last_seen_ms = Some(SEEN_TODAY);
            }),
        ),
        (
            "play-key-changed",
            with(|s| {
                let m = &mut s.servers[0];
                m.pinned = true;
                m.status = StatusView::KeyChanged;
                m.players = None;
                m.ping_ms = None;
            }),
        ),
        (
            "play-unsupported",
            with(|s| {
                s.platform = PlatformView {
                    os: "windows".into(),
                    can_play: false,
                    can_download: false,
                    home: None,
                };
            }),
        ),
        (
            "servers",
            with(|s| s.servers = vec![moonward(), jade_hall(), ember_peak()]),
        ),
        (
            "servers-key-change",
            with(|s| {
                s.servers = vec![moonward(), jade_hall(), ember_peak()];
                s.key_change = Some(KeyChangeView {
                    server_id: 3,
                    name: "Ember Peak".into(),
                    had_key: true,
                });
            }),
        ),
        (
            "client-setup",
            with(|s| {
                no_client(s);
                s.client.manual = Some(manual(None));
            }),
        ),
        (
            // A copy Steam installed, at the pinned build: the Game popover offers it first
            // (Amendment A, LA1; the redesign's Set up board).
            "client-steam",
            with(|s| {
                no_client(s);
                s.client.manual = Some(manual(None));
                s.client.steam = Some(SteamCopyView {
                    folder:
                        "/home/player/.local/share/Steam/steamapps/common/Swords of Legends Online"
                            .into(),
                    build: "10015727".into(),
                    pinned: true,
                });
            }),
        ),
        (
            "client-fetching",
            with(|s| {
                no_client(s);
                s.job = Some(JobView::Fetching {
                    tool: "DepotDownloader".into(),
                    percent: Some(40.0),
                });
            }),
        ),
        (
            "client-waiting-for-steam",
            with(|s| {
                s.client.check = CheckView::None;
                // Wherever a download works, so does the player's own terminal (spec §3.12).
                s.client.manual = Some(manual(None));
                s.job = Some(JobView::Downloading {
                    repair: false,
                    percent: None,
                    file: None,
                    waiting_for_steam: true,
                });
            }),
        ),
        (
            "client-downloading",
            with(|s| {
                s.client.check = CheckView::None;
                s.job = Some(JobView::Downloading {
                    repair: false,
                    percent: Some(42.0),
                    file: Some("data/_index/sc_res.idx".into()),
                    waiting_for_steam: false,
                });
            }),
        ),
        (
            "client-repair",
            with(|s| {
                s.client.check = CheckView::NeedsRepair {
                    fine: 209,
                    files: vec![FileProblem {
                        path: "data/_index/sc_res.idx".into(),
                        problem: Problem::WrongSize,
                    }],
                    at_ms: CHECKED,
                };
                s.client.manual = Some(manual(Some(Path::new(REPAIR_LIST))));
            }),
        ),
        (
            "client-verified",
            with(|s| {
                s.client.check = CheckView::Verified {
                    level: LevelView::Full,
                    at_ms: CHECKED,
                };
            }),
        ),
        ("settings", base()),
        (
            "update-ready",
            with(|s| {
                s.update.available = Some("0.5.1".into());
                s.update.ready = true;
                s.update.notes = Some(NOTES.into());
                s.update.show_dialog = true;
                s.update.up_to_date = false;
            }),
        ),
    ]
}

#[test]
fn golden_snapshots_match_their_files() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/launcher/fixtures/snapshots");
    let bless = std::env::var_os("FENJI_BLESS").is_some();
    let mut names = Vec::new();
    for (name, snap) in all() {
        let text = serde_json::to_string_pretty(&snap).unwrap() + "\n";
        let path = dir.join(format!("{name}.json"));
        if bless {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&path, &text).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(&path).unwrap_or_default(),
            text,
            "{name}: run with FENJI_BLESS=1"
        );
        names.push(format!("{name}.json"));
    }
    let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    on_disk.sort();
    names.sort();
    assert_eq!(on_disk, names, "a snapshot file no state writes");
}

#[test]
fn every_intent_has_an_example_and_every_example_reads() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/launcher/fixtures/intents.json");
    let examples: Vec<Intent> =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut seen: Vec<&str> = examples.iter().map(super::intent::name).collect();
    seen.sort();
    seen.dedup();
    let mut all = super::intent::ALL.to_vec();
    all.sort();
    assert_eq!(seen, all);
}

#[test]
fn a_password_never_shows_in_debug() {
    let i: Intent = serde_json::from_str(
        r#"{"type":"signIn","username":"lin","password":"hunter2hunter2","remember":true}"#,
    )
    .unwrap();
    assert!(!format!("{i:?}").contains("hunter2"));
    assert!(
        serde_json::from_str::<Intent>(
            r#"{"type":"signIn","username":"lin","password":"x","remember":true,"extra":1}"#
        )
        .is_err()
    );
}

#[test]
fn every_server_state_has_a_sentence() {
    use crate::api::ApiError;
    let say = |e: ApiError| super::words::api_error(&e, "Moonward", Some("Wen"), Some("lin"));
    assert_eq!(
        say(ApiError::KeyChanged),
        "Moonward's key has changed. Ask Wen for a new link."
    );
    assert_eq!(
        say(ApiError::OwnKey),
        "Moonward uses its own key. Open the link Wen sent you."
    );
    assert_eq!(
        say(ApiError::Banned {
            reason: "griefing".into()
        }),
        "Wen has banned this account from Moonward: griefing."
    );
    assert_eq!(
        say(ApiError::Maintenance {
            message: "Back at 9".into(),
            until: None
        }),
        "Moonward is in maintenance: Back at 9."
    );
    let code = |c: &str| ApiError::Server {
        status: 400,
        code: c.into(),
        retry_after_secs: Some(30),
    };
    assert_eq!(
        say(code("username_taken")),
        "“lin” is taken on Moonward. Try another."
    );
    assert_eq!(
        say(code("try_later")),
        "Too many tries. Wait 30 s and try again."
    );
    assert_eq!(
        super::words::api_error(&code("invalid_invite"), "Moonward", None, None),
        "That invite is used or expired. Ask your host for a new one."
    );
    assert_eq!(
        super::words::api_error(
            &ApiError::Banned {
                reason: String::new()
            },
            "Moonward",
            None,
            None
        ),
        "Your host has banned this account from Moonward."
    );
}
