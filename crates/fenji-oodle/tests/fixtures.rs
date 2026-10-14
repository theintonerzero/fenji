//! Part of fenji-oodle, a Rust port of the Oodle decoders in ooz's `kraken.cpp`
//! (github.com/powzix/ooz, commit 05038060aa68), Copyright (C) 2016 Powzix.
//! Changes Copyright (C) 2026 the Fenji contributors.
//! GPL-3.0-or-later: see COPYING and NOTICE.
//!
//! Each fixture walks now; Tasks 4–6 turn on decoding, codec by codec.
mod common;
use common::*;

#[test]
fn every_fixture_walks_and_names_its_decoder() {
    let all = fixtures();
    assert!(!all.is_empty());
    for f in &all {
        let mut out = 0;
        for c in fenji_oodle::chunks(&f.stream, f.input.len()) {
            let c = c.unwrap_or_else(|e| panic!("{}: {e}", f.file));
            assert!(
                c.mode == fenji_oodle::ChunkMode::Memset
                    || c.mode == fenji_oodle::ChunkMode::Uncompressed
                    || c.decoder == decoder_of(f.compressor),
                "{}",
                f.file
            );
            out += c.raw_len;
        }
        assert_eq!(out, f.input.len(), "{}", f.file);
    }
}
