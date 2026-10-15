//! The admin subcommands (M3 spec D13, §4 `cli`), `init` and the first run (M4 hosting spec §3.6).
use std::path::Path;
use std::process::{ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

use fenji_server::cli::{AccountCmd, Command, InviteCmd, admin};
use fenji_server::clock::TestClock;
use fenji_server::config::{Config, FIRST_RUN, template};

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

/// `fenji-server` started in `dir` with `args`, stdin closed and its log in a file of its own. It's
/// killed if the test ends first, so that none outlives a failed test holding its ports.
struct Running {
    child: std::process::Child,
    log: tempfile::NamedTempFile,
}

/// Free ports for both listeners, whatever the config says (Ruling HP7), so that no test needs
/// fixed ones.
const ANY_PORT: [(&str, &str); 2] = [
    ("FENJI_LOGIN_BIND", "127.0.0.1:0"),
    ("FENJI_API_BIND", "127.0.0.1:0"),
];

impl Running {
    fn start(dir: &Path, args: &[&str]) -> Self {
        Self::start_with(dir, args, &ANY_PORT)
    }

    fn start_with(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Self {
        let log = tempfile::NamedTempFile::new().unwrap();
        let child = std::process::Command::new(env!("CARGO_BIN_EXE_fenji-server"))
            .current_dir(dir)
            .args(args)
            .env("RUST_LOG", "info")
            .envs(env.iter().copied())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log.reopen().unwrap())
            .spawn()
            .unwrap();
        Self { child, log }
    }

    /// What it has logged so far.
    fn said(&self) -> String {
        String::from_utf8_lossy(&std::fs::read(self.log.path()).unwrap()).into_owned()
    }

    /// Waits until it has logged `text`. Panics if it stops first, or after 60 s.
    fn wait_for(&mut self, text: &str) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while !self.said().contains(text) {
            if let Some(status) = self.child.try_wait().unwrap() {
                let said = self.said();
                let hint = if said.contains("Address already in use") {
                    "; is another fenji-server on 7800/7801?"
                } else {
                    ""
                };
                panic!("it stopped ({status}) before logging {text:?}{hint}\n{said}");
            }
            assert!(
                Instant::now() < deadline,
                "no {text:?} within 60 s:\n{}",
                self.said()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Its exit status once it stops: within 30 s.
    fn wait(&mut self) -> ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "still running after 30 s:\n{}",
                self.said()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Stops it as systemd would, with SIGTERM, on Unix; kills it elsewhere. Its exit status.
    fn stop(&mut self) -> ExitStatus {
        #[cfg(unix)]
        {
            let pid = self.child.id().to_string();
            let sent = std::process::Command::new("kill")
                .args(["-s", "TERM", &pid])
                .status()
                .unwrap();
            assert!(sent.success(), "kill -s TERM {pid}: {sent}");
        }
        #[cfg(not(unix))]
        self.child.kill().unwrap();
        self.wait()
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn first_run_writes_the_config_and_data_then_starts() {
    // H9, with Amendment A's HA3: with no config at the default path, fenji.toml in the working
    // directory, the server writes it and data/, makes its own key, starts and says who can join.
    // The template listens on every interface; the environment moves both listeners to free
    // loopback ports here (Ruling HP7). A child process, as the default path is the working
    // directory's.
    let dir = tempfile::tempdir().unwrap();
    let mut server = Running::start(dir.path(), &[]);
    let [wrote, next, last] = FIRST_RUN;
    let (wrote, _) = wrote.split_once("<folder>").unwrap();
    // The last line comes once the server catches SIGTERM.
    server.wait_for(last);
    let said = server.said();
    let at = |text: &str| {
        said.find(text)
            .unwrap_or_else(|| panic!("no {text:?}:\n{said}"))
    };
    // What it wrote comes before the start, so that a first run that can't start still says; what
    // to do next comes once it runs.
    assert!(at(wrote) < at("Fenji is running"), "{said}");
    assert!(at("Fenji is running") < at(next), "{said}");
    let folder = dir.path().file_name().unwrap().to_str().unwrap();
    let line = said[at(wrote)..].lines().next().unwrap();
    assert!(
        line.contains(&format!("{folder}.")),
        "names its folder: {line}"
    );
    // Both on loopback, where the environment put them (the log may colour its fields).
    let running = said[at("Fenji is running")..].lines().next().unwrap();
    assert!(
        running.matches("127.0.0.1:").count() == 2 && !running.contains("0.0.0.0"),
        "where the environment put them: {running}"
    );
    for warning in ["trusted_proxies", "invite links need"] {
        assert!(!said.contains(warning), "nothing to warn about: {said}");
    }
    assert!(dir.path().join("data/fenji.db").is_file());
    let status = server.stop();
    assert!(
        cfg!(not(unix)) || status.success(),
        "{status}:\n{}",
        server.said()
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("fenji.toml")).unwrap(),
        template()
    );
    assert!(dir.path().join("data/tls/cert.pem").is_file());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &str| {
            std::fs::metadata(dir.path().join(p))
                .unwrap()
                .permissions()
                .mode()
                & 0o777
        };
        assert_eq!(mode("data"), 0o700, "data/ is owner-only");
        assert_eq!(mode("data/tls"), 0o700, "data/tls/ is owner-only");
        assert_eq!(mode("data/tls/key.pem"), 0o600, "the key is owner-only");
    }
}

#[test]
fn a_bind_in_the_environment_that_doesnt_parse_stops_the_start_and_is_named() {
    let dir = tempfile::tempdir().unwrap();
    let config =
        "[server]\nname = \"Fenji\"\nlogin_bind = \"127.0.0.1:0\"\napi_bind = \"127.0.0.1:0\"\n";
    std::fs::write(dir.path().join("fenji.toml"), config).unwrap();
    let mut server = Running::start_with(dir.path(), &[], &[("FENJI_API_BIND", "localhost:7801")]);
    let status = server.wait();
    let said = server.said();
    assert!(!status.success(), "{said}");
    assert!(said.contains("FENJI_API_BIND"), "{said}");
    assert!(!said.contains("Fenji is running"), "{said}");
}

#[test]
fn an_existing_config_at_the_default_path_is_used_as_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let mine = "[server]\nname = \"Mine\"\nlogin_bind = \"127.0.0.1:0\"\napi_bind = \"127.0.0.1:0\"\n\
                [store]\npath = \"mine.db\"\n";
    std::fs::write(dir.path().join("fenji.toml"), mine).unwrap();
    let mut server = Running::start(dir.path(), &[]);
    server.wait_for("Fenji is running");
    let status = server.stop();
    let said = server.said();
    assert!(cfg!(not(unix)) || status.success(), "{status}:\n{said}");
    assert!(!said.contains("First run"), "{said}");
    // A loopback API with no public_api has no proxy in front to trust: nothing to warn about.
    assert!(!said.contains("trusted_proxies"), "{said}");
    let now = std::fs::read(dir.path().join("fenji.toml")).unwrap();
    assert_eq!(now, mine.as_bytes(), "the file stays byte for byte");
    assert!(dir.path().join("mine.db").is_file());
    assert!(!dir.path().join("data").exists(), "no first run's data/");
}

#[test]
fn run_with_a_config_path_that_is_missing_says_how_to_write_one_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = Running::start(dir.path(), &["--config", "missing.toml", "run"]);
    let status = server.wait();
    let said = server.said();
    assert!(!status.success(), "{said}");
    assert!(
        said.contains("`fenji-server --config missing.toml init`"),
        "{said}"
    );
    let left = std::fs::read_dir(dir.path()).unwrap().count();
    assert_eq!(left, 0, "nothing is written: {said}");
}

#[test]
fn admin_commands_need_a_config_and_make_no_first_run() {
    let dir = tempfile::tempdir().unwrap();
    for args in [&["invite", "list"][..], &["backup"]] {
        let out = fenji_server_in(dir.path(), args);
        let said = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{args:?}: {said}");
        assert!(said.contains("fenji.toml"), "{args:?}: {said}");
        let left = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(left, 0, "{args:?}: nothing is written");
    }
}

#[test]
fn init_writes_the_template_here_or_at_the_config_path_and_never_over_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let written = template();
    let out = fenji_server_in(dir.path(), &["init"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let here = std::fs::read_to_string(dir.path().join("fenji.toml")).unwrap();
    assert_eq!(here, written);
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
    assert_eq!(std::fs::read_to_string(&path).unwrap(), written);
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

#[test]
fn invite_create_right_after_init_makes_the_data_folder() {
    // A VPS host makes the first invite before the service first starts.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fenji.toml");
    let p = path.to_str().unwrap();
    let out = fenji_server_in(dir.path(), &["--config", p, "init"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = fenji_server_in(dir.path(), &["--config", p, "invite", "create"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("code: "));
    let names = |d: &Path| {
        let mut n: Vec<String> = std::fs::read_dir(d)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        n.sort();
        n
    };
    assert_eq!(names(dir.path()), ["data", "fenji.toml"], "nothing else");
    let data = dir.path().join("data");
    let db = names(&data);
    assert!(
        db.iter().all(|n| n.starts_with("fenji.db")) && db.contains(&"fenji.db".into()),
        "the database alone: {db:?}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&data).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "data/ is owner-only");
    }
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
    // Amendment A, HA4: each link labelled, then the code.
    assert!(
        text.starts_with(&format!(
            "link (public_api): https://play.example.net/#invite={code}\ncode: {code}\n"
        )),
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

/// The link lines of `invite create`'s output, in order.
fn links(text: &str) -> Vec<&str> {
    text.lines().filter(|l| l.starts_with("link (")).collect()
}

#[tokio::test]
async fn an_invite_link_names_the_account_api_not_the_gate() {
    let dir = tempfile::tempdir().unwrap();
    // The default api_bind, 127.0.0.1:7801, is loopback: the link is that address, over http,
    // which fenji-launch accepts for loopback only. public_host names the gate, not the API.
    let text = invite_with(dir.path(), "public_host = \"127.0.0.1\"\n").await;
    let code = text.lines().find_map(|l| l.strip_prefix("code: ")).unwrap();
    assert_eq!(
        links(&text),
        [format!(
            "link (this computer): http://127.0.0.1:7801/#invite={code}"
        )],
        "{text}"
    );
    // #174: a public gate behind a loopback API, with no public_api: no link rather than a
    // wrong one, and what to set.
    let text = invite_with(dir.path(), "public_host = \"203.0.113.5\"\n").await;
    assert!(!text.contains("#invite="), "{text}");
    assert!(
        text.contains("public_api") && text.contains("tls = \"self\""),
        "says what to set: {text}"
    );
    // An API that listens beyond loopback, without TLS or public_api: plain http is for this
    // computer only, and the rest needs one of those.
    let text = invite_with(dir.path(), "api_bind = \"0.0.0.0:7801\"\n").await;
    let code = text.lines().find_map(|l| l.strip_prefix("code: ")).unwrap();
    assert_eq!(
        links(&text),
        [format!(
            "link (this computer): http://127.0.0.1:7801/#invite={code}"
        )],
        "{text}"
    );
    assert!(text.contains("public_api"), "says what to set: {text}");
}

#[tokio::test]
async fn with_the_servers_own_key_every_link_carries_its_pin() {
    // Amendment A, HA4: with neither public_api nor public_host, a link for each of the machine's
    // addresses (which this test can't know), then this computer's, then a word on the internet.
    let dir = tempfile::tempdir().unwrap();
    let server = "tls = \"self\"\napi_bind = \"0.0.0.0:7801\"\n";
    let cfg = Config::parse(&format!("[server]\nname = \"Fenji\"\n{server}"), dir.path()).unwrap();
    let tls = fenji_server::tls::folder(&cfg.store.path);
    let key = fenji_server::tls::ServerKey::load_or_make(&tls, "127.0.0.1").unwrap();
    let pin = &key.pin;
    let text = invite_with(dir.path(), server).await;
    let code = text.lines().find_map(|l| l.strip_prefix("code: ")).unwrap();
    let links = links(&text);
    assert_eq!(
        links.last().copied(),
        Some(
            format!("link (this computer): https://127.0.0.1:7801/#invite={code}&pin={pin}")
                .as_str()
        ),
        "{text}"
    );
    for link in &links {
        let (_, url) = link.split_once("): https://").unwrap();
        assert!(
            url.ends_with(&format!(":7801/#invite={code}&pin={pin}")),
            "{link}"
        );
    }
    let lines: Vec<&str> = text.lines().collect();
    let code_at = lines.iter().position(|l| l.starts_with("code: ")).unwrap();
    assert_eq!(code_at, links.len(), "the links, then the code: {text}");
    assert!(
        lines[code_at + 1].contains("7800 and 7801") && lines[code_at + 1].contains("VPN"),
        "{text}"
    );
    // With public_host, one link, there.
    let text = invite_with(
        dir.path(),
        &format!("{server}public_host = \"203.0.113.5\"\n"),
    )
    .await;
    let code = text.lines().find_map(|l| l.strip_prefix("code: ")).unwrap();
    assert_eq!(
        self::links(&text),
        [format!(
            "link (public_host): https://203.0.113.5:7801/#invite={code}&pin={pin}"
        )],
        "{text}"
    );
    assert!(!text.contains("VPN"), "{text}");
}

#[tokio::test]
async fn before_the_server_has_made_its_key_an_invite_is_its_code_alone_and_no_key_is_made() {
    // `invite create` only reads the key: one made here could be another user's than the
    // server's, which then couldn't read it.
    let dir = tempfile::tempdir().unwrap();
    let text = invite_with(dir.path(), "tls = \"self\"\napi_bind = \"0.0.0.0:7801\"\n").await;
    assert!(text.starts_with("code: "), "{text}");
    assert!(!text.contains("#invite="), "{text}");
    assert!(text.contains("key"), "says why: {text}");
    assert!(!dir.path().join("tls").exists());
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
    assert!(text.contains("\ncode: "), "{text}");
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
