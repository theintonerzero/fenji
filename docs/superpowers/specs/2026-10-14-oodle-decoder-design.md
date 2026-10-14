# The open Oodle decoder — Design Spec

**Status:** designed with the owner on 2026-10-14. The owner chose the decoder alone as this piece's scope, and a safe Rust port of `ooz` as the approach. The owner approved each design section ("looks right", "continue"), including fixtures made with the client's own DLL (§4.2). This is the written form, for the owner's review.

**Parents:**
- the owner's decision of 2026-10-14, #225: decompress the game's Oodle data with an open-source decompressor; Epic's own Oodle library stays dev-only, inside the sandbox;
- the [Fenji design spec](2026-10-11-fenji-design.md), §2 (ground rules 3 and 6) and §6.3 (data);
- [`m1-archives.md`](../../re/m1-archives.md), and [`m2-summary.md`](../../re/m2-summary.md) (gate G3: the client's DLL for archives, dev analysis only, in the sandbox).

## 1. Purpose

`fenji-data` decodes every Oodle-compressed file in the pinned client natively. It runs on Linux without Wine, and gets the same bytes as the client's own Oodle DLL. With it, Fenji's tools can read game data from a host's own install now, and the server can later.

**Success:**
1. `archive verify-oodle`, run on the owner's game box, finds zero mismatches. It covers every Oodle block of every live Oodle file in the pinned client's 100 parts, and of the files inside its containers.
2. `archive extract`, `archive paths` and the container listing decode inline, with no spool and no "exit 3: blocks wait for the helper".
3. The decoder is safe Rust, the workspace's `unsafe_code = "forbid"` still holds, and it adds no external dependency.
4. CI runs the decoder against committed synthetic fixtures. Bad input gives an error, never a panic.

**Not in this piece:**
- compression;
- the LZNA, Bitknit and Hydra decoders, and any codec §4.1's census doesn't find in the pinned client;
- server features that read game data, such as outfits, NPCs and scenes: each gets its own piece;
- the network, which needs no Oodle: stored packets pass the client's decoder ([`transport.md`](../../protocol/transport.md) 5.8).

## 2. Background

- **The archives** ([`m1-archives.md`](../../re/m1-archives.md) §2–§3):
  - Of the client's 224,032 live files, 209,595 are Oodle (codec 4) and 14,437 are stored.
  - Each blob has a 32-byte header, a list of block sizes, then blocks of up to 512 KiB.
  - Each Oodle block starts with its raw length, as a little-endian `u32`, followed by the Oodle stream (3.4).
  - The exe's four Oodle codecs (Kraken, Mermaid, Selkie and Leviathan) share one decompressor (`FUN_14024E060`; G5).
- **Today:**
  - `fenji_data::Decompressor` (`crates/fenji-data/src/codec.rs`) decodes stored and LZ4 blocks.
  - Oodle blocks go to an `OodleSpool`, which `tools/oodle-helper` decodes with the client's own DLL, under Wine, in the sandbox (gate G3, dev only).
- **`ooz`** (github.com/powzix/ooz):
  - Its `kraken.cpp` begins "Copyright (C) 2016, Powzix", under the GNU General Public License, "either version 3 of the License, or (at your option) any later version": GPL-3.0-or-later.
  - It decodes Kraken, Mermaid, Selkie, Leviathan, LZNA and Bitknit, and compresses only through Epic's DLL.
  - Its last change was in 2019.

## 3. Design

### 3.1 The crate `fenji-oodle`

- **What it is:** `crates/fenji-oodle`, a Rust port of `ooz`'s decoders.
  - `license = "GPL-3.0-or-later"` and `publish = false`.
  - The crate holds the GPL-3.0 text (`COPYING`) and a `NOTICE`.
  - Every source file's header says it is a port of `ooz`'s `kraken.cpp`, © 2016 Powzix, with changes © the Fenji contributors, under GPL-3.0-or-later.
  - Fenji as a whole stays AGPL-3.0-or-later. The two licences combine, by section 13 of each.
- **Its API:**
  - `pub fn decompress(input: &[u8], raw_len: usize) -> Result<Vec<u8>, Error>`: one Oodle stream to exactly `raw_len` bytes.
  - `pub fn chunks(input: &[u8]) -> impl Iterator<Item = Result<ChunkInfo, Error>>`: each chunk's decoder type, mode (compressed, uncompressed, memset) and sizes, read from the headers alone, for the census.
  - `pub enum Error`: input cut short, a bad stream, chunk or quantum header, an unknown or unported decoder (with its number), an invalid entropy table, or output that overruns or underruns `raw_len`.
- **Inside:**
  - the stream, chunk and quantum headers;
  - the entropy decoders the codecs share: Huffman, tANS, RLE, and the recursive and multi-array modes, as `ooz` has them;
  - one LZ decoder per codec the census finds: `kraken`, `mermaid` (which covers Selkie) and `leviathan`.
- **Rules:**
  - safe Rust only, with checked arithmetic;
  - every length is checked against the input left before it's used;
  - the output is exactly `raw_len`, and scratch space is bounded by the chunk size;
  - no external dependency.

### 3.2 `fenji-data`

- `BuiltinCodecs` decodes `Codec::Oodle`:
  - It reads the block's `u32` raw length, and checks that it equals the raw length the blob header gives that block. That is at most the blob's block size, 512 KiB.
  - Then it calls `fenji_oodle::decompress` on the rest.
  - Errors become `Error::Oodle { index, reason }`.
- `OodleSpool` stays, as the dev-only oracle for §4.3.

### 3.3 `tools/archive`

- `extract`, `paths` and the container listing decode inline by default. `--spool DIR` keeps the old route, for cross-checks.
- `census-oodle` reads every Oodle block's chunk headers, and decodes nothing. It counts chunks by decoder type and mode, and writes the counts under `re/`.
- `verify-oodle` hashes every Oodle block's output twice, with SHA-256:
  - once decoded natively;
  - once decoded by the client's DLL, through the spool and `scripts/spike/run-oodle-helper.sh`, in the sandbox.

  It reports the counts, and each mismatch by key and block index, under `re/`. Decoded data never leaves `re/`.

### 3.4 `oodle-helper`

- A new `compress` mode, dev-only, in the sandbox. It compresses §4.2's synthetic inputs with the client's own DLL, for each codec the census found, at a fast level and at an optimal one (the levels that pick different entropy modes), and writes the fixtures.

## 4. Testing

### 4.1 The census first
`archive census-oodle` runs before any codec is ported. Its counts go in `m1-archives.md`, and decide which codecs §3.1 ports.

### 4.2 Fixtures: synthetic data only
- **The inputs:** a seeded generator in the crate's tests makes them: text, zeros, random bytes, repeats and mixes. Their sizes sit around the 256 KiB chunk and 512 KiB block edges.
- **The fixtures:** `oodle-helper compress` compresses each input once. The compressed bytes are committed as `.hex` files under `crates/fenji-oodle/tests/fixtures/`, with a manifest: the seed, kind, size, codec, level, and the SHA-256 of the input and of the output.
- **The tests** regenerate each input from its seed, decode its fixture, and compare.
- **The owner's approval:** on 2026-10-14 the owner approved using Epic's DLL to make these committed files. They hold only Fenji's own data, compressed; no Epic code and no game data.

### 4.3 The real check
On the owner's game box, `archive verify-oodle` decodes every Oodle block both ways and compares them. Zero mismatches is the bar for this piece. Its counts and throughput go in `m1-archives.md`.

### 4.4 Robustness
- **A stable test with fixed seeds** feeds every fixture truncated at each chunk edge and at random points, bit-flipped, and replaced with random bytes. Each must give an `Err`, or an `Ok` of exactly `raw_len`. Never a panic, and never memory beyond the bound in §3.1.
- **A `cargo-fuzz` target** (`crates/fenji-oodle/fuzz/`) is there for longer runs on nightly Rust, locally, outside CI.

### 4.5 Speed
Measured in release builds, on `_index` and on the full run, and recorded. It isn't a gate.

## 5. Licensing and provenance

- `docs/re/sources.md` gains `ooz`'s row:
  - licence: GPL-3.0-or-later;
  - provenance: a public decompressor its author wrote from Oodle, with nothing SoLO-specific;
  - use: ported in `fenji-oodle`.
- `deny.toml` doesn't change: Fenji's own crates are private, and the port adds no dependency.
- No game data goes in the repo. The fixtures are synthetic, and decoded data stays in `re/`.
- Epic's DLL stays dev-only, in the sandbox. Its two uses here are the §4.3 check and the §4.2 fixtures.

## 6. Delivery

Its own plan, with tasks as issues under the M5 milestone, merged into `dev` as each is done. The order, with each task's lane:
1. **The census** (local, needs the client).
2. **The fixtures:** the generator, `oodle-helper compress`, and the committed `.hex` files (local, the sandbox and the DLL).
3. **The stream, chunk and quantum headers, and the entropy decoders** (repository only).
4. **The codecs the census finds** (repository only).
5. **`fenji-data`'s integration,** inline decoding in `archive`, and `census-oodle` and `verify-oodle` (repository only).
6. **The full check** on the owner's game box: zero mismatches (local).
7. **Robustness tests, the fuzz target,** and the licence docs (repository only).

At most two agents run at once.

## 7. Risks

- **Port bugs:** §4.3's check over every block, and §4.4's tests.
- **A rare mode the fixtures don't reach,** such as one of Leviathan's: the census names the modes in use, the port covers every mode of each codec it ports, and §4.3 reaches every block.
- **`ooz`'s own bugs, if it differs from the DLL:** §4.3 finds them, and the fix goes in the port.
- **A slow decoder:** measured, and optimised later if it matters.
- **Legal:** the formats are decoded with a GPL reimplementation, and Epic's DLL never leaves dev use in the sandbox (#225).
