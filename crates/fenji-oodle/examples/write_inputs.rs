//! Part of fenji-oodle, a Rust port of the Oodle decoders in ooz's `kraken.cpp`
//! (github.com/powzix/ooz, commit 05038060aa68), Copyright (C) 2016 Powzix.
//! Changes Copyright (C) 2026 the Fenji contributors.
//! GPL-3.0-or-later: see COPYING and NOTICE.
//!
//! Writes the fixture inputs (Fenji's own synthetic bytes) to DIR/<kind>-<size>-<seed>.raw, for
//! oodle-helper's compress mode. DIR must be under re/.
#[path = "../tests/gen/mod.rs"]
mod generator;

fn main() -> std::io::Result<()> {
    let dir = std::env::args().nth(1).expect("usage: write_inputs DIR");
    std::fs::create_dir_all(&dir)?;
    for &(kind, size, seed) in generator::CASES {
        let name = format!("{dir}/{}-{size}-{seed}.raw", kind.name());
        std::fs::write(name, generator::input(kind, size, seed))?;
    }
    Ok(())
}
