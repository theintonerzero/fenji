#![cfg(unix)]
//! The unluac runner, with a stand-in for `java -jar unluac.jar FILE`: it echoes, fails or hangs
//! depending on FILE's name.
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fenji_luatool::decompile::{Outcome, Unluac};

fn fake_java(dir: &Path) -> PathBuf {
    let p = dir.join("fake-java");
    std::fs::write(
        &p,
        "#!/bin/sh\ncase \"$3\" in *slow*) sleep 10 ;; *fail*) echo boom >&2; exit 1 ;; esac\necho \"-- decompiled $3\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn unluac(dir: &Path, timeout: Duration) -> Unluac {
    let jar = dir.join("unluac.jar");
    std::fs::write(&jar, b"").unwrap();
    Unluac::locate(Some(fake_java(dir)), Some(jar), dir, timeout).unwrap()
}

#[test]
fn outcomes_and_outputs() {
    let t = tempfile::tempdir().unwrap();
    let u = unluac(t.path(), Duration::from_secs(5));
    let jobs: Vec<(PathBuf, PathBuf)> = ["ok", "fail", "ok2"]
        .iter()
        .map(|n| {
            (
                t.path().join(format!("{n}.luac")),
                t.path().join(format!("{n}.lua")),
            )
        })
        .collect();
    assert_eq!(
        u.run_all(&jobs, 2),
        [Outcome::Ok, Outcome::Failed, Outcome::Ok]
    );
    assert!(
        std::fs::read_to_string(t.path().join("ok.lua"))
            .unwrap()
            .starts_with("-- decompiled")
    );
    assert!(!t.path().join("ok.lua.err").exists());
    assert_eq!(
        std::fs::read_to_string(t.path().join("fail.lua.err"))
            .unwrap()
            .trim(),
        "boom"
    );
}

#[test]
fn a_hanging_decompile_is_stopped() {
    let t = tempfile::tempdir().unwrap();
    let u = unluac(t.path(), Duration::from_millis(500));
    let started = Instant::now();
    let outcome = u
        .run(&t.path().join("slow.luac"), &t.path().join("slow.lua"))
        .unwrap();
    assert_eq!(outcome, Outcome::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_missing_jar_is_reported() {
    let t = tempfile::tempdir().unwrap();
    let err = Unluac::locate(
        Some("java".into()),
        Some(t.path().join("nope.jar")),
        t.path(),
        Duration::from_secs(1),
    )
    .err()
    .unwrap();
    assert!(err.to_string().contains("unluac jar not found"));
}

#[test]
#[ignore = "needs Java and the pinned unluac jar (scripts/bootstrap.sh)"]
fn real_unluac_decompiles_a_stripped_chunk() {
    let t = tempfile::tempdir().unwrap();
    let lua = mlua::Lua::new();
    let chunk = lua
        .load("local s = { host = '127.0.0.1' } return s")
        .into_function()
        .unwrap()
        .dump(true);
    let input = t.path().join("t.luac");
    std::fs::write(&input, chunk).unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let u = Unluac::locate(None, None, &repo, Duration::from_secs(60)).unwrap();
    assert_eq!(u.run(&input, &t.path().join("t.lua")).unwrap(), Outcome::Ok);
    assert!(
        std::fs::read_to_string(t.path().join("t.lua"))
            .unwrap()
            .contains("127.0.0.1")
    );
}
