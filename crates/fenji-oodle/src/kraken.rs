//! Part of fenji-oodle, a Rust port of the Oodle decoders in ooz's `kraken.cpp`
//! (github.com/powzix/ooz, commit 05038060aa68), Copyright (C) 2016 Powzix.
//! Changes Copyright (C) 2026 the Fenji contributors.
//! GPL-3.0-or-later: see COPYING and NOTICE.
//!
//! Kraken's LZ decoder, stream decoder 6 (kraken.cpp:2209–2725). A quantum holds chunks of up to
//! 128 KiB. Each is entropy-coded bytes, stored bytes, or LZ: literal, command, offset and length
//! streams, decoded into the scratch first and then run.
use std::ops::Range;

use crate::Error;
use crate::bits::BitReader;
use crate::entropy::{decode_bytes, decode_bytes_to_scratch};
use crate::lz::copy_match;

/// The most output a chunk holds (kraken.cpp:2686).
const CHUNK: usize = 0x20000;

/// ooz's `KrakenLzTable` takes the first 64 bytes of its scratch (kraken.cpp:2704–2711), on a
/// 64-bit build. The port keeps the table apart, and keeps the room, so the scratch left over
/// matches ooz's.
const LZ_TABLE_SIZE: usize = 64;

/// The error for ooz's `return false` and `return -1` in this file.
fn bad() -> Error {
    Error::corrupt("a bad Kraken chunk")
}

/// `x` rounded up to 16, as ooz's `ALIGN_POINTER(p, 16)` (kraken.cpp:178) on the scratch, which
/// is 16-aligned where the LZ tables start.
fn align16(x: usize) -> usize {
    (x + 15) & !15
}

/// Port of `KrakenLzTable` (kraken.cpp:53): one chunk's streams. The literals and commands are in
/// the scratch; the offsets and lengths, `int` arrays in ooz's scratch, are vectors, sized after
/// ooz's check for their room.
struct LzTable {
    lit: Range<usize>,
    cmd: Range<usize>,
    offs: Vec<i32>,
    lens: Vec<i32>,
}

/// Port of `Kraken_DecodeQuantum` (kraken.cpp:2678): fills `dst[start..]` from `src`, one quantum
/// of decoder 6, in chunks that match back into all of `dst`. Returns how many bytes of `src` it
/// read; an error's `at` is where its chunk starts in `src`.
pub(crate) fn decode_quantum(
    dst: &mut [u8],
    start: usize,
    src: &[u8],
    scratch: &mut [u8],
) -> Result<usize, Error> {
    let mut pos = start;
    let mut at = 0;
    while pos < dst.len() {
        let dst_count = (dst.len() - pos).min(CHUNK);
        let chunk = src.get(at..).ok_or_else(bad)?;
        let used = decode_chunk(dst, pos, dst_count, chunk, scratch).map_err(|e| match e {
            Error::Corrupt { what, .. } => Error::Corrupt { at, what },
            e => e,
        })?;
        at += used;
        pos += dst_count;
    }
    Ok(at)
}

