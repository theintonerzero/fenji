//! luatool end to end, on a synthetic gujianol.exe built from our own Lua source (no game bytes).
use std::path::{Path, PathBuf};
use std::process::Command;

use fenji_data::test_util::{exe_with_files, lz4_blob};
use fenji_lua::Dialect;

fn client_chunk(src: &str) -> Vec<u8> {
    let lua = mlua::Lua::new();
    let stock = lua.load(src).into_function().unwrap().dump(true);
    fenji_lua::write(
        &fenji_lua::parse(&stock, Dialect::Stock53).unwrap(),
        Dialect::Client,
    )
}

/// A damaged entry: its header and its file entry agree on the size, but the LZ4 block claims
/// 100 bytes of garbage after its real end.
fn corrupt_blob() -> Vec<u8> {
    let mut big = lz4_blob(&[7u8; 5000], 0);
    let comp = u32::from_le_bytes(big[32..36].try_into().unwrap()) + 100;
    big[32..36].copy_from_slice(&comp.to_le_bytes());
    let stored = u64::from_le_bytes(big[16..24].try_into().unwrap()) + 100;
    big[16..24].copy_from_slice(&stored.to_le_bytes());
    big.extend_from_slice(&[0x5A; 100]);
    big
}

/// A temporary checkout (`.git/`, `re/`) with a synthetic exe: a Lua chunk and an XML file in
/// `data/interface`, and a damaged entry in `data/scripts`.
fn setup() -> (tempfile::TempDir, PathBuf) {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir(repo.path().join(".git")).unwrap();
    std::fs::create_dir(repo.path().join("re")).unwrap();
    let pe = exe_with_files(&[
        (
            "interface/addons/Login/Test.luac",
            lz4_blob(&client_chunk("if IsServer() then print('s') end"), 0),
        ),
        ("interface/addons/Login/Test.xml", lz4_blob(b"<UI/>", 0)),
        ("scripts/combat/big.luac", corrupt_blob()),
    ]);
    let exe = repo.path().join("gujianol.exe");
    std::fs::write(&exe, pe).unwrap();
    (repo, exe)
}

fn luatool(repo: &Path, exe: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_luatool"));
    c.current_dir(repo).arg("--exe").arg(exe).arg("--any-build");
    c
}

#[test]
fn cli_lists_entries_with_status() {
    let (repo, exe) = setup();
    let out = luatool(repo.path(), &exe).arg("list").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 4, "{text}");
    assert!(
        lines[1].starts_with("data/interface/addons/Login/Test.luac\t")
            && lines[1].ends_with("\tlua\tok")
    );
    assert!(lines[2].ends_with("\tfile\tok"));
    assert!(
        lines[3].starts_with("data/scripts/combat/big.luac\t") && lines[3].contains("undecodable")
    );
}

#[test]
fn cli_extracts_and_disassembles_under_re() {
    let (repo, exe) = setup();
    let ok = luatool(repo.path(), &exe)
        .args(["extract", "--prefix", "data/interface/", "--out", "re/x"])
        .status()
        .unwrap();
    assert!(ok.success());
    assert_eq!(
        std::fs::read(
            repo.path()
                .join("re/x/data/interface/addons/Login/Test.xml")
        )
        .unwrap(),
        b"<UI/>"
    );
    assert!(
        repo.path()
            .join("re/x/data/interface/addons/Login/Test.luac")
            .is_file()
    );
    assert!(
        !repo.path().join("re/x/data/scripts").exists(),
        "--prefix selects"
    );
    let ok = luatool(repo.path(), &exe)
        .args(["disasm", "--out", "re/d"])
        .status()
        .unwrap();
    assert!(ok.success());
    let dis = std::fs::read_to_string(
        repo.path()
            .join("re/d/data/interface/addons/Login/Test.luac.dis"),
    )
    .unwrap();
    assert!(dis.contains("\"IsServer\""));
    let failures = std::fs::read_to_string(repo.path().join("re/d/disasm-failures.tsv")).unwrap();
    assert!(failures.contains("data/scripts/combat/big.luac"));
}

#[test]
fn cli_refuses_out_dir_outside_re() {
    let (repo, exe) = setup();
    let out = luatool(repo.path(), &exe)
        .args(["extract", "--out", "docs"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not inside"));
    assert!(!repo.path().join("docs").exists());
}

#[test]
fn cli_refuses_unpinned_build() {
    let (repo, exe) = setup();
    let out = Command::new(env!("CARGO_BIN_EXE_luatool"))
        .current_dir(repo.path())
        .arg("--exe")
        .arg(&exe)
        .arg("list")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("not the pinned build") && err.contains(fenji_data::PINNED_EXE_SHA256),
        "{err}"
    );
}
