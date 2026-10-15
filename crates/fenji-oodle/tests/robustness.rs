//! Part of fenji-oodle, a Rust port of the Oodle decoders in ooz's `kraken.cpp`
//! (github.com/powzix/ooz, commit 05038060aa68), Copyright (C) 2016 Powzix.
//! Changes Copyright (C) 2026 the Fenji contributors.
//! GPL-3.0-or-later: see COPYING and NOTICE.
//!
//! Bad input is an error or a correct length, never a panic (Review Focus 2–4, spec §4.4). Every
//! fixture is cut short at its edges and at random points, its quanta are cut short, and its bits
//! and bytes are changed at random; random data goes behind valid headers; a raw length asks for
//! more than its stream holds; and blocks nest without end. Safe Rust panics at an index out of
//! bounds, so no panic is also no read outside the input. The seeds are fixed; `fuzz/` runs the
//! same check for as long as it's given, locally (its README).
mod common;
use std::ops::Range;

use common::{Fixture, fixtures, generator};
use fenji_oodle::{ChunkMode, Error, QUANTUM};

/// Output bytes per Kraken chunk (kraken.rs's `CHUNK`): two to a quantum.
const CHUNK: usize = 0x20000;

/// Decodes `src` to `raw_len` bytes: an error, or exactly `raw_len` bytes. The header walk reads
/// the headers the decode reads, so where the decode succeeds, the walk does too.
fn check(src: &[u8], raw_len: usize, what: &str) -> Result<Vec<u8>, Error> {
    let got = fenji_oodle::decompress(src, raw_len);
    let walked: Result<usize, Error> = fenji_oodle::chunks(src, raw_len)
        .map(|c| c.map(|c| c.raw_len))
        .sum();
    if let Ok(out) = &got {
        assert_eq!(out.len(), raw_len, "{what}");
        assert_eq!(walked, Ok(raw_len), "{what}: the walk");
    }
    got
}

/// Runs `test(r, i)` for each `i` below `n`, spread over 8 threads, each with a generator of its
/// own seeded from `seed` and its number, so that every run is the same.
fn in_parallel(n: usize, seed: u64, test: impl Fn(&mut generator::Rng, usize) + Sync) {
    const THREADS: usize = 8;
    std::thread::scope(|s| {
        for t in 0..THREADS {
            let test = &test;
            s.spawn(move || {
                let mut r = generator::Rng::new(seed + t as u64);
                for i in (t..n).step_by(THREADS) {
                    test(&mut r, i);
                }
            });
        }
    });
}

/// Runs `test` on every fixture, as [`in_parallel`] does.
fn each_fixture(seed: u64, test: impl Fn(&Fixture, &mut generator::Rng) + Sync) {
    let all = fixtures();
    in_parallel(all.len(), seed, |r, i| test(&all[i], r));
}

/// A number below `n`, which is at least 1.
fn below(r: &mut generator::Rng, n: usize) -> usize {
    (r.next() % n as u64) as usize
}

fn random_bytes(r: &mut generator::Rng, n: usize) -> Vec<u8> {
    (0..n).map(|_| r.next() as u8).collect()
}

/// One quantum of a fixture's stream: where its stream header and its quantum header start, its
/// data after them, and, if it's compressed, where each Kraken chunk starts in that data.
struct Quantum {
    start: usize,
    header: usize,
    data: Range<usize>,
    mode: ChunkMode,
    chunks: Vec<usize>,
}

/// `f`'s quanta, from walking its headers as the decoder does.
fn quanta(f: &Fixture) -> Vec<Quantum> {
    let mut all = Vec::new();
    let mut at = 0;
    for c in fenji_oodle::chunks(&f.stream, f.input.len()) {
        let c = c.unwrap_or_else(|e| panic!("{}: {e}", f.file));
        // The stream header's 2 bytes, then the quantum header's 3, 6 with a checksum, or 4 for a
        // memset. An uncompressed quantum has none.
        let start = at;
        let header = start + 2;
        let data = match c.mode {
            ChunkMode::Uncompressed => header..header + c.raw_len,
            ChunkMode::Memset => header + 4..header + 4,
            ChunkMode::Stored | ChunkMode::Compressed => {
                let data = header + if c.checksums { 6 } else { 3 };
                data..data + c.compressed_len
            }
        };
        let chunks = if c.mode == ChunkMode::Compressed {
            kraken_chunks(&f.stream[data.clone()], c.raw_len)
        } else {
            Vec::new()
        };
        at = data.end;
        all.push(Quantum {
            start,
            header,
            data,
            mode: c.mode,
            chunks,
        });
    }
    assert_eq!(at, f.stream.len(), "{}: the quanta fill the stream", f.file);
    all
}

