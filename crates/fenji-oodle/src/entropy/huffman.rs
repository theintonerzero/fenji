//! Part of fenji-oodle, a Rust port of the Oodle decoders in ooz's `kraken.cpp`
//! (github.com/powzix/ooz, commit 05038060aa68), Copyright (C) 2016 Powzix.
//! Changes Copyright (C) 2026 the Fenji contributors.
//! GPL-3.0-or-later: see COPYING and NOTICE.
//!
//! Huffman blocks (types 2 and 4), and the Golomb-Rice code lengths tANS tables share
//! (kraken.cpp:574–1300).
use crate::Error;
use crate::bits::{BitReader, BitReader2, byte_at, le16_at, le32_at, shl};

/// The first slot in `syms` for each code length (kraken.cpp:1200).
const CODE_PREFIX_ORG: [u32; 12] = [
    0x0, 0x0, 0x2, 0x6, 0xE, 0x1E, 0x3E, 0x7E, 0xFE, 0x1FE, 0x2FE, 0x3FE,
];

/// ooz's `kRiceCodeBits2Value` (kraken.cpp:853): for a byte read highest bit first, the zeros
/// before each of its 1 bits, a nibble each, and the zeros after the last in the top nibble.
#[rustfmt::skip]
const RICE_CODE_BITS2_VALUE: [u32; 256] = [
  0x80000000, 0x00000007, 0x10000006, 0x00000006, 0x20000005, 0x00000105, 0x10000005, 0x00000005,
  0x30000004, 0x00000204, 0x10000104, 0x00000104, 0x20000004, 0x00010004, 0x10000004, 0x00000004,
  0x40000003, 0x00000303, 0x10000203, 0x00000203, 0x20000103, 0x00010103, 0x10000103, 0x00000103,
  0x30000003, 0x00020003, 0x10010003, 0x00010003, 0x20000003, 0x01000003, 0x10000003, 0x00000003,
  0x50000002, 0x00000402, 0x10000302, 0x00000302, 0x20000202, 0x00010202, 0x10000202, 0x00000202,
  0x30000102, 0x00020102, 0x10010102, 0x00010102, 0x20000102, 0x01000102, 0x10000102, 0x00000102,
  0x40000002, 0x00030002, 0x10020002, 0x00020002, 0x20010002, 0x01010002, 0x10010002, 0x00010002,
  0x30000002, 0x02000002, 0x11000002, 0x01000002, 0x20000002, 0x00000012, 0x10000002, 0x00000002,
  0x60000001, 0x00000501, 0x10000401, 0x00000401, 0x20000301, 0x00010301, 0x10000301, 0x00000301,
  0x30000201, 0x00020201, 0x10010201, 0x00010201, 0x20000201, 0x01000201, 0x10000201, 0x00000201,
  0x40000101, 0x00030101, 0x10020101, 0x00020101, 0x20010101, 0x01010101, 0x10010101, 0x00010101,
  0x30000101, 0x02000101, 0x11000101, 0x01000101, 0x20000101, 0x00000111, 0x10000101, 0x00000101,
  0x50000001, 0x00040001, 0x10030001, 0x00030001, 0x20020001, 0x01020001, 0x10020001, 0x00020001,
  0x30010001, 0x02010001, 0x11010001, 0x01010001, 0x20010001, 0x00010011, 0x10010001, 0x00010001,
  0x40000001, 0x03000001, 0x12000001, 0x02000001, 0x21000001, 0x01000011, 0x11000001, 0x01000001,
  0x30000001, 0x00000021, 0x10000011, 0x00000011, 0x20000001, 0x00001001, 0x10000001, 0x00000001,
  0x70000000, 0x00000600, 0x10000500, 0x00000500, 0x20000400, 0x00010400, 0x10000400, 0x00000400,
  0x30000300, 0x00020300, 0x10010300, 0x00010300, 0x20000300, 0x01000300, 0x10000300, 0x00000300,
  0x40000200, 0x00030200, 0x10020200, 0x00020200, 0x20010200, 0x01010200, 0x10010200, 0x00010200,
  0x30000200, 0x02000200, 0x11000200, 0x01000200, 0x20000200, 0x00000210, 0x10000200, 0x00000200,
  0x50000100, 0x00040100, 0x10030100, 0x00030100, 0x20020100, 0x01020100, 0x10020100, 0x00020100,
  0x30010100, 0x02010100, 0x11010100, 0x01010100, 0x20010100, 0x00010110, 0x10010100, 0x00010100,
  0x40000100, 0x03000100, 0x12000100, 0x02000100, 0x21000100, 0x01000110, 0x11000100, 0x01000100,
  0x30000100, 0x00000120, 0x10000110, 0x00000110, 0x20000100, 0x00001100, 0x10000100, 0x00000100,
  0x60000000, 0x00050000, 0x10040000, 0x00040000, 0x20030000, 0x01030000, 0x10030000, 0x00030000,
  0x30020000, 0x02020000, 0x11020000, 0x01020000, 0x20020000, 0x00020010, 0x10020000, 0x00020000,
  0x40010000, 0x03010000, 0x12010000, 0x02010000, 0x21010000, 0x01010010, 0x11010000, 0x01010000,
  0x30010000, 0x00010020, 0x10010010, 0x00010010, 0x20010000, 0x00011000, 0x10010000, 0x00010000,
  0x50000000, 0x04000000, 0x13000000, 0x03000000, 0x22000000, 0x02000010, 0x12000000, 0x02000000,
  0x31000000, 0x01000020, 0x11000010, 0x01000010, 0x21000000, 0x01001000, 0x11000000, 0x01000000,
  0x40000000, 0x00000030, 0x10000020, 0x00000020, 0x20000010, 0x00001010, 0x10000010, 0x00000010,
  0x30000000, 0x00002000, 0x10001000, 0x00001000, 0x20000000, 0x00100000, 0x10000000, 0x00000000,
];

