//! Part of fenji-oodle, a Rust port of the Oodle decoders in ooz's `kraken.cpp`
//! (github.com/powzix/ooz, commit 05038060aa68), Copyright (C) 2016 Powzix.
//! Changes Copyright (C) 2026 the Fenji contributors.
//! GPL-3.0-or-later: see COPYING and NOTICE.
//!
//! Each fixture matches its manifest, walks, and decodes byte for byte when its codec is ported.
mod common;
use common::*;

/// The decoders ported so far: Kraken (6), the only codec the pinned client uses.
const PORTED: &[u8] = &[6];

#[test]
fn every_fixture_of_a_ported_codec_decodes_to_its_input() {
    let mut decoded = 0;
    for f in fixtures()
        .iter()
        .filter(|f| PORTED.contains(&decoder_of(f.compressor)))
    {
        let got = fenji_oodle::decompress(&f.stream, f.input.len())
            .unwrap_or_else(|e| panic!("{}: {e}", f.file));
        assert!(got == f.input, "{}: decoded bytes differ", f.file);
        decoded += 1;
    }
    assert!(decoded > 0, "no fixture for the ported codecs");
}

#[test]
fn every_fixture_walks_and_names_its_decoder() {
    let all = fixtures();
    assert!(!all.is_empty());
    for f in &all {
        let want = decoder_of(f.compressor);
        let mut walk = fenji_oodle::chunks(&f.stream, f.input.len());
        // The walk covers the input's length with no error, every chunk naming the decoder (memset
        // and uncompressed ones too), and then it ends.
        let mut out = 0;
        while out < f.input.len() {
            let c = walk
                .next()
                .unwrap_or_else(|| panic!("{}: the walk stops at byte {out}", f.file))
                .unwrap_or_else(|e| panic!("{}: {e}", f.file));
            assert_eq!(
                c.decoder, want,
                "{}: the {:?} chunk at byte {out}",
                f.file, c.mode
            );
            out += c.raw_len;
        }
        let rest = walk.next();
        assert!(
            rest.is_none(),
            "{}: past the input, the walk gives {rest:?}",
            f.file
        );
    }
}

#[test]
fn every_input_and_stream_matches_its_manifest_hashes() {
    // A generator that drifts, or a fixture that changes, fails here before any decode does.
    for f in &fixtures() {
        assert_eq!(
            fnv64(&f.input),
            f.input_fnv64,
            "{}: the regenerated input",
            f.file
        );
        assert_eq!(fnv64(&f.stream), f.output_fnv64, "{}: the stream", f.file);
    }
}

#[test]
fn fnv64_gives_the_published_fnv_1a_vectors() {
    assert_eq!(fnv64(b""), "cbf29ce484222325");
    assert_eq!(fnv64(b"a"), "af63dc4c8601ec8c");
    assert_eq!(fnv64(b"foobar"), "85944171f73967e8");
}
