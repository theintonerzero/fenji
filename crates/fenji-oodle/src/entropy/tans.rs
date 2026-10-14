//! Part of fenji-oodle, a Rust port of the Oodle decoders in ooz's `kraken.cpp`
//! (github.com/powzix/ooz, commit 05038060aa68), Copyright (C) 2016 Powzix.
//! Changes Copyright (C) 2026 the Fenji contributors.
//! GPL-3.0-or-later: see COPYING and NOTICE.
//!
//! tANS blocks (type 1, kraken.cpp:1636–2061).
use super::huffman::{HuffRange, convert_to_ranges, decode_golomb_rice_lengths, read_fluff};
use crate::Error;
use crate::bits::{BitReader, le32_at, len, shl, shr};

/// The error for ooz's `return -1` and `return false` in this file.
fn bad() -> Error {
    Error::corrupt("a bad tANS block")
}

/// Port of `TansData` (kraken.cpp:1636): the symbols of weight 1 in `a`, and the others in `b`
/// as `symbol << 16 | weight`.
struct TansData {
    a_used: usize,
    b_used: usize,
    a: [u8; 256],
    b: [u32; 256],
}

/// ooz's `sizeof(TansLutEnt)`, for its check of the scratch's room.
const LUT_ENTRY_SIZE: usize = 8;

/// Port of `TansLutEnt` (kraken.cpp:1793).
#[derive(Debug, Clone, Copy, Default)]
struct TansLutEnt {
    x: u32,
    bits_x: u8,
    symbol: u8,
    w: u16,
}

/// Port of `Tans_DecodeTable` (kraken.cpp:1654): `false` where ooz returns false.
fn decode_table(bits: &mut BitReader<'_>, l_bits: u32, t: &mut TansData) -> bool {
    bits.refill();
    if bits.read_bit_no_refill() != 0 {
        let q = bits.read_bits_no_refill(3);
        let num_symbols = bits.read_bits_no_refill(8) as usize + 1;
        if num_symbols < 2 {
            return false;
        }
        let fluff = read_fluff(bits, num_symbols);
        let total_rice_values = fluff + num_symbols;
        let mut rice = [0u8; 512 + 16];

        // Another bit reader.
        let Some(mut br2) = bits.byte_reader() else {
            return false;
        };
        if !decode_golomb_rice_lengths(&mut rice, total_rice_values, &mut br2) {
            return false;
        }
        let Some(past) = rice.get_mut(total_rice_values..total_rice_values + 16) else {
            return false;
        };
        past.fill(0);

        // Back to the first.
        bits.resume_at(&br2);

        let mut range = [HuffRange::default(); 133];
        let Some(rest) = rice.get(num_symbols..) else {
            return false;
        };
        let Ok(ranges) = convert_to_ranges(&mut range, num_symbols, fluff, rest, bits) else {
            return false;
        };

        bits.refill();

        let l = 1i32 << l_bits;
        let mut cur_rice = rice.iter();
        let mut average: i32 = 6;
        let mut somesum: i32 = 0;
        let (mut a_at, mut b_at) = (0usize, 0usize);
        for r in &range[..ranges] {
            for symbol in r.symbol..r.symbol + r.num {
                bits.refill();
                let Some(&rice) = cur_rice.next() else {
                    return false;
                };
                let nextra = q + u32::from(rice);
                if nextra > 15 {
                    return false;
                }
                let mut v =
                    (bits.read_bits_no_refill_zero(nextra) + (1 << nextra) - (1 << q)) as i32;

                let average_div4 = average >> 2;
                let mut limit = 2 * average_div4;
                if v <= limit {
                    // ooz's `-(v & 1) ^ ((uint32)v >> 1)`, in unsigned arithmetic.
                    let zigzag = (-(v & 1)) as u32 ^ ((v as u32) >> 1);
                    v = (average_div4 as u32).wrapping_add(zigzag) as i32;
                }
                if limit > v {
                    limit = v;
                }
                v += 1;
                average += limit - average_div4;
                // ooz writes both tables, and keeps the entry in one of them, or neither.
                let (Some(a), Some(b)) = (t.a.get_mut(a_at), t.b.get_mut(b_at)) else {
                    return false;
                };
                *a = symbol as u8;
                *b = ((symbol as u32) << 16).wrapping_add(v as u32);
                a_at += usize::from(v == 1);
                b_at += usize::from(v >= 2);
                somesum += v;
            }
        }
        t.a_used = a_at;
        t.b_used = b_at;
        somesum == l
    } else {
        let mut seen = [false; 256];
        let l = 1u32 << l_bits;

        let mut count = bits.read_bits_no_refill(3) + 1;

        let bits_per_sym = l_bits.ilog2() + 1;
        let max_delta_bits = bits.read_bits_no_refill(bits_per_sym);

        if max_delta_bits == 0 || max_delta_bits > l_bits {
            return false;
        }

        let (mut a_at, mut b_at) = (0usize, 0usize);
        let mut weight: u32 = 0;
        let mut total_weights: u32 = 0;

        loop {
            bits.refill();

            let sym = bits.read_bits_no_refill(8) as usize;
            if seen[sym] {
                return false;
            }

            let delta = bits.read_bits_no_refill(max_delta_bits);

            weight += delta;

            if weight == 0 {
                return false;
            }

            seen[sym] = true;
            let kept = if weight == 1 {
                t.a.get_mut(a_at).map(|a| {
                    *a = sym as u8;
                    a_at += 1;
                })
            } else {
                t.b.get_mut(b_at).map(|b| {
                    *b = ((sym as u32) << 16) + weight;
                    b_at += 1;
                })
            };
            if kept.is_none() {
                return false;
            }

            total_weights += weight;
            count -= 1;
            if count == 0 {
                break;
            }
        }

        bits.refill();

        let sym = bits.read_bits_no_refill(8) as usize;
        if seen[sym] {
            return false;
        }

        // Unsigned, as in ooz: a total past L wraps.
        let rest = l.wrapping_sub(total_weights);
        if rest < weight || rest <= 1 {
            return false;
        }

        let Some(b) = t.b.get_mut(b_at) else {
            return false;
        };
        *b = ((sym as u32) << 16).wrapping_add(rest);
        b_at += 1;

        t.a_used = a_at;
        t.b_used = b_at;

        simple_sort(&mut t.a[..a_at]);
        simple_sort(&mut t.b[..b_at]);
        true
    }
}

