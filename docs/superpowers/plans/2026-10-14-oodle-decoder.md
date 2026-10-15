# The Open Oodle Decoder Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `fenji-data` decodes every Oodle file in the pinned SoLO client natively, on Linux without Wine, byte for byte as the client's own `oo2core_6_win64.dll` does.

**Architecture:**
- A new crate, `crates/fenji-oodle` (GPL-3.0-or-later), ports `ooz`'s decoders to safe Rust:
  - the stream and quantum headers;
  - the entropy decoders;
  - the LZ decoders of the codecs the pinned client uses.
- `fenji-data`'s `BuiltinCodecs` decodes `Codec::Oodle` through it.
- `tools/archive` decodes inline, and gains `census-oodle` and `verify-oodle`.
- `tools/oodle-helper` gains two dev-only modes, both inside the sandbox with the client's own DLL:
  - `compress`, for synthetic CI fixtures;
  - `hash-parts`, for the full check.

**Tech Stack:** Rust 1.97.1, safe code only, with no new external dependencies. Wine (GE-Proton11-7) and the sandbox run the dev-only helper.

**Spec:** [`docs/superpowers/specs/2026-10-14-oodle-decoder-design.md`](../specs/2026-10-14-oodle-decoder-design.md). The owner approved it on 2026-10-14. Read it with this plan.

## Global Constraints

- **Ground rules** (CLAUDE.md):
  - **The clean room.** Rule 2 holds. `ooz` is allowed community material: it's clearly licensed, and `docs/re/sources.md` lists it from Task 1.
  - **No game data in the repo** (rule 3). Fixtures are synthetic, and decoded data stays in `re/`.
  - **Epic's DLL is dev-only,** inside the sandbox (#225, gate G3).
  - **The helper runs only through `scripts/spike/run-oodle-helper.sh`.** The game client is never started.
- **The reference** is `ooz` at commit `05038060aa68f9187ae9923b2388ca8db40e58d1`: `kraken.cpp`, blob `a4983d6de308f92c4a7f71f8d0f87bb2bdc3cdd2`, 4,452 lines, "Copyright (C) 2016, Powzix", GPL-3.0-or-later.
  - Fetch it with `gh api repos/powzix/ooz/contents/kraken.cpp?ref=05038060aa68f9187ae9923b2388ca8db40e58d1 --jq .content | base64 -d`, into your scratchpad or under `re/`. Never commit it.
  - Every ported function's doc comment names its source: ``/// Port of `Kraken_DecodeStep` (kraken.cpp:4020).``
- **`fenji-oodle`'s manifest:** `license = "GPL-3.0-or-later"` (not the workspace's) and `publish = false`.
  - **Code:** safe Rust only; the workspace's `unsafe_code = "forbid"` holds.
  - **Dependencies:** none, external or workspace, including dev-dependencies.
- **Never out of bounds.** `ooz` reads and writes past its buffers: its `SAFE_SPACE` is 64 bytes, and its bit reader over-reads. The port never does. Every index is checked, and a check that fails is `Error::Corrupt` or `Error::Truncated`, never a panic.
- **Decoder numbers in a stream header:** 6 Kraken, 10 Mermaid and Selkie (one format), 12 Leviathan. 5 (LZNA) and 11 (Bitknit) are `Error::Unsupported`. A codec Task 2's census doesn't find is never ported.
- **The archives' Oodle block:** a `u32` little-endian raw length (at most 512 KiB, the blob's block size), then one Oodle stream (`m1-archives.md` 3.4).
- **Commits:**
  - Conventional Commits, with the scopes `data` (fenji-oodle, fenji-data), `tools` (archive, oodle-helper, `scripts/`) and `docs`.
  - Subjects at most 100 characters, and PR titles at most 93.
  - Exactly one trailer, `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`, and no session links.
  - Stage by path, never `git add -A`; never stage `.claude/settings.json`.
- **Tracking:** each task is an issue under milestone M5, and branch names carry it, as in `feat/data-<issue>-oodle-headers`. One PR per task, squash-merged into `dev`.
- **The gate before every PR:**
  - `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo deny check`;
  - `scripts/check-forbidden-files.sh`;
  - ShellCheck on any script you touch.

## Review Focus

These are the five inputs most likely to bite that no step's happy-path test reaches. Each has its test in the task named.
1. **A block whose `u32` prefix disagrees with the blob header's raw length for that block** (a truncated or tampered archive): `Error::Oodle`, never a decode into a wrong-sized buffer. Task 2: `a_prefix_that_disagrees_with_the_blob_is_refused`.
2. **A stream cut short** mid-quantum, right after a header, or inside a quantum header: `Error::Truncated`, never a panic. Task 1: `cut_streams_are_truncated`; Task 9 sweeps every fixture.
3. **A quantum header claiming more bytes than its output can hold**, or more than are left: `QuantumTooLarge` or `Truncated`. Task 1: `oversized_quanta_are_refused`.
4. **A match offset reaching before the start of the output**, possible in every codec: `Error::Corrupt`, never an out-of-bounds read. Tasks 4–6: `a_match_before_the_start_is_corrupt`.
5. **A blob whose header claims blocks larger than 512 KiB,** which would mean a huge allocation from a tampered header: refused before any decode. Task 7: `oversized_oodle_blocks_are_refused`.

## Corrections to the spec

The plan changes four details of the spec. Each is a controller ruling, to be put in the ledger when the plan is dispatched.
1. **`chunks` takes the raw length:** `chunks(input: &[u8], raw_len: usize)`. Each quantum's output length depends on the bytes still to come, so a walk can't size its chunks without it. (Spec §3.1 gives `chunks(input)`.)
2. **`verify-oodle` compares against hashes, not a spool.**
   - Spooling every block would copy the whole compressed archive set into `re/`.
   - Instead, `oodle-helper hash-parts` walks the parts itself, with `fenji-data`, inside the sandbox. It writes one SHA-256 per block, of the DLL's output.
   - `archive verify-oodle` decodes natively and compares. (Spec §3.3 says through the spool.)
3. **Fixtures are binary `.oz` files**, under a 2 MiB budget for the folder. Hex would double their size. (Spec §4.2 says `.hex`.)
4. **Task 4 ports the entropy decoders together with the first codec.** The entropy decoders can't be tested apart without an encoder; with the first codec, they get the fixtures end to end. (Spec §6, items 3 and 4.)

## How this plan runs

| Task | Lane | Needs merged first | What it gives |
|---|---|---|---|
| 1 The crate, headers, the driver and the chunk walk | cloud | — | `decompress` for uncompressed, memset and stored quanta; `chunks` |
| 2 The census | local | 1 | which decoders and modes the pinned client uses |
| 3 The fixtures | local | 2 | synthetic `.oz` fixtures for each codec found |
| 4 The entropy decoders and the first codec | cloud | 3 | the census's most common codec decodes |
| 5 The second codec (if the census finds one) | cloud | 4 | it decodes |
| 6 The third codec (if the census finds one) | cloud | 4 | it decodes |
| 7 `fenji-data`, inline decoding, and `verify-oodle` | cloud | 4 | the archive tools without Wine; the comparison |
| 8 The full check | local | 5, 6, 7 | zero mismatches over every block, recorded |
| 9 Robustness and the fuzz target | cloud | 4 | bad input is an error, never a panic |

- At most two agents run at once.
- The local lane needs the client (`source ~/.config/fenji/env.sh`), and the owner's machine.
- Tasks 5 and 6 can run side by side: they share only one line in `stream.rs`, a `match` arm each.

## File map

**New:**
| File | What it holds | Task |
|---|---|---|
| `crates/fenji-oodle/Cargo.toml`, `COPYING`, `NOTICE` | the crate, the GPL-3.0 text, and the attribution | 1 |
| `crates/fenji-oodle/src/lib.rs` | the face: `decompress`, `chunks`, `ChunkInfo`, `ChunkMode`, `Error`, `QUANTUM` | 1 |
| `crates/fenji-oodle/src/error.rs` | `Error` | 1 |
| `crates/fenji-oodle/src/header.rs` | stream and quantum headers | 1 |
| `crates/fenji-oodle/src/stream.rs` | the driver, and the chunk walk | 1 |
| `crates/fenji-oodle/tests/gen/mod.rs` | the seeded generator of synthetic inputs | 3 |
| `crates/fenji-oodle/examples/write_inputs.rs` | writes the inputs for the helper to compress | 3 |
| `crates/fenji-oodle/tests/fixtures/*.oz`, `manifest.tsv` | the compressed fixtures | 3 |
| `crates/fenji-oodle/tests/common/mod.rs` | the fixture loader, shared by the integration tests | 3 |
| `crates/fenji-oodle/tests/fixtures.rs` | each fixture walks, and decodes to its input | 3, 4–6 |
| `crates/fenji-oodle/src/bits.rs` | the bit readers | 4 |
| `crates/fenji-oodle/src/entropy/{mod,huffman,tans,rle,recursive,multi_array}.rs` | the entropy decoders | 4 |
| `crates/fenji-oodle/src/{kraken,mermaid,leviathan}.rs` | the codecs the census finds | 4–6 |
| `crates/fenji-oodle/src/lz.rs` | `copy_match`, which every codec uses | 4 |
| `crates/fenji-oodle/tests/robustness.rs` | truncations, bit flips and random input | 9 |
| `crates/fenji-oodle/fuzz/` | the `cargo-fuzz` target, outside the workspace | 9 |

