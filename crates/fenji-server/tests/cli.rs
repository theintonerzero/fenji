//! The admin subcommands (M3 spec D13, §4 `cli`).
use std::path::Path;
use std::process::Output;
use std::time::Duration;

use fenji_server::cli::{AccountCmd, Command, InviteCmd, admin};
use fenji_server::clock::TestClock;
use fenji_server::config::Config;

fn config(dir: &Path) -> Config {
    Config::parse(
        "[server]\nname = \"Fenji\"\npublic_host = \"play.example.net\"\npublic_api = \"https://play.example.net\"\n",
        dir,
    )
    .unwrap()
}

/// The code and link lines `invite create` prints under `server`.
async fn invite_with(dir: &Path, server: &str) -> String {
    let cfg = Config::parse(&format!("[server]\nname = \"Fenji\"\n{server}"), dir).unwrap();
    run(
        &cfg,
        Command::Invite {
            cmd: InviteCmd::Create {
                label: None,
                expires: None,
            },
        },
    )
    .await
}

async fn run(cfg: &Config, cmd: Command) -> String {
    let mut out = Vec::new();
    admin(cfg, &cmd, &mut out, &TestClock::at(1_800_000_000_000))
        .await
        .unwrap();
    String::from_utf8(out).unwrap()
}

/// `fenji-server` itself, with a `fenji.toml` in `dir` that keeps its database there.
fn fenji_server(dir: &Path, args: &[&str]) -> Output {
    let config = dir.join("fenji.toml");
    std::fs::write(&config, "[server]\nname = \"Fenji\"\n").unwrap();
    std::process::Command::new(env!("CARGO_BIN_EXE_fenji-server"))
        .arg("--config")
        .arg(&config)
        .args(args)
        .env("RUST_LOG", "warn")
        .output()
        .unwrap()
}

#[tokio::test]
async fn invite_create_refuses_the_labels_invite_refuses_and_stores_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let long = "x".repeat(65);
    for (label, reason) in [
        ("\u{1b}]0;owned\u{7}", "plain text only"),
        ("for\u{200B}bob", "plain text only"),
        (long.as_str(), "at most 64 characters"),
    ] {
        let out = fenji_server(dir.path(), &["invite", "create", "--label", label]);
        let said = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{label:?}: {said}");
        assert!(said.contains(reason), "{label:?}: {said}");
        assert!(out.stdout.is_empty(), "{label:?}: no code is shown");
    }
    let store = fenji_store::Store::open(&dir.path().join("fenji.db"))
        .await
        .unwrap();
    assert!(store.list_invites().await.unwrap().is_empty());
    let out = fenji_server(
        dir.path(),
        &["invite", "create", "--label", &"é".repeat(64)],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(store.list_invites().await.unwrap().len(), 1);
}

#[tokio::test]
async fn invite_list_escapes_hidden_characters_in_labels_stored_before_they_were_checked() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    // An invite from M3, whose labels were never checked.
    let store = fenji_store::Store::open(&cfg.store.path).await.unwrap();
    store
        .create_invite(
            &[7; 32],
            Some("for\tBob\u{1b}]0;owned\u{7}\u{202E}"),
            1,
            None,
        )
        .await
        .unwrap();
    let list = run(
        &cfg,
        Command::Invite {
            cmd: InviteCmd::List,
        },
    )
    .await;
    assert!(
        list.contains("\tfor\\u{9}Bob\\u{1b}]0;owned\\u{7}\\u{202e}\t"),
        "{list:?}"
    );
    let raw = list
        .chars()
        .filter(|&c| fenji_server::text::hidden(c) && c != '\t' && c != '\n');
    assert_eq!(raw.count(), 0, "{list:?}");
}

#[tokio::test]
async fn invite_create_prints_the_code_and_a_link_with_the_code_in_the_fragment() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let text = run(
        &cfg,
        Command::Invite {
            cmd: InviteCmd::Create {
                label: Some("for bob".into()),
                expires: None,
            },
        },
    )
    .await;
    let code = text.lines().find_map(|l| l.strip_prefix("code: ")).unwrap();
    assert!(fenji_auth::invite::InviteCode::parse(code).is_some());
    assert!(
        text.contains(&format!("https://play.example.net/#invite={code}")),
        "{text}"
    );
    let list = run(
        &cfg,
        Command::Invite {
            cmd: InviteCmd::List,
        },
    )
    .await;
    assert!(list.contains("for bob") && list.contains("open"), "{list}");
    assert!(!list.contains(code), "the list never shows codes: {list}");
}

#[tokio::test]
async fn an_invite_link_names_the_account_api_not_the_gate() {
    let dir = tempfile::tempdir().unwrap();
    // The default api_bind, 127.0.0.1:7801, is loopback: the link is that address, over http,
    // which fenji-launch accepts for loopback only. public_host names the gate, not the API.
    let text = invite_with(dir.path(), "public_host = \"127.0.0.1\"\n").await;
    let code = text.lines().find_map(|l| l.strip_prefix("code: ")).unwrap();
    assert!(
        text.contains(&format!("link: http://127.0.0.1:7801/#invite={code}")),
        "{text}"
    );
    // An API that listens beyond loopback, with no public_api: no link rather than a wrong one.
    let text = invite_with(dir.path(), "api_bind = \"0.0.0.0:7801\"\n").await;
    assert!(!text.contains("#invite="), "{text}");
    assert!(text.contains("public_api"), "says what to set: {text}");
}

