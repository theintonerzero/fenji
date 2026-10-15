//! archive end to end, on a synthetic client built from our own bytes (no game bytes).
use std::path::Path;
use std::process::Command;

use fenji_data::test_util::{lz4_blob_blocks, stored_blob, zfs_part};
use sha1::{Digest, Sha1};

const VIDEO: &[u8] = b"KB2j fake video bytes";
const CONTAINER: &[u8] = b"[SP] fake container";

/// An Oodle blob (codec 4) of one block, which decodes to "abcabcabcabc".
fn oodle_blob() -> Vec<u8> {
    fenji_data::test_util::oodle_blob(b"abcabcabcabc", 12)
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
        .args(["paths", "--spool", "re/oodle-spool", "--out", "re/p"])
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

/// A repo whose _index names three paths (see `paths_maps_index_paths_to_records`).
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

/// A synthetic client whose _index holds `text`, in one Oodle block. For runs with --spool, its
/// decoded output is also placed in the spool, as the helper would.
fn repo_with_index(text: &str) -> tempfile::TempDir {
    let repo = setup();
    let index = fenji_data::test_util::oodle_blob(text.as_bytes(), 512 * 1024);
    std::fs::write(repo.path().join("client/data/_index"), &index).unwrap();
    spool_index_output(repo.path(), text);
    repo
}

/// Puts `text` in the spool as the helper's output for _index's one block.
fn spool_index_output(repo: &Path, text: &str) {
    use fenji_data::oodle_spool::OodleSpool;
    let index = std::fs::read(repo.join("client/data/_index")).unwrap();
    let block = &index[fenji_data::FIXED_LEN + 4..];
    let raw_len = u64::from_le_bytes(index[..8].try_into().unwrap()) as usize;
    let spool = repo.join("re/oodle-spool");
    std::fs::create_dir_all(spool.join("out")).unwrap();
    std::fs::write(
        spool.join("out").join(OodleSpool::name(block, raw_len)),
        text,
    )
    .unwrap();
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

#[test]
fn paths_decodes_index_natively_and_never_spools() {
    // setup()'s _index is an Oodle blob of "abcabcabcabc": one line that isn't a path.
    let repo = setup();
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
        summary.contains("0 paths") && summary.contains("1 bad lines"),
        "{summary}"
    );
    assert!(!repo.path().join("re/oodle-spool").exists());
}

#[test]
fn paths_takes_the_helpers_output_with_spool() {
    // The spool's output for _index differs from what it decodes to: a path ends in z, not y.
    let repo = decoded_index_repo();
    let text = format!(
        "{}\tmovie/intro.bk2\n{}\tdata/x.tab\n{}\tgone/z.tab\n",
        hex(&sha1(VIDEO)),
        hex(&[9; 20]),
        hex(&[7; 20])
    );
    spool_index_output(repo.path(), &text);
    let out = archive(repo.path())
        .args(["paths", "--spool", "re/oodle-spool", "--out", "re/p"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let tsv = std::fs::read_to_string(repo.path().join("re/p/paths.tsv")).unwrap();
    assert!(
        tsv.contains("gone/z.tab") && !tsv.contains("gone/y.tab"),
        "{tsv}"
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
    // The record's key names other content: a damaged archive or a bad decode
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
        .args([
            "dump",
            "--table",
            "items",
            "--spool",
            "re/oodle-spool",
            "--out",
            "re/d",
        ])
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
fn dump_decodes_an_oodle_cache_natively() {
    // The cache in two Oodle blocks, and _index in one: no spool.
    let cache = cache_bytes();
    let repo = repo_with_index(&format!(
        "{}\tdata/caches/all_tables.new_cache\n",
        hex(&sha1(&cache))
    ));
    std::fs::remove_dir_all(repo.path().join("re/oodle-spool")).unwrap();
    let blob = fenji_data::test_util::oodle_blob(&cache, 64);
    let part = zfs_part(&[vec![(1, sha1(&cache), blob)]]);
    std::fs::write(repo.path().join("client/data/data002"), part).unwrap();
    let out = archive(repo.path())
        .args(["dump", "--table", "items", "--out", "re/d"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join("re/d/items.jsonl")).unwrap(),
        "{\"$key\":7,\"a\":1,\"b\":2}\n{\"$key\":8,\"a\":5,\"effects\":[9]}\n"
    );
    assert!(!repo.path().join("re/oodle-spool").exists());
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
    // data001's Oodle record: its one block, decoded in the spool as the helper would. The
    // helper's output stands out from the block's own bytes, so it shows which route ran.
    let repo = setup();
    let blob = oodle_blob();
    let block = &blob[fenji_data::FIXED_LEN + 4..];
    let out_dir = repo.path().join("re/oodle-spool/out");
    std::fs::create_dir_all(&out_dir).unwrap();
    std::fs::write(out_dir.join(OodleSpool::name(block, 12)), b"ABCABCABCABC").unwrap();
    let key = hex(&[9; 20]);
    let out = archive(repo.path())
        .args([
            "extract",
            "--key",
            &key,
            "--spool",
            "re/oodle-spool",
            "--out",
            "re/k",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read(repo.path().join("re/k").join(&key)).unwrap(),
        b"ABCABCABCABC"
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("doesn't match the key"), "{text}");
}

#[test]
fn extract_key_decodes_an_oodle_file_natively() {
    // A file of 700,000 bytes in two Oodle blocks, under the SHA-1 of its content; no spool.
    let repo = setup();
    let content: Vec<u8> = (0..700_000u32).map(|i| (i % 241) as u8).collect();
    let blob = fenji_data::test_util::oodle_blob(&content, 512 * 1024);
    let part = zfs_part(&[vec![(1, sha1(&content), blob)]]);
    std::fs::write(repo.path().join("client/data/data002"), part).unwrap();
    let key = hex(&sha1(&content));
    let out = archive(repo.path())
        .args(["extract", "--key", &key, "--out", "re/k"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(std::fs::read(repo.path().join("re/k").join(&key)).unwrap() == content);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("its SHA-1 matches the key"), "{text}");
    assert!(!repo.path().join("re/oodle-spool").exists());
}

#[test]
fn a_block_that_doesnt_decode_is_an_error_not_a_wait() {
    // data000's Oodle file, its stream cut short: without --spool, an error (exit 1), never the
    // spool's exit 3.
    let repo = setup();
    let mut blob = oodle_blob();
    let last = blob.len() - 1;
    blob.truncate(last);
    let stored = u64::from_le_bytes(blob[16..24].try_into().unwrap()) - 1;
    blob[16..24].copy_from_slice(&stored.to_le_bytes());
    let block = blob.len() as u32 - 36;
    blob[32..36].copy_from_slice(&block.to_le_bytes());
    let part = zfs_part(&[vec![(1, [0x2B; 20], blob)]]);
    std::fs::write(repo.path().join("client/data/data002"), part).unwrap();
    let out = archive(repo.path())
        .args(["extract", "--key", &hex(&[0x2B; 20]), "--out", "re/k"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("Oodle block 0"), "{err}");
    assert!(!repo.path().join("re/oodle-spool").exists());
}

#[test]
fn extract_key_waits_for_the_helper() {
    let repo = setup();
    let out = archive(repo.path())
        .args([
            "extract",
            "--key",
            &hex(&[2; 20]),
            "--spool",
            "re/oodle-spool",
            "--out",
            "re/k",
        ])
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
    // setup()'s _index is an Oodle blob that isn't in the spool yet.
    let repo = setup();
    let out = archive(repo.path())
        .args([
            "dump",
            "--table",
            "items",
            "--spool",
            "re/oodle-spool",
            "--out",
            "re/d",
        ])
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
#[ignore = "needs the pinned client: FENJI_CLIENT_PATH=~/fenji-client cargo test -p fenji-archive --test cli -- --ignored"]
fn dump_items_from_the_client() {
    // Run from the repository root, which has re/; the cache and _index decode natively.
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

/// An Oodle stream that departs from ooz where Ruling O14 counts (fenji-oodle's Kraken tests): an
/// LZ chunk whose new offset coding makes a match 1 byte back. Its raw bytes too.
fn near_match_stream() -> (Vec<u8>, Vec<u8>) {
    let mut body = b"ABCDEFGH".to_vec();
    body.extend([0x00, 0x00, 0x09, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    body.extend([
        0x80, 0x01, 0xFF, 0x80, 0x80, 0x01, 1, 0x80, 0x02, 4, 20, 0x80,
    ]);
    let mut s = vec![0x8C, 0x06, 0x00, 0x00, body.len() as u8 + 2, 0x88, 0x00];
    s.push(body.len() as u8);
    s.extend(body);
    let mut raw = b"ABCDEFGH\x01\x02\x03\x04\x05\x06\x07".to_vec();
    raw.extend([7; 37]);
    raw.extend([8, 9]);
    (s, raw)
}

/// An Oodle stream that departs from ooz where Ruling O14 counts: 128 KiB of 'X' in an
/// entropy-only chunk of RLE, then 100 bytes in an entropy-only chunk of one stored block. Its raw
/// bytes too.
fn stored_entropy_stream() -> (Vec<u8>, Vec<u8>) {
    let mut s = vec![0x8C, 0x06, 0x00, 0x00, 0x70];
    s.extend([0x37, 0xFF, 0xFC, 0x00, 0x05, 0x00, b'X', 0xFF, 0x0C, 0x01]);
    s.extend([0x00, 0x00, 100]);
    s.extend(0..100u8);
    let mut raw = vec![b'X'; 0x2_0000];
    raw.extend(0..100u8);
    (s, raw)
}

/// One block's name in the hash list, its index, and the bytes it decodes to.
type Expected = (String, usize, Vec<u8>);

/// A synthetic client for `verify-oodle`: `_index` in two memset blocks; and in data000, a memset
/// file, a container (a memset file in three blocks, and a stored one), and two files whose one
/// block each departs from ooz. Every Oodle block it holds, in the walk's order.
fn verify_repo() -> (tempfile::TempDir, Vec<Expected>) {
    use fenji_data::test_util::{container, oodle_memset_blob};
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir(repo.path().join(".git")).unwrap();
    std::fs::create_dir(repo.path().join("re")).unwrap();
    let data = repo.path().join("client/data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(
        data.join("_index"),
        oodle_memset_blob(700_000, 512 * 1024, 1),
    )
    .unwrap();
    let inner = container(&[
        ([0xD1; 20], "asset/a.dds", oodle_memset_blob(10, 4, 3)),
        ([0xD2; 20], "asset/b.txt", stored_blob(b"plain")),
    ]);
    let one_block = |(stream, raw): &(Vec<u8>, Vec<u8>)| {
        let block = oodle_block(raw.len() as u32, &[stream]);
        oodle_stream_blob(&[block], raw.len() as u64, 512 * 1024)
    };
    let (near, stored) = (near_match_stream(), stored_entropy_stream());
    let part = zfs_part(&[vec![
        (1, [0xA; 20], oodle_memset_blob(1000, 512 * 1024, 2)),
        (1, [0xC; 20], stored_blob(&inner)),
        (1, [0x5A; 20], one_block(&near)),
        (1, [0xE; 20], one_block(&stored)),
    ]]);
    std::fs::write(data.join("data000"), part).unwrap();
    let inner_file = format!("{}/0/{}", hex(&[0xC; 20]), hex(&[0xD1; 20]));
    let blocks = vec![
        ("_index".to_string(), 0, vec![1; 524_288]),
        ("_index".to_string(), 1, vec![1; 700_000 - 524_288]),
        (inner_file.clone(), 0, vec![3; 4]),
        (inner_file.clone(), 1, vec![3; 4]),
        (inner_file, 2, vec![3; 2]),
        (hex(&[0xA; 20]), 0, vec![2; 1000]),
        (hex(&[0x5A; 20]), 0, near.1),
        (hex(&[0xE; 20]), 0, stored.1),
    ];
    (repo, blocks)
}

/// `oodle-helper hash-parts`' TSV for `blocks`: `key\tblock\traw_len\tsha256` lines.
fn dll_hashes(blocks: &[Expected]) -> String {
    blocks
        .iter()
        .map(|(key, index, raw)| {
            let sha = fenji_data::sha256_hex(raw);
            format!("{key}\t{index}\t{}\t{sha}\n", raw.len())
        })
        .collect()
}

/// `verify-oodle` against `tsv` as the DLL's hashes, with its output in re/oodle-verify.
fn verify(repo: &Path, tsv: &str) -> std::process::Output {
    std::fs::create_dir_all(repo.join("re/v")).unwrap();
    std::fs::write(repo.join("re/v/dll.tsv"), tsv).unwrap();
    archive(repo)
        .args(["verify-oodle", "--dll-hashes", "re/v/dll.tsv"])
        .output()
        .unwrap()
}

fn verified(repo: &Path, file: &str) -> String {
    std::fs::read_to_string(repo.join("re/oodle-verify").join(file)).unwrap()
}

const MISMATCHES: &str = "key\tblock\tproblem\n";

#[test]
fn verify_oodle_matches_every_block_with_the_dlls_hash() {
    let (repo, blocks) = verify_repo();
    let out = verify(repo.path(), &dll_hashes(&blocks));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        out.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.starts_with(
            "8 blocks match, 0 differ, 0 failed natively, 0 failed in the DLL, \
             0 have no DLL hash, and 0 DLL hashes match no block; 0.8 MB decoded in "
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "walked 1 files and 2 blocks in _index, 1 files and 3 blocks in the containers, \
             3 files and 3 blocks in the parts\n"
        ),
        "{text}"
    );
    // Rulings O14 and O15: each departure from ooz, counted, and listed with what the DLL made of
    // it.
    assert!(
        text.contains(
            "(Rulings O13 to O15): 1 chunks in 1 blocks match 1 to 7 bytes back, and \
             1 chunks in 1 blocks are one stored block in an entropy-only chunk; \
             2 of those 2 blocks match the DLL"
        ),
        "{text}"
    );
    assert_eq!(verified(repo.path(), "mismatches.tsv"), MISMATCHES);
    assert_eq!(
        verified(repo.path(), "departures.tsv"),
        format!(
            "key\tblock\tnear_match_chunks\tstored_entropy_chunks\tdll\n\
             {}\t0\t1\t0\tsame\n{}\t0\t0\t1\tsame\n",
            hex(&[0x5A; 20]),
            hex(&[0xE; 20])
        )
    );
    assert_eq!(verified(repo.path(), "summary.txt"), text);
}

#[test]
fn verify_oodle_names_a_block_that_differs() {
    let (repo, blocks) = verify_repo();
    let tsv = dll_hashes(&blocks).replace(&fenji_data::sha256_hex(&[2; 1000]), &"0".repeat(64));
    let out = verify(repo.path(), &tsv);
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with("7 blocks match, 1 differ, "), "{text}");
    assert_eq!(
        verified(repo.path(), "mismatches.tsv"),
        format!("{MISMATCHES}{}\t0\tbytes differ\n", hex(&[0xA; 20]))
    );
}

#[test]
fn verify_oodle_fails_a_block_with_no_dll_hash() {
    let (repo, mut blocks) = verify_repo();
    let (name, ..) = blocks.remove(3);
    let out = verify(repo.path(), &dll_hashes(&blocks));
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.starts_with(
            "7 blocks match, 0 differ, 0 failed natively, 0 failed in the DLL, 1 have no DLL hash, "
        ),
        "{text}"
    );
    assert_eq!(
        verified(repo.path(), "mismatches.tsv"),
        format!("{MISMATCHES}{name}\t1\tno DLL hash\n")
    );
}

#[test]
fn verify_oodle_counts_the_dlls_failures_and_hashes_of_no_block() {
    let (repo, blocks) = verify_repo();
    let last = fenji_data::sha256_hex(&blocks[1].2);
    let mut tsv = dll_hashes(&blocks).replace(&last, "FAILED");
    tsv.push_str(&format!("{}\t0\t10\t{}\n", hex(&[0xF; 20]), "0".repeat(64)));
    let out = verify(repo.path(), &tsv);
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.starts_with(
            "7 blocks match, 0 differ, 0 failed natively, 1 failed in the DLL, \
             0 have no DLL hash, and 1 DLL hashes match no block;"
        ),
        "{text}"
    );
    assert_eq!(
        verified(repo.path(), "mismatches.tsv"),
        format!(
            "{MISMATCHES}_index\t1\tthe DLL failed\n{}\t0\tno such block here\n",
            hex(&[0xF; 20])
        )
    );
}

#[test]
fn verify_oodle_reports_a_block_that_fails_natively() {
    // data001: a stream cut short, which the DLL is said to have decoded.
    let (repo, mut blocks) = verify_repo();
    let cut = oodle_stream_blob(
        &[oodle_block(10, &[&[0x8C, 6], &[0, 0, 4], &[0x33; 2]])],
        10,
        10,
    );
    let part = zfs_part(&[vec![(1, [0xB; 20], cut)]]);
    std::fs::write(repo.path().join("client/data/data001"), part).unwrap();
    blocks.push((hex(&[0xB; 20]), 0, vec![0x33; 10]));
    let out = verify(repo.path(), &dll_hashes(&blocks));
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.starts_with("8 blocks match, 0 differ, 1 failed natively, "),
        "{text}"
    );
    let mismatches = verified(repo.path(), "mismatches.tsv");
    let want = format!(
        "{MISMATCHES}{}\t0\tnative: Oodle block 0: ",
        hex(&[0xB; 20])
    );
    assert!(mismatches.starts_with(&want), "{mismatches}");
}

#[test]
fn verify_oodle_names_a_blob_it_cant_read() {
    // data001: an Oodle record whose size runs past its part. The walk stops before any decode,
    // and names it.
    let (repo, blocks) = verify_repo();
    let blob = fenji_data::test_util::oodle_memset_blob(10, 10, 1);
    let mut part = zfs_part(&[vec![(1, [0xB; 20], blob)]]);
    let r = 12; // the first record, after "ZFS\0", "[IX]" and the next block's offset
    let size = u32::from_le_bytes(part[r + 24..r + 28].try_into().unwrap()) + 1;
    part[r + 24..r + 28].copy_from_slice(&size.to_le_bytes());
    let crc = fenji_data::archive::crc16_arc(&part[r..r + 28]);
    part[r + 28..r + 30].copy_from_slice(&crc.to_le_bytes());
    std::fs::write(repo.path().join("client/data/data001"), part).unwrap();
    let out = verify(repo.path(), &dll_hashes(&blocks));
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains(&hex(&[0xB; 20])), "{err}");
    assert!(out.stdout.is_empty());
}

#[test]
fn verify_oodle_refuses_outside_re_and_a_hash_list_it_cant_read() {
    let (repo, blocks) = verify_repo();
    std::fs::create_dir_all(repo.path().join("re/v")).unwrap();
    std::fs::write(repo.path().join("re/v/dll.tsv"), dll_hashes(&blocks)).unwrap();
    let outside = archive(repo.path())
        .args([
            "verify-oodle",
            "--dll-hashes",
            "re/v/dll.tsv",
            "--out",
            "elsewhere",
        ])
        .output()
        .unwrap();
    assert!(!outside.status.success());
    let err = String::from_utf8_lossy(&outside.stderr);
    assert!(err.contains("elsewhere is not inside"), "{err}");
    assert!(!repo.path().join("elsewhere").exists());
    let out = verify(repo.path(), "_index\tzero\t1\tFAILED\n");
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("dll.tsv, line 1"), "{err}");
}