**Modified:**
| File | What changes | Task |
|---|---|---|
| `Cargo.toml` | the member `crates/fenji-oodle`; `exclude = ["crates/fenji-oodle/fuzz"]` | 1, 9 |
| `docs/re/sources.md` | `ooz`'s row | 1 |
| `crates/fenji-data/src/{lib,codec,error}.rs`, `Cargo.toml` | `oodle_payload`, `Error::Oodle`, `blocks` exported; Oodle in `BuiltinCodecs` | 2, 7 |
| `tools/archive/src/main.rs` | `census-oodle`; inline decoding; `verify-oodle` | 2, 7 |
| `tools/oodle-helper/src/{dll,main}.rs`, `Cargo.toml` | the `compress` and `hash-parts` modes | 3, 7 |
| `scripts/spike/run-oodle-helper.sh`, its test | the two modes | 3, 7 |
| `docs/re/m1-archives.md` | the census (§3.6), and the full check (§5) | 2, 8 |

---

## Task 1 [cloud]: The crate, its headers, the stream driver, and the chunk walk

**Files:**
- Create: `crates/fenji-oodle/{Cargo.toml,COPYING,NOTICE}`, `crates/fenji-oodle/src/{lib,error,header,stream}.rs`
- Modify: `Cargo.toml` (`members`), `docs/re/sources.md`
- Test: the `#[cfg(test)]` modules of `header.rs` and `stream.rs`

**Interfaces:**
- Produces: `fenji_oodle::{decompress(src: &[u8], raw_len: usize) -> Result<Vec<u8>, Error>, chunks(src: &[u8], raw_len: usize) -> Chunks<'_>, Chunks (Iterator<Item = Result<ChunkInfo, Error>>), ChunkInfo { decoder: u8, mode: ChunkMode, checksums: bool, raw_len: usize, compressed_len: usize }, ChunkMode::{Uncompressed, Memset, Stored, Compressed}, Error::{Truncated { at }, BadStreamHeader { at, byte }, BadQuantumHeader { at }, Unsupported { decoder }, QuantumTooLarge { at, size, room }, Corrupt { at, what }, TrailingBytes { left }}, QUANTUM = 0x40000}`.
- Produces, crate-internal, for Tasks 4–6: `stream::decode_quantum(decoder: u8, dst: &mut [u8], start: usize, src: &[u8], scratch: &mut Vec<u8>) -> Result<usize, Error>`, the dispatch each codec adds an arm to. `dst[..start]` is earlier output, the window for matches; the quantum fills `dst[start..]`. It returns how many bytes of `src` it read.

- [ ] **Step 1: The crate's manifest, its licence files, and the workspace member.**

`crates/fenji-oodle/Cargo.toml`:
```toml
[package]
name = "fenji-oodle"
description = "A Rust port of ooz's Oodle decoders (Kraken, Mermaid and Selkie, Leviathan), for the SoLO client's archives"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true
# Not the workspace's AGPL: this crate ports GPL-3.0-or-later code (NOTICE).
license = "GPL-3.0-or-later"

[lints]
workspace = true
```

Add `"crates/fenji-oodle"` to the root `Cargo.toml`'s `members`, after `"crates/fenji-data"`.

Fetch `COPYING` with `curl -fsSL https://www.gnu.org/licenses/gpl-3.0.txt -o crates/fenji-oodle/COPYING`, the GPL-3.0 text, unchanged.

`crates/fenji-oodle/NOTICE`:
```text
fenji-oodle is a Rust port of the Oodle decoders in ooz
(https://github.com/powzix/ooz, commit 05038060aa68f9187ae9923b2388ca8db40e58d1),
kraken.cpp, Copyright (C) 2016, Powzix, licensed under the GNU General Public License,
version 3 or (at your option) any later version.

The port's changes are Copyright the Fenji contributors, under the same licence: see COPYING.
Fenji as a whole is AGPL-3.0-or-later; the two combine by section 13 of each licence.

Oodle is Epic Games' proprietary compression. This crate contains none of Epic's code; it
decodes the format as ooz does.
```

- [ ] **Step 2: `ooz`'s row in `docs/re/sources.md`.** Append to the "Used" table:
```markdown
| [ooz](https://github.com/powzix/ooz) at `05038060aa68` (`kraken.cpp`) | GPL-3.0-or-later (the file's header) | Yes: a public Kraken/Mermaid/Selkie/Leviathan decompressor its author wrote from Oodle; nothing SoLO-specific | Ported to safe Rust in `crates/fenji-oodle`, to decode the client's Oodle archives (#225) | 2026-10-14 |
```