/// ooz's `kRiceCodeBits2Len` (kraken.cpp:888): how many codes a byte ends, its count of 1 bits.
#[rustfmt::skip]
const RICE_CODE_BITS2_LEN: [u8; 256] = [
  0, 1, 1, 2, 1, 2, 2, 3, 1, 2, 2, 3, 2, 3, 3, 4, 1, 2, 2, 3, 2, 3, 3, 4, 2, 3, 3, 4, 3, 4, 4, 5,
  1, 2, 2, 3, 2, 3, 3, 4, 2, 3, 3, 4, 3, 4, 4, 5, 2, 3, 3, 4, 3, 4, 4, 5, 3, 4, 4, 5, 4, 5, 5, 6,
  1, 2, 2, 3, 2, 3, 3, 4, 2, 3, 3, 4, 3, 4, 4, 5, 2, 3, 3, 4, 3, 4, 4, 5, 3, 4, 4, 5, 4, 5, 5, 6,
  2, 3, 3, 4, 3, 4, 4, 5, 3, 4, 4, 5, 4, 5, 5, 6, 3, 4, 4, 5, 4, 5, 5, 6, 4, 5, 5, 6, 5, 6, 6, 7,
  1, 2, 2, 3, 2, 3, 3, 4, 2, 3, 3, 4, 3, 4, 4, 5, 2, 3, 3, 4, 3, 4, 4, 5, 3, 4, 4, 5, 4, 5, 5, 6,
  2, 3, 3, 4, 3, 4, 4, 5, 3, 4, 4, 5, 4, 5, 5, 6, 3, 4, 4, 5, 4, 5, 5, 6, 4, 5, 5, 6, 5, 6, 6, 7,
  2, 3, 3, 4, 3, 4, 4, 5, 3, 4, 4, 5, 4, 5, 5, 6, 3, 4, 4, 5, 4, 5, 5, 6, 4, 5, 5, 6, 5, 6, 6, 7,
  3, 4, 4, 5, 4, 5, 5, 6, 4, 5, 5, 6, 5, 6, 6, 7, 4, 5, 5, 6, 5, 6, 6, 7, 5, 6, 6, 7, 6, 7, 7, 8,
];

/// The error for ooz's `return -1` and `return false` in this file.
fn bad() -> Error {
    Error::corrupt("a bad Huffman block")
}

/// Port of `NewHuffLut` (kraken.cpp:1143): code length and symbol by the next 11 bits, the first
/// bit highest. ooz's 16 bytes of slack for its fills go: the port fills exactly.
struct Lut {
    len: [u8; 2048],
    sym: [u8; 2048],
}

/// Port of `HuffRevLut` (kraken.cpp:160): the same, by the next 11 bits with the first bit lowest.
struct RevLut {
    len: [u8; 2048],
    sym: [u8; 2048],
}

/// Port of `ReverseBitsArray2048` (kraken.cpp:574): `output[i]` is `input` at `i` with its 11 bits
/// reversed, which ooz does with SSE2 shuffles.
fn reverse_bits_array_2048(input: &[u8; 2048], output: &mut [u8; 2048]) {
    for (i, o) in output.iter_mut().enumerate() {
        *o = input[((i as u32).reverse_bits() >> 21) as usize];
    }
}