/// One pass of `Kraken_DecodeQuantum`'s loop (kraken.cpp:2685–2722): `dst[pos..pos + dst_count]`
/// from the chunk at the start of `src`. Returns the bytes read.
fn decode_chunk(
    dst: &mut [u8],
    pos: usize,
    dst_count: usize,
    src: &[u8],
    scratch: &mut [u8],
) -> Result<usize, Error> {
    let &[h0, h1, h2, _, ..] = src else {
        return Err(Error::corrupt("a chunk header is cut short"));
    };
    let chunkhdr = u32::from(h0) << 16 | u32::from(h1) << 8 | u32::from(h2);
    let dst = dst.get_mut(..pos + dst_count).ok_or_else(bad)?;
    if chunkhdr & 0x80_0000 == 0 {
        // Entropy-coded, with no matches. A stored block here, ooz leaves where it is: its local
        // `out` points at the input, and the output is never written (kraken.cpp:2692–2693 and
        // 2149). The port copies it into the output, which is what the chunk says.
        let (used, written) = decode_bytes(src, &mut dst[pos..], scratch)?;
        if written != dst_count {
            return Err(Error::corrupt("a chunk decodes to a different length"));
        }
        return Ok(used);
    }
    let src_used = (chunkhdr & 0x7_FFFF) as usize;
    let mode = (chunkhdr >> 19) & 0xF;
    let body = src
        .get(3..3 + src_used)
        .ok_or(Error::corrupt("a chunk runs past its quantum"))?;
    if src_used < dst_count {
        let scratch_usage = (3 * dst_count + 32 + 0xD000)
            .min(0x6C000)
            .min(scratch.len());
        if scratch_usage < LZ_TABLE_SIZE {
            return Err(bad());
        }
        let scratch = &mut scratch[LZ_TABLE_SIZE..scratch_usage];
        let table = read_lz_table(mode, body, dst, pos, dst_count, scratch)?;
        process_lz_runs(mode, dst, pos, &table, scratch)?;
    } else if src_used > dst_count || mode != 0 {
        return Err(bad());
    } else {
        // Stored.
        dst[pos..].copy_from_slice(body);
    }
    Ok(3 + src_used)
}

/// Port of `Kraken_ReadLzTable` (kraken.cpp:2319): the streams of an LZ chunk, from its data in
/// `src`, into the scratch. At the very start of the output (`offset` 0), the first 8 bytes come
/// as they are, straight into `dst[0..8]`.
fn read_lz_table(
    mode: u32,
    src: &[u8],
    dst: &mut [u8],
    offset: usize,
    dst_size: usize,
    scratch: &mut [u8],
) -> Result<LzTable, Error> {
    if mode > 1 {
        return Err(bad());
    }

    if src.len() < 13 {
        return Err(bad());
    }

    let mut at = 0;
    if offset == 0 {
        // ooz's COPY_64 writes 8 bytes even into a smaller output, which then fails.
        dst.get_mut(..8).ok_or_else(bad)?.copy_from_slice(&src[..8]);
        at = 8;
    }

    if src[at] & 0x80 != 0 {
        // A reserved flag, or excess bytes, which ooz doesn't support either.
        return Err(bad());
    }

    // Each stream goes into the scratch after the last, bounded as ooz bounds it.
    let mut s = 0;
    let mut stream = |at: &mut usize, cap: usize| -> Result<Range<usize>, Error> {
        let rest = scratch.get_mut(s..).ok_or_else(bad)?;
        let cap = cap.min(rest.len());
        let (n, size) = decode_bytes_to_scratch(src.get(*at..).ok_or_else(bad)?, cap, rest)?;
        *at += n;
        let range = s..s + size;
        s += size;
        Ok(range)
    };

    // The literals, bounded by `dst_size`.
    let lit = stream(&mut at, dst_size)?;

    // The commands, bounded by `dst_size`.
    let cmd = stream(&mut at, dst_size)?;

    if src.len() - at < 3 {
        return Err(bad());
    }

    let mut offs_scaling = 0;
    let mut packed_offs_extra = None;
    let packed_offs = if src[at] & 0x80 != 0 {
        // Distances coded with two tables.
        offs_scaling = i32::from(src[at]) - 127;
        at += 1;
        let packed = stream(&mut at, cmd.len())?;
        if offs_scaling != 1 {
            let extra = stream(&mut at, packed.len())?;
            if extra.len() != packed.len() {
                return Err(bad());
            }
            packed_offs_extra = Some(extra);
        }
        packed
    } else {
        // The packed offsets, bounded by the commands.
        stream(&mut at, cmd.len())?
    };

    // The packed lengths, bounded by a quarter of `dst_size`.
    let packed_lens = stream(&mut at, dst_size >> 2)?;

    // Room for the 32-bit offsets and lengths, aligned as ooz aligns them, and ooz's 64 bytes of
    // slack.
    let end = align16(align16(packed_lens.end) + packed_offs.len() * 4) + packed_lens.len() * 4;
    if end + 64 > scratch.len() {
        return Err(bad());
    }

    let (offs, lens) = unpack_offsets(
        &src[at..],
        &scratch[packed_offs],
        packed_offs_extra.map(|r| &scratch[r]),
        offs_scaling,
        &scratch[packed_lens],
    )?;
    Ok(LzTable {
        lit,
        cmd,
        offs,
        lens,
    })
}

