//! `fenji-launch` as a player or the runner calls it (M3 plan Task 7; gate #114, option a).
mod common;

use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output};

use common::*;

fn launch(args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fenji-launch"));
    // No real umu-run (or anything else) can be found and started from a test, and nothing
    // reaches the real data folders.
    cmd.args(args)
        .env_remove("FENJI_PASSWORD")
        .env("PATH", "")
        .env("HOME", "/nonexistent/fenji-test-home")
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_CONFIG_HOME");
    for k in [
        "http_proxy",
        "HTTP_PROXY",
        "https_proxy",
        "HTTPS_PROXY",
        "all_proxy",
        "ALL_PROXY",
    ] {
        cmd.env_remove(k);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

fn play(server: &str) -> Output {
    launch(
        &[
            "play",
            "--server",
            server,
            "--username",
            "tester",
            "--print",
        ],
        &[("FENJI_PASSWORD", "a long password")],
    )
}

#[cfg(target_os = "linux")]
#[test]
fn play_prints_only_the_login_paths_pairs_and_signs_out() {
    let (server, seen) = canned(Box::new(usual));
    let out = play(&server);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        format!(
            "gj_uinfo=1:90:Fenji:0:tester:5\ngj_gates=127.0.0.1:7800\ngj_ticket=fenji1-{}\n",
            "ab".repeat(32)
        )
    );
    assert_eq!(
        *seen.lock().unwrap(),
        [
            "GET /api/v1/server",
            "POST /api/v1/sessions",
            "POST /api/v1/game-tokens",
            "DELETE /api/v1/sessions"
        ]
    );
}

#[cfg(target_os = "linux")]
#[test]
fn play_checks_the_gate_before_any_password_is_sent() {
    let (server, seen) = canned(Box::new(|m, p| match (m, p) {
        ("GET", "/api/v1/server") => ("200 OK", server_info("no-such-host.invalid")),
        _ => usual(m, p),
    }));
    let out = play(&server);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("no-such-host.invalid"),
        "{}",
        text(&out.stderr)
    );
    assert!(out.stdout.is_empty());
    assert_eq!(*seen.lock().unwrap(), ["GET /api/v1/server"]);
}

#[cfg(target_os = "linux")]
#[test]
fn a_server_cannot_put_escape_sequences_on_the_terminal() {
    let (server, seen) = canned(Box::new(|m, p| match (m, p) {
        ("GET", "/api/v1/server") => ("200 OK", server_info("\\u001b[2Jgate.example")),
        _ => usual(m, p),
    }));
    let out = play(&server);
    assert_eq!(out.status.code(), Some(1));
    assert!(!out.stderr.contains(&0x1b), "{:?}", text(&out.stderr));
    assert_eq!(*seen.lock().unwrap(), ["GET /api/v1/server"]);
}

