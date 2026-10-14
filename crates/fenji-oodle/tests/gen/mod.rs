//! Part of fenji-oodle, a Rust port of the Oodle decoders in ooz's `kraken.cpp`
//! (github.com/powzix/ooz, commit 05038060aa68), Copyright (C) 2016 Powzix.
//! Changes Copyright (C) 2026 the Fenji contributors.
//! GPL-3.0-or-later: see COPYING and NOTICE.
//!
//! Synthetic inputs for the fixtures: Fenji's own bytes, never the game's.
#![allow(dead_code)]

/// xorshift64*: a fixed, dependency-free generator.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Zeros,
    Random,
    Repeats,
    Mixed,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Zeros => "zeros",
            Self::Random => "random",
            Self::Repeats => "repeats",
            Self::Mixed => "mixed",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        [
            Self::Text,
            Self::Zeros,
            Self::Random,
            Self::Repeats,
            Self::Mixed,
        ]
        .into_iter()
        .find(|k| k.name() == s)
    }
}

const WORDS: &[&str] = &[
    "sword", "lantern", "river", "mountain", "the", "of", "a", "jade", "wind", "quest", "scroll",
    "and", "into", "spirit", "blade", "temple", "under", "moon", "path", "silent",
];

/// `size` bytes of `kind`, from `seed`.
pub fn input(kind: Kind, size: usize, seed: u64) -> Vec<u8> {
    let mut r = Rng::new(seed);
    let mut out = Vec::with_capacity(size);
    match kind {
        Kind::Zeros => out.resize(size, 0),
        Kind::Random => {
            while out.len() < size {
                out.extend_from_slice(&r.next().to_le_bytes());
            }
        }
        Kind::Text => {
            while out.len() < size {
                out.extend_from_slice(WORDS[(r.next() % WORDS.len() as u64) as usize].as_bytes());
                let newline = r.next().is_multiple_of(12);
                out.push(if newline { b'\n' } else { b' ' });
            }
        }
        Kind::Repeats => {
            let period = 3 + (r.next() % 61) as usize;
            let unit: Vec<u8> = (0..period).map(|_| r.next() as u8).collect();
            while out.len() < size {
                out.extend_from_slice(&unit);
            }
        }
        Kind::Mixed => {
            while out.len() < size {
                let kinds = [Kind::Text, Kind::Zeros, Kind::Random, Kind::Repeats];
                let k = kinds[(r.next() % 4) as usize];
                let len = 1 + (r.next() % 20_000) as usize;
                out.extend_from_slice(&input(k, len, r.next()));
            }
        }
    }
    out.truncate(size);
    out
}

/// The cases, sized around the 256 KiB quantum and the 512 KiB block. Random data barely
/// compresses, so it stays small; the fixtures folder stays under 2 MiB.
pub const CASES: &[(Kind, usize, u64)] = &[
    (Kind::Text, 4096, 1),
    (Kind::Text, 262_145, 2),
    (Kind::Text, 524_288, 3),
    (Kind::Zeros, 262_144, 4),
    (Kind::Zeros, 524_288, 5),
    (Kind::Repeats, 65_536, 6),
    (Kind::Repeats, 262_145, 7),
    (Kind::Random, 64, 8),
    (Kind::Random, 4096, 9),
    (Kind::Random, 16_384, 10),
    (Kind::Mixed, 524_288, 11),
    // A full quantum, then a partial one of about 105 KiB: the size of `_index`'s last block
    // (docs/re/m1-archives.md 3.4).
    (Kind::Mixed, 367_689, 12),
];