/// Port of `CombineScaledOffsetArrays` (kraken.cpp:2209).
fn combine_scaled_offset_arrays(offs_stream: &mut [i32], scale: i32, low_bits: &[u8]) {
    for (o, &low) in offs_stream.iter_mut().zip(low_bits) {
        *o = scale.wrapping_mul(*o).wrapping_sub(i32::from(low));
    }
}

/// An offset coded the new way (kraken.cpp:2268–2279): `cmd`'s low 3 bits under a leading 1,
/// shifted up by `cmd >> 3` more bits read from `bits`.
fn new_offset(bits: &mut BitReader<'_>, cmd: u8, backwards: bool) -> Result<i32, Error> {
    let cmd = u32::from(cmd);
    let n = cmd >> 3;
    if n > 26 {
        return Err(bad());
    }
    let more = if backwards {
        bits.read_more_than_24_bits_b(n)
    } else {
        bits.read_more_than_24_bits(n)
    };
    let offs = ((8 + (cmd & 7)) << n) | more;
    Ok(8i32.wrapping_sub(offs as i32))
}

/// Port of `Kraken_UnpackOffsets` (kraken.cpp:2215), as Kraken calls it, without excess bytes:
/// the 32-bit offsets and lengths, from their packed bytes and the bits of `src`, read from both
/// ends until they meet.
fn unpack_offsets(
    src: &[u8],
    packed_offs: &[u8],
    packed_offs_extra: Option<&[u8]>,
    multi_dist_scale: i32,
    packed_lens: &[u8],
) -> Result<(Vec<i32>, Vec<i32>), Error> {
    let mut bits_a = BitReader::new(src);
    let mut bits_b = BitReader::new_backwards(src);

    // How many lengths take 32 bits: a gamma code, first from the back.
    if bits_b.bits < 0x2000 {
        return Err(bad());
    }
    let n = bits_b.bits.leading_zeros();
    bits_b.bitpos += n as i32;
    bits_b.bits <<= n;
    bits_b.refill_backwards();
    let n = n + 1;
    let u32_len_stream_size = ((bits_b.bits >> (32 - n)) - 1) as usize;
    bits_b.bitpos += n as i32;
    bits_b.bits <<= n;
    bits_b.refill_backwards();

    let mut offs = Vec::with_capacity(packed_offs.len());
    let mut packed = packed_offs.iter();
    if multi_dist_scale == 0 {
        // The traditional way of coding offsets.
        while let Some(&v) = packed.next() {
            offs.push((bits_a.read_distance(u32::from(v)) as i32).wrapping_neg());
            let Some(&v) = packed.next() else {
                break;
            };
            offs.push((bits_b.read_distance_b(u32::from(v)) as i32).wrapping_neg());
        }
    } else {
        // The new way.
        while let Some(&cmd) = packed.next() {
            offs.push(new_offset(&mut bits_a, cmd, false)?);
            let Some(&cmd) = packed.next() else {
                break;
            };
            offs.push(new_offset(&mut bits_b, cmd, true)?);
        }
        if multi_dist_scale != 1 {
            let low_bits = packed_offs_extra.ok_or_else(bad)?;
            combine_scaled_offset_arrays(&mut offs, multi_dist_scale, low_bits);
        }
    }

    // At most 128 KiB / 256 of them.
    if u32_len_stream_size > 512 {
        return Err(bad());
    }
    let mut u32_len_stream = [0u32; 512];
    let mut i = 0;
    while i + 1 < u32_len_stream_size {
        u32_len_stream[i] = bits_a.read_length().ok_or_else(bad)?;
        u32_len_stream[i + 1] = bits_b.read_length_b().ok_or_else(bad)?;
        i += 2;
    }
    if i < u32_len_stream_size {
        u32_len_stream[i] = bits_a.read_length().ok_or_else(bad)?;
    }

    bits_a.p -= ((24 - bits_a.bitpos) >> 3) as isize;
    bits_b.p += ((24 - bits_b.bitpos) >> 3) as isize;

    if bits_a.p != bits_b.p {
        return Err(bad());
    }

    let mut long = u32_len_stream[..u32_len_stream_size].iter();
    let mut lens = Vec::with_capacity(packed_lens.len());
    for &v in packed_lens {
        let mut v = u32::from(v);
        if v == 255 {
            v = long.next().ok_or_else(bad)?.wrapping_add(255);
        }
        lens.push(v.wrapping_add(3) as i32);
    }
    if long.next().is_some() {
        return Err(bad());
    }
    Ok((offs, lens))
}

