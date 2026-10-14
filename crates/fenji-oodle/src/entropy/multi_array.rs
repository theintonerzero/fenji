//! Part of fenji-oodle, a Rust port of the Oodle decoders in ooz's `kraken.cpp`
//! (github.com/powzix/ooz, commit 05038060aa68), Copyright (C) 2016 Powzix.
//! Changes Copyright (C) 2026 the Fenji contributors.
//! GPL-3.0-or-later: see COPYING and NOTICE.
//!
//! Multi-arrays (kraken.cpp:1294–1516): output arrays put together from intervals of entropy
//! blocks.
use super::block_size;
use crate::Error;
use crate::bits::{le32_at, shr};

/// The error for ooz's `return -1` in this file.
fn bad() -> Error {
    Error::corrupt("a bad multi-array")
}

/// ooz's `bitmasks` (kraken.cpp:1294): `n + 1` low bits.
fn bitmask(n: u32) -> u32 {
    shr(u32::MAX, 31u32.saturating_sub(n))
}

/// Port of `Kraken_DecodeMultiArray` (kraken.cpp:1301): one array per entry of `array_lens` into
/// `dst`, one after another, from the multi-array at the start of `src`. Returns the bytes read
/// and the total decoded, and sets each array's length.
///
/// Kraken's one caller, `Krak_DecodeRecursive`, gives an output apart from the scratch. ooz's
/// split of the scratch for an output at its start (kraken.cpp:1316–1320) serves Leviathan, which
/// isn't ported. ooz's 32 entropy arrays become 64, as many as the 6-bit count can name, where ooz
/// would write past its arrays.
pub(super) fn decode_multi_array(
    src: &[u8],
    dst: &mut [u8],
    array_lens: &mut [usize],
    scratch: &mut [u8],
    depth: u32,
) -> Result<(usize, usize), Error> {
    if src.len() < 4 {
        return Err(bad());
    }
    let num_arrays_in_file = src[0];
    if num_arrays_in_file & 0x80 == 0 {
        return Err(bad());
    }
    let num_arrays_in_file = usize::from(num_arrays_in_file & 0x3F);
    let mut at = 1;
    let mut total_size = 0;

    if num_arrays_in_file == 0 {
        let mut done = 0;
        for len in array_lens.iter_mut() {
            let (used, decoded) = super::decode(
                src.get(at..).ok_or_else(bad)?,
                &mut dst[done..],
                scratch,
                depth + 1,
            )?;
            done += decoded;
            *len = decoded;
            at += used;
            total_size += decoded;
        }
        return Ok((at, total_size));
    }

    // The entropy arrays, decoded into the scratch: (start, bytes left).
    let mut entropy = [(0usize, 0usize); 64];
    let mut cur = 0;
    for e in entropy.iter_mut().take(num_arrays_in_file) {
        let rest = scratch.get_mut(cur..).ok_or_else(bad)?;
        let (used, decoded) =
            super::decode_to_scratch(src.get(at..).ok_or_else(bad)?, rest.len(), rest, depth + 1)?;
        *e = (cur, decoded);
        cur += decoded;
        total_size += decoded;
        at += used;
    }

    if src.len() - at < 3 {
        return Err(bad());
    }

    let q = u32::from(src[at]) | u32::from(src[at + 1]) << 8;
    at += 2;

    let num_indexes = block_size(&src[at..], total_size)?;

    let array_count = array_lens.len();
    let Some(mut num_lens) = num_indexes.checked_sub(array_count).filter(|&n| n >= 1) else {
        return Err(bad());
    };

    if scratch.len() - cur < num_indexes {
        return Err(bad());
    }
    let interval_lenlog2 = cur;
    cur += num_indexes;

    if scratch.len() - cur < num_indexes {
        return Err(bad());
    }
    let interval_indexes = cur;
    cur += num_indexes;

    {
        let (done, rest) = scratch.split_at_mut(cur);
        let indexes = &mut done[interval_indexes..interval_indexes + num_indexes];
        let (n, size_out) = super::decode(&src[at..], indexes, rest, depth + 1)?;
        if size_out != num_indexes {
            return Err(bad());
        }
        at += n;

        if q & 0x8000 != 0 {
            let (lenlog2, indexes) = done[interval_lenlog2..].split_at_mut(num_indexes);
            for (l, i) in lenlog2.iter_mut().zip(indexes.iter_mut()) {
                let t = *i;
                *l = t >> 4;
                *i = t & 0xF;
            }
            num_lens = num_indexes;
        } else {
            let lenlog2_chunksize = num_indexes - array_count;
            let lenlog2 = &mut done[interval_lenlog2..interval_lenlog2 + lenlog2_chunksize];
            let (n, size_out) =
                super::decode(src.get(at..).ok_or_else(bad)?, lenlog2, rest, depth + 1)?;
            if size_out != lenlog2_chunksize {
                return Err(bad());
            }
            at += n;
            if lenlog2.iter().any(|&l| l > 16) {
                return Err(bad());
            }
        }
    }

    if scratch.len() - cur < 4 {
        return Err(bad());
    }
    // ooz aligns its u32 array's address to 4 (kraken.cpp:1421); the port counts from the
    // scratch's start, which moves this room check by 3 bytes at most.
    let decoded_intervals = (cur + 3) & !3;
    if scratch.len().saturating_sub(decoded_intervals) < num_lens * 4 {
        return Err(bad());
    }

    let varbits_complen = (q & 0x3FFF) as usize;
    if src.len() - at < varbits_complen {
        return Err(bad());
    }

    // Two bit streams over the next `varbits_complen` bytes: forwards from the start, highest bit
    // first, and backwards from the end. ooz checks neither for running past the other.
    let mut f = at as isize;
    let mut bits_f = 0u32;
    let mut bitpos_f = 24i32;

    let src_end_actual = at + varbits_complen;

    let mut b = src_end_actual as isize;
    let mut bits_b = 0u32;
    let mut bitpos_b = 24i32;

    let (head, intervals) = scratch.split_at_mut(decoded_intervals);
    let lenlog2 = &head[interval_lenlog2..interval_lenlog2 + num_indexes];
    // ooz's room check above leaves room for every interval. Should a later change break that, an
    // interval past the scratch is corrupt data, never one quietly dropped.
    let mut put = |i: usize, v: u32| -> Result<(), Error> {
        intervals
            .get_mut(i * 4..i * 4 + 4)
            .ok_or_else(bad)?
            .copy_from_slice(&v.to_le_bytes());
        Ok(())
    };

    let mut i = 0;
    while i + 2 <= num_lens {
        bits_f |= shr(le32_at(src, f).swap_bytes(), (24 - bitpos_f).unsigned_abs());
        f += ((bitpos_f + 7) >> 3) as isize;

        bits_b |= shr(le32_at(src, b - 4), (24 - bitpos_b).unsigned_abs());
        b -= ((bitpos_b + 7) >> 3) as isize;

        let numbits_f = u32::from(lenlog2[i]);
        let numbits_b = u32::from(lenlog2[i + 1]);

        bits_f = (bits_f | 1).rotate_left(numbits_f);
        bitpos_f += numbits_f as i32 - 8 * ((bitpos_f + 7) >> 3);

        bits_b = (bits_b | 1).rotate_left(numbits_b);
        bitpos_b += numbits_b as i32 - 8 * ((bitpos_b + 7) >> 3);

        let value_f = bits_f & bitmask(numbits_f);
        bits_f &= !bitmask(numbits_f);

        let value_b = bits_b & bitmask(numbits_b);
        bits_b &= !bitmask(numbits_b);

        put(i, value_f)?;
        put(i + 1, value_b)?;
        i += 2;
    }

    // The last one, since the loop reads two.
    if i < num_lens {
        bits_f |= shr(le32_at(src, f).swap_bytes(), (24 - bitpos_f).unsigned_abs());
        let numbits_f = u32::from(lenlog2[i]);
        bits_f = (bits_f | 1).rotate_left(numbits_f);
        put(i, bits_f & bitmask(numbits_f))?;
    }

    let indexes = &head[interval_indexes..interval_indexes + num_indexes];
    if indexes[num_indexes - 1] != 0 {
        return Err(bad());
    }

    let interval = |i: usize| {
        intervals
            .get(i * 4..i * 4 + 4)
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .map(|b| u32::from_le_bytes(b) as usize)
    };
    let (mut indi, mut leni) = (0usize, 0usize);
    let increment_leni = usize::from(q & 0x8000 != 0);
    let mut done = 0usize;

    for len in array_lens.iter_mut() {
        let start = done;
        if indi >= num_indexes {
            return Err(bad());
        }
        loop {
            let source = usize::from(*indexes.get(indi).ok_or_else(bad)?);
            indi += 1;
            if source == 0 {
                break;
            }
            if source > num_arrays_in_file || leni >= num_lens {
                return Err(bad());
            }
            let cur_len = interval(leni).ok_or_else(bad)?;
            leni += 1;
            let (from, bytes_left) = entropy[source - 1];
            if cur_len > bytes_left || cur_len > dst.len() - done {
                return Err(bad());
            }
            dst[done..done + cur_len].copy_from_slice(&head[from..from + cur_len]);
            entropy[source - 1] = (from + cur_len, bytes_left - cur_len);
            done += cur_len;
        }
        leni += increment_leni;
        *len = done - start;
    }

    if indi != num_indexes || leni != num_lens {
        return Err(bad());
    }

    if entropy
        .iter()
        .take(num_arrays_in_file)
        .any(|&(_, left)| left != 0)
    {
        return Err(bad());
    }
    Ok((src_end_actual, total_size))
}
