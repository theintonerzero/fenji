//! Part of fenji-oodle, a Rust port of the Oodle decoders in ooz's `kraken.cpp`
//! (github.com/powzix/ooz, commit 05038060aa68), Copyright (C) 2016 Powzix.
//! Changes Copyright (C) 2026 the Fenji contributors.
//! GPL-3.0-or-later: see COPYING and NOTICE.
//!
//! The committed fixtures: Oodle streams the client's own DLL made from Fenji's synthetic input.
//! `tests/fixtures/manifest.tsv` lists each: its file, the input's kind, size and seed, the
//! compressor and level, and the [`fnv64`] hashes of the input and of the stream.
#![allow(dead_code)]
#[path = "../gen/mod.rs"]
pub mod generator;

use std::path::{Path, PathBuf};

/// The manifest's first line.
pub const MANIFEST_HEADER: &str =
    "file\tkind\tsize\tseed\tcompressor\tlevel\tinput_fnv64\toutput_fnv64";

pub struct Fixture {
    pub file: String,
    pub input: Vec<u8>,
    pub stream: Vec<u8>,
    pub compressor: u32,
    pub level: i32,
    /// The manifest's hashes of the input and of the stream.
    pub input_fnv64: String,
    pub output_fnv64: String,
}

/// 64-bit FNV-1a, in lowercase hex: the manifest's pin on each input and stream.
pub fn fnv64(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// The stream decoder a compressor writes (kraken.cpp:4178 against Kraken_ParseHeader).
pub fn decoder_of(compressor: u32) -> u8 {
    match compressor {
        8 => 6,
        9 | 11 => 10,
        13 => 12,
        c => panic!("compressor {c} isn't in the fixtures"),
    }
}

/// Every fixture in the manifest, its input regenerated from its seed. It panics, naming the line,
/// at a malformed row, a file name its row doesn't give, or a file that's missing; and it panics
/// unless the folder's `.oz` files are exactly the manifest's, each listed once.
pub fn fixtures() -> Vec<Fixture> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let manifest = std::fs::read_to_string(dir.join("manifest.tsv"))
        .unwrap_or_else(|e| panic!("tests/fixtures/manifest.tsv: {e}"));
    let mut lines = manifest.lines();
    assert_eq!(lines.next(), Some(MANIFEST_HEADER), "manifest.tsv's header");
    let all: Vec<Fixture> = lines
        .enumerate()
        .map(|(i, line)| row(&dir, i + 2, line))
        .collect();
    let mut listed: Vec<&str> = all.iter().map(|f| f.file.as_str()).collect();
    listed.sort_unstable();
    if let Some(twice) = listed.windows(2).find(|w| w[0] == w[1]) {
        panic!("manifest.tsv lists {} twice", twice[0]);
    }
    let mut unlisted: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| {
            let e = e.unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
            e.file_name().to_string_lossy().into_owned()
        })
        .filter(|name| name.ends_with(".oz") && listed.binary_search(&name.as_str()).is_err())
        .collect();
    unlisted.sort_unstable();
    assert!(
        unlisted.is_empty(),
        "tests/fixtures holds .oz files manifest.tsv doesn't list: {unlisted:?}"
    );
    all
}

/// The fixture on line `n` of the manifest.
fn row(dir: &Path, n: usize, line: &str) -> Fixture {
    let fields: Vec<&str> = line.split('\t').collect();
    let &[
        file,
        kind,
        size,
        seed,
        compressor,
        level,
        input_fnv64,
        output_fnv64,
    ] = fields.as_slice()
    else {
        panic!(
            "manifest.tsv line {n}: {} fields, not 8: {line:?}",
            fields.len()
        );
    };
    let bad = |what: &str, value: &str| -> String {
        format!("manifest.tsv line {n} ({file}): {what} {value:?}")
    };
    let kind_ = generator::Kind::from_name(kind).unwrap_or_else(|| panic!("{}", bad("kind", kind)));
    let size: usize = size
        .parse()
        .unwrap_or_else(|e| panic!("{}: {e}", bad("size", size)));
    let seed: u64 = seed
        .parse()
        .unwrap_or_else(|e| panic!("{}: {e}", bad("seed", seed)));
    let compressor: u32 = compressor
        .parse()
        .unwrap_or_else(|e| panic!("{}: {e}", bad("compressor", compressor)));
    let level: i32 = level
        .parse()
        .unwrap_or_else(|e| panic!("{}: {e}", bad("level", level)));
    assert_eq!(
        file,
        format!("{kind}-{size}-{seed}.c{compressor}.l{level}.oz"),
        "manifest.tsv line {n}: the file name and the row disagree"
    );
    Fixture {
        file: file.to_string(),
        input: generator::input(kind_, size, seed),
        stream: std::fs::read(dir.join(file))
            .unwrap_or_else(|e| panic!("manifest.tsv line {n} ({file}): {e}")),
        compressor,
        level,
        input_fnv64: input_fnv64.to_string(),
        output_fnv64: output_fnv64.to_string(),
    }
}