- [ ] **Step 3: Write the failing tests.** `crates/fenji-oodle/src/stream.rs` starts as only its tests. `src/lib.rs` declares `mod error; mod header; mod stream;` and re-exports the items below, so that the tests compile against missing items and fail:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// A stream header: the 0x_C magic with bits 4–5 clear, the restart bit, and the decoder.
    fn hdr(uncompressed: bool, decoder: u8, checksums: bool) -> [u8; 2] {
        [0x8C | (u8::from(uncompressed) << 6), decoder | (u8::from(checksums) << 7)]
    }

    /// A quantum header for `size` bytes of data: the 18-bit field holds size − 1.
    fn data_q(size: usize) -> [u8; 3] {
        let v = (size - 1) as u32;
        [(v >> 16) as u8, (v >> 8) as u8, v as u8]
    }

    /// The memset quantum: the reserved size 0x3FFFF, kind 1, then the byte.
    fn memset_q(byte: u8) -> [u8; 4] {
        [0x07, 0xFF, 0xFF, byte]
    }

    fn bytes(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn an_uncompressed_stream_is_its_bytes() {
        let data = bytes(100);
        let src = [&hdr(true, 6, false)[..], &data].concat();
        assert_eq!(decompress(&src, 100).unwrap(), data);
    }

    #[test]
    fn every_quantum_starts_with_a_stream_header() {
        let data = bytes(QUANTUM + 50);
        let src = [
            &hdr(true, 6, false)[..],
            &data[..QUANTUM],
            &hdr(true, 12, false),
            &data[QUANTUM..],
        ]
        .concat();
        assert_eq!(decompress(&src, data.len()).unwrap(), data);
    }

    #[test]
    fn a_memset_quantum_repeats_its_byte() {
        let src = [&hdr(false, 6, false)[..], &memset_q(0xAB)].concat();
        assert_eq!(decompress(&src, 1000).unwrap(), vec![0xAB; 1000]);
    }

    #[test]
    fn a_quantum_as_long_as_its_output_is_stored() {
        let data = bytes(100);
        let src = [&hdr(false, 10, false)[..], &data_q(100), &data].concat();
        assert_eq!(decompress(&src, 100).unwrap(), data);
    }

    #[test]
    fn checksums_add_three_bytes_and_are_not_checked() {
        // ooz's Kraken_GetCrc is unimplemented (kraken.cpp:567); the port parses the checksum
        // and never checks it.
        let data = bytes(10);
        let src = [&hdr(false, 12, true)[..], &data_q(10), &[1, 2, 3], &data].concat();
        assert_eq!(decompress(&src, 10).unwrap(), data);
    }

    #[test]
    fn two_quanta() {
        let tail = bytes(10);
        let src = [
            &hdr(false, 6, false)[..],
            &memset_q(7),
            &hdr(false, 6, false),
            &data_q(10),
            &tail,
        ]
        .concat();
        let mut want = vec![7u8; QUANTUM];
        want.extend_from_slice(&tail);
        assert_eq!(decompress(&src, QUANTUM + 10).unwrap(), want);
    }

    #[test]
    fn bad_headers_are_errors() {
        assert_eq!(
            decompress(&[0x00, 0x06], 10),
            Err(Error::BadStreamHeader { at: 0, byte: 0 })
        );
        assert_eq!(
            decompress(&[0x9C, 0x06], 10), // bits 4–5 set
            Err(Error::BadStreamHeader { at: 0, byte: 0x9C })
        );
        for d in [5u8, 11, 7, 0] {
            let src = [&hdr(false, d, false)[..], &memset_q(1)].concat();
            assert_eq!(decompress(&src, 10), Err(Error::Unsupported { decoder: d }), "{d}");
        }
        // The reserved size with a kind other than memset.
        let src = [&hdr(false, 6, false)[..], &[0x0B, 0xFF, 0xFF, 0]].concat();
        assert_eq!(decompress(&src, 10), Err(Error::BadQuantumHeader { at: 2 }));
    }

    #[test]
    fn cut_streams_are_truncated() {
        // Review Focus 2.
        let full = [&hdr(false, 6, false)[..], &data_q(10), &bytes(10)].concat();
        for cut in 0..full.len() {
            let r = decompress(&full[..cut], 10);
            assert!(matches!(r, Err(Error::Truncated { .. })), "cut at {cut}: {r:?}");
        }
        let unc = [&hdr(true, 6, false)[..], &bytes(9)].concat();
        assert_eq!(decompress(&unc, 10), Err(Error::Truncated { at: 2 }));
    }

    #[test]
    fn oversized_quanta_are_refused() {
        // Review Focus 3.
        let src = [&hdr(false, 6, false)[..], &data_q(11), &bytes(11)].concat();
        assert_eq!(
            decompress(&src, 10),
            Err(Error::QuantumTooLarge { at: 5, size: 11, room: 10 })
        );
    }

    #[test]
    fn leftover_bytes_are_an_error() {
        let src = [&hdr(true, 6, false)[..], &bytes(11)].concat();
        assert_eq!(decompress(&src, 10), Err(Error::TrailingBytes { left: 1 }));
        assert_eq!(decompress(&[0x8C], 0), Err(Error::TrailingBytes { left: 1 }));
        assert_eq!(decompress(&[], 0), Ok(Vec::new()));
    }

    #[test]
    fn a_compressed_quantum_needs_its_decoder() {
        // Tasks 4–6 each remove the decoder they port from this list and add its fixtures.
        for d in [6u8, 10, 12] {
            let src = [&hdr(false, d, false)[..], &data_q(5), &bytes(5)].concat();
            assert_eq!(decompress(&src, 10), Err(Error::Unsupported { decoder: d }), "{d}");
        }
    }

    #[test]
    fn the_chunk_walk_reads_headers_only() {
        let tail = bytes(10);
        let src = [
            &hdr(false, 6, false)[..],
            &memset_q(7),
            &hdr(false, 6, true),
            &data_q(10),
            &[0, 0, 0],
            &tail,
        ]
        .concat();
        let got: Vec<_> = chunks(&src, QUANTUM + 10).collect();
        assert_eq!(
            got,
            vec![
                Ok(ChunkInfo { decoder: 6, mode: ChunkMode::Memset, checksums: false, raw_len: QUANTUM, compressed_len: 0 }),
                Ok(ChunkInfo { decoder: 6, mode: ChunkMode::Stored, checksums: true, raw_len: 10, compressed_len: 10 }),
            ]
        );
        // A compressed quantum is walked over, not decoded, and a walk ends at its first error.
        let src = [&hdr(false, 12, false)[..], &data_q(5), &bytes(5), &[9]].concat();
        let got: Vec<_> = chunks(&src, 10).collect();
        assert_eq!(
            got,
            vec![
                Ok(ChunkInfo { decoder: 12, mode: ChunkMode::Compressed, checksums: false, raw_len: 10, compressed_len: 5 }),
                Err(Error::TrailingBytes { left: 1 }),
            ]
        );
    }
}
```

`header.rs` gets its own unit tests, written first in the same way:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_header_fields() {
        assert_eq!(
            stream_header(&[0x4C, 0x86], 0),
            Ok((StreamHeader { decoder: 6, uncompressed: true, checksums: true }, 2))
        );
        assert_eq!(stream_header(&[0x8C], 0), Err(Error::Truncated { at: 0 }));
    }

    #[test]
    fn quantum_header_sizes_and_memset() {
        assert_eq!(
            quantum_header(&[0x03, 0xFF, 0xFE], 0, false),
            Ok((Quantum::Data { size: 0x3FFFF, checksum: None }, 3))
        );
        assert_eq!(
            quantum_header(&[0x00, 0x00, 0x00, 0xA, 0xB, 0xC], 0, true),
            Ok((Quantum::Data { size: 1, checksum: Some(0x0A0B0C) }, 6))
        );
        assert_eq!(quantum_header(&[0x07, 0xFF, 0xFF, 0x5A], 0, true), Ok((Quantum::Memset { byte: 0x5A }, 4)));
        assert_eq!(quantum_header(&[0x07, 0xFF, 0xFF], 0, false), Err(Error::Truncated { at: 3 }));
    }
}
```

- [ ] **Step 4: Watch them fail.** Run `cargo test -p fenji-oodle`. Expected: FAIL to compile, because `decompress`, `chunks`, `stream_header`, `quantum_header`, `Quantum`, `StreamHeader`, `ChunkInfo`, `ChunkMode`, `Error` and `QUANTUM` don't exist.

- [ ] **Step 5: `src/lib.rs` and `src/error.rs`.**

`src/lib.rs`:
```rust
//! A Rust port of the Oodle decoders in ooz (github.com/powzix/ooz, commit 05038060aa68),
//! `kraken.cpp`, Copyright (C) 2016, Powzix; the port's changes Copyright the Fenji contributors.
//! GPL-3.0-or-later: see COPYING and NOTICE.
//!
//! [`decompress`] turns one Oodle stream into exactly `raw_len` bytes. [`chunks`] walks a
//! stream's headers without decoding, for the census in tools/archive. Only the Kraken family
//! (decoders 6, 10 and 12) is decoded; checksums are parsed and never checked, as in ooz.
mod error;
mod header;
mod stream;

pub use error::Error;
pub use stream::{ChunkInfo, ChunkMode, Chunks, QUANTUM, chunks, decompress};
```

`src/error.rs`:
```rust
use std::fmt;

/// Why a stream didn't decode. `at` is a byte offset into the stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The stream ended inside a header or a quantum.
    Truncated { at: usize },
    /// A stream header whose first byte isn't `0x_C` with bits 4–5 clear.
    BadStreamHeader { at: usize, byte: u8 },
    /// A quantum header with the reserved size and a kind other than memset.
    BadQuantumHeader { at: usize },
    /// A decoder this crate doesn't decode: LZNA (5), Bitknit (11), a number ooz doesn't know, or
    /// one not ported (a codec the pinned client doesn't use).
    Unsupported { decoder: u8 },
    /// A quantum claiming more bytes than its output holds.
    QuantumTooLarge { at: usize, size: usize, room: usize },
    /// Malformed quantum data; `what` says how.
    Corrupt { at: usize, what: &'static str },
    /// Bytes left over once the output is complete.
    TrailingBytes { left: usize },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated { at } => write!(f, "the stream ends early, at byte {at}"),
            Self::BadStreamHeader { at, byte } => {
                write!(f, "not an Oodle stream header at byte {at} ({byte:#04x})")
            }
            Self::BadQuantumHeader { at } => write!(f, "a bad quantum header at byte {at}"),
            Self::Unsupported { decoder } => write!(f, "decoder {decoder} isn't one Fenji decodes"),
            Self::QuantumTooLarge { at, size, room } => write!(
                f,
                "the quantum at byte {at} claims {size} bytes where its output holds {room}"
            ),
            Self::Corrupt { at, what } => write!(f, "corrupt data at byte {at}: {what}"),
            Self::TrailingBytes { left } => {
                write!(f, "{left} bytes are left over after the output is complete")
            }
        }
    }
}

impl std::error::Error for Error {}
```

- [ ] **Step 6: `src/header.rs`.** Above its tests:
```rust
//! The stream and quantum headers: ports of `Kraken_ParseHeader` (kraken.cpp:465) and
//! `Kraken_ParseQuantumHeader` (kraken.cpp:482).
use crate::Error;

/// The two-byte header before every 256 KiB of output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StreamHeader {
    /// 6 Kraken, 10 Mermaid and Selkie, 12 Leviathan, 5 LZNA, 11 Bitknit.
    pub decoder: u8,
    pub uncompressed: bool,
    pub checksums: bool,
}

/// Port of `Kraken_ParseHeader` (kraken.cpp:465): the header at `at`, and where what follows it
/// starts.
pub(crate) fn stream_header(src: &[u8], at: usize) -> Result<(StreamHeader, usize), Error> {
    let Some(&[b0, b1]) = src.get(at..at + 2) else {
        return Err(Error::Truncated { at });
    };
    if b0 & 0x0F != 0x0C || (b0 >> 4) & 3 != 0 {
        return Err(Error::BadStreamHeader { at, byte: b0 });
    }
    let decoder = b1 & 0x7F;
    if !matches!(decoder, 5 | 6 | 10 | 11 | 12) {
        return Err(Error::Unsupported { decoder });
    }
    let h = StreamHeader {
        decoder,
        uncompressed: (b0 >> 6) & 1 == 1,
        checksums: b1 >> 7 == 1,
    };
    Ok((h, at + 2))
}

/// A Kraken-family quantum header (decoders 6, 10 and 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Quantum {
    /// `size` bytes of quantum data follow. As long as the quantum's output, they're stored as is.
    Data { size: usize, checksum: Option<u32> },
    /// The output is one byte, repeated.
    Memset { byte: u8 },
}

/// Port of `Kraken_ParseQuantumHeader` (kraken.cpp:482).
pub(crate) fn quantum_header(
    src: &[u8],
    at: usize,
    checksums: bool,
) -> Result<(Quantum, usize), Error> {
    let b = src.get(at..at + 3).ok_or(Error::Truncated { at })?;
    let v = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
    let size = v & 0x3FFFF;
    if size != 0x3FFFF {
        let size = size as usize + 1;
        if !checksums {
            return Ok((Quantum::Data { size, checksum: None }, at + 3));
        }
        let c = src.get(at + 3..at + 6).ok_or(Error::Truncated { at: at + 3 })?;
        let checksum = u32::from(c[0]) << 16 | u32::from(c[1]) << 8 | u32::from(c[2]);
        return Ok((Quantum::Data { size, checksum: Some(checksum) }, at + 6));
    }
    if v >> 18 == 1 {
        let byte = *src.get(at + 3).ok_or(Error::Truncated { at: at + 3 })?;
        return Ok((Quantum::Memset { byte }, at + 4));
    }
    Err(Error::BadQuantumHeader { at })
}
```