#[cfg(target_os = "linux")]
#[test]
fn play_signs_out_even_when_the_game_token_fails() {
    let (server, seen) = canned(Box::new(|m, p| match (m, p) {
        ("POST", "/api/v1/game-tokens") => {
            ("401 Unauthorized", r#"{"error":"bad_session"}"#.into())
        }
        _ => usual(m, p),
    }));
    let out = play(&server);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert!(
        text(&out.stderr).contains("bad_session"),
        "{}",
        text(&out.stderr)
    );
    assert!(
        seen.lock()
            .unwrap()
            .contains(&"DELETE /api/v1/sessions".to_string())
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_grant_that_would_bend_the_login_fields_is_refused() {
    for name in ["Fen:ji", "Fen\\nji"] {
        let (server, _seen) = canned(Box::new(move |m, p| match (m, p) {
            ("POST", "/api/v1/game-tokens") => ("200 OK", grant(name)),
            _ => usual(m, p),
        }));
        let out = play(&server);
        assert_eq!(out.status.code(), Some(1), "{name}");
        assert!(out.stdout.is_empty(), "{name}: {}", text(&out.stdout));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn play_without_print_needs_a_client_folder() {
    let out = launch(
        &[
            "play",
            "--server",
            "http://127.0.0.1:9",
            "--username",
            "tester",
        ],
        &[("FENJI_PASSWORD", "a long password")],
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(
        text(&out.stderr).contains("--client"),
        "{}",
        text(&out.stderr)
    );
}

#[cfg(target_os = "linux")]
#[test]
fn play_refuses_a_client_folder_that_isnt_the_pinned_build() {
    let (server, seen) = canned(Box::new(usual));
    let d = tempfile::tempdir().unwrap();
    let out = launch(
        &[
            "play",
            "--server",
            &server,
            "--username",
            "tester",
            "--client",
            d.path().to_str().unwrap(),
            "--umu-run",
            "/nonexistent/umu-run",
        ],
        &[("FENJI_PASSWORD", "a long password")],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("isn't the pinned build"),
        "{}",
        text(&out.stderr)
    );
    assert!(
        seen.lock().unwrap().is_empty(),
        "nothing is sent for a bad folder"
    );
}

/// Only the pinned build starts: no other manifest can be named (ground rule 1).
#[cfg(target_os = "linux")]
#[test]
fn play_takes_no_other_manifest() {
    let out = launch(
        &[
            "play",
            "--server",
            "http://127.0.0.1:9",
            "--username",
            "tester",
            "--client",
            "/nonexistent",
            "--manifest",
            "/nonexistent/m.tsv",
        ],
        &[("FENJI_PASSWORD", "a long password")],
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(
        text(&out.stderr).contains("unexpected argument '--manifest'"),
        "{}",
        text(&out.stderr)
    );
}

#[cfg(target_os = "linux")]
#[test]
fn play_says_how_to_get_umu_run_when_it_isnt_on_path() {
    let (server, seen) = canned(Box::new(usual));
    let d = tempfile::tempdir().unwrap();
    let out = launch(
        &[
            "play",
            "--server",
            &server,
            "--username",
            "tester",
            "--client",
            d.path().to_str().unwrap(),
        ],
        &[("FENJI_PASSWORD", "a long password")],
    );
    assert_eq!(out.status.code(), Some(1));
    let err = text(&out.stderr);
    assert!(
        err.contains("umu-launcher") && err.contains("--umu-run"),
        "{err}"
    );
    assert!(seen.lock().unwrap().is_empty(), "nothing is sent");
}

/// Every other system is refused with one neutral line, before anything is sent (plan
/// Amendment B; ground rule 6).
#[cfg(not(target_os = "linux"))]
#[test]
fn play_is_refused_off_linux_even_with_print() {
    let (server, seen) = canned(Box::new(usual));
    let out = play(&server);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        text(&out.stderr).trim_end(),
        "fenji-launch: Fenji can't start the client on this system yet"
    );
    assert!(out.stdout.is_empty());
    assert!(seen.lock().unwrap().is_empty(), "nothing is sent");
}

#[test]
fn this_machine_is_never_reached_through_a_proxy() {
    let (server, seen) = canned(Box::new(usual));
    let trap = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy = format!("http://{}", trap.local_addr().unwrap());
    let env: Vec<(&str, &str)> = ["http_proxy", "HTTP_PROXY", "all_proxy", "ALL_PROXY"]
        .into_iter()
        .map(|k| (k, proxy.as_str()))
        .chain([("FENJI_PASSWORD", "a long password")])
        .collect();
    let out = launch(
        &[
            "redeem",
            "--server",
            &server,
            "--invite",
            "ABCD-EFGH-JKMN-PQRS",
            "--username",
            "tester",
        ],
        &env,
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(*seen.lock().unwrap(), ["POST /api/v1/accounts"]);
    trap.set_nonblocking(true).unwrap();
    assert!(
        trap.accept().is_err(),
        "the password went through the proxy"
    );
}

#[test]
fn verify_reports_what_differs_and_exits_1() {
    let d = tempfile::tempdir().unwrap();
    let client = d.path().join("client");
    std::fs::create_dir_all(client.join("bin64")).unwrap();
    std::fs::write(client.join("bin64/gujianol.exe"), b"exe!").unwrap();
    let manifest = d.path().join("m.tsv");
    let made = launch(
        &[
            "manifest",
            "--client",
            client.to_str().unwrap(),
            "--out",
            manifest.to_str().unwrap(),
        ],
        &[],
    );
    assert!(made.status.success(), "{}", text(&made.stderr));
    assert!(
        std::fs::read_to_string(&manifest)
            .unwrap()
            .starts_with("# ")
    );
    let args = |m: &Path| {
        vec![
            "verify".to_string(),
            "--client".to_string(),
            client.to_str().unwrap().to_string(),
            "--manifest".to_string(),
            m.to_str().unwrap().to_string(),
        ]
    };
    let a = args(&manifest);
    let ok = launch(&a.iter().map(String::as_str).collect::<Vec<_>>(), &[]);
    assert_eq!(ok.status.code(), Some(0), "{}", text(&ok.stdout));
    std::fs::write(client.join("bin64/gujianol.exe"), b"exe?").unwrap();
    let bad = launch(&a.iter().map(String::as_str).collect::<Vec<_>>(), &[]);
    assert_eq!(bad.status.code(), Some(1));
    assert!(
        text(&bad.stdout).contains("bin64/gujianol.exe"),
        "{}",
        text(&bad.stdout)
    );
    // Against the pinned build, an almost empty folder misses nearly everything.
    let pinned = launch(&["verify", "--client", client.to_str().unwrap()], &[]);
    assert_eq!(pinned.status.code(), Some(1));
    assert!(
        text(&pinned.stdout).contains("missing"),
        "{}",
        text(&pinned.stdout)
    );
    let nowhere = launch(
        &[
            "verify",
            "--client",
            d.path().join("nope").to_str().unwrap(),
        ],
        &[],
    );
    assert_eq!(nowhere.status.code(), Some(1));
    assert!(
        text(&nowhere.stderr).contains("no folder"),
        "{}",
        text(&nowhere.stderr)
    );
}

#[test]
fn a_password_never_comes_from_the_command_line() {
    let out = launch(
        &[
            "redeem",
            "--server",
            "http://127.0.0.1:9",
            "--invite",
            "ABCD-EFGH-JKMN-PQRS",
            "--username",
            "tester",
        ],
        &[],
    );
    assert!(!out.status.success());
    let err = text(&out.stderr);
    assert!(
        err.contains("FENJI_PASSWORD") && err.contains("--password-file"),
        "{err}"
    );
    let out = launch(
        &[
            "redeem",
            "--server",
            "http://127.0.0.1:9",
            "--invite",
            "ABCD-EFGH-JKMN-PQRS",
            "--username",
            "tester",
            "--password",
            "x",
        ],
        &[],
    );
    assert!(
        text(&out.stderr).contains("unexpected argument '--password'"),
        "{}",
        text(&out.stderr)
    );
}
