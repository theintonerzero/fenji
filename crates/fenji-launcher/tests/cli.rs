//! `fenji-launch` as a player or the runner calls it (M3 plan Task 7; gate #114, option a).
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

fn launch(args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fenji-launch"));
    cmd.args(args).env_remove("FENJI_PASSWORD");
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

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// What the canned API answers: a status line ("200 OK") and a JSON body, by method and path.
type Answer = Box<dyn Fn(&str, &str) -> (&'static str, String) + Send>;

/// A canned account API on 127.0.0.1 that records each request's method and path.
fn canned(answer: Answer) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    std::thread::spawn(move || {
        for sock in listener.incoming() {
            let Ok(mut sock) = sock else { return };
            let mut reader = BufReader::new(sock.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let mut len = 0;
            loop {
                let mut h = String::new();
                if reader.read_line(&mut h).is_err() || h == "\r\n" || h.is_empty() {
                    break;
                }
                if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0; len];
            let _ = reader.read_exact(&mut body);
            let req: Vec<&str> = line.split(' ').take(2).collect();
            if req.len() < 2 {
                continue;
            }
            log.lock().unwrap().push(req.join(" "));
            let (status, json) = answer(req[0], req[1]);
            let reply = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{json}",
                json.len()
            );
            let _ = sock.write_all(reply.as_bytes());
        }
    });
    (format!("http://{addr}"), seen)
}

fn server_info(gate_host: &str) -> String {
    format!(
        r#"{{"name":"Fenji","version":"0.4.0","pinned_build":"10015727","gate_host":"{gate_host}","gate_port":7800,"server_id":1,"api_version":1}}"#
    )
}

fn grant(server_name: &str) -> String {
    format!(
        r#"{{"ticket":"fenji1-{}","expires_in_secs":60,"gate_host":"127.0.0.1","gate_port":7800,"server_id":1,"server_name":"{server_name}","account":"tester","user_id":5}}"#,
        "ab".repeat(32)
    )
}

/// The usual answers: server info with a local gate, a session, a game token, a sign-out.
fn usual(method: &str, path: &str) -> (&'static str, String) {
    match (method, path) {
        ("GET", "/api/v1/server") => ("200 OK", server_info("127.0.0.1")),
        ("POST", "/api/v1/accounts") => ("201 Created", r#"{"username":"tester"}"#.into()),
        ("POST", "/api/v1/sessions") => (
            "200 OK",
            r#"{"token":"fenji1-session","expires_at":1}"#.into(),
        ),
        ("POST", "/api/v1/game-tokens") => ("200 OK", grant("Fenji")),
        ("DELETE", "/api/v1/sessions") => ("204 No Content", String::new()),
        _ => ("404 Not Found", r#"{"error":"not_found"}"#.into()),
    }
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

/// Starting the client: the folder is checked, the prefix is made ready before the token is taken,
/// and the client gets the ticket in its environment only, run from bin64.
#[cfg(unix)]
#[test]
fn play_prepares_the_prefix_then_starts_the_client_with_the_ticket_in_its_environment() {
    use std::os::unix::fs::PermissionsExt;

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
    assert!(made.status.success());
    let log = d.path().join("log");
    // A fake umu-run that records its arguments, folder and the variables a launch sets.
    let fake = d.path().join("umu-run");
    std::fs::write(
        &fake,
        format!(
            "#!/bin/sh\n{{ echo \"umu $*\"; echo \"cwd $(pwd)\"; env | grep -E '^(gj_|WINEPREFIX=|GAMEID=)' | sort; }} >> {}\n",
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let api_log = log.clone();
    let (server, _seen) = canned(Box::new(move |m, p| {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&api_log)
            .unwrap();
        writeln!(f, "api {m} {p}").unwrap();
        usual(m, p)
    }));
    let prefix = d.path().join("prefix");
    let out = launch(
        &[
            "play",
            "--server",
            &server,
            "--username",
            "tester",
            "--client",
            client.to_str().unwrap(),
            "--manifest",
            manifest.to_str().unwrap(),
            "--umu-run",
            fake.to_str().unwrap(),
            "--prefix",
            prefix.to_str().unwrap(),
        ],
        &[("FENJI_PASSWORD", "a long password")],
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(out.stdout.is_empty(), "nothing on stdout when starting");
    let ticket = format!("fenji1-{}", "ab".repeat(32));
    let seen = std::fs::read_to_string(&log).unwrap();
    let lines: Vec<&str> = seen.lines().collect();
    let wineboot = lines
        .iter()
        .position(|l| *l == "umu wineboot -u")
        .expect("prepared");
    let sign_in = lines
        .iter()
        .position(|l| *l == "api POST /api/v1/sessions")
        .unwrap();
    let start = lines
        .iter()
        .position(|l| l.starts_with("umu ") && l.contains("gujianol.exe"))
        .expect("started");
    assert!(wineboot < sign_in && sign_in < start, "{seen}");
    // Before the token: no login variables at all.
    assert!(
        !lines[wineboot..sign_in]
            .iter()
            .any(|l| l.starts_with("gj_")),
        "{seen}"
    );
    // The client: run from bin64, the --fs argument, the ticket only in the environment.
    assert_eq!(
        lines[start],
        format!(
            "umu {}/bin64/gujianol.exe --fs=data:_index:_index",
            client.display()
        )
    );
    assert!(!lines[start].contains(&ticket));
    let cwd = format!("cwd {}/bin64", client.display());
    assert!(lines[start..].contains(&cwd.as_str()), "{seen}");
    let pair = format!("gj_ticket={ticket}");
    assert!(lines[start..].contains(&pair.as_str()), "{seen}");
    assert!(lines[start..].contains(&"GAMEID=umu-1418100"), "{seen}");
    let wp = format!("WINEPREFIX={}", prefix.display());
    assert!(lines[start..].contains(&wp.as_str()), "{seen}");
    assert!(
        !text(&out.stderr).contains(&ticket),
        "the ticket is never printed"
    );
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