- [ ] **Step 7: `src/stream.rs`.** Above its tests:
```rust
//! The stream driver and the chunk walk: ports of `Kraken_DecodeStep` (kraken.cpp:4020) and
//! `Kraken_Decompress` (kraken.cpp:4130), for the Kraken family only.
use crate::Error;
use crate::header::{Quantum, StreamHeader, quantum_header, stream_header};

/// Output bytes per quantum in the Kraken family; a stream header precedes each.
pub const QUANTUM: usize = 0x40000;

/// Scratch for a codec's first phase, as ooz sizes it (kraken.cpp:452).
pub(crate) const SCRATCH: usize = 0x6C000;

/// Port of `Kraken_Decompress` (kraken.cpp:4130): `src`, one whole Oodle stream, decoded into
/// exactly `raw_len` bytes.
pub fn decompress(src: &[u8], raw_len: usize) -> Result<Vec<u8>, Error> {
    let mut out = vec![0u8; raw_len];
    let mut scratch = Vec::new();
    let mut at = 0;
    let mut done = 0;
    let mut h = StreamHeader { decoder: 0, uncompressed: false, checksums: false };
    while done < raw_len {
        if done % QUANTUM == 0 {
            (h, at) = stream_header(src, at)?;
        }
        let n = (raw_len - done).min(QUANTUM);
        if h.uncompressed {
            let bytes = src.get(at..at + n).ok_or(Error::Truncated { at })?;
            out[done..done + n].copy_from_slice(bytes);
            at += n;
            done += n;
            continue;
        }
        if !matches!(h.decoder, 6 | 10 | 12) {
            return Err(Error::Unsupported { decoder: h.decoder });
        }
        let q;
        (q, at) = quantum_header(src, at, h.checksums)?;
        match q {
            Quantum::Memset { byte } => out[done..done + n].fill(byte),
            Quantum::Data { size, .. } => {
                if size > n {
                    return Err(Error::QuantumTooLarge { at, size, room: n });
                }
                let data = src.get(at..at + size).ok_or(Error::Truncated { at })?;
                if size == n {
                    out[done..done + n].copy_from_slice(data);
                } else {
                    if scratch.is_empty() {
                        scratch.resize(SCRATCH, 0);
                    }
                    let used = decode_quantum(h.decoder, &mut out[..done + n], done, data, &mut scratch)?;
                    if used != size {
                        return Err(Error::Corrupt {
                            at,
                            what: "the quantum's decoder read a different length than its header gives",
                        });
                    }
                }
                at += size;
            }
        }
        done += n;
    }
    if at != src.len() {
        return Err(Error::TrailingBytes { left: src.len() - at });
    }
    Ok(out)
}

/// The quantum decoders, by stream decoder number. Each fills `dst[start..]` from `src`, reading
/// earlier output in `dst[..start]` for matches, and returns how many bytes of `src` it read.
/// Tasks 4–6 add one arm each, for the codecs Task 2's census finds.
pub(crate) fn decode_quantum(
    decoder: u8,
    dst: &mut [u8],
    start: usize,
    src: &[u8],
    scratch: &mut Vec<u8>,
) -> Result<usize, Error> {
    let _ = (dst, start, src, scratch);
    Err(Error::Unsupported { decoder })
}

/// How a chunk's output is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkMode {
    /// The stream header says uncompressed: the bytes follow as they are.
    Uncompressed,
    /// One byte, repeated.
    Memset,
    /// Quantum data as long as its output: stored as is.
    Stored,
    /// Quantum data for the codec.
    Compressed,
}

/// One quantum's headers, read without decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkInfo {
    pub decoder: u8,
    pub mode: ChunkMode,
    pub checksums: bool,
    pub raw_len: usize,
    pub compressed_len: usize,
}

/// Walks `src`'s headers as [`decompress`] would, one quantum at a time, without decoding any.
/// It yields one error and stops at the first problem, leftover bytes included.
pub fn chunks(src: &[u8], raw_len: usize) -> Chunks<'_> {
    Chunks {
        src,
        raw_len,
        at: 0,
        done: 0,
        h: StreamHeader { decoder: 0, uncompressed: false, checksums: false },
        over: false,
    }
}

/// The iterator [`chunks`] returns.
pub struct Chunks<'a> {
    src: &'a [u8],
    raw_len: usize,
    at: usize,
    done: usize,
    h: StreamHeader,
    over: bool,
}

impl Iterator for Chunks<'_> {
    type Item = Result<ChunkInfo, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.over {
            return None;
        }
        if self.done >= self.raw_len {
            self.over = true;
            return (self.at != self.src.len())
                .then(|| Err(Error::TrailingBytes { left: self.src.len() - self.at }));
        }
        let r = self.step();
        if r.is_err() {
            self.over = true;
        }
        Some(r)
    }
}

impl Chunks<'_> {
    fn step(&mut self) -> Result<ChunkInfo, Error> {
        if self.done % QUANTUM == 0 {
            (self.h, self.at) = stream_header(self.src, self.at)?;
        }
        let h = self.h;
        let n = (self.raw_len - self.done).min(QUANTUM);
        let info = |mode, compressed_len| ChunkInfo {
            decoder: h.decoder,
            mode,
            checksums: h.checksums,
            raw_len: n,
            compressed_len,
        };
        let chunk = if h.uncompressed {
            self.src.get(self.at..self.at + n).ok_or(Error::Truncated { at: self.at })?;
            self.at += n;
            info(ChunkMode::Uncompressed, n)
        } else {
            if !matches!(h.decoder, 6 | 10 | 12) {
                return Err(Error::Unsupported { decoder: h.decoder });
            }
            let q;
            (q, self.at) = quantum_header(self.src, self.at, h.checksums)?;
            match q {
                Quantum::Memset { .. } => info(ChunkMode::Memset, 0),
                Quantum::Data { size, .. } => {
                    if size > n {
                        return Err(Error::QuantumTooLarge { at: self.at, size, room: n });
                    }
                    self.src
                        .get(self.at..self.at + size)
                        .ok_or(Error::Truncated { at: self.at })?;
                    self.at += size;
                    let mode = if size == n { ChunkMode::Stored } else { ChunkMode::Compressed };
                    info(mode, size)
                }
            }
        };
        self.done += n;
        Ok(chunk)
    }
}
```

- [ ] **Step 8: Watch them pass.** Run `cargo test -p fenji-oodle`. Expected: PASS, all 14 tests (12 in `stream.rs`, 2 in `header.rs`).

- [ ] **Step 9: The gate, the commit, and the PR.**
  - Run the full gate.
  - Commit `crates/fenji-oodle`, `Cargo.toml`, `Cargo.lock` and `docs/re/sources.md` as `feat(data): add fenji-oodle with Oodle's stream headers and the chunk walk`.
  - Open the PR into `dev`, with `Closes #<Task 1 issue>`.

---

## Task 2 [local]: The census

**Files:**
- Modify: `crates/fenji-data/src/{lib,codec,error}.rs`, `tools/archive/Cargo.toml` and `src/main.rs`, `docs/re/m1-archives.md`
- Test: `crates/fenji-data/src/codec.rs`'s tests

**Interfaces:**
- Consumes: `fenji_oodle::{chunks, ChunkInfo, ChunkMode, Error}` (Task 1).
- Produces:
  - `fenji_data::oodle_payload(input: &[u8], raw_len: usize, index: usize) -> Result<&[u8]>`;
  - `fenji_data::Error::Oodle { index: usize, reason: String }`;
  - `fenji_data::{Block, blocks}`, re-exported from `blob`;
  - `archive census-oodle --out DIR`.

- [ ] **Step 1: The failing test.** In `crates/fenji-data/src/codec.rs`'s tests:
```rust
#[test]
fn a_prefix_that_disagrees_with_the_blob_is_refused() {
    // Review Focus 1: m1-archives 3.4's u32 raw length must match the blob header's.
    let mut block = 100u32.to_le_bytes().to_vec();
    block.extend_from_slice(&[0x4C, 0x06]);
    assert_eq!(crate::oodle_payload(&block, 100, 0).unwrap(), &[0x4C, 0x06]);
    let e = crate::oodle_payload(&block, 99, 3).unwrap_err();
    assert!(matches!(&e, Error::Oodle { index: 3, reason } if reason.contains("100") && reason.contains("99")), "{e}");
    assert!(matches!(crate::oodle_payload(&[1, 2, 3], 1, 0), Err(Error::Oodle { .. })));
}
```
Run `cargo test -p fenji-data a_prefix`. Expected: FAIL to compile, because there's no `oodle_payload` and no `Error::Oodle`.

