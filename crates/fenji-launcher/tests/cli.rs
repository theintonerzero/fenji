//! `fenji-launch` as a player or the runner calls it (M3 plan Task 7).
use std::path::Path;
use std::process::{Command, Output};

fn launch(args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fenji-launch"));
    cmd.args(args).env_remove("FENJI_PASSWORD");
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[cfg(not(feature = "dev-login-path"))]
#[test]
fn play_in_a_default_build_explains_the_gate_and_exits_2() {
    let out = launch(
        &[
            "play",
            "--server",
            "https://play.example.net",
            "--username",
            "tester",
            "--print",
        ],
        &[("FENJI_PASSWORD", "a long password")],
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains("#114"), "{}", text(&out.stderr));
    assert!(out.stdout.is_empty());
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
    let out = launch(&["redeem", "--password", "x"], &[]);
    assert!(!out.status.success(), "no --password option");
}

#[cfg(feature = "dev-login-path")]
mod dev {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    use super::*;

    /// A canned account API on 127.0.0.1: sign-in, a game token, sign-out. Records each request's
    /// method and path.
    fn api() -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        std::thread::spawn(move || {
            for sock in listener.incoming() {
                let mut sock = sock.unwrap();
                let mut reader = BufReader::new(sock.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let mut len = 0;
                loop {
                    let mut h = String::new();
                    reader.read_line(&mut h).unwrap();
                    if h == "\r\n" || h.is_empty() {
                        break;
                    }
                    if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; len];
                reader.read_exact(&mut body).unwrap();
                let req: Vec<&str> = line.split(' ').take(2).collect();
                log.lock().unwrap().push(req.join(" "));
                let (status, json) = match (req[0], req[1]) {
                    ("POST", "/api/v1/sessions") => {
                        ("200 OK", r#"{"token":"fenji1-session","expires_at":1}"#.to_string())
                    }
                    ("POST", "/api/v1/game-tokens") => (
                        "200 OK",
                        r#"{"ticket":"fenji1-ticket","expires_in_secs":60,"gate_host":"127.0.0.1","gate_port":7800,"server_id":1,"server_name":"Fenji","account":"tester","user_id":5}"#
                            .to_string(),
                    ),
                    ("DELETE", "/api/v1/sessions") => ("204 No Content", String::new()),
                    _ => ("404 Not Found", r#"{"error":"not_found"}"#.to_string()),
                };
                let reply = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{json}",
                    json.len()
                );
                sock.write_all(reply.as_bytes()).unwrap();
            }
        });
        (format!("http://{addr}"), seen)
    }

    #[test]
    fn play_prints_only_the_login_paths_pairs_and_signs_out() {
        let (server, seen) = api();
        let out = launch(
            &[
                "play",
                "--server",
                &server,
                "--username",
                "tester",
                "--print",
            ],
            &[("FENJI_PASSWORD", "a long password")],
        );
        assert!(out.status.success(), "{}", text(&out.stderr));
        assert_eq!(
            text(&out.stdout),
            "gj_uinfo=1:90:Fenji:0:tester:5\ngj_gates=127.0.0.1:7800\ngj_ticket=fenji1-ticket\n"
        );
        assert_eq!(
            *seen.lock().unwrap(),
            [
                "POST /api/v1/sessions",
                "POST /api/v1/game-tokens",
                "DELETE /api/v1/sessions"
            ]
        );
    }

    #[test]
    fn play_without_print_has_nothing_to_start_in_m3() {
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
            text(&out.stderr).contains("--print"),
            "{}",
            text(&out.stderr)
        );
    }
}
