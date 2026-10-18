//! The early check's loader (retranslation plan, Task 10), on an invented copy: no game bytes.
use std::collections::HashMap;
use std::path::Path;

use fenji_data::archive::parse_part_header;
use fenji_data::cache::{self, Cache, Table, TableId, Value};
use fenji_data::test_util::{stored_blob, zfs_part};
use fenji_data::{BlobHeader, BuiltinCodecs, FIXED_LEN, decode_blob, header_check, index};
use fenji_pack::fingerprint;
use fenji_translate::early::{PATCH_PART, apply, revert, run_copy};
use fenji_translate::extract::LOCALE_CACHE;
use sha1::{Digest, Sha1};

fn key(bytes: &[u8]) -> [u8; 20] {
    Sha1::digest(bytes).into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn s(t: &str) -> Value<'_> {
    Value::Str(t.as_bytes())
}

/// An invented locale cache: its main table holds two lines.
fn locale() -> Vec<u8> {
    cache::write(&Cache {
        tables: vec![
            Table {
                array: vec![],
                hash: vec![(s(""), Value::Table(TableId(1)))],
            },
            Table {
                array: vec![],
                hash: vec![(s("你好"), s("Hello")), (s("再见"), s("Bye"))],
            },
        ],
        values: vec![Value::Table(TableId(0))],
    })
}

/// A copy at `root`: one part holding the locale cache and one other file, and an `_index`
/// that names both.
fn make_copy(root: &Path) {
    let data = root.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let files = [
        (LOCALE_CACHE, locale()),
        ("data/tables/a.tab", b"a\tb\n".to_vec()),
    ];
    let records = files
        .iter()
        .map(|(_, b)| (1, key(b), stored_blob(b)))
        .collect();
    std::fs::write(data.join("data000"), zfs_part(&[records])).unwrap();
    let index: String = files
        .iter()
        .map(|(path, b)| format!("{}\t{path}\n", hex(&key(b))))
        .collect();
    std::fs::write(data.join("_index"), stored_blob(index.as_bytes())).unwrap();
}

/// The English of `source` in a locale cache's main table.
fn english_of(bytes: &[u8], source: &str) -> String {
    let c = cache::read(bytes).unwrap();
    let Value::Table(root) = &c.values[0] else {
        panic!("no root")
    };
    let Some((_, Value::Table(main))) = c.tables[root.0].hash.first() else {
        panic!("no main table")
    };
    c.tables[main.0]
        .hash
        .iter()
        .find_map(|(k, v)| match (k, v) {
            (Value::Str(k), Value::Str(v)) if *k == source.as_bytes() => {
                Some(String::from_utf8(v.to_vec()).unwrap())
            }
            _ => None,
        })
        .unwrap()
}

/// A blob's header passes the engine's check (docs/re/retranslation-locale.md 3.9).
fn checked(blob: &[u8]) -> bool {
    let fixed: [u8; FIXED_LEN] = blob[..FIXED_LEN].try_into().unwrap();
    BlobHeader::parse(blob, 0).unwrap().check == header_check(&fixed)
}

#[test]
fn apply_puts_the_new_cache_where_the_engine_looks_and_revert_restores() {
    let dir = tempfile::tempdir().unwrap();
    let (copy, work) = (dir.path().join("client"), dir.path().join("work"));
    make_copy(&copy);
    let read = |path: &str| std::fs::read(copy.join(path)).unwrap();
    let (index_before, part_before) = (read("data/_index"), read("data/data000"));
    let english = HashMap::from([(fingerprint("你好".as_bytes()), "FENJI CHECK".to_string())]);
    let applied = apply(&copy, &work, &english, &[]).unwrap();
    assert_eq!(applied.replaced, 1);

    // As the engine reads it (docs/re/retranslation-locale.md 3.4–3.9): the index's line for the
    // cache names a key the patch part holds, and the blob there is the new cache.
    let index_blob = read("data/_index");
    assert!(checked(&index_blob), "an index header the engine takes");
    let map = index::build(&decode_blob(&index_blob, 0, &BuiltinCodecs).unwrap());
    let new_key = map.by_path[LOCALE_CACHE];
    assert_ne!(new_key, key(&locale()));
    assert_eq!(map.by_path.len(), 2, "every other path as it was");
    let part = read(PATCH_PART);
    let h = parse_part_header(&part).unwrap();
    let record = h
        .live()
        .find(|r| r.key == new_key)
        .expect("the patch part holds the key");
    let blob = &part[record.offset as usize..][..record.size as usize];
    assert!(checked(blob), "a record header the engine takes");
    let new_cache = decode_blob(blob, 0, &BuiltinCodecs).unwrap();
    assert_eq!(key(&new_cache), new_key, "keyed by its own SHA-1");
    assert_eq!(english_of(&new_cache, "你好"), "FENJI CHECK");
    assert_eq!(english_of(&new_cache, "再见"), "Bye");
    assert_eq!(
        read("data/data000"),
        part_before,
        "the copy's own parts untouched"
    );

    assert!(
        apply(&copy, &work, &english, &[]).is_err(),
        "once, until it's reverted"
    );
    assert!(revert(&copy, &work).unwrap());
    assert_eq!(read("data/_index"), index_before);
    assert!(!copy.join(PATCH_PART).exists());
    assert!(!revert(&copy, &work).unwrap(), "nothing left to revert");
}

#[test]
fn only_a_run_workspaces_copy_is_touched() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(repo.join("re/runs/client")).unwrap();
    std::fs::create_dir_all(repo.join("pinned")).unwrap();
    assert_eq!(
        run_copy(&repo, &repo.join("re/runs/client"), None).unwrap(),
        repo.join("re/runs/client")
    );
    assert!(
        run_copy(&repo, &repo.join("pinned"), None).is_err(),
        "outside re/"
    );
    let pinned = repo.join("re/pinned");
    std::fs::create_dir_all(&pinned).unwrap();
    assert!(
        run_copy(&repo, &pinned, Some(&pinned)).is_err(),
        "never the pinned copy"
    );
    assert!(run_copy(&repo, &repo.join("re/missing"), None).is_err());
}