/// The next code of a stream, its bits lowest first in `bits`.
fn next_symbol(lut: &RevLut, bits: &mut u32, bitpos: &mut i32) -> u8 {
    let k = (*bits & 0x7FF) as usize;
    let n = lut.len[k];
    *bits >>= n;
    *bitpos -= i32::from(n);
    lut.sym[k]
}

/// Port of `Kraken_DecodeBytesCore` (kraken.cpp:614): three streams of codes into `out`, forwards
/// from `src` to `mid`, forwards from `mid`, and backwards from `end` until it meets the second.
/// The positions index `buf`; `false` where ooz returns false.
fn decode_bytes_core(
    buf: &[u8],
    out: &mut [u8],
    src: usize,
    mid: usize,
    end: usize,
    lut: &RevLut,
) -> bool {
    // Positions as isize: ooz's pointers step a few bytes past each other before its checks.
    let (mut src, mut src_mid, mut src_end) = (src as isize, mid as isize, end as isize);
    let src_mid_org = src_mid;
    let (mut src_bits, mut src_mid_bits, mut src_end_bits) = (0u32, 0u32, 0u32);
    let (mut src_bitpos, mut src_mid_bitpos, mut src_end_bitpos) = (0i32, 0i32, 0i32);
    let mut dst = 0usize;
    let mut dst_end = out.len();

    if src > src_mid {
        return false;
    }

    if src_end - src_mid >= 4 && dst_end - dst >= 6 {
        dst_end -= 5;
        src_end -= 4;
        while dst < dst_end && src <= src_mid && src_mid <= src_end {
            src_bits |= shl(le32_at(buf, src), src_bitpos.unsigned_abs());
            src += ((31 - src_bitpos) >> 3) as isize;

            src_end_bits |= shl(
                le32_at(buf, src_end).swap_bytes(),
                src_end_bitpos.unsigned_abs(),
            );
            src_end -= ((31 - src_end_bitpos) >> 3) as isize;

            src_mid_bits |= shl(le32_at(buf, src_mid), src_mid_bitpos.unsigned_abs());
            src_mid += ((31 - src_mid_bitpos) >> 3) as isize;

            src_bitpos |= 0x18;
            src_end_bitpos |= 0x18;
            src_mid_bitpos |= 0x18;

            out[dst] = next_symbol(lut, &mut src_bits, &mut src_bitpos);
            out[dst + 1] = next_symbol(lut, &mut src_end_bits, &mut src_end_bitpos);
            out[dst + 2] = next_symbol(lut, &mut src_mid_bits, &mut src_mid_bitpos);
            out[dst + 3] = next_symbol(lut, &mut src_bits, &mut src_bitpos);
            out[dst + 4] = next_symbol(lut, &mut src_end_bits, &mut src_end_bitpos);
            out[dst + 5] = next_symbol(lut, &mut src_mid_bits, &mut src_mid_bitpos);
            dst += 6;
        }
        dst_end += 5;

        src -= (src_bitpos >> 3) as isize;
        src_bitpos &= 7;

        src_end += 4 + (src_end_bitpos >> 3) as isize;
        src_end_bitpos &= 7;

        src_mid -= (src_mid_bitpos >> 3) as isize;
        src_mid_bitpos &= 7;
    }

    loop {
        if dst >= dst_end {
            break;
        }

        if src_mid - src <= 1 {
            if src_mid - src == 1 {
                src_bits |= shl(byte_at(buf, src), src_bitpos.unsigned_abs());
            }
        } else {
            src_bits |= shl(le16_at(buf, src), src_bitpos.unsigned_abs());
        }
        out[dst] = next_symbol(lut, &mut src_bits, &mut src_bitpos);
        dst += 1;
        src += ((7 - src_bitpos) >> 3) as isize;
        src_bitpos &= 7;

        if dst < dst_end {
            if src_end - src_mid <= 1 {
                if src_end - src_mid == 1 {
                    src_end_bits |= shl(byte_at(buf, src_mid), src_end_bitpos.unsigned_abs());
                    src_mid_bits |= shl(byte_at(buf, src_mid), src_mid_bitpos.unsigned_abs());
                }
            } else {
                let v = le16_at(buf, src_end - 2);
                src_end_bits |= shl(
                    ((v >> 8) | (v << 8)) & 0xFFFF,
                    src_end_bitpos.unsigned_abs(),
                );
                src_mid_bits |= shl(le16_at(buf, src_mid), src_mid_bitpos.unsigned_abs());
            }
            out[dst] = next_symbol(lut, &mut src_end_bits, &mut src_end_bitpos);
            dst += 1;
            src_end -= ((7 - src_end_bitpos) >> 3) as isize;
            src_end_bitpos &= 7;
            if dst < dst_end {
                out[dst] = next_symbol(lut, &mut src_mid_bits, &mut src_mid_bitpos);
                dst += 1;
                src_mid += ((7 - src_mid_bitpos) >> 3) as isize;
                src_mid_bitpos &= 7;
            }
        }
        if src > src_mid || src_mid > src_end {
            return false;
        }
    }
    src == src_mid_org && src_end == src_mid
}

