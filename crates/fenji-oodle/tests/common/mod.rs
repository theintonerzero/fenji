//! Part of fenji-oodle, a Rust port of the Oodle decoders in ooz's `kraken.cpp`
//! (github.com/powzix/ooz, commit 05038060aa68), Copyright (C) 2016 Powzix.
//! Changes Copyright (C) 2026 the Fenji contributors.
//! GPL-3.0-or-later: see COPYING and NOTICE.
//!
//! The committed fixtures: Oodle streams the client's own DLL made from Fenji's synthetic input.
#![allow(dead_code)]
#[path = "../gen/mod.rs"]
pub mod generator;

use std::path::PathBuf;

pub struct Fixture {
    pub file: String,
    pub input: Vec<u8>,
    pub stream: Vec<u8>,
    pub compressor: u32,
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

pub fn fixtures() -> Vec<Fixture> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let manifest = std::fs::read_to_string(dir.join("manifest.tsv")).unwrap();
    manifest
        .lines()
        .skip(1)
        .map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            let kind = generator::Kind::from_name(f[1]).unwrap();
            let (size, seed): (usize, u64) = (f[2].parse().unwrap(), f[3].parse().unwrap());
            Fixture {
                file: f[0].to_string(),
                input: generator::input(kind, size, seed),
                stream: std::fs::read(dir.join(f[0])).unwrap(),
                compressor: f[4].parse().unwrap(),
            }
        })
        .collect()
}