/// Port of `SimpleSort` (kraken.cpp:1643), an insertion sort: any sort gives the same order.
fn simple_sort<T: Ord>(items: &mut [T]) {
    items.sort_unstable();
}

/// Port of `Tans_InitLut` (kraken.cpp:1800). `false` for a table no valid stream has, where ooz
/// would write outside its table, shift by a negative count, or loop for good.
fn init_lut(t: &TansData, l_bits: u32, lut: &mut [TansLutEnt]) -> bool {
    let l = 1i32 << l_bits;
    let Some(slots_left_to_alloc) = (1usize << l_bits).checked_sub(t.a_used) else {
        return false;
    };

    let sa = slots_left_to_alloc >> 2;
    let mut pointers = [0usize; 4];
    let mut sb = sa + usize::from((slots_left_to_alloc & 3) > 0);
    pointers[1] = sb;
    sb += sa + usize::from((slots_left_to_alloc & 3) > 1);
    pointers[2] = sb;
    sb += sa + usize::from((slots_left_to_alloc & 3) > 2);
    pointers[3] = sb;

    // The entries with weight 1.
    let single = TansLutEnt {
        x: (1 << l_bits) - 1,
        bits_x: l_bits as u8,
        symbol: 0,
        w: 0,
    };
    for (i, &symbol) in t.a[..t.a_used].iter().enumerate() {
        let Some(e) = lut.get_mut(slots_left_to_alloc + i) else {
            return false;
        };
        *e = TansLutEnt { symbol, ..single };
    }

    // The entries with weight 2 or more.
    let mut weights_sum: i32 = 0;
    for &entry in &t.b[..t.b_used] {
        let weight = (entry & 0xFFFF) as i32;
        let symbol = (entry >> 16) as u8;
        if weight > 4 {
            let sym_bits = weight.ilog2();
            let mut z = l_bits as i32 - sym_bits as i32;
            if z < 1 {
                // A weight of L or more: ooz would go on to shift by -1.
                return false;
            }
            let mut le = TansLutEnt {
                symbol,
                bits_x: z as u8,
                x: (1 << z) - 1,
                w: ((l - 1) & (weight << z)) as u16,
            };
            let mut what_to_add = 1i32 << z;
            let mut x = (1i32 << (sym_bits + 1)) - weight;

            for (j, pointer) in pointers.iter_mut().enumerate() {
                let mut dst = *pointer;

                let y = (weight + ((weights_sum - j as i32 - 1) & 3)) >> 2;
                let mut put = |le: &mut TansLutEnt, n: i32, what_to_add: i32| {
                    for _ in 0..n {
                        let Some(e) = lut.get_mut(dst) else {
                            return false;
                        };
                        *e = *le;
                        dst += 1;
                        le.w = le.w.wrapping_add(what_to_add as u16);
                    }
                    true
                };
                if x >= y {
                    if !put(&mut le, y, what_to_add) {
                        return false;
                    }
                    x -= y;
                } else {
                    if !put(&mut le, x, what_to_add) {
                        return false;
                    }
                    z -= 1;
                    if z < 0 {
                        return false;
                    }

                    what_to_add >>= 1;
                    le.bits_x = z as u8;
                    le.w = 0;
                    le.x >>= 1;
                    if !put(&mut le, y - x, what_to_add) {
                        return false;
                    }
                    x = weight;
                }
                *pointer = dst;
            }
        } else {
            if weight < 1 {
                // ooz asserts it away, then loops for good.
                return false;
            }
            let mut bits = ((1u32 << weight) - 1) << (weights_sum & 3);
            bits |= bits >> 4;
            // ooz's `ww` runs from the weight up, one slot each.
            for ww in weight..2 * weight {
                let idx = bits.trailing_zeros() as usize;
                bits &= bits.wrapping_sub(1);
                let Some(pointer) = pointers.get_mut(idx) else {
                    return false;
                };
                let Some(e) = lut.get_mut(*pointer) else {
                    return false;
                };
                *pointer += 1;
                let weight_bits = ww.ilog2();
                let z = l_bits - weight_bits;
                *e = TansLutEnt {
                    symbol,
                    bits_x: z as u8,
                    x: (1 << z) - 1,
                    w: ((l - 1) & (ww << z)) as u16,
                };
            }
        }
        weights_sum += weight;
    }
    true
}