- [ ] **Step 2: Make it pass.**
  - `error.rs`: add the variant `#[error("Oodle block {index}: {reason}")] Oodle { index: usize, reason: String },`.
  - `codec.rs`:
```rust
/// An Oodle block's stream, once its leading raw length (`m1-archives.md` 3.4) matches the
/// blob header's `raw_len` for that block.
pub fn oodle_payload(input: &[u8], raw_len: usize, index: usize) -> Result<&[u8]> {
    let Some((prefix, stream)) = input.split_first_chunk::<4>() else {
        return Err(Error::Oodle { index, reason: "shorter than its 4-byte raw length".into() });
    };
    let said = u32::from_le_bytes(*prefix) as usize;
    if said != raw_len {
        return Err(Error::Oodle {
            index,
            reason: format!("it says {said} raw bytes where the blob header says {raw_len}"),
        });
    }
    Ok(stream)
}
```
  - `lib.rs`: `pub use codec::{BuiltinCodecs, Decompressor, oodle_payload};`, and `pub use blob::{Block, blocks};`, beside the existing `blob` re-exports.
  - Run `cargo test -p fenji-data`. Expected: PASS.

- [ ] **Step 3: `archive census-oodle`.**
  - In `tools/archive/Cargo.toml`, add `fenji-oodle = { path = "../../crates/fenji-oodle" }`.
  - In `main.rs`, add the subcommand `CensusOodle { #[arg(long, default_value = "re/oodle-census")] out: PathBuf }`, documented as `/// Count the Oodle chunk headers of every live Oodle record, decoding nothing. Writes OUT/summary.tsv`, and dispatch it to:
```rust
fn census_oodle(client: &Path, out: &Path) -> Result<ExitCode> {
    use std::collections::BTreeMap;
    let repo = fenji_data::re_dir::find_repo_root(&std::env::current_dir()?)?;
    let out = fenji_data::re_dir::resolve_out_dir(&repo, out)?;
    let places = live_places(client)?;
    let (mut files, mut blocks_seen, mut bad_prefix) = (0u64, 0u64, 0u64);
    let mut chunks: BTreeMap<(u8, &'static str, bool), u64> = BTreeMap::new();
    let mut errors: BTreeMap<String, u64> = BTreeMap::new();
    for p in places.by_key.values().filter(|p| p.codec == "oodle") {
        let blob = read_record(client, p)?;
        let (_, blocks) = fenji_data::blocks(&blob, 0)?;
        files += 1;
        for b in blocks {
            blocks_seen += 1;
            let Ok(stream) = fenji_data::oodle_payload(b.input, b.raw_len, b.index) else {
                bad_prefix += 1;
                continue;
            };
            for c in fenji_oodle::chunks(stream, b.raw_len) {
                match c {
                    Ok(c) => {
                        let mode = match c.mode {
                            fenji_oodle::ChunkMode::Uncompressed => "uncompressed",
                            fenji_oodle::ChunkMode::Memset => "memset",
                            fenji_oodle::ChunkMode::Stored => "stored",
                            fenji_oodle::ChunkMode::Compressed => "compressed",
                        };
                        *chunks.entry((c.decoder, mode, c.checksums)).or_default() += 1;
                    }
                    Err(e) => {
                        let kind = format!("{e:?}");
                        let kind = kind.split([' ', '{']).next().unwrap_or("?").to_string();
                        *errors.entry(kind).or_default() += 1;
                    }
                }
            }
        }
    }
    let mut tsv = String::from("decoder\tmode\tchecksums\tchunks\n");
    for ((d, m, c), n) in &chunks {
        tsv.push_str(&format!("{d}\t{m}\t{c}\t{n}\n"));
    }
    tsv.push_str(&format!("# files\t{files}\n# blocks\t{blocks_seen}\n# bad_prefix\t{bad_prefix}\n"));
    for (e, n) in &errors {
        tsv.push_str(&format!("# walk_error\t{e}\t{n}\n"));
    }
    std::fs::write(out.join("summary.tsv"), &tsv)?;
    print!("{tsv}");
    Ok(ExitCode::SUCCESS)
}
```
  - Run `cargo clippy -p archive -- -D warnings`. Expected: clean.

- [ ] **Step 4: Run the census on the owner's game box.** `source ~/.config/fenji/env.sh && cargo run --release -p archive -- census-oodle`. It reads only headers, and decodes nothing.
  - **Expected:** `files` = 209,595 (`m1-archives.md` 2.3), zero `bad_prefix`, and no walk errors.
  - **Check the containers.** `m1-archives.md` 4.3 says the containers' records name keys among the parts' live records, so this walk covers their 16,162 files. Check that sentence against 4.3's evidence, and say in the report whether it holds.
  - **If not as expected,** write the counts in the report. Don't change the walk to hide them.

