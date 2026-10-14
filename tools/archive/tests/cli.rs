//! archive end to end, on a synthetic client built from our own bytes (no game bytes).
use std::path::Path;
use std::process::Command;

use fenji_data::test_util::{lz4_blob, lz4_blob_blocks, stored_blob, zfs_part};
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
    assert!(String::from_utf8_lossy(&all.stderr).contains("--stored-only"));
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

#[test]
fn paths_refuses_outside_re() {
    let repo = setup();
    let out = archive(repo.path())
        .args(["paths", "--out", "/tmp/x"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("is not inside"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let spool_outside = archive(repo.path())
        .args(["paths", "--spool", "elsewhere", "--out", "re/p"])
        .output()
        .unwrap();
    assert!(!spool_outside.status.success());
    assert!(
        String::from_utf8_lossy(&spool_outside.stderr).contains("elsewhere is not inside"),
        "{}",
        String::from_utf8_lossy(&spool_outside.stderr)
    );
    assert!(!repo.path().join("elsewhere").exists());
}

#[test]
fn paths_spools_index_and_waits_for_the_helper() {
    let repo = setup();
    let out = archive(repo.path())
        .args(["paths", "--out", "re/p"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("1 blocks spooled"), "{text}");
    assert_eq!(
        std::fs::read_dir(repo.path().join("re/oodle-spool/in"))
            .unwrap()
            .count(),
        1
    );
    assert!(!repo.path().join("re/p/paths.tsv").exists());
}

/// A repo whose _index is decoded in the spool: three paths (see `paths_maps_index_paths_to_records`).
fn decoded_index_repo() -> tempfile::TempDir {
    // An _index of three paths: one stored file in data000, one Oodle file in data001, one key
    // no part holds.
    repo_with_index(&format!(
        "{}\tmovie/intro.bk2\n{}\tdata/x.tab\n{}\tgone/y.tab\n",
        hex(&sha1(VIDEO)),
        hex(&[9; 20]),
        hex(&[7; 20])
    ))
}

/// A synthetic client whose _index holds `text`. Its one block stands in for Oodle data; its
/// "decoded" output is placed in the spool as the helper would.
fn repo_with_index(text: &str) -> tempfile::TempDir {
    use fenji_data::oodle_spool::OodleSpool;
    let repo = setup();
    let mut index = lz4_blob(text.as_bytes(), 0);
    index[28] = 4;
    std::fs::write(repo.path().join("client/data/_index"), &index).unwrap();
    let block = &index[fenji_data::FIXED_LEN + 4..];
    let name = OodleSpool::name(block, text.len());
    let spool = repo.path().join("re/oodle-spool");
    std::fs::create_dir_all(spool.join("out")).unwrap();
    std::fs::write(spool.join("out").join(name), text).unwrap();
    repo
}

#[test]
fn paths_maps_index_paths_to_records() {
    let repo = decoded_index_repo();
    let out = archive(repo.path())
        .args(["paths", "--out", "re/p"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let summary = String::from_utf8_lossy(&out.stdout);
    assert!(
        summary.contains("3 paths") && summary.contains("2 found in a part, 1 not found"),
        "{summary}"
    );
    let tsv = std::fs::read_to_string(repo.path().join("re/p/paths.tsv")).unwrap();
    let lines: Vec<&str> = tsv.lines().collect();
    assert_eq!(
        lines,
        [
            "key\tpath\tpart\toffset\tsize\tcodec".to_string(),
            format!(
                "{}\tdata/x.tab\t001\t0x2000c\t{}\toodle",
                hex(&[9; 20]),
                oodle_blob().len()
            ),
            format!("{}\tgone/y.tab\t-\t-\t-\t-", hex(&[7; 20])),
            format!(
                "{}\tmovie/intro.bk2\t000\t0x2000c\t53\tstored",
                hex(&sha1(VIDEO))
            ),
        ]
    );
}

#[cfg(unix)]
#[test]
fn paths_replaces_a_link_instead_of_writing_through_it() {
    let repo = decoded_index_repo();
    std::fs::create_dir(repo.path().join("docs")).unwrap();
    std::fs::write(repo.path().join("docs/tracked.md"), "keep").unwrap();
    std::fs::create_dir_all(repo.path().join("re/p")).unwrap();
    let tsv = repo.path().join("re/p/paths.tsv");
    std::os::unix::fs::symlink(repo.path().join("docs/tracked.md"), &tsv).unwrap();
    let ok = archive(repo.path())
        .args(["paths", "--out", "re/p"])
        .status()
        .unwrap();
    assert!(ok.success());
    assert_eq!(
        std::fs::read_to_string(repo.path().join("docs/tracked.md")).unwrap(),
        "keep"
    );
    assert!(
        !std::fs::symlink_metadata(&tsv)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        std::fs::read_to_string(&tsv)
            .unwrap()
            .starts_with("key\tpath")
    );
}

/// A table cache (docs/re/m2-tables.md §2) with one sheet, `items`, shaped like the client's: each
/// row's `__index` refers to the row itself (2.8). Four tables are referable; row 8's `effects`
/// is a fifth, which isn't.
fn cache_bytes() -> Vec<u8> {
    let mut b = vec![0x81, 0x00, 0x00, 0x04]; // one value; four referable tables
    b.extend([0x0B, 0, 0, 0, 0]); // 0: root
    b.extend([0x08, 5]);
    b.extend(b"items");
    b.extend([0x0B, 0, 0, 0, 0]); // 1: the sheet
    b.extend([0x03, 7, 0x0B, 0, 0, 0, 0]); // row 7 (2)
    b.extend([0x08, 7]);
    b.extend(b"__index");
    b.extend([0x0E, 2]); // itself
    b.extend([0x08, 1, b'a', 0x03, 1, 0x08, 1, b'b', 0x03, 2, 0x00]); // row 7 ends
    b.extend([0x03, 8, 0x0B, 0, 0, 0, 0]); // row 8 (3)
    b.extend([0x08, 7]);
    b.extend(b"__index");
    b.extend([0x0E, 3]); // itself
    b.extend([0x08, 1, b'a', 0x03, 5]);
    b.extend([0x08, 7]);
    b.extend(b"effects");
    b.extend([0x0B, 1, 0, 0, 0, 0x03, 9, 0x00]); // 4: [9]
    b.push(0x00); // row 8 ends
    b.extend([0x00, 0x00]); // the sheet, then the root, end
    b
}

/// A client whose _index names the table cache, held as a stored blob in a third part.
fn cache_repo() -> tempfile::TempDir {
    let cache = cache_bytes();
    let key = sha1(&cache);
    let repo = repo_with_index(&format!(
        "{}\tdata/caches/all_tables.new_cache\n",
        hex(&key)
    ));
    let part = zfs_part(&[vec![(1, key, stored_blob(&cache))]]);
    std::fs::write(repo.path().join("client/data/data002"), part).unwrap();
    repo
}

#[test]
fn dump_writes_a_sheet_as_json_lines() {
    let repo = cache_repo();
    let out = archive(repo.path())
        .args(["dump", "--table", "items", "--out", "re/d"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("2 rows of items"),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join("re/d/items.jsonl")).unwrap(),
        "{\"$key\":7,\"a\":1,\"b\":2}\n{\"$key\":8,\"a\":5,\"effects\":[9]}\n"
    );
}

#[test]
fn dump_refuses_a_cache_whose_sha1_isnt_its_key() {
    // The record's key names other content: a damaged archive or a bad spool output
    // (m2-tables.md 1.6: a file's key is the SHA-1 of its decoded content).
    let key = [5; 20];
    let repo = repo_with_index(&format!(
        "{}\tdata/caches/all_tables.new_cache\n",
        hex(&key)
    ));
    let part = zfs_part(&[vec![(1, key, stored_blob(&cache_bytes()))]]);
    std::fs::write(repo.path().join("client/data/data002"), part).unwrap();
    let out = archive(repo.path())
        .args(["dump", "--table", "items", "--out", "re/d"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("isn't its key"), "{err}");
    assert!(!repo.path().join("re/d/items.jsonl").exists());
}

#[cfg(unix)]
#[test]
fn dump_replaces_a_link_instead_of_writing_through_it() {
    let repo = cache_repo();
    std::fs::create_dir(repo.path().join("docs")).unwrap();
    std::fs::write(repo.path().join("docs/tracked.md"), "keep").unwrap();
    std::fs::create_dir_all(repo.path().join("re/d")).unwrap();
    let file = repo.path().join("re/d/items.jsonl");
    std::os::unix::fs::symlink(repo.path().join("docs/tracked.md"), &file).unwrap();
    let ok = archive(repo.path())
        .args(["dump", "--table", "items", "--out", "re/d"])
        .status()
        .unwrap();
    assert!(ok.success());
    assert_eq!(
        std::fs::read_to_string(repo.path().join("docs/tracked.md")).unwrap(),
        "keep"
    );
    assert!(
        !std::fs::symlink_metadata(&file)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[cfg(unix)]
#[test]
fn extract_key_replaces_a_link_instead_of_writing_through_it() {
    let repo = cache_repo();
    std::fs::create_dir(repo.path().join("docs")).unwrap();
    std::fs::write(repo.path().join("docs/tracked.md"), "keep").unwrap();
    std::fs::create_dir_all(repo.path().join("re/k")).unwrap();
    let key = hex(&sha1(&cache_bytes()));
    let file = repo.path().join("re/k").join(&key);
    std::os::unix::fs::symlink(repo.path().join("docs/tracked.md"), &file).unwrap();
    let ok = archive(repo.path())
        .args(["extract", "--key", &key, "--out", "re/k"])
        .status()
        .unwrap();
    assert!(ok.success());
    assert_eq!(
        std::fs::read_to_string(repo.path().join("docs/tracked.md")).unwrap(),
        "keep"
    );
    assert_eq!(std::fs::read(&file).unwrap(), cache_bytes());
}

#[test]
fn dump_waits_for_the_caches_own_blocks() {
    // _index is decoded, but the cache it names is an Oodle blob nobody has decoded yet.
    let key = [6; 20];
    let repo = repo_with_index(&format!(
        "{}\tdata/caches/all_tables.new_cache\n",
        hex(&key)
    ));
    let part = zfs_part(&[vec![(1, key, oodle_blob())]]);
    std::fs::write(repo.path().join("client/data/data002"), part).unwrap();
    let out = archive(repo.path())
        .args(["dump", "--table", "items", "--out", "re/d"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("1 blocks spooled"));
    assert!(!repo.path().join("re/d/items.jsonl").exists());
}

#[test]
fn extract_key_writes_one_stored_file() {
    let repo = cache_repo();
    let key = hex(&sha1(&cache_bytes()));
    let out = archive(repo.path())
        .args(["extract", "--key", &key, "--out", "re/k"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read(repo.path().join("re/k").join(&key)).unwrap(),
        cache_bytes()
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("its SHA-1 matches the key"), "{text}");
}

#[test]
fn extract_key_decodes_an_oodle_file_through_the_spool() {
    use fenji_data::oodle_spool::OodleSpool;
    // data001's Oodle record: its one block, decoded in the spool as the helper would.
    let repo = setup();
    let blob = oodle_blob();
    let block = &blob[fenji_data::FIXED_LEN + 4..];
    let out_dir = repo.path().join("re/oodle-spool/out");
    std::fs::create_dir_all(&out_dir).unwrap();
    std::fs::write(out_dir.join(OodleSpool::name(block, 12)), b"abcabcabcabc").unwrap();
    let key = hex(&[9; 20]);
    let out = archive(repo.path())
        .args(["extract", "--key", &key, "--out", "re/k"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read(repo.path().join("re/k").join(&key)).unwrap(),
        b"abcabcabcabc"
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("doesn't match the key"), "{text}");
}

#[test]
fn extract_key_waits_for_the_helper() {
    let repo = setup();
    let out = archive(repo.path())
        .args(["extract", "--key", &hex(&[2; 20]), "--out", "re/k"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("1 blocks spooled"));
    assert!(!repo.path().join("re/k").join(hex(&[2; 20])).exists());
}

#[test]
fn extract_key_refuses_a_bad_key_an_unknown_one_and_outside_re() {
    let repo = setup();
    let run = |args: &[&str]| archive(repo.path()).args(args).output().unwrap();
    let bad = run(&["extract", "--key", "zz", "--out", "re/k"]);
    assert!(!bad.status.success());
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("40 hex digits"),
        "{}",
        String::from_utf8_lossy(&bad.stderr)
    );
    let unknown_key = hex(&[7; 20]);
    let unknown = run(&["extract", "--key", &unknown_key, "--out", "re/k"]);
    assert!(!unknown.status.success());
    let err = String::from_utf8_lossy(&unknown.stderr);
    assert!(
        err.contains(&unknown_key) && err.contains("no live record"),
        "{err}"
    );
    let outside = run(&["extract", "--key", &hex(&sha1(VIDEO)), "--out", "/tmp/x"]);
    assert!(!outside.status.success());
    assert!(String::from_utf8_lossy(&outside.stderr).contains("is not inside"));
    let both = run(&[
        "extract",
        "--key",
        &hex(&sha1(VIDEO)),
        "--part",
        "0",
        "--out",
        "re/k",
    ]);
    assert!(!both.status.success(), "--key and --part together");
}

#[test]
fn dump_refuses_outside_re_and_names_an_unknown_sheet() {
    let repo = cache_repo();
    let outside = archive(repo.path())
        .args(["dump", "--table", "items", "--out", "/tmp/x"])
        .output()
        .unwrap();
    assert!(!outside.status.success());
    assert!(String::from_utf8_lossy(&outside.stderr).contains("is not inside"));
    let unknown = archive(repo.path())
        .args(["dump", "--table", "nosuch", "--out", "re/d"])
        .output()
        .unwrap();
    assert!(!unknown.status.success());
    assert!(
        String::from_utf8_lossy(&unknown.stderr).contains("nosuch"),
        "{}",
        String::from_utf8_lossy(&unknown.stderr)
    );
}

#[test]
fn dump_waits_for_the_helper() {
    // setup()'s _index is an Oodle blob nobody has decoded yet.
    let repo = setup();
    let out = archive(repo.path())
        .args(["dump", "--table", "items", "--out", "re/d"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("1 blocks spooled"));
}

#[test]
#[ignore = "needs the pinned client and a decoded spool: FENJI_CLIENT_PATH=~/fenji-client cargo test -p fenji-archive --test cli -- --ignored"]
fn dump_items_from_the_client() {
    // Run from the repository root, whose re/oodle-spool the helper has filled (M2 plan Task 13).
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let client = std::env::var_os("FENJI_CLIENT_PATH").expect("set FENJI_CLIENT_PATH");
    let out = Command::new(env!("CARGO_BIN_EXE_archive"))
        .current_dir(&root)
        .arg("--client")
        .arg(client)
        .args(["dump", "--table", "items", "--out", "re/m2/dump-test"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lines = std::fs::read_to_string(root.join("re/m2/dump-test/items.jsonl"))
        .unwrap()
        .lines()
        .count();
    assert_eq!(lines, 47_112, "docs/re/m2-tables.md 3.1");
}

/// One Oodle block as the archives hold it (m1-archives.md 3.4): its `u32` raw length, then the
/// stream's pieces.
fn oodle_block(raw_len: u32, stream: &[&[u8]]) -> Vec<u8> {
    [&raw_len.to_le_bytes()[..], &stream.concat()].concat()
}

/// An Oodle blob (codec 4) of `blocks`, `block_size` raw bytes each but the last.
fn oodle_stream_blob(blocks: &[Vec<u8>], raw_size: u64, block_size: u32) -> Vec<u8> {
    let mut b = lz4_blob_blocks(blocks, raw_size, block_size);
    b[28] = 4;
    b
}

/// A synthetic client for `census-oodle`, one part of five live Oodle files: Kraken uncompressed in
/// two blocks; a Kraken memset quantum, then a stored one with checksums; Leviathan compressed with
/// checksums; a block whose `u32` raw length disagrees with its blob header; and a stream cut
/// short. A deleted Oodle record and a stored file are left out of the census.
fn census_repo() -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir(repo.path().join(".git")).unwrap();
    std::fs::create_dir(repo.path().join("re")).unwrap();
    let data = repo.path().join("client/data");
    std::fs::create_dir_all(&data).unwrap();
    // Stream headers: 0x8C, plus 0x40 when uncompressed; then the decoder, plus 0x80 with
    // checksums. A quantum header for n bytes of data holds n - 1 in 18 bits; a checksum follows
    // it in 3 bytes when the stream header says so. A memset quantum is 0x07FFFF, then its byte.
    let uncompressed = oodle_stream_blob(
        &[
            oodle_block(64, &[&[0xCC, 6], &[7; 64]]),
            oodle_block(36, &[&[0xCC, 6], &[7; 36]]),
        ],
        100,
        64,
    );
    let quantum = 0x4_0000;
    let memset_then_stored = oodle_stream_blob(
        &[oodle_block(
            quantum + 10,
            &[
                &[0x8C, 6],
                &[0x07, 0xFF, 0xFF, 0x5A],
                &[0x8C, 0x86],
                &[0, 0, 9],
                &[1, 2, 3],
                &[0x11; 10],
            ],
        )],
        u64::from(quantum) + 10,
        0x8_0000,
    );
    let leviathan = || {
        oodle_stream_blob(
            &[oodle_block(
                10,
                &[&[0x8C, 0x8C], &[0, 0, 4], &[1, 2, 3], &[0x22; 5]],
            )],
            10,
            10,
        )
    };
    let bad_prefix = oodle_stream_blob(&[oodle_block(99, &[&[0xCC, 6], &[7; 100]])], 100, 100);
    let cut_short = oodle_stream_blob(
        &[oodle_block(10, &[&[0x8C, 6], &[0, 0, 4], &[0x33; 2]])],
        10,
        10,
    );
    let part = zfs_part(&[vec![
        (1, [0xA; 20], uncompressed),
        (1, [0xB; 20], memset_then_stored),
        (1, [0xC; 20], leviathan()),
        (0xff, [0xD; 20], leviathan()),
        (1, [0xE; 20], bad_prefix),
        (1, [0xF; 20], cut_short),
        (1, sha1(VIDEO), stored_blob(VIDEO)),
    ]]);
    std::fs::write(data.join("data000"), part).unwrap();
    repo
}

const CENSUS: &str = "decoder\tmode\tchecksums\tchunks\n\
                      6\tmemset\tfalse\t1\n\
                      6\tstored\ttrue\t1\n\
                      6\tuncompressed\tfalse\t2\n\
                      12\tcompressed\ttrue\t1\n\
                      # files\t5\n\
                      # blocks\t6\n\
                      # bad_prefix\t1\n\
                      # walk_error\tTruncated\t1\n";

#[test]
fn census_oodle_counts_chunks_by_decoder_mode_and_checksums() {
    let repo = census_repo();
    let out = archive(repo.path()).arg("census-oodle").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap(), CENSUS);
    assert_eq!(
        std::fs::read_to_string(repo.path().join("re/oodle-census/summary.tsv")).unwrap(),
        CENSUS
    );
}

#[test]
fn census_oodle_refuses_outside_re() {
    let repo = census_repo();
    let out = archive(repo.path())
        .args(["census-oodle", "--out", "elsewhere"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("elsewhere is not inside"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!repo.path().join("elsewhere").exists());
}

#[cfg(unix)]
#[test]
fn census_oodle_replaces_a_link_instead_of_writing_through_it() {
    let repo = census_repo();
    std::fs::create_dir(repo.path().join("docs")).unwrap();
    std::fs::write(repo.path().join("docs/tracked.md"), "keep").unwrap();
    std::fs::create_dir_all(repo.path().join("re/c")).unwrap();
    let tsv = repo.path().join("re/c/summary.tsv");
    std::os::unix::fs::symlink(repo.path().join("docs/tracked.md"), &tsv).unwrap();
    let ok = archive(repo.path())
        .args(["census-oodle", "--out", "re/c"])
        .status()
        .unwrap();
    assert!(ok.success());
    assert_eq!(
        std::fs::read_to_string(repo.path().join("docs/tracked.md")).unwrap(),
        "keep"
    );
    assert!(
        !std::fs::symlink_metadata(&tsv)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read_to_string(&tsv).unwrap(), CENSUS);
}