/// One of `Tans_Decode`'s two bit streams.
struct Stream {
    ptr: isize,
    bits: u32,
    bitpos: i32,
}

impl Stream {
    /// ooz's `TANS_FORWARD_BITS` (kraken.cpp:1916).
    fn forward_bits(&mut self, src: &[u8]) {
        self.bits |= shl(le32_at(src, self.ptr), self.bitpos.unsigned_abs());
        self.ptr += ((31 - self.bitpos) >> 3) as isize;
        self.bitpos |= 24;
    }

    /// ooz's `TANS_BACKWARD_BITS` (kraken.cpp:1930).
    fn backward_bits(&mut self, src: &[u8]) {
        self.bits |= shl(
            le32_at(src, self.ptr - 4).swap_bytes(),
            self.bitpos.unsigned_abs(),
        );
        self.ptr -= ((31 - self.bitpos) >> 3) as isize;
        self.bitpos |= 24;
    }
}

/// Port of `Tans_Decode` (kraken.cpp:1903): five interleaved states, their bits read forwards
/// and backwards, until `dst_end`; then the five final states are the last five bytes.
fn tans_decode(
    src: &[u8],
    lut: &[TansLutEnt],
    out: &mut [u8],
    dst_end: usize,
    mut f: Stream,
    mut b: Stream,
    mut states: [u32; 5],
) -> bool {
    if f.ptr > b.ptr {
        return false;
    }
    let mut dst = 0;
    if dst < dst_end {
        // ooz's unrolled rounds: refill, two states, refill, two, refill, one; forwards, then
        // backwards.
        'rounds: loop {
            for backwards in [false, true] {
                let s = if backwards { &mut b } else { &mut f };
                for group in [&[0usize, 1][..], &[2, 3], &[4]] {
                    if backwards {
                        s.backward_bits(src);
                    } else {
                        s.forward_bits(src);
                    }
                    for &k in group {
                        let Some(e) = lut.get(states[k] as usize) else {
                            return false;
                        };
                        out[dst] = e.symbol;
                        dst += 1;
                        s.bitpos -= i32::from(e.bits_x);
                        states[k] = (s.bits & e.x) + u32::from(e.w);
                        s.bits = shr(s.bits, u32::from(e.bits_x));
                        if dst >= dst_end {
                            break 'rounds;
                        }
                    }
                }
            }
        }
    }

    if b.ptr - f.ptr + (f.bitpos >> 3) as isize + (b.bitpos >> 3) as isize != 0 {
        return false;
    }

    if states.iter().fold(0, |or, s| or | s) & !0xFF != 0 {
        return false;
    }

    let Some(last) = out.get_mut(dst_end..dst_end + 5) else {
        return false;
    };
    for (o, &s) in last.iter_mut().zip(&states) {
        *o = s as u8;
    }
    true
}

