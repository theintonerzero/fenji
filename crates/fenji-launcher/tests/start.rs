//! `start::play`, the whole start as a player's launcher runs it (M3 plan Task 7; #138), against the
//! canned account API and a fake umu-run. Linux only: no other system starts the client yet.
#![cfg(target_os = "linux")]
mod common;

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::*;
use fenji_launcher::launch::Platform;
use fenji_launcher::manifest::Manifest;
use fenji_launcher::start::{Login, play};

/// A client folder of one file, `client/` in `d`, and its manifest.
fn client(d: &Path) -> Manifest {
    let c = d.join("client");
    std::fs::create_dir_all(c.join("bin64")).unwrap();
    std::fs::write(c.join("bin64/gujianol.exe"), b"exe!").unwrap();
    Manifest::generate(&c).unwrap()
}

/// A fake umu-run. `createprefix` logs itself, builds what Proton builds (when `builds`) and exits
/// 1, as umu-launcher 1.4 does; anything else is the client, which logs its arguments, folder and
/// launch variables, then exits 3.
fn fake_umu(d: &Path, log: &Path, builds: bool) -> PathBuf {
    let p = d.join("umu-run");
    let build = if builds {
        "mkdir -p \"$WINEPREFIX/drive_c/windows\" && echo 'WINE REGISTRY Version 2' > \"$WINEPREFIX/system.reg\""
    } else {
        ":"
    };
    std::fs::write(
        &p,
        format!(
            "#!/bin/sh\nif [ \"$1\" = createprefix ]; then\n  echo 'umu createprefix' >> {log}\n  {build}\n  exit 1\nfi\n{{ echo \"umu $*\"; echo \"cwd $(pwd)\"; env | grep -E '^(gj_|WINEPREFIX=|GAMEID=|FENJI_)' | sort; }} >> {log}\nexit 3\n",
            log = log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

/// The canned API, writing each request to `log`, where the fake umu-run writes too.
fn api(log: &Path) -> String {
    let log = log.to_path_buf();
    let (server, _seen) = canned(Box::new(move |m, p| {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .unwrap();
        writeln!(f, "api {m} {p}").unwrap();
        usual(m, p)
    }));
    server
}

#[test]
fn play_prepares_the_prefix_then_starts_the_client_with_the_ticket_in_its_environment() {
    let d = tempfile::tempdir().unwrap();
    let manifest = client(d.path());
    let log = d.path().join("log");
    let platform = Platform::Linux {
        umu_run: fake_umu(d.path(), &log, true),
        prefix: d.path().join("prefix"),
        proton: None,
    };
    let server = api(&log);
    let login = Login {
        server: &server,
        username: "tester",
        password: "a long password",
    };
    let mut notes = Vec::new();
    // The client folder is given relative to where the command ran.
    let status = play(
        Path::new("client"),
        d.path(),
        &manifest,
        &platform,
        &login,
        &mut |m| notes.push(m.to_string()),
    )
    .unwrap();
    assert_eq!(status.code(), Some(3), "the client's own exit status");

    let seen = std::fs::read_to_string(&log).unwrap();
    let lines: Vec<&str> = seen.lines().collect();
    let at = |what: &str| {
        lines
            .iter()
            .position(|l| *l == what)
            .unwrap_or_else(|| panic!("no {what:?} in {seen}"))
    };
    let (gate, prepared, signed_in) = (
        at("api GET /api/v1/server"),
        at("umu createprefix"),
        at("api POST /api/v1/sessions"),
    );
    let started = lines
        .iter()
        .position(|l| l.starts_with("umu ") && l.contains("gujianol.exe"))
        .expect("started");
    assert!(
        gate < prepared && prepared < signed_in && signed_in < started,
        "the gate, then the prefix, then the password, then the client: {seen}"
    );
    assert!(lines.contains(&"api DELETE /api/v1/sessions"), "{seen}");

    let client = d.path().join("client");
    assert_eq!(
        lines[started],
        format!(
            "umu {}/bin64/gujianol.exe --fs=data:_index:_index",
            client.display()
        ),
        "an absolute path, though the folder was given relative"
    );
    let ticket = format!("fenji1-{}", "ab".repeat(32));
    assert!(
        !lines[started].contains(&ticket),
        "never on the command line"
    );
    let after = &lines[started..];
    for want in [
        format!("cwd {}/bin64", client.display()),
        format!("gj_ticket={ticket}"),
        "gj_uinfo=1:90:Fenji:0:tester:5".to_string(),
        "gj_gates=127.0.0.1:7800".to_string(),
        "GAMEID=umu-1418100".to_string(),
        format!("WINEPREFIX={}", d.path().join("prefix").display()),
    ] {
        assert!(after.contains(&want.as_str()), "{want}: {seen}");
    }
    assert!(
        !seen.contains("a long password"),
        "the password never reaches umu-run or the client"
    );
    assert!(
        notes.iter().all(|n| !n.contains(&ticket)),
        "the ticket is never shown: {notes:?}"
    );
}

#[test]
fn a_prefix_that_isnt_ready_stops_the_start_before_any_password_is_sent() {
    let d = tempfile::tempdir().unwrap();
    let manifest = client(d.path());
    let log = d.path().join("log");
    let platform = Platform::Linux {
        umu_run: fake_umu(d.path(), &log, false),
        prefix: d.path().join("prefix"),
        proton: None,
    };
    let server = api(&log);
    let login = Login {
        server: &server,
        username: "tester",
        password: "a long password",
    };
    let e = play(
        &d.path().join("client"),
        d.path(),
        &manifest,
        &platform,
        &login,
        &mut |_| {},
    )
    .unwrap_err();
    assert!(format!("{e:#}").contains("isn't ready"), "{e:#}");
    let seen = std::fs::read_to_string(&log).unwrap();
    assert!(seen.contains("umu createprefix"), "{seen}");
    assert!(
        !seen.contains("/api/v1/sessions"),
        "no password sent: {seen}"
    );
    assert!(!seen.contains("gujianol.exe"), "the client never started");
}
