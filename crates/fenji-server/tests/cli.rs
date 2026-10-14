//! The admin subcommands (M3 spec D13, §4 `cli`), `init` and the first run (M4 hosting spec §3.6).
use std::path::Path;
use std::process::{ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

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

/// `fenji-server` itself, run in `dir` with `args` and no config of the test's.
fn fenji_server_in(dir: &Path, args: &[&str]) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_fenji-server"))
        .current_dir(dir)
        .args(args)
        .env("RUST_LOG", "warn")
        .output()
        .unwrap()
}

/// A running `fenji-server`, killed if the test ends before it stops, so that none outlives a
/// failed test holding its ports.
struct Running(std::process::Child);

impl Running {
    /// Stops it as systemd would, with SIGTERM, on Unix; kills it elsewhere. Its exit status.
    fn stop(&mut self) -> ExitStatus {
        #[cfg(unix)]
        {
            let pid = self.0.id().to_string();
            let sent = std::process::Command::new("kill")
                .args(["-s", "TERM", &pid])
                .status()
                .unwrap();
            assert!(sent.success(), "kill -s TERM {pid}: {sent}");
        }
        #[cfg(not(unix))]
        self.0.kill().unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "still running 30 s after the stop"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn first_run_writes_the_config_and_data_then_starts() {
    // H9: with no config at the default path, fenji.toml in the working directory, the server
    // writes it and data/, starts on the template's loopback addresses (127.0.0.1:7800 and 7801,
    // which must be free) and says so. A child process, as the default path is the working
    // directory's.
    let dir = tempfile::tempdir().unwrap();
    let log = tempfile::NamedTempFile::new().unwrap();
    let mut server = Running(
        std::process::Command::new(env!("CARGO_BIN_EXE_fenji-server"))
            .current_dir(dir.path())
            .env("RUST_LOG", "info")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log.reopen().unwrap())
            .spawn()
            .unwrap(),
    );
    let said = || String::from_utf8_lossy(&std::fs::read(log.path()).unwrap()).into_owned();
    let db = dir.path().join("data/fenji.db");
    let deadline = Instant::now() + Duration::from_secs(60);
    while !(db.is_file() && said().contains("First run: wrote fenji.toml")) {
        if let Some(status) = server.0.try_wait().unwrap() {
            panic!(
                "the server stopped ({status}) before its first run:\n{}",
                said()
            );
        }
        assert!(
            Instant::now() < deadline,
            "no first run within 60 s:\n{}",
            said()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let status = server.stop();
    assert!(cfg!(not(unix)) || status.success(), "{status}:\n{}", said());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("fenji.toml")).unwrap(),
        fenji_server::config::template()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let data = std::fs::metadata(dir.path().join("data")).unwrap();
        assert_eq!(
            data.permissions().mode() & 0o777,
            0o700,
            "data/ is owner-only"
        );
    }
}

#[test]
fn init_writes_the_template_here_or_at_the_config_path_and_never_over_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let template = fenji_server::config::template();
    let out = fenji_server_in(dir.path(), &["init"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let here = std::fs::read_to_string(dir.path().join("fenji.toml")).unwrap();
    assert_eq!(here, template);
    // For a VPS: a path whose folders don't exist yet, given after the command or before it.
    let path = dir.path().join("etc/fenji/fenji.toml");
    let p = path.to_str().unwrap();
    let out = fenji_server_in(dir.path(), &["init", "--config", p]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains(p));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), template);
    std::fs::write(&path, "[server]\nname = \"Mine\"\n").unwrap();
    let out = fenji_server_in(dir.path(), &["--config", p, "init"]);
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success() && said.contains("already exists"),
        "{said}"
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "[server]\nname = \"Mine\"\n",
        "the host's file stays as it was"
    );
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

#[tokio::test]
async fn backup_prints_the_new_copy_which_opens_as_a_store() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    // The server holds the database open meanwhile.
    let server = fenji_store::Store::open(&cfg.store.path).await.unwrap();
    server
        .create_invite(&[1; 32], Some("for bob"), 1, None)
        .await
        .unwrap();
    let text = run(&cfg, Command::Backup { keep: 14 }).await;
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1, "{text}");
    let made = Path::new(lines[0]);
    assert_eq!(made.parent(), Some(dir.path().join("backups").as_path()));
    assert!(made.is_file(), "{text}");
    let copy = fenji_store::Store::open(made).await.unwrap();
    assert_eq!(copy.integrity_check().await.unwrap(), "ok");
    let invites = copy.list_invites().await.unwrap();
    assert_eq!(invites.len(), 1, "the copy holds what the database did");
    assert_eq!(invites[0].label.as_deref(), Some("for bob"));
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
