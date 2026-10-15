//! Part of fenji-oodle, a Rust port of the Oodle decoders in ooz's `kraken.cpp`
//! (github.com/powzix/ooz, commit 05038060aa68), Copyright (C) 2016 Powzix.
//! Changes Copyright (C) 2026 the Fenji contributors.
//! GPL-3.0-or-later: see COPYING and NOTICE.
//!
//! Each input is an archive's Oodle block (`docs/re/m1-archives.md` 3.4): a little-endian `u32`
//! raw length, here at most 512 KiB as fenji-data bounds it, then one Oodle stream. It decodes to
//! an error or to exactly that many bytes, never a panic, as `tests/robustness.rs` checks on fixed
//! seeds; and the header walk, which reads the headers the decode reads, succeeds where it does.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Some((len, stream)) = data.split_first_chunk::<4>() {
        let raw_len = u32::from_le_bytes(*len) as usize % (512 * 1024 + 1);
        let walked: Result<usize, fenji_oodle::Error> = fenji_oodle::chunks(stream, raw_len)
            .map(|c| c.map(|c| c.raw_len))
            .sum();
        if let Ok(out) = fenji_oodle::decompress(stream, raw_len) {
            assert_eq!(out.len(), raw_len);
            assert_eq!(walked, Ok(raw_len));
        }
    }
});