/// Where each Kraken chunk of a compressed quantum's `data` starts. A chunk holds 128 KiB of the
/// quantum's `raw_len`, or what's left, and its header gives its length (kraken.rs's
/// `decode_chunk`): an LZ or stored chunk's 19-bit size, or an entropy-coded chunk's block header,
/// a stored block's 18-bit size or another kind's long header (entropy/mod.rs's `header`).
fn kraken_chunks(data: &[u8], raw_len: usize) -> Vec<usize> {
    let mut starts = Vec::new();
    let (mut at, mut out) = (0, 0);
    while out < raw_len {
        starts.push(at);
        let b: [usize; 5] =
            std::array::from_fn(|i| usize::from(data.get(at + i).copied().unwrap_or(0)));
        let v = b[0] << 16 | b[1] << 8 | b[2];
        at += if v & 0x80_0000 != 0 {
            3 + (v & 0x7_FFFF)
        } else if b[0] >> 4 == 0 {
            3 + v
        } else {
            5 + ((b[2] & 3) << 16 | b[3] << 8 | b[4])
        };
        out += CHUNK.min(raw_len - out);
    }
    assert_eq!(at, data.len(), "the chunks fill their quantum");
    starts
}

/// The edges in `f`'s stream: where each quantum, quantum header, quantum's data and Kraken chunk
/// starts, and where the stream ends.
fn edges(f: &Fixture) -> Vec<usize> {
    let mut edges = vec![f.stream.len()];
    for q in quanta(f) {
        edges.extend([q.start, q.header, q.data.start]);
        edges.extend(q.chunks.iter().map(|c| q.data.start + c));
    }
    edges.sort_unstable();
    edges.dedup();
    edges
}

