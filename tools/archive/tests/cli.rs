//! archive end to end, on a synthetic client built from our own bytes (no game bytes).
use std::path::Path;
use std::process::Command;

use fenji_data::test_util::{lz4_blob, stored_blob, zfs_part};
use sha1::{Digest, Sha1};

const VIDEO: &[u8] = b"KB2j fake video bytes";
const CONTAINER: &[u8] = b"[SP] fake container";

/// An LZ4 blob relabelled codec 4 (Oodle): the tool reads only its header.
fn oodle_blob() -> Vec<u8> {
    let mut b = lz4_blob(b"abcabcabcabc", 0);
    b[28] = 4;
    b
}

fn sha1(bytes: &[u8]) -> [u8; 20] {
    Sha1::digest(bytes).into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A temporary checkout (`.git/`, `re/`) with a synthetic client. data000 has two index blocks: a
/// stored video and an Oodle file, then a deleted record and a stored container. data001 has an
/// Oodle file and a stored file whose key isn't its SHA-1. _index is an Oodle blob.
fn setup() -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir(repo.path().join(".git")).unwrap();
    std::fs::create_dir(repo.path().join("re")).unwrap();
    let data = repo.path().join("client/data");
    std::fs::create_dir_all(&data).unwrap();
    let part0 = zfs_part(&[
        vec![
            (1, sha1(VIDEO), stored_blob(VIDEO)),
            (1, [2; 20], oodle_blob()),
        ],
        vec![
            (0xff, [3; 20], stored_blob(b"old")),
            (1, sha1(CONTAINER), stored_blob(CONTAINER)),
        ],
    ]);
    std::fs::write(data.join("data000"), part0).unwrap();
    let part1 = zfs_part(&[vec![
        (1, [9; 20], oodle_blob()),
        (1, [8; 20], stored_blob(b"wrong key")),
    ]]);
    std::fs::write(data.join("data001"), part1).unwrap();
    std::fs::write(data.join("_index"), oodle_blob()).unwrap();
    repo
}

fn archive(repo: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_archive"));
    c.current_dir(repo).arg("--client").arg(repo.join("client"));
    c
}

#[test]
fn info_counts_each_part() {
    let repo = setup();
    let out = archive(repo.path()).arg("info").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines,
        [
            "part\tblocks\tlive\tdeleted\tcrc_bad\tstored\tlz4\toodle\tother\tcontainers\tsize_mismatch\tbad_header",
            "000\t2\t3\t1\t0\t2\t0\t1\t0\t1\t0\t0",
            "001\t1\t2\t0\t0\t1\t0\t1\t0\t0\t0\t0",
        ]
    );
}

#[test]
fn list_shows_every_record_with_its_blob() {
    let repo = setup();
    let out = archive(repo.path())
        .args(["list", "--part", "0"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 5, "a header and four records: {text}");
    assert_eq!(
        lines[1],
        format!(
            "0\t{}\t0x2000c\t53\tused\tstored\t21\t53",
            hex(&sha1(VIDEO))
        )
    );
    assert!(lines[2].contains("\tused\toodle\t12\t"), "{text}");
    assert!(
        lines[3].starts_with("4096\t") && lines[3].contains("\tdeleted\t"),
        "{text}"
    );
}

#[test]
fn extract_writes_stored_files_named_by_key() {
    let repo = setup();
    let out = archive(repo.path())
        .args(["extract", "--part", "0", "--stored-only", "--out", "re/a"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let dir = repo.path().join("re/a/data000");
    assert_eq!(
        std::fs::read(dir.join(format!("{}.bk2", hex(&sha1(VIDEO))))).unwrap(),
        VIDEO
    );
    assert!(dir.join(format!("{}.sp", hex(&sha1(CONTAINER)))).is_file());
    assert_eq!(
        std::fs::read_dir(&dir).unwrap().count(),
        2,
        "neither the deleted record nor the Oodle file is written"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("2 stored files written") && err.contains("2 match their key"),
        "{err}"
    );
}

#[test]
fn extract_reports_a_file_that_doesnt_match_its_key() {
    let repo = setup();
    let out = archive(repo.path())
        .args(["extract", "--part", "1", "--stored-only", "--out", "re/a"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("1 stored files written") && err.contains("1 don't"),
        "{err}"
    );
}

#[test]
fn extract_refuses_without_stored_only_or_outside_re() {
    let repo = setup();
    let all = archive(repo.path())
        .args(["extract", "--part", "0", "--out", "re/a"])
        .output()
        .unwrap();
    assert!(!all.status.success());
    assert!(String::from_utf8_lossy(&all.stderr).contains("decision gate"));
    let outside = archive(repo.path())
        .args([
            "extract",
            "--part",
            "0",
            "--stored-only",
            "--out",
            "elsewhere",
        ])
        .status()
        .unwrap();
    assert!(!outside.success());
    assert!(!repo.path().join("elsewhere").exists());
}

#[cfg(unix)]
#[test]
fn extract_replaces_a_link_instead_of_writing_through_it() {
    let repo = setup();
    std::fs::create_dir(repo.path().join("docs")).unwrap();
    std::fs::write(repo.path().join("docs/tracked.md"), "keep").unwrap();
    let dir = repo.path().join("re/a/data000");
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join(format!("{}.bk2", hex(&sha1(VIDEO))));
    std::fs::hard_link(repo.path().join("docs/tracked.md"), &target).unwrap();
    let ok = archive(repo.path())
        .args(["extract", "--part", "0", "--stored-only", "--out", "re/a"])
        .status()
        .unwrap();
    assert!(ok.success());
    assert_eq!(
        std::fs::read_to_string(repo.path().join("docs/tracked.md")).unwrap(),
        "keep"
    );
    assert_eq!(std::fs::read(&target).unwrap(), VIDEO);
}

#[test]
fn index_reads_the_index_header() {
    let repo = setup();
    let out = archive(repo.path()).arg("index").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.contains("codec 4 (oodle)") && text.contains("blocks 1"),
        "{text}"
    );
}