/// How far back an offset reaches: ooz's offsets are negative. One pointing forwards is corrupt;
/// [`copy_match`] refuses 0.
fn distance(offset: i32) -> Result<usize, Error> {
    usize::try_from(-i64::from(offset))
        .map_err(|_| Error::corrupt("a match offset points forwards"))
}

/// A run of literals at `pos`: copied (mode 1), or each added to the byte `last_offset` back
/// (mode 0), as ooz's `COPY_64_ADD` (kraken.cpp:449) does 8 at a time.
fn copy_literals(
    dst: &mut [u8],
    pos: usize,
    lits: &[u8],
    delta: bool,
    last_offset: i32,
) -> Result<(), Error> {
    let end = pos
        .checked_add(lits.len())
        .filter(|&e| e <= dst.len())
        .ok_or(Error::corrupt("literals run past the chunk's output"))?;
    if !delta {
        dst[pos..end].copy_from_slice(lits);
        return Ok(());
    }
    let back = distance(last_offset)?;
    let from = pos
        .checked_sub(back)
        .filter(|_| back != 0)
        .ok_or(Error::corrupt("literals add bytes from before the output"))?;
    for (i, &lit) in lits.iter().enumerate() {
        dst[pos + i] = lit.wrapping_add(dst[from + i]);
    }
    Ok(())
}