/// A quantum header for `size` bytes of data: the 18-bit field holds size − 1.
fn quantum_header(size: usize) -> [u8; 3] {
    let v = size - 1;
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

/// An entropy block's long header (entropy/mod.rs's `header`): `kind`, `src_size` bytes of data in
/// and `dst_size` out. The first byte's top bit is clear, as an entropy-coded chunk's must be.
fn block_header(kind: u8, src_size: usize, dst_size: usize) -> [u8; 5] {
    let bits = ((dst_size - 1) & 0x3FFF) << 18 | src_size;
    [
        kind << 4 | ((dst_size - 1) >> 14) as u8,
        (bits >> 24) as u8,
        (bits >> 16) as u8,
        (bits >> 8) as u8,
        bits as u8,
    ]
}

/// A stored entropy block of `data`, behind the 3-byte header whose top bit is clear.
fn stored(data: &[u8]) -> Vec<u8> {
    let n = data.len();
    [&[(n >> 16) as u8, (n >> 8) as u8, n as u8][..], data].concat()
}

/// An entropy-coded chunk of a random kind, 1 to 7 (6 and 7 are reserved): random bytes behind a
/// valid block header. With the output length it's for.
fn entropy_chunk(r: &mut generator::Rng) -> (Vec<u8>, usize) {
    let n = 1 + below(r, 2048);
    let data = random_bytes(r, n);
    let raw_len = n + 1 + below(r, 8192);
    let kind = 1 + below(r, 7) as u8;
    let mut chunk = block_header(kind, n, raw_len).to_vec();
    chunk.extend(data);
    (chunk, raw_len)
}

/// An LZ chunk at the start of the output, of random streams that read no bits: 8 bytes as they
/// are; stored blocks of literals, of commands, of offsets coded the new way at a random scale
/// (packed values under 8 read no bits), and of lengths (under 255, none either); then the one
/// byte of bits where the two bit readers meet, as kraken.rs's tests build theirs. The commands
/// are random, so most chunks run a match or literals that don't fit. With the output length.
fn lz_chunk(r: &mut generator::Rng) -> (Vec<u8>, usize) {
    let raw_len = 512 + below(r, 8192);
    let mut body = random_bytes(r, 8);
    let n = below(r, raw_len / 2);
    body.extend(stored(&random_bytes(r, n)));
    let n = below(r, 64);
    let commands = random_bytes(r, n);
    body.extend(stored(&commands));
    let scale = 1 + below(r, 8) as u8;
    body.push(0x7F + scale);
    let n = below(r, commands.len() + 1);
    let offsets: Vec<u8> = (0..n).map(|_| below(r, 8) as u8).collect();
    body.extend(stored(&offsets));
    if scale != 1 {
        body.extend(stored(&random_bytes(r, n)));
    }
    let n = below(r, 16);
    let lengths: Vec<u8> = (0..n).map(|_| below(r, 255) as u8).collect();
    body.extend(stored(&lengths));
    body.push(0x80);
    let (mode, len) = (below(r, 2) as u8, body.len());
    let mut chunk = vec![
        0x80 | mode << 3 | (len >> 16) as u8,
        (len >> 8) as u8,
        len as u8,
    ];
    chunk.extend(body);
    (chunk, raw_len)
}

#[test]
fn every_fixture_cut_short_is_truncated() {
    // Review Focus 2, on every fixture. The stream driver reads a quantum's headers and all its
    // data before its codec sees any, so a cut anywhere is Truncated, at or before the cut.
    each_fixture(0xC07, |f, r| {
        let len = f.stream.len();
        let mut cuts: Vec<usize> = edges(f)
            .into_iter()
            .flat_map(|e| e.saturating_sub(3)..e + 4)
            .filter(|&cut| cut < len)
            .collect();
        cuts.extend((0..64).map(|_| below(r, len)));
        for cut in cuts {
            let what = format!("{} cut at {cut}", f.file);
            let got = check(&f.stream[..cut], f.input.len(), &what);
            assert!(
                matches!(got, Err(Error::Truncated { at }) if at <= cut),
                "{what}: {:?}",
                got.map(|out| out.len())
            );
        }
    });
}

#[test]
fn every_quantum_cut_short_is_an_error() {
    // Cuts its quantum header agrees with, so that the codec gets the cut data: at and around
    // each of its chunk edges and its end, and at random points.
    each_fixture(0xC07C, |f, r| {
        for q in quanta(f) {
            if q.mode != ChunkMode::Compressed {
                continue;
            }
            let size = q.data.len();
            let mut cuts: Vec<usize> = q
                .chunks
                .iter()
                .copied()
                .chain([size])
                .flat_map(|c| c.saturating_sub(2)..c + 3)
                .filter(|cut| (1..size).contains(cut))
                .collect();
            cuts.extend((0..32).map(|_| 1 + below(r, size - 1)));
            for cut in cuts {
                // The stream up to this quantum's data, its header's 18-bit size now for `cut`
                // bytes, and that many bytes of the data.
                let mut s = f.stream[..q.data.start + cut].to_vec();
                let h = &mut s[q.header..q.header + 3];
                let v = (u32::from_be_bytes([0, h[0], h[1], h[2]]) & !0x3_FFFF) | (cut - 1) as u32;
                h.copy_from_slice(&v.to_be_bytes()[1..]);
                let what = format!("{}: the quantum at {} cut to {cut} bytes", f.file, q.start);
                let got = check(&s, f.input.len(), &what);
                assert!(got.is_err(), "{what} decodes");
            }
        }
    });
}

#[test]
fn every_fixture_bit_flipped_is_an_error_or_its_length() {
    // Every bit of the 8 bytes after each edge, where the headers and tables are, and 256 bits at
    // random.
    each_fixture(0xF00D, |f, r| {
        let len = f.stream.len();
        let mut bits: Vec<usize> = edges(f)
            .into_iter()
            .flat_map(|e| e * 8..(e + 8).min(len) * 8)
            .collect();
        bits.sort_unstable();
        bits.dedup();
        bits.extend((0..256).map(|_| below(r, len * 8)));
        let mut s = f.stream.clone();
        for bit in bits {
            s[bit / 8] ^= 1 << (bit % 8);
            let _ = check(&s, f.input.len(), &format!("{} bit {bit} flipped", f.file));
            s[bit / 8] ^= 1 << (bit % 8);
        }
    });
}

#[test]
fn every_fixture_overwritten_with_random_bytes_is_an_error_or_its_length() {
    // Runs of up to 64 random bytes, and one time in four everything from a random byte on.
    each_fixture(0x5EED, |f, r| {
        let len = f.stream.len();
        let mut s = f.stream.clone();
        for i in 0..64 {
            let from = below(r, len);
            let to = if i % 4 == 0 {
                len
            } else {
                len.min(from + 1 + below(r, 64))
            };
            for b in &mut s[from..to] {
                *b = r.next() as u8;
            }
            let what = format!("{} bytes {from}..{to} random", f.file);
            let _ = check(&s, f.input.len(), &what);
            s[from..to].copy_from_slice(&f.stream[from..to]);
        }
    });
}

#[test]
fn random_streams_behind_a_valid_header_never_panic() {
    // Random bytes behind a stream header. Few get a quantum header right, so the next test goes
    // further, behind valid quantum and chunk headers.
    in_parallel(10_000, 0xBEEF, |r, i| {
        let len = 1 + below(r, 4096);
        let mut s = vec![0x8C, [6u8, 10, 12][below(r, 3)]];
        s.extend(random_bytes(r, len));
        let raw_len = 1 + below(r, 300_000);
        let _ = check(&s, raw_len, &format!("random stream {i}"));
    });
}

#[test]
fn random_chunks_behind_valid_headers_are_an_error_or_their_length() {
    // Random bytes for the entropy decoders, behind a block of a random kind, and random streams
    // for the LZ runs: each the one chunk of a Kraken quantum.
    in_parallel(10_000, 0xC4A7, |r, i| {
        let (chunk, raw_len) = if i % 2 == 0 {
            entropy_chunk(r)
        } else {
            lz_chunk(r)
        };
        let mut s = vec![0x8C, 0x06];
        s.extend(quantum_header(chunk.len()));
        s.extend(chunk);
        let _ = check(&s, raw_len, &format!("random chunk {i}"));
    });
}

#[test]
fn a_raw_len_the_stream_does_not_back_is_an_error_not_an_allocation() {
    // The output grows a quantum at a time as the stream backs it, so asking for more than the
    // stream holds is an error at most a quantum later: never an allocation of the raw length,
    // which at usize::MAX would be a capacity-overflow panic. Asking for less is an error, or,
    // where a memset quantum ends the stream, that many bytes.
    each_fixture(0x1E7, |f, r| {
        let n = f.input.len();
        for raw_len in [0, 1, n / 2, n - 1, n + 1] {
            let what = format!("{} as {raw_len} bytes", f.file);
            let _ = check(&f.stream, raw_len, &what);
        }
        let beyond = [n + QUANTUM, u32::MAX as usize, usize::MAX];
        let random: Vec<usize> = (0..8)
            .map(|_| (r.next() as usize).max(n + QUANTUM))
            .collect();
        for raw_len in beyond.into_iter().chain(random) {
            let what = format!("{} as {raw_len} bytes", f.file);
            let got = check(&f.stream, raw_len, &what);
            assert!(got.is_err(), "{what} decodes");
        }
    });
}

#[test]
fn blocks_nested_without_end_are_corrupt_not_a_stack_overflow() {
    // ooz decodes a recursive block's parts, recursive blocks among them, with no limit on the
    // depth, so a hostile stream could exhaust the stack: an abort, not even a panic. The port
    // stops at a fixed depth.
    const RAW: usize = 100_000;
    // `levels` recursive blocks (kind 5), each of two parts, the next block in and an empty
    // stored block, around an RLE block of one byte: RAW bytes of 'x' at every level.
    let nested = |levels: usize| {
        let mut chunk = Vec::new();
        for k in (1..=levels).rev() {
            // The count of parts, the 6 + 9(k − 1) bytes of the block inside, the empty block.
            chunk.extend(block_header(5, 1 + 6 + 9 * (k - 1) + 3, RAW));
            chunk.push(2);
        }
        chunk.extend(block_header(3, 1, RAW));
        chunk.push(b'x');
        for _ in 0..levels {
            chunk.extend([0, 0, 0]);
        }
        let mut s = vec![0x8C, 0x06];
        s.extend(quantum_header(chunk.len()));
        s.extend(chunk);
        s
    };
    let got = check(&nested(3), RAW, "3 levels");
    assert!(
        got.as_ref().is_ok_and(|out| out.iter().all(|&b| b == b'x')),
        "3 levels: {:?}",
        got.map(|out| out.len())
    );
    for levels in [100, 10_000] {
        let got = check(&nested(levels), RAW, &format!("{levels} levels"));
        assert!(
            matches!(got, Err(Error::Corrupt { .. })),
            "{levels} levels: {:?}",
            got.map(|out| out.len())
        );
    }
}