- [ ] **Step 5: Record it.**
  - Add `m1-archives.md` 3.6, "M5 update: the Oodle census". Give the table of decoder × mode × checksums counts, the totals, and the date, tagged **Observed**, with the evidence key `AR` (`re/oodle-census/summary.tsv`). Add `AR` to the evidence table.
  - State which decoders appear, so the controller can rule which of Tasks 4–6 run, and which codec is the most common (Task 4's).

- [ ] **Step 6: The gate, the commit, and the PR.** Commit as `feat(tools): count the client's Oodle chunks by decoder and mode`, with `Closes #<Task 2 issue>`. The report gives the census table.

---

## Task 3 [local]: Synthetic fixtures, made once with the client's DLL

**Files:**
- Create: `crates/fenji-oodle/tests/gen/mod.rs`, `crates/fenji-oodle/tests/common/mod.rs`, `crates/fenji-oodle/examples/write_inputs.rs`, `crates/fenji-oodle/tests/fixtures/` (`*.oz`, `manifest.tsv`), `crates/fenji-oodle/tests/fixtures.rs`
- Modify: `tools/oodle-helper/src/{dll,main}.rs`, `scripts/spike/run-oodle-helper.sh` and its test (`scripts/spike/test-run-oodle-helper.sh`)

**Interfaces:**
- Consumes: Task 2's census, through the controller's ruling on which compressors: Kraken 8, Mermaid 9, Selkie 11, Leviathan 13 (`ooz`'s `kCompressor_*`, kraken.cpp:4178). The stream decoders are Kraken 6, Mermaid and Selkie 10, Leviathan 12.
- Produces:
  - `tests/gen::{Rng, Kind, input, CASES}`;
  - fixtures named `<kind>-<size>-<seed>.c<compressor>.l<level>.oz`, listed in `manifest.tsv` (`file`, `kind`, `size`, `seed`, `compressor`, `level`);
  - `oodle-helper DLL compress IN OUT COMPRESSOR LEVEL`.

- [ ] **Step 1: Check that the client's DLL can compress.**
  - Add `oodle-helper DLL probe`, which prints whether `OodleLZ_Compress` resolves.
  - Run it through `scripts/spike/run-oodle-helper.sh --probe`, after the Step 3 changes to the script.
  - **If it doesn't resolve,** stop and report NEEDS_CONTEXT: the controller then rules on a fallback, such as tests that run only against local data.

- [ ] **Step 2: The generator** (`crates/fenji-oodle/tests/gen/mod.rs`). Fenji's own data, never the game's:
```rust
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
        [Self::Text, Self::Zeros, Self::Random, Self::Repeats, Self::Mixed]
            .into_iter()
            .find(|k| k.name() == s)
    }
}

const WORDS: &[&str] = &[
    "sword", "lantern", "river", "mountain", "the", "of", "a", "jade", "wind", "quest",
    "scroll", "and", "into", "spirit", "blade", "temple", "under", "moon", "path", "silent",
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
                out.push(if r.next() % 12 == 0 { b'\n' } else { b' ' });
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
];
```

`crates/fenji-oodle/examples/write_inputs.rs`:
```rust
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
```

- [ ] **Step 3: The helper's `compress` and `probe` modes.**
  - **`dll.rs`:** resolve `OodleLZ_Compress` as `ooz` declares it (kraken.cpp:4288):
```rust
/// ooz's OodLZ_CompressFunc (kraken.cpp:4288): compressor, input, its length, output, level, then
/// options, dictionary, unused, scratch and its size, all zero (kraken.cpp:4375).
type Compress = unsafe extern "C" fn(
    compressor: i32,
    src: *const u8,
    src_len: usize,
    dst: *mut u8,
    level: i32,
    options: *const std::ffi::c_void,
    dictionary: usize,
    unused: usize,
    scratch: *mut std::ffi::c_void,
    scratch_size: usize,
) -> i32;
```
    Load it as an `Option<Compress>` (`lib.get(b"OodleLZ_Compress\0").ok()`). Add:
```rust
/// `input` compressed with `compressor` at `level`, or None if the DLL refused.
pub fn compress(&self, compressor: i32, level: i32, input: &[u8]) -> Option<Vec<u8>> {
    let f = self.compress?;
    // Oodle's worst case is the raw length plus a few hundred bytes per 256 KiB: this is ample.
    let mut out = vec![0u8; input.len() + input.len() / 8 + 65_536];
    // SAFETY: the arguments ooz passes (kraken.cpp:4375); `out` outlasts the call and is larger
    // than any output Oodle writes for this input.
    let n = unsafe {
        f(compressor, input.as_ptr(), input.len(), out.as_mut_ptr(), level,
          std::ptr::null(), 0, 0, std::ptr::null_mut(), 0)
    };
    let n = usize::try_from(n).ok().filter(|&n| n > 0 && n <= out.len())?;
    out.truncate(n);
    Some(out)
}
```
  - **`main.rs`:** keep the existing two-argument form (`oodle-helper DLL SPOOL`, the spool mode). Add:
    - `oodle-helper DLL probe`;
    - `oodle-helper DLL compress IN OUT COMPRESSOR LEVEL`: for each `IN/*.raw` it writes `OUT/<stem>.c<COMPRESSOR>.l<LEVEL>.oz`, and exits 1 if any fails.
  - **`run-oodle-helper.sh`:** add `--probe`, and `--compress IN OUT COMPRESSOR LEVEL`. Bind `IN` read-only and `OUT` read-write, both under `re/`. Pass the mode through.
  - **Its test,** test-first: the two new forms' argument checks, and the binds.

- [ ] **Step 4: Make the fixtures.**
  - `cargo run -p fenji-oodle --example write_inputs re/oodle-fixtures/in`.
  - For each compressor Task 2's census found, at levels 4 and 8: `scripts/spike/run-oodle-helper.sh --compress re/oodle-fixtures/in re/oodle-fixtures/out <compressor> <level>`.
  - Copy `re/oodle-fixtures/out/*.oz` to `crates/fenji-oodle/tests/fixtures/`.
  - Write `manifest.tsv`: `file\tkind\tsize\tseed\tcompressor\tlevel`, one line each.
  - **Check:** `du -sh crates/fenji-oodle/tests/fixtures` is at most 2 MiB. If it isn't, drop the largest random cases from `CASES` and make them again.

- [ ] **Step 5: The fixture test, which walks the fixtures and doesn't decode them yet.**

`crates/fenji-oodle/tests/common/mod.rs`, the loader every integration test shares:
```rust
//! The committed fixtures: Oodle streams the client's own DLL made from Fenji's synthetic input.
#![allow(dead_code)]
#[path = "../gen/mod.rs"]
pub mod gen;

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
            let kind = gen::Kind::from_name(f[1]).unwrap();
            let (size, seed): (usize, u64) = (f[2].parse().unwrap(), f[3].parse().unwrap());
            Fixture {
                file: f[0].to_string(),
                input: gen::input(kind, size, seed),
                stream: std::fs::read(dir.join(f[0])).unwrap(),
                compressor: f[4].parse().unwrap(),
            }
        })
        .collect()
}
```

`crates/fenji-oodle/tests/fixtures.rs`:
```rust
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
            assert!(c.mode == fenji_oodle::ChunkMode::Memset
                || c.mode == fenji_oodle::ChunkMode::Uncompressed
                || c.decoder == decoder_of(f.compressor), "{}", f.file);
            out += c.raw_len;
        }
        assert_eq!(out, f.input.len(), "{}", f.file);
    }
}
```
  - Run `cargo test -p fenji-oodle --test fixtures`. Expected: PASS. A failure here means the census's chunk walk and the DLL disagree, which is a finding: report it.

- [ ] **Step 6: The gate, the commit, and the PR.**
  - Commit as `test(data): Oodle fixtures from Fenji's synthetic data, made with the client's DLL`.
  - Stage by path: the generator, the example, the fixtures and manifest, `fixtures.rs`, the helper's two files, and the script and its test.
  - The PR body says the fixtures hold only Fenji's synthetic bytes, compressed. It ends with `Closes #<Task 3 issue>`.

---

## Task 4 [cloud]: The bit readers, the entropy decoders, and the first codec

The census's most common decoder goes first. The controller names it when dispatching: Kraken (6), Mermaid and Selkie (10), or Leviathan (12).

**Files:**
- Create: `crates/fenji-oodle/src/bits.rs`, `src/lz.rs`, `src/entropy/{mod,huffman,tans,rle,recursive,multi_array}.rs`, and `src/<codec>.rs`
- Modify: `src/lib.rs` (the `mod` lines), `src/stream.rs` (one arm in `decode_quantum`, and `a_compressed_quantum_needs_its_decoder` loses this decoder), `tests/fixtures.rs` (the decode test)

**Interfaces:**
- Consumes: `stream::decode_quantum`'s contract (Task 1), and the fixtures (Task 3).
- Produces:
  - `entropy::decode_bytes(src: &[u8], out: &mut [u8], force_copy: bool, scratch: &mut [u8]) -> Result<(usize, usize), Error>`, which returns the bytes read and the bytes decoded: a port of `Kraken_DecodeBytes`;
  - `lz::copy_match(dst: &mut [u8], pos: usize, offset: usize, len: usize) -> Result<(), Error>`;
  - `<codec>::decode_quantum(dst: &mut [u8], start: usize, src: &[u8], scratch: &mut Vec<u8>) -> Result<usize, Error>`.

**The port map** (kraken.cpp line numbers; port each function into the file named):
| `ooz` (lines) | Rust |
|---|---|
| `BitReader_*`, `CountLeadingZeros`, `Log2RoundUp`, `BSR`, `BSF` (201–436) | `bits.rs`: `BitReader` (forwards and backwards), `BitReader2`; `u32::leading_zeros` and `trailing_zeros` for BSR and BSF |
| `ReverseBitsArray2048`, `Kraken_DecodeBytesCore`, `Huff_ReadCodeLengthsOld`/`New`, `BitReader_ReadFluff`, `DecodeGolombRiceLengths`/`Bits`, `Huff_ConvertToRanges`, `FillByteOverflow16`, `Huff_MakeLut`, `Kraken_DecodeBytes_Type12` (574–1300) | `entropy/huffman.rs` |
| `Kraken_DecodeMultiArray` (1301–1517) | `entropy/multi_array.rs` |
| `Krak_DecodeRecursive` (1518–1555) | `entropy/recursive.rs` |
| `Krak_DecodeRLE` (1556–1642) | `entropy/rle.rs` |
| `SimpleSort`, `Tans_DecodeTable`, `Tans_InitLut`, `Tans_Decode`, `Krak_DecodeTans` (1643–2062) | `entropy/tans.rs` |
| `Kraken_GetBlockSize`, `Kraken_DecodeBytes` (2063–2208) | `entropy/mod.rs` |
| Kraken: `CombineScaledOffsetArrays`, `Kraken_UnpackOffsets`, `Kraken_ReadLzTable`, `Kraken_ProcessLzRuns_Type0`/`Type1`, `Kraken_ProcessLzRuns`, `Kraken_DecodeQuantum` (2209–2740) | `kraken.rs` |
| Mermaid/Selkie: `Mermaid_DecodeFarOffsets`, `Mermaid_CombineOffs16`, `Mermaid_ReadLzTable`, `Mermaid_Mode0`/`Mode1`, `Mermaid_ProcessLzRuns`, `Mermaid_DecodeQuantum` (3389–3997) | `mermaid.rs` |
| Leviathan: `Leviathan_ReadLzTable`, `Leviathan_ProcessLz`, `Leviathan_ProcessLzRuns`, `Leviathan_DecodeQuantum` (2741–3388) | `leviathan.rs` |

**The porting rules** (a reviewer checks each):
1. **Pointers become slices and indices.**
   - `memcpy` and `memmove` become `copy_from_slice` and `copy_within`.
   - Each `src < src_end`-style check stays, and every read past the end the C code would make is `Error::Truncated` or `Error::Corrupt`.
   - `ooz`'s over-reads for speed (64-byte copies, 4-byte refills) become exact reads. The bit reader yields zeros past the end, which is what the C code's padded buffers give; a stream that relies on more is corrupt.
2. **Every match goes through `lz::copy_match`.** It refuses an offset of 0 or beyond `pos` (Review Focus 4). It copies byte by byte when `offset < len` (overlapping), and with `copy_within` otherwise. The SIMD macros become scalar code, and `COPY_64_ADD` becomes a byte-wise `wrapping_add` over the 8 bytes.
3. **Arithmetic:**
   - `wrapping_*` only where the C code's unsigned arithmetic wraps on purpose (bit buffers);
   - `checked_*`, failing to `Error::Corrupt`, where a wrap would mean corrupt data (sizes, counts, offsets);
   - C's `int` returns of −1 for failure become `Err`.
4. **Each ported function's doc comment** names its source with a line number.
5. **No `unsafe`,** no new dependencies, and no allocation larger than the quantum's output or `SCRATCH`.

- [ ] **Step 1: The failing tests.**
  - In `tests/fixtures.rs`:
```rust
/// The decoders ported so far. Tasks 5 and 6 add theirs.
const PORTED: &[u8] = &[/* Task 4's decoder, for example 6 */];

#[test]
fn every_fixture_of_a_ported_codec_decodes_to_its_input() {
    let mut decoded = 0;
    for f in fixtures().iter().filter(|f| PORTED.contains(&decoder_of(f.compressor))) {
        let got = fenji_oodle::decompress(&f.stream, f.input.len())
            .unwrap_or_else(|e| panic!("{}: {e}", f.file));
        assert!(got == f.input, "{}: decoded bytes differ", f.file);
        decoded += 1;
    }
    assert!(decoded > 0, "no fixture for the ported codecs");
}
```
  - In the codec module's tests:
```rust
#[test]
fn a_match_before_the_start_is_corrupt() {
    // Review Focus 4, through the shared helper every codec uses.
    let mut dst = vec![0u8; 16];
    assert!(matches!(crate::lz::copy_match(&mut dst, 4, 5, 3), Err(crate::Error::Corrupt { .. })));
    assert!(matches!(crate::lz::copy_match(&mut dst, 4, 0, 3), Err(crate::Error::Corrupt { .. })));
    crate::lz::copy_match(&mut dst, 4, 2, 6).unwrap(); // overlapping: repeats dst[2..4]
}
```
  - Run `cargo test -p fenji-oodle`. Expected: FAIL, because `lz` doesn't exist and the fixtures' decode is `Unsupported`.

- [ ] **Step 2: `lz.rs`.**
```rust
use crate::Error;

/// Copies `len` bytes from `offset` back, to `pos`. Overlapping copies repeat bytes as LZ needs.
pub(crate) fn copy_match(dst: &mut [u8], pos: usize, offset: usize, len: usize) -> Result<(), Error> {
    if offset == 0 || offset > pos {
        return Err(Error::Corrupt { at: pos, what: "a match reaches before the start of the output" });
    }
    let end = pos.checked_add(len).filter(|&e| e <= dst.len())
        .ok_or(Error::Corrupt { at: pos, what: "a match runs past the quantum's output" })?;
    if offset >= len {
        dst.copy_within(pos - offset..pos - offset + len, pos);
    } else {
        for i in pos..end {
            dst[i] = dst[i - offset];
        }
    }
    Ok(())
}
```

- [ ] **Step 3: Port the bit readers and the entropy decoders** into `bits.rs` and `entropy/`, by the map and the rules. Unit-test the bit readers against hand-made bit strings: a known byte sequence's gamma codes, distances and lengths, computed by hand from the C code's arithmetic.

- [ ] **Step 4: Port the codec** into `<codec>.rs`, and add its arm in `stream::decode_quantum`: `6 => crate::kraken::decode_quantum(dst, start, src, scratch),`, or 10 or 12. Remove its decoder from `a_compressed_quantum_needs_its_decoder`'s list.

- [ ] **Step 5: Watch them pass.** Run `cargo test -p fenji-oodle`. Expected: PASS, every fixture of this codec, at both levels, byte for byte. A fixture that fails points at a port difference: fix the port, and never the fixture.

- [ ] **Step 6: The gate, the commit, and the PR.** Commit as `feat(data): decode Oodle <Codec> in fenji-oodle`, with `Closes #<Task 4 issue>`.

---

## Task 5 [cloud]: The second codec (only if the census finds one)

The same as Task 4, Steps 1, 2 and 4–6, for the census's second decoder, with `bits.rs`, `entropy/` and `lz.rs` already in place.
- Add the decoder to `PORTED`, and remove it from `a_compressed_quantum_needs_its_decoder`.
- Add `a_match_before_the_start_is_corrupt`'s case to the codec's own tests.
- Commit as `feat(data): decode Oodle <Codec> in fenji-oodle`.

## Task 6 [cloud]: The third codec (only if the census finds one)

As Task 5, for the third.

---

## Task 7 [cloud]: `fenji-data`, inline decoding in `archive`, and `verify-oodle`

**Files:**
- Modify:
  - `crates/fenji-data/{Cargo.toml,src/codec.rs,src/error.rs}`;
  - `tools/archive/src/main.rs`;
  - `tools/oodle-helper/{Cargo.toml,src/main.rs}`;
  - `scripts/spike/run-oodle-helper.sh` and its test.
- Test: `crates/fenji-data/src/codec.rs`'s tests; the tests in `tools/archive/src/main.rs`.

**Interfaces:**
- Consumes: `fenji_oodle::decompress`, and `fenji_data::{oodle_payload, Error::Oodle, blocks}` (Task 2).
- Produces:
  - `BuiltinCodecs` decodes `Codec::Oodle`;
  - `archive verify-oodle --dll-hashes FILE --out DIR`;
  - `oodle-helper DLL hash-parts CLIENT OUT_TSV`, whose lines are `key<TAB>block<TAB>raw_len<TAB>sha256`.

- [ ] **Step 1: The failing tests,** in `codec.rs`:
```rust
#[test]
fn builtin_codecs_decode_oodle() {
    // A memset stream (fenji-oodle's own format test), behind the archive's raw-length prefix.
    let mut block = 1000u32.to_le_bytes().to_vec();
    block.extend_from_slice(&[0x8C, 0x06, 0x07, 0xFF, 0xFF, 0x2A]);
    assert_eq!(BuiltinCodecs.decompress_block(Codec::Oodle, &block, 1000, 0).unwrap(), vec![0x2A; 1000]);
}

#[test]
fn oversized_oodle_blocks_are_refused() {
    // Review Focus 5: a tampered header can't make the decoder allocate more than a block.
    let mut block = (600 * 1024u32).to_le_bytes().to_vec();
    block.extend_from_slice(&[0x8C, 0x06, 0x07, 0xFF, 0xFF, 0x2A]);
    let e = BuiltinCodecs.decompress_block(Codec::Oodle, &block, 600 * 1024, 0).unwrap_err();
    assert!(matches!(e, Error::Oodle { .. }), "{e}");
}
```
  Run `cargo test -p fenji-data`. Expected: FAIL, because Oodle is still `CodecUnavailable`.

- [ ] **Step 2: Make them pass.**
  - In `crates/fenji-data/Cargo.toml`, add `fenji-oodle = { path = "../fenji-oodle" }`.
  - In `BuiltinCodecs::decompress_block`, add, before `other =>`:
```rust
            Codec::Oodle => {
                // The engine's blocks are at most 512 KiB (m1-archives.md 2.4, 3.1).
                const MAX_BLOCK: usize = 512 * 1024;
                if raw_len > MAX_BLOCK {
                    return Err(Error::Oodle { index, reason: format!("{raw_len} raw bytes, more than a block holds") });
                }
                let stream = crate::oodle_payload(input, raw_len, index)?;
                fenji_oodle::decompress(stream, raw_len)
                    .map_err(|e| Error::Oodle { index, reason: e.to_string() })?
            }
```
  - Change `CodecUnavailable`'s message to `"codec {0} isn't one Fenji decodes"`.
  - Run `cargo test -p fenji-data`. Expected: PASS.

- [ ] **Step 3: Inline decoding in `archive`.**
  - `extract`, `paths` and `dump` decode with `BuiltinCodecs` by default.
  - `--spool DIR` becomes an `Option<PathBuf>`. When it's given, they keep today's route through `OodleSpool` and `decode_spooled`. When it isn't, they never spool, and never exit 3.
  - Update their doc comments: drop "exit 3 means they wait for the helper", except where `--spool` is given.

- [ ] **Step 4: `oodle-helper hash-parts`.**
  - In `tools/oodle-helper/Cargo.toml`, add `fenji-data = { path = "../../crates/fenji-data" }`. The crate is pure Rust, so it builds for the Windows target.
  - In `main.rs`, `oodle-helper DLL hash-parts CLIENT OUT_TSV` walks `CLIENT/data/data*` as `archive`'s `live_places` does: the first live record per key, codec Oodle.
  - For each block, it calls `oodle_payload`, decodes with the DLL, and writes `hex(key)\tindex\traw_len\tsha256(output)`, or `hex(key)\tindex\traw_len\tFAILED`.
  - It prints the totals, and exits 0. A FAILED line is data, not a helper failure.
  - **`run-oodle-helper.sh`:** add `--hash-parts OUT_TSV`. It binds the client's `data/` read-only and the TSV's directory (under `re/`) read-write, and runs the helper. Test it first, as in Task 3.

- [ ] **Step 5: `archive verify-oodle`.** The subcommand is `VerifyOodle { #[arg(long)] dll_hashes: PathBuf, #[arg(long, default_value = "re/oodle-verify")] out: PathBuf }`, dispatched to:
```rust
fn verify_oodle(client: &Path, dll_hashes: &Path, out: &Path) -> Result<ExitCode> {
    use fenji_data::Decompressor;
    use sha2::{Digest, Sha256};
    let repo = fenji_data::re_dir::find_repo_root(&std::env::current_dir()?)?;
    let out = fenji_data::re_dir::resolve_out_dir(&repo, out)?;
    let dll = std::fs::read_to_string(dll_hashes).with_context(|| dll_hashes.display().to_string())?;
    let mut want: HashMap<(String, usize), String> = HashMap::new();
    for line in dll.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if let [key, index, _raw, sha] = f[..] {
            want.insert((key.to_string(), index.parse()?), sha.to_string());
        }
    }
    let places = live_places(client)?;
    let (mut same, mut differ, mut missing, mut native_failed) = (0u64, 0u64, 0u64, 0u64);
    let mut report = String::from("key\tblock\tproblem\n");
    let started = std::time::Instant::now();
    let mut raw_bytes = 0u64;
    for (key, p) in places.by_key.iter().filter(|(_, p)| p.codec == "oodle") {
        let blob = read_record(client, p)?;
        let (_, blocks) = fenji_data::blocks(&blob, 0)?;
        for b in blocks {
            let k = (hex(key), b.index);
            let native = fenji_data::BuiltinCodecs.decompress_block(fenji_data::Codec::Oodle, b.input, b.raw_len, b.index);
            match (native, want.get(&k)) {
                (_, None) => { missing += 1; report.push_str(&format!("{}\t{}\tno DLL hash\n", k.0, k.1)); }
                (Err(e), Some(_)) => { native_failed += 1; report.push_str(&format!("{}\t{}\tnative: {e}\n", k.0, k.1)); }
                (Ok(raw), Some(sha)) => {
                    raw_bytes += raw.len() as u64;
                    if hex(&Sha256::digest(&raw)) == *sha { same += 1 } else {
                        differ += 1;
                        report.push_str(&format!("{}\t{}\tbytes differ\n", k.0, k.1));
                    }
                }
            }
        }
    }
    let secs = started.elapsed().as_secs_f64();
    std::fs::write(out.join("mismatches.tsv"), &report)?;
    println!("{same} blocks match, {differ} differ, {native_failed} failed natively, {missing} have no DLL hash; {:.1} MB decoded in {secs:.0} s",
             raw_bytes as f64 / 1e6);
    Ok(if differ + native_failed + missing == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}
```
  - Add `sha2` to `tools/archive/Cargo.toml` from the workspace if it isn't there.
  - **The test first,** in `main.rs`'s tests, with `fenji_data::test_util` blobs whose blocks are memset streams: a matching hash gives SUCCESS; a changed hash gives FAILURE, with the key in `mismatches.tsv`; a missing hash gives FAILURE.

- [ ] **Step 6: The gate, the commit, and the PR.** Commit as `feat(data): decode Oodle archives natively, and verify them against the client's DLL`, with `Closes #<Task 7 issue>`.

---

## Task 8 [local]: The full check on the owner's game box

**Files:**
- Modify: `docs/re/m1-archives.md` (§5, and the evidence table)

- [ ] **Step 1: The DLL's hashes.** Run `source ~/.config/fenji/env.sh && scripts/spike/run-oodle-helper.sh --hash-parts re/oodle-verify/dll.tsv`, inside the sandbox. It writes one line per Oodle block. Its FAILED lines are counted, and are a finding if there are any.
- [ ] **Step 2: The comparison.** Run `cargo run --release -p fenji-archive -- verify-oodle --dll-hashes re/oodle-verify/dll.tsv`.
  - **Expected:** every block matches; 0 differ, 0 failed natively, 0 have no DLL hash. The exit status is 0.
  - **If anything differs,** it's a port bug. Report the keys and blocks (they're in `re/oodle-verify/mismatches.tsv`), and the controller opens a fix task on the codec. The bar is zero.
- [ ] **Step 3: Record it** in `m1-archives.md` §5, "What Oodle unlocked":
  - the counts, the decoded megabytes, the wall time and the throughput, tagged **Observed**, with the evidence key `AT` (`re/oodle-verify/`);
  - the containers' files, per Task 2's finding.
- [ ] **Step 4: The commit and the PR.** Commit as `docs(re): every Oodle block in the pinned client decodes natively, matching its DLL`, with `Closes #<Task 8 issue>`.

---

## Task 9 [cloud]: Robustness, and the fuzz target

**Files:**
- Create: `crates/fenji-oodle/tests/robustness.rs`, `crates/fenji-oodle/fuzz/{Cargo.toml,fuzz_targets/decompress.rs}`
- Modify: the root `Cargo.toml` (`exclude`)

- [ ] **Step 1: The tests.** Each must hold before and after; none needs a code change unless it fails, in which case it has found a panic or an unbounded read.
```rust
//! Bad input is an error or a correct length, never a panic (Review Focus 2–4).
mod common;
use common::{fixtures, gen};

fn check(src: &[u8], raw_len: usize, what: &str) {
    match fenji_oodle::decompress(src, raw_len) {
        Ok(out) => assert_eq!(out.len(), raw_len, "{what}"),
        Err(_) => {}
    }
}

#[test]
fn cut_and_flipped_fixtures_never_panic() {
    let mut r = gen::Rng::new(0xF00D);
    for f in fixtures() {
        let n = f.input.len();
        for cut in (0..f.stream.len()).step_by((f.stream.len() / 64).max(1)) {
            check(&f.stream[..cut], n, &format!("{} cut at {cut}", f.file));
        }
        for _ in 0..256 {
            let mut s = f.stream.clone();
            let i = (r.next() as usize) % s.len();
            s[i] ^= 1 << (r.next() % 8);
            check(&s, n, &format!("{} flipped at {i}", f.file));
        }
    }
}

#[test]
fn random_streams_behind_a_valid_header_never_panic() {
    let mut r = gen::Rng::new(0xBEEF);
    for _ in 0..10_000 {
        let len = 1 + (r.next() as usize) % 4096;
        let mut s = vec![0x8C, [6u8, 10, 12][(r.next() % 3) as usize]];
        s.extend((0..len).map(|_| r.next() as u8));
        check(&s, 1 + (r.next() as usize) % 300_000, "random");
    }
}
```
  - Run `cargo test -p fenji-oodle --release --test robustness`. Expected: PASS, in under a minute.

- [ ] **Step 2: The fuzz target, outside the workspace.**
  - In the root `Cargo.toml`'s `[workspace]`, add `exclude = ["crates/fenji-oodle/fuzz"]`.
  - `crates/fenji-oodle/fuzz/Cargo.toml`:
```toml
[package]
name = "fenji-oodle-fuzz"
version = "0.0.0"
publish = false
edition = "2024"
license = "GPL-3.0-or-later"

[package.metadata]
cargo-fuzz = true

[dependencies]
libfuzzer-sys = "0.4"
fenji-oodle = { path = ".." }

[[bin]]
name = "decompress"
path = "fuzz_targets/decompress.rs"
test = false
doc = false
```
  - `fuzz_targets/decompress.rs`:
```rust
#![no_main]
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Some((len, stream)) = data.split_first_chunk::<4>() {
        let raw_len = u32::from_le_bytes(*len) as usize % (512 * 1024 + 1);
        if let Ok(out) = fenji_oodle::decompress(stream, raw_len) {
            assert_eq!(out.len(), raw_len);
        }
    }
});
```
  - Add `crates/fenji-oodle/fuzz/{target,corpus,artifacts}/` to `.gitignore`.
  - In `crates/fenji-oodle/fuzz/README.md`, the run: `cargo +nightly fuzz run decompress`, seeded from the fixtures, locally and never in CI.

- [ ] **Step 3: The gate, the commit, and the PR.** Commit as `test(data): bad Oodle input is an error, never a panic; and a fuzz target`, with `Closes #<Task 9 issue>`.

---

## Self-review notes (the plan's author)

- **Spec coverage:**
  - §3.1: Tasks 1 and 4–6.
  - §3.2: Tasks 2 and 7.
  - §3.3: Tasks 2 and 7, with Correction 2.
  - §3.4: Task 3.
  - §4.1: Task 2.
  - §4.2: Task 3.
  - §4.3: Tasks 7 and 8.
  - §4.4: Tasks 1, 4–6 and 9.
  - §4.5: Task 8's throughput line.
  - §5: Task 1 (`sources.md`, `COPYING`, `NOTICE`).
  - §6: this table.
  - §7: the porting rules and Task 8.
- **Names used across tasks:** `decode_quantum`, `oodle_payload`, `Error::Oodle`, `blocks`, `chunks`, `ChunkMode`, `QUANTUM`, `SCRATCH`, `copy_match`, `fixtures()` and `decoder_of`. They match their definitions.
- **Where the plan can't give finished code:** Tasks 4–6 are a port of about 3,500 lines of C. They give the exact source (a pinned commit and line ranges), the Rust layout, the rules a reviewer checks, and tests that decide.