/// Port of `Krak_DecodeTans` (kraken.cpp:1980): returns the bytes read.
pub(super) fn decode(src: &[u8], out: &mut [u8], scratch: &mut [u8]) -> Result<usize, Error> {
    if src.len() < 8 || out.len() < 5 {
        return Err(bad());
    }

    let mut br = BitReader::new(src);

    // A reserved bit.
    if br.read_bit_no_refill() != 0 {
        return Err(bad());
    }

    let l_bits = br.read_bits_no_refill(2) + 8;

    let mut t = TansData {
        a_used: 0,
        b_used: 0,
        a: [0; 256],
        b: [0; 256],
    };
    if !decode_table(&mut br, l_bits, &mut t) {
        return Err(bad());
    }

    let mut at = br.p - ((24 - br.bitpos) / 8) as isize;
    let mut src_end = len(src);

    if at >= src_end {
        return Err(bad());
    }

    // ooz builds the table in its scratch (kraken.cpp:2009–2018); the port keeps the table apart,
    // and keeps the check for its room.
    let lut_space_required = ((LUT_ENTRY_SIZE << l_bits) + 15) & !15;
    if lut_space_required > scratch.len() {
        return Err(bad());
    }

    let mut lut = [TansLutEnt::default(); 1 << 11];
    let lut = &mut lut[..1 << l_bits];
    if !init_lut(&t, l_bits, lut) {
        return Err(bad());
    }

    // The initial states.
    let l_mask = (1u32 << l_bits) - 1;
    let mut bits_f = le32_at(src, at);
    at += 4;
    let mut bits_b = le32_at(src, src_end - 4).swap_bytes();
    src_end -= 4;
    let mut bitpos_f: u32 = 32;
    let mut bitpos_b: u32 = 32;

    // The first two.
    let state_0 = bits_f & l_mask;
    let state_1 = bits_b & l_mask;
    bits_f >>= l_bits;
    bitpos_f -= l_bits;
    bits_b >>= l_bits;
    bitpos_b -= l_bits;

    // The next two.
    let state_2 = bits_f & l_mask;
    let state_3 = bits_b & l_mask;
    bits_f >>= l_bits;
    bitpos_f -= l_bits;
    bits_b >>= l_bits;
    bitpos_b -= l_bits;

    // More bits.
    bits_f |= shl(le32_at(src, at), bitpos_f);
    at += ((31 - bitpos_f) >> 3) as isize;
    bitpos_f |= 24;

    // The last.
    let state_4 = bits_f & l_mask;
    bits_f >>= l_bits;
    bitpos_f -= l_bits;

    let f = Stream {
        ptr: at - (bitpos_f >> 3) as isize,
        bits: bits_f,
        bitpos: (bitpos_f & 7) as i32,
    };
    let b = Stream {
        ptr: src_end + (bitpos_b >> 3) as isize,
        bits: bits_b,
        bitpos: (bitpos_b & 7) as i32,
    };
    let dst_end = out.len() - 5;
    let states = [state_0, state_1, state_2, state_3, state_4];
    if !tans_decode(src, lut, out, dst_end, f, b, states) {
        return Err(bad());
    }

    Ok(src.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A table, highest bit first: 0 (reserved), 00 (L = 256), 0 (sparse), 000 (one symbol, then
    /// the last), 1000 (weights in 8 bits), 'a' of weight 128, and 'b', the last, of the rest.
    const TABLE: [u8; 5] = [0x01, 0x0C, 0x30, 0x0C, 0x40];

    fn tans(block: &[u8], n: usize) -> Result<Vec<u8>, Error> {
        let mut out = vec![0u8; n];
        let used = decode(block, &mut out, &mut [0u8; 4096])?;
        assert_eq!(used, block.len());
        Ok(out)
    }

    #[test]
    fn with_five_bytes_the_states_are_the_output() {
        // Forwards states 0, 2 and 4; from the end, 1 and 3. Five bytes leave no rounds to run.
        let mut block = TABLE.to_vec();
        block.extend([10, 30, 50, 40, 20]);
        assert_eq!(tans(&block, 5).unwrap(), [10, 20, 30, 40, 50]);
        // A byte left over: the streams don't meet.
        block.push(99);
        assert!(tans(&block, 5).is_err());
    }

    #[test]
    fn a_round_then_the_states() {
        // Each state has 1 bit. State 10 is 'a' with a base of 20 (the table spreads 'a' with
        // bases 0, 2, ... over its first 32 entries); the next forward bit, the lowest of 0x01,
        // makes it 21.
        let mut block = TABLE.to_vec();
        block.extend([10, 30, 50, 0x01, 40, 20]);
        assert_eq!(tans(&block, 6).unwrap(), [b'a', 21, 20, 30, 40, 50]);
        // ooz's room check for the table in its scratch: 8 bytes an entry.
        let mut out = [0u8; 6];
        assert!(decode(&block, &mut out, &mut [0u8; 2047]).is_err());
        assert_eq!(decode(&block, &mut out, &mut [0u8; 2048]), Ok(block.len()));
    }
}