/// Port of `Huff_ReadCodeLengthsOld` (kraken.cpp:753): each symbol into `syms` at its code length's
/// slot, and how many symbols there are.
fn read_code_lengths_old(
    bits: &mut BitReader<'_>,
    syms: &mut [u8; 1280],
    code_prefix: &mut [u32; 12],
) -> Result<usize, Error> {
    if bits.read_bit_no_refill() != 0 {
        let mut sym: u32 = 0;
        let mut num_symbols: u32 = 0;
        let mut avg_bits_x4: i32 = 32;
        let forced_bits = bits.read_bits_no_refill(2);
        let thres_for_valid_gamma_bits = 1u32 << (31 - (20u32 >> forced_bits));
        // ooz jumps into the loop past the first run of zeros (`goto SKIP_INITIAL_ZEROS`).
        let mut skip_initial_zeros = bits.read_bit() != 0;
        loop {
            if !skip_initial_zeros {
                // A run of zeros. The gamma code's leading 1 makes the value at least 2.
                if bits.bits & 0xFF00_0000 == 0 {
                    return Err(bad());
                }
                sym += bits.read_bits_no_refill(2 * (bits.bits.leading_zeros() + 1)) - 2 + 1;
                if sym >= 256 {
                    break;
                }
            }
            skip_initial_zeros = false;
            bits.refill();
            // The gamma value for the number of symbols.
            if bits.bits & 0xFF00_0000 == 0 {
                return Err(bad());
            }
            let mut n = bits.read_bits_no_refill(2 * (bits.bits.leading_zeros() + 1)) - 2 + 1;
            if sym + n > 256 {
                return Err(bad());
            }
            bits.refill();
            num_symbols += n;
            loop {
                if bits.bits < thres_for_valid_gamma_bits {
                    return Err(bad()); // too big a gamma value
                }
                let lz = bits.bits.leading_zeros() as i32;
                let v = bits.read_bits_no_refill((lz + forced_bits as i32 + 1).unsigned_abs())
                    as i32
                    + ((lz - 1) << forced_bits);
                let codelen = (-(v & 1) ^ (v >> 1)) + ((avg_bits_x4 + 2) >> 2);
                if !(1..=11).contains(&codelen) {
                    return Err(bad());
                }
                avg_bits_x4 = codelen + ((3 * avg_bits_x4 + 2) >> 2);
                bits.refill();
                put_symbol(syms, code_prefix, codelen.unsigned_abs(), sym)?;
                sym += 1;
                n -= 1;
                if n == 0 {
                    break;
                }
            }
            if sym == 256 {
                break;
            }
        }
        if sym == 256 && num_symbols >= 2 {
            Ok(num_symbols as usize)
        } else {
            Err(bad())
        }
    } else {
        // Sparse symbols.
        let num_symbols = bits.read_bits_no_refill(8);
        if num_symbols == 0 {
            return Err(bad());
        }
        if num_symbols == 1 {
            syms[0] = bits.read_bits_no_refill(8) as u8;
        } else {
            let codelen_bits = bits.read_bits_no_refill(3);
            if codelen_bits > 4 {
                return Err(bad());
            }
            for _ in 0..num_symbols {
                bits.refill();
                let sym = bits.read_bits_no_refill(8);
                let codelen = bits.read_bits_no_refill_zero(codelen_bits) + 1;
                if codelen > 11 {
                    return Err(bad());
                }
                put_symbol(syms, code_prefix, codelen, sym)?;
            }
        }
        Ok(num_symbols as usize)
    }
}

/// ooz's `syms[code_prefix[codelen]++] = sym`, for a code length from 1 to 11.
fn put_symbol(
    syms: &mut [u8; 1280],
    code_prefix: &mut [u32; 12],
    codelen: u32,
    sym: u32,
) -> Result<(), Error> {
    let slot = code_prefix.get_mut(codelen as usize).ok_or_else(bad)?;
    *syms.get_mut(*slot as usize).ok_or_else(bad)? = sym as u8;
    *slot += 1;
    Ok(())
}