/// Port of `Kraken_ProcessLzRuns` (kraken.cpp:2663), with `Kraken_ProcessLzRuns_Type0`
/// (kraken.cpp:2442) and `Kraken_ProcessLzRuns_Type1` (kraken.cpp:2552), which differ only in
/// their literals: mode 0 adds them to the bytes at the last offset, and mode 1 copies them.
///
/// Fills `dst[pos..]` (the chunk ends where `dst` does) from the table's streams, matching back
/// into all of `dst`. ooz copies 8 bytes at a time, past the end of each run, and checks where a
/// match starts but not where it ends; the port copies exactly and checks both.
fn process_lz_runs(
    mode: u32,
    dst: &mut [u8],
    pos: usize,
    t: &LzTable,
    scratch: &[u8],
) -> Result<(), Error> {
    let lits = scratch.get(t.lit.clone()).ok_or_else(bad)?;
    let cmds = scratch.get(t.cmd.clone()).ok_or_else(bad)?;
    let delta = mode == 0;

    let mut pos = if pos == 0 { 8 } else { pos };
    let mut lit = 0usize;
    let mut offs = t.offs.iter();
    let mut lens = t.lens.iter();
    let mut next_len = || lens.next().map(|&l| l as u32 as usize).ok_or_else(bad);

    // The three recent offsets are 3 to 5; 6 takes a new one, and 0 to 2 are scratch space for
    // ooz's shuffle.
    let mut recent_offs = [0, 0, 0, -8, -8, -8, 0];
    let mut last_offset = -8;

    for &f in cmds {
        let f = u32::from(f);
        let mut litlen = (f & 3) as usize;
        let offs_index = (f >> 6) as usize;
        let matchlen = ((f >> 2) & 0xF) as usize;

        if litlen == 3 {
            litlen = next_len()?;
        }
        let these = lit
            .checked_add(litlen)
            .and_then(|end| lits.get(lit..end))
            .ok_or(Error::corrupt("a chunk runs out of literals"))?;
        copy_literals(dst, pos, these, delta, last_offset)?;
        pos += litlen;
        lit += litlen;

        if offs_index == 3 {
            recent_offs[6] = *offs.next().ok_or_else(bad)?;
        }
        let offset = recent_offs[offs_index + 3];
        recent_offs[offs_index + 3] = recent_offs[offs_index + 2];
        recent_offs[offs_index + 2] = recent_offs[offs_index + 1];
        recent_offs[offs_index + 1] = recent_offs[offs_index];
        recent_offs[3] = offset;
        last_offset = offset;

        let len = if matchlen != 15 {
            matchlen + 2
        } else {
            14 + next_len()?
        };
        copy_match(dst, pos, distance(offset)?, len)?;
        pos += len;
    }

    // Every offset and length used.
    if offs.next().is_some() || lens.next().is_some() {
        return Err(bad());
    }

    // The literals left fill the rest of the chunk.
    if dst.len() - pos != lits.len() - lit {
        return Err(bad());
    }
    copy_literals(dst, pos, &lits[lit..], delta, last_offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_0_literals_add_the_bytes_at_the_last_offset() {
        let mut dst = [1, 2, 3, 4, 5, 6, 7, 8, 0, 0, 0];
        copy_literals(&mut dst, 8, &[10, 20, 250], true, -8).unwrap();
        assert_eq!(&dst[8..], [11, 22, 253]);
        copy_literals(&mut dst, 8, &[10, 20, 250], false, -8).unwrap();
        assert_eq!(&dst[8..], [10, 20, 250]);
        assert!(copy_literals(&mut dst, 8, &[1], true, -9).is_err());
        assert!(copy_literals(&mut dst, 8, &[1, 2, 3, 4], false, -8).is_err());
    }

    /// A stream of one quantum: 128 KiB of 'X' in an entropy-coded chunk, an RLE block, then 100
    /// bytes in a stored chunk of `mode`.
    fn two_chunks(mode: u8) -> Vec<u8> {
        let mut s = vec![0x8C, 0x06, 0x00, 0x00, 0x70];
        // The block's long header: type 3, 5 bytes, 0x20000 out. Its first byte's top bit is
        // clear, which makes the chunk entropy-only.
        s.extend([0x37, 0xFF, 0xFC, 0x00, 0x05]);
        // RLE: from the back, 0x01 takes 'X'; 0x0C_FF repeats (0xCFF - 0x8FF) * 128 times.
        s.extend([0x00, b'X', 0xFF, 0x0C, 0x01]);
        s.extend([0x80 | mode << 3, 0x00, 100]);
        s.extend(0..100u8);
        s
    }

    #[test]
    fn an_entropy_coded_chunk_then_a_stored_one() {
        let mut want = vec![b'X'; CHUNK];
        want.extend(0..100u8);
        assert_eq!(crate::decompress(&two_chunks(0), want.len()), Ok(want));
    }

    #[test]
    fn a_corrupt_chunk_names_its_place_in_the_stream() {
        // A stored chunk must be mode 0. It starts after the stream header (2), the quantum
        // header (3) and the first chunk (10).
        let got = crate::decompress(&two_chunks(1), CHUNK + 100);
        assert!(matches!(got, Err(Error::Corrupt { at: 15, .. })), "{got:?}");
    }

    #[test]
    fn a_match_before_the_start_is_corrupt() {
        // Review Focus 4, through the shared helper every codec uses.
        let mut dst = vec![0u8; 16];
        assert!(matches!(
            crate::lz::copy_match(&mut dst, 4, 5, 3),
            Err(crate::Error::Corrupt { .. })
        ));
        assert!(matches!(
            crate::lz::copy_match(&mut dst, 4, 0, 3),
            Err(crate::Error::Corrupt { .. })
        ));
        crate::lz::copy_match(&mut dst, 4, 2, 6).unwrap(); // overlapping: repeats dst[2..4]
    }
}