#[tokio::test]
async fn invites_expire_and_revoke() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    run(
        &cfg,
        Command::Invite {
            cmd: InviteCmd::Create {
                label: Some("short".into()),
                expires: Some(Duration::from_secs(0)),
            },
        },
    )
    .await;
    run(
        &cfg,
        Command::Invite {
            cmd: InviteCmd::Create {
                label: Some("revoke me".into()),
                expires: None,
            },
        },
    )
    .await;
    let text = run(
        &cfg,
        Command::Invite {
            cmd: InviteCmd::Revoke { id: 2 },
        },
    )
    .await;
    assert!(text.contains("revoked invite 2"), "{text}");
    let list = run(
        &cfg,
        Command::Invite {
            cmd: InviteCmd::List,
        },
    )
    .await;
    assert!(
        list.contains("expired") && list.contains("revoked"),
        "{list}"
    );
    let text = run(
        &cfg,
        Command::Invite {
            cmd: InviteCmd::Revoke { id: 2 },
        },
    )
    .await;
    assert!(text.contains("nothing to revoke"), "{text}");
}

#[tokio::test]
async fn cli_writes_while_a_server_holds_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let held = fenji_store::Store::open(&cfg.store.path).await.unwrap();
    // A writer holds the database for a moment, as a running server's transaction would.
    let busy = held.clone();
    let (taken, lock_held) = tokio::sync::oneshot::channel();
    let holder = tokio::spawn(async move {
        busy.hold_write_lock(Duration::from_millis(200), taken)
            .await
    });
    lock_held.await.unwrap();
    let text = run(
        &cfg,
        Command::Invite {
            cmd: InviteCmd::Create {
                label: None,
                expires: None,
            },
        },
    )
    .await;
    assert!(text.starts_with("code: "), "{text}");
    holder.await.unwrap().unwrap();
    assert_eq!(held.list_invites().await.unwrap().len(), 1);
}

#[tokio::test]
async fn accounts_list_and_delete() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let text = run(
        &cfg,
        Command::Account {
            cmd: AccountCmd::Delete {
                username: "ghost".into(),
            },
        },
    )
    .await;
    assert!(text.contains("no account named ghost"), "{text}");
    let store = fenji_store::Store::open(&cfg.store.path).await.unwrap();
    store.create_invite(&[1; 32], None, 1, None).await.unwrap();
    store
        .redeem_invite(&[1; 32], "bob", "$phc", 1)
        .await
        .unwrap();
    let list = run(
        &cfg,
        Command::Account {
            cmd: AccountCmd::List,
        },
    )
    .await;
    assert!(list.contains("bob"), "{list}");
    let text = run(
        &cfg,
        Command::Account {
            cmd: AccountCmd::Delete {
                username: "BOB".into(),
            },
        },
    )
    .await;
    assert!(text.contains("deleted BOB"), "{text}");
}

#[tokio::test]
async fn account_delete_leaves_no_copy_in_the_write_ahead_log() {
    // PRIVACY.md, "Deleting your data". A running server keeps the database open, so the CLI's
    // own connections closing doesn't fold the log into the file: `account delete` empties it.
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let server = fenji_store::Store::open(&cfg.store.path).await.unwrap();
    server.create_invite(&[1; 32], None, 1, None).await.unwrap();
    server
        .redeem_invite(&[1; 32], "witness-in-the-log", "$witness-hash", 1)
        .await
        .unwrap();
    let text = run(
        &cfg,
        Command::Account {
            cmd: AccountCmd::Delete {
                username: "witness-in-the-log".into(),
            },
        },
    )
    .await;
    assert!(text.contains("deleted witness-in-the-log"), "{text}");
    let mut wal = cfg.store.path.clone().into_os_string();
    wal.push("-wal");
    for file in [std::path::PathBuf::from(wal), cfg.store.path.clone()] {
        let bytes = std::fs::read(&file).unwrap_or_default();
        for witness in [&b"witness-in-the-log"[..], b"$witness-hash"] {
            assert!(
                !bytes.windows(witness.len()).any(|w| w == witness),
                "{} still holds {}",
                file.display(),
                String::from_utf8_lossy(witness)
            );
        }
    }
    drop(server);
}

#[test]
fn durations_parse() {
    use fenji_server::cli::parse_duration;
    assert_eq!(parse_duration("30m").unwrap(), Duration::from_secs(1800));
    assert_eq!(parse_duration("12h").unwrap(), Duration::from_secs(43_200));
    assert_eq!(parse_duration("7d").unwrap(), Duration::from_secs(604_800));
    assert!(parse_duration("7").is_err());
    assert!(parse_duration("x d").is_err());
}