/// Port of `BitReader_ReadFluff` (kraken.cpp:819): how many range values follow a table of
/// `num_symbols` symbols, 1 to 256.
pub(super) fn read_fluff(bits: &mut BitReader<'_>, num_symbols: usize) -> usize {
    if num_symbols >= 256 {
        return 0;
    }
    let x = ((257 - num_symbols).min(num_symbols) * 2) as u32;
    let y = (x.saturating_sub(1)).checked_ilog2().unwrap_or(0) + 1;
    let v = bits.bits >> (32 - y);
    let z = (1u32 << y).saturating_sub(x);
    if (v >> 1) >= z {
        bits.bits <<= y;
        bits.bitpos += y as i32;
        (v - z) as usize
    } else {
        bits.bits <<= y - 1;
        bits.bitpos += (y - 1) as i32;
        (v >> 1) as usize
    }
}

/// Port of `DecodeGolombRiceLengths` (kraken.cpp:900): `size` unary codes from `br` into `dst`.
///
/// Like ooz, it writes 8 bytes at a time and so up to 7 past `size`: `dst` has room for them.
pub(super) fn decode_golomb_rice_lengths(
    dst: &mut [u8],
    size: usize,
    br: &mut BitReader2<'_>,
) -> bool {
    let src = br.src;
    let mut p = br.p;
    // Its callers ask for one code at least; with none, ooz would scan a cleared byte.
    if size == 0 {
        return false;
    }
    let Some(&first) = src.get(p) else {
        return false;
    };
    p += 1;
    let mut count = -(br.bitpos as i32);
    let mut v = u32::from(first) & (255 >> br.bitpos);
    let mut d = 0usize;
    loop {
        if v == 0 {
            count += 8;
        } else {
            let x = RICE_CODE_BITS2_VALUE[v as usize];
            let Some(w) = dst.get_mut(d..d + 8) else {
                return false;
            };
            // ooz's two u32 stores: the first four codes plus the count carried in, and the rest.
            w[..4].copy_from_slice(&(count as u32).wrapping_add(x & 0x0F0F_0F0F).to_le_bytes());
            w[4..].copy_from_slice(&((x >> 4) & 0x0F0F_0F0F).to_le_bytes());
            d += usize::from(RICE_CODE_BITS2_LEN[v as usize]);
            if d >= size {
                break;
            }
            count = (x >> 28) as i32;
        }
        let Some(&next) = src.get(p) else {
            return false;
        };
        p += 1;
        v = u32::from(next);
    }
    // Too far: give back the codes past `size`. Fewer than the byte holds, so `v` keeps a bit.
    for _ in size..d {
        v &= v - 1;
    }
    // A byte not finished: step back into it.
    let mut bitpos = 0;
    if v & 1 == 0 {
        p -= 1;
        bitpos = 8 - v.trailing_zeros();
    }
    br.p = p;
    br.bitpos = bitpos;
    true
}

/// Port of `DecodeGolombRiceBits` (kraken.cpp:942): `bitcount` more low bits, 0 to 3, for each of
/// the `size` values in `dst`.
///
/// ooz works on 8 values at a time as one little-endian u64, shifted up and added to. A value too
/// big for its byte carries into the next, as there; the 8 bytes past `size` come back as they
/// were, as there.
fn decode_golomb_rice_bits(
    dst: &mut [u8],
    size: usize,
    bitcount: u32,
    br: &mut BitReader2<'_>,
) -> bool {
    if bitcount == 0 {
        return true;
    }
    let p = br.p;
    let bits_required = br.bitpos as usize + bitcount as usize * size;
    let bytes_required = (bits_required + 7) >> 3;
    if bytes_required > br.src.len().saturating_sub(p) {
        return false;
    }
    let bitpos = br.bitpos;
    br.p = p + (bits_required >> 3);
    br.bitpos = (bits_required & 7) as u32;

    let mut bak = [0u8; 8];
    let Some(past) = dst.get(size..size + 8) else {
        return false;
    };
    bak.copy_from_slice(past);
    let mut q = p;
    let mut d = 0;
    while d < size {
        // The next 8 values' bits, from bit `bitpos` of `src[q]`; the first value is highest.
        let window = le32_at(br.src, q as isize).swap_bytes();
        let field = (window >> (32 - 8 * bitcount - bitpos)) & ((1 << (8 * bitcount)) - 1);
        let mut add = 0u64;
        for k in 0..8 {
            let value = (field >> (bitcount * (7 - k))) & ((1 << bitcount) - 1);
            add |= u64::from(value) << (8 * k);
        }
        let Some(chunk) = dst.get_mut(d..d + 8) else {
            return false;
        };
        let mut word = [0u8; 8];
        word.copy_from_slice(chunk);
        let word = (u64::from_le_bytes(word) << bitcount).wrapping_add(add);
        chunk.copy_from_slice(&word.to_le_bytes());
        q += bitcount as usize;
        d += 8;
    }
    if let Some(past) = dst.get_mut(size..size + 8) {
        past.copy_from_slice(&bak);
    }
    true
}

/// Port of `HuffRange` (kraken.cpp:1004): `num` symbols from `symbol` on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct HuffRange {
    pub symbol: usize,
    pub num: usize,
}

/// Port of `Huff_ConvertToRanges` (kraken.cpp:1009): which symbols a table's `num_symbols` lengths
/// are for, as ranges, from the `p` Golomb-Rice values in `symlen` and more bits. Returns how many
/// ranges there are.
pub(super) fn convert_to_ranges(
    range: &mut [HuffRange],
    num_symbols: usize,
    p: usize,
    symlen: &[u8],
    bits: &mut BitReader<'_>,
) -> Result<usize, Error> {
    let num_ranges = p >> 1;
    let mut sym_idx = 0usize;
    let mut s = 0usize;

    // Start with a space?
    if p & 1 != 0 {
        bits.refill();
        let v = u32::from(*symlen.first().ok_or_else(bad)?);
        if v >= 8 {
            return Err(bad());
        }
        sym_idx = (bits.read_bits_no_refill(v + 1) + (1 << (v + 1)) - 1) as usize;
        s = 1;
    }
    let mut syms_used = 0usize;

    for i in 0..num_ranges {
        bits.refill();
        let v = u32::from(*symlen.get(s).ok_or_else(bad)?);
        if v >= 9 {
            return Err(bad());
        }
        let num = (bits.read_bits_no_refill_zero(v) + (1 << v)) as usize;
        let v = u32::from(*symlen.get(s + 1).ok_or_else(bad)?);
        if v >= 8 {
            return Err(bad());
        }
        let space = (bits.read_bits_no_refill(v + 1) + (1 << (v + 1)) - 1) as usize;
        *range.get_mut(i).ok_or_else(bad)? = HuffRange {
            symbol: sym_idx,
            num,
        };
        syms_used += num;
        sym_idx += num + space;
        s += 2;
    }

    if sym_idx >= 256 || syms_used >= num_symbols || sym_idx + num_symbols - syms_used > 256 {
        return Err(bad());
    }

    *range.get_mut(num_ranges).ok_or_else(bad)? = HuffRange {
        symbol: sym_idx,
        num: num_symbols - syms_used,
    };
    Ok(num_ranges + 1)
}

/// Port of `Huff_ReadCodeLengthsNew` (kraken.cpp:1048).
fn read_code_lengths_new(
    bits: &mut BitReader<'_>,
    syms: &mut [u8; 1280],
    code_prefix: &mut [u32; 12],
) -> Result<usize, Error> {
    let forced_bits = bits.read_bits_no_refill(2);
    let num_symbols = bits.read_bits_no_refill(8) as usize + 1;
    let fluff = read_fluff(bits, num_symbols);

    // ooz's `code_len[512]`; the 16 more bytes hold its 8-byte writes and 16-byte clear past the
    // codes, which ooz lets run off its array.
    let mut code_len = [0u8; 512 + 16];
    let mut br2 = bits.byte_reader().ok_or_else(bad)?;
    if !decode_golomb_rice_lengths(&mut code_len, num_symbols + fluff, &mut br2) {
        return Err(bad());
    }
    code_len
        .get_mut(num_symbols + fluff..num_symbols + fluff + 16)
        .ok_or_else(bad)?
        .fill(0);
    if !decode_golomb_rice_bits(&mut code_len, num_symbols, forced_bits, &mut br2) {
        return Err(bad());
    }

    // Reset the bit reader.
    bits.resume_at(&br2);

    // ooz's scalar filter; its SSE version sits in an `if (0)`.
    let mut running_sum: u32 = 0x1E;
    for c in &mut code_len[..num_symbols] {
        let v = i32::from(*c);
        let v = -(v & 1) ^ (v >> 1);
        *c = (v as u32).wrapping_add(running_sum >> 2).wrapping_add(1) as u8;
        if !(1..=11).contains(&*c) {
            return Err(bad());
        }
        running_sum = running_sum.wrapping_add(v as u32);
    }

    let mut range = [HuffRange::default(); 128];
    let ranges = convert_to_ranges(
        &mut range,
        num_symbols,
        fluff,
        &code_len[num_symbols..],
        bits,
    )?;

    let mut lens = code_len.iter();
    for r in &range[..ranges] {
        for sym in r.symbol..r.symbol + r.num {
            let &len = lens.next().ok_or_else(bad)?;
            put_symbol(syms, code_prefix, u32::from(len), sym as u32)?;
        }
    }
    Ok(num_symbols)
}

/// Port of `Huff_MakeLut` (kraken.cpp:1155) and `FillByteOverflow16` (kraken.cpp:1151): the codes
/// in order of length, `prefix_cur` past each length's symbols. `false` unless they fill the table.
fn make_lut(prefix_cur: &[u32; 12], lut: &mut Lut, syms: &[u8; 1280]) -> bool {
    let mut currslot = 0usize;
    for i in 1..11 {
        let start = CODE_PREFIX_ORG[i] as usize;
        let count = prefix_cur[i].wrapping_sub(CODE_PREFIX_ORG[i]) as usize;
        if count != 0 {
            let stepsize = 1usize << (11 - i);
            let num_to_set = count << (11 - i);
            if currslot + num_to_set > 2048 {
                return false;
            }
            lut.len[currslot..currslot + num_to_set].fill(i as u8);
            let Some(these) = syms.get(start..start + count) else {
                return false;
            };
            for (step, &sym) in lut.sym[currslot..currslot + num_to_set]
                .chunks_exact_mut(stepsize)
                .zip(these)
            {
                step.fill(sym);
            }
            currslot += num_to_set;
        }
    }
    let count = prefix_cur[11].wrapping_sub(CODE_PREFIX_ORG[11]) as usize;
    if count != 0 {
        if currslot + count > 2048 {
            return false;
        }
        let start = CODE_PREFIX_ORG[11] as usize;
        let Some(these) = syms.get(start..start + count) else {
            return false;
        };
        lut.len[currslot..currslot + count].fill(11);
        lut.sym[currslot..currslot + count].copy_from_slice(these);
        currslot += count;
    }
    currslot == 2048
}

/// Port of `Kraken_DecodeBytes_Type12` (kraken.cpp:1184): a Huffman block in three streams (`kind`
/// 1, entropy type 2) or two halves of three (`kind` 2, type 4). Returns the bytes read.
pub(super) fn decode(src: &[u8], out: &mut [u8], kind: u8) -> Result<usize, Error> {
    let mut bits = BitReader::new(src);
    let mut code_prefix = CODE_PREFIX_ORG;
    let mut syms = [0u8; 1280];
    let num_syms = if bits.read_bit_no_refill() == 0 {
        read_code_lengths_old(&mut bits, &mut syms, &mut code_prefix)?
    } else if bits.read_bit_no_refill() == 0 {
        read_code_lengths_new(&mut bits, &mut syms, &mut code_prefix)?
    } else {
        return Err(bad());
    };
    if num_syms < 1 {
        return Err(bad());
    }
    let pos = bits.p - ((24 - bits.bitpos) / 8) as isize;
    let pos = usize::try_from(pos).map_err(|_| bad())?;

    if num_syms == 1 {
        // ooz fills the output, then returns `src - src_end` (kraken.cpp:1218): never the block's
        // size, so its caller refuses the block, and so does the port's.
        out.fill(syms[0]);
        return usize::try_from(pos as isize - src.len() as isize).map_err(|_| bad());
    }

    let mut lut = Lut {
        len: [0; 2048],
        sym: [0; 2048],
    };
    if !make_lut(&code_prefix, &mut lut, &syms) {
        return Err(bad());
    }
    let mut rev = RevLut {
        len: [0; 2048],
        sym: [0; 2048],
    };
    reverse_bits_array_2048(&lut.len, &mut rev.len);
    reverse_bits_array_2048(&lut.sym, &mut rev.sym);

    if kind == 1 {
        if pos + 3 > src.len() {
            return Err(bad());
        }
        let split_mid = le16_at(src, pos as isize) as usize;
        let pos = pos + 2;
        if !decode_bytes_core(src, out, pos, pos + split_mid, src.len(), &rev) {
            return Err(bad());
        }
    } else {
        if pos + 6 > src.len() {
            return Err(bad());
        }
        let half_output_size = (out.len() + 1) >> 1;
        let split_mid = (le32_at(src, pos as isize) & 0xFF_FFFF) as usize;
        let pos = pos + 3;
        if split_mid > src.len() - pos {
            return Err(bad());
        }
        let src_mid = pos + split_mid;
        let split_left = le16_at(src, pos as isize) as usize;
        let pos = pos + 2;
        // Signed, as in ooz: the middle can come before `pos`.
        if (src_mid as isize - pos as isize) < (split_left + 2) as isize || src.len() - src_mid < 3
        {
            return Err(bad());
        }
        let split_right = le16_at(src, src_mid as isize) as usize;
        if src.len() - (src_mid + 2) < split_right + 2 {
            return Err(bad());
        }
        let (first, second) = out.split_at_mut(half_output_size);
        if !decode_bytes_core(src, first, pos, pos + split_left, src_mid, &rev) {
            return Err(bad());
        }
        let right = src_mid + 2;
        if !decode_bytes_core(src, second, right, right + split_right, src.len(), &rev) {
            return Err(bad());
        }
    }
    Ok(src.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rice_tables_say_what_their_bytes_mean() {
        // Each byte, highest bit first: the zeros before each 1, the first four in the low
        // nibbles of bytes 0-3 and the rest in the high nibbles, and the zeros after the last 1 in
        // the top nibble. ooz's tables were copied, so check them against that.
        for v in 0..256u32 {
            let (mut want, mut k, mut zeros) = (0u32, 0, 0);
            for bit in (0..8).rev() {
                if v >> bit & 1 == 1 {
                    want |= zeros << if k < 4 { 8 * k } else { 8 * (k - 4) + 4 };
                    k += 1;
                    zeros = 0;
                } else {
                    zeros += 1;
                }
            }
            want |= zeros << 28;
            assert_eq!(RICE_CODE_BITS2_VALUE[v as usize], want, "{v:#04x}");
            assert_eq!(u32::from(RICE_CODE_BITS2_LEN[v as usize]), v.count_ones());
        }
    }

    #[test]
    fn golomb_rice_lengths_stop_inside_a_byte() {
        // 01 01 1000: codes of 1 and 1 zero, then a third code not wanted, so the reader stops
        // after four bits.
        let src = [0b0101_1000];
        let mut br = BitReader2 {
            src: &src,
            p: 0,
            bitpos: 0,
        };
        let mut dst = [0u8; 16];
        assert!(decode_golomb_rice_lengths(&mut dst, 2, &mut br));
        assert_eq!(&dst[..2], [1, 1]);
        assert_eq!((br.p, br.bitpos), (0, 4));
    }

    #[test]
    fn golomb_rice_bits_shift_in_low_bits_as_ooz_does() {
        // Three bits each, 101 and 010: 1 * 8 + 5 and 2 * 8 + 2. The bytes past the values come
        // back as they were.
        let src = [0b1010_1000];
        let mut br = BitReader2 {
            src: &src,
            p: 0,
            bitpos: 0,
        };
        let mut dst = [1, 2, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9];
        assert!(decode_golomb_rice_bits(&mut dst, 2, 3, &mut br));
        assert_eq!(dst, [13, 18, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9]);
        assert_eq!((br.p, br.bitpos), (0, 6));
        // ooz shifts 8 values as one u64: 40 * 8 carries 1 into the next value.
        let mut br = BitReader2 {
            src: &src,
            p: 0,
            bitpos: 0,
        };
        let mut dst = [40, 2, 0, 0, 0, 0, 0, 0, 0, 0];
        assert!(decode_golomb_rice_bits(&mut dst, 2, 3, &mut br));
        assert_eq!(&dst[..2], [64 + 5, 2 * 8 + 2 + 1]);
    }

    #[test]
    fn old_sparse_lengths_and_three_streams() {
        // The table, highest bit first: 0 (old), 0 (sparse), 00000010 (two symbols), 000 (no
        // length bits: each code is 1 bit), 'a', 'b'. Then the middle stream's offset, 1, and a
        // byte each for the three streams, read lowest bit first: forwards 0, 0 ('a', 'a'); the
        // middle, 1 ('b'); and backwards from the end, 1 ('b'). They take turns: forwards, end,
        // middle.
        let block = [0x00, 0x83, 0x0B, 0x10, 0x01, 0x00, 0x00, 0x01, 0x01];
        let mut out = [0u8; 4];
        assert_eq!(decode(&block, &mut out, 1), Ok(block.len()));
        assert_eq!(&out, b"abba");
        // The middle stream starting past the end, and a table of no symbols.
        let mut bad_split = block;
        bad_split[4] = 9;
        assert!(decode(&bad_split, &mut [0u8; 4], 1).is_err());
        let mut no_symbols = block;
        no_symbols[1] = 0x03;
        assert!(decode(&no_symbols, &mut [0u8; 4], 1).is_err());
    }
}
