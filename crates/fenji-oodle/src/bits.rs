//! Part of fenji-oodle, a Rust port of the Oodle decoders in ooz's `kraken.cpp`
//! (github.com/powzix/ooz, commit 05038060aa68), Copyright (C) 2016 Powzix.
//! Changes Copyright (C) 2026 the Fenji contributors.
//! GPL-3.0-or-later: see COPYING and NOTICE.
//!
//! ooz's bit readers (kraken.cpp:201–436). `BSR` and `BSF` become `u32::leading_zeros` and
//! `trailing_zeros` where they're used. `BitReader_ReadGamma`, `BitReader_ReadGammaX` and
//! `Log2RoundUp` aren't ported: nothing in kraken.cpp calls them.

/// `x << n`, which is 0 once `n` reaches 32 (C leaves that shift undefined).
pub(crate) fn shl(x: u32, n: u32) -> u32 {
    x.checked_shl(n).unwrap_or(0)
}

/// `x >> n`, which is 0 once `n` reaches 32.
pub(crate) fn shr(x: u32, n: u32) -> u32 {
    x.checked_shr(n).unwrap_or(0)
}

/// Port of ooz's `BitReader` (kraken.cpp:151), forwards or backwards.
///
/// `bits` holds the next bits at its top, the next bit highest. The next byte loads at bit
/// `bitpos`, so `24 − bitpos` bits are valid, and a refill tops them up to between 24 and 31.
/// Forwards the next byte is `src[p]`, backwards `src[p − 1]`. `p` runs on past either end of `src`,
/// where the reader reads zeros: ooz reads its padded buffers there, and a stream that needs those
/// bits fails its decoder's end checks.
#[derive(Debug, Clone)]
pub(crate) struct BitReader<'a> {
    src: &'a [u8],
    pub p: isize,
    pub bits: u32,
    pub bitpos: i32,
}

impl<'a> BitReader<'a> {
    /// A forwards reader at the start of `src`, refilled, as ooz sets one up.
    pub fn new(src: &'a [u8]) -> Self {
        let mut b = Self {
            src,
            p: 0,
            bits: 0,
            bitpos: 24,
        };
        b.refill();
        b
    }

    /// A backwards reader from the end of `src`, refilled.
    pub fn new_backwards(src: &'a [u8]) -> Self {
        let mut b = Self {
            src,
            p: len(src),
            bits: 0,
            bitpos: 24,
        };
        b.refill_backwards();
        b
    }

    fn byte(&self, p: isize) -> u32 {
        byte_at(self.src, p)
    }

    /// Port of `BitReader_Refill` (kraken.cpp:214).
    pub fn refill(&mut self) {
        while self.bitpos > 0 {
            self.bits |= shl(self.byte(self.p), self.bitpos.unsigned_abs());
            self.bitpos -= 8;
            self.p += 1;
        }
    }

    /// Port of `BitReader_RefillBackwards` (kraken.cpp:225).
    pub fn refill_backwards(&mut self) {
        while self.bitpos > 0 {
            self.p -= 1;
            self.bits |= shl(self.byte(self.p), self.bitpos.unsigned_abs());
            self.bitpos -= 8;
        }
    }

    /// Port of `BitReader_ReadBit` (kraken.cpp:235).
    pub fn read_bit(&mut self) -> u32 {
        self.refill();
        self.read_bit_no_refill()
    }

    /// Port of `BitReader_ReadBitNoRefill` (kraken.cpp:244).
    pub fn read_bit_no_refill(&mut self) -> u32 {
        let r = self.bits >> 31;
        self.bits <<= 1;
        self.bitpos += 1;
        r
    }

    /// Port of `BitReader_ReadBitsNoRefill` (kraken.cpp:254): `n` bits, 1 to 32.
    pub fn read_bits_no_refill(&mut self, n: u32) -> u32 {
        let n = n.min(32);
        let r = shr(self.bits, 32 - n);
        self.bits = shl(self.bits, n);
        self.bitpos += n as i32;
        r
    }

    /// Port of `BitReader_ReadBitsNoRefillZero` (kraken.cpp:262): `n` bits, 0 to 31.
    pub fn read_bits_no_refill_zero(&mut self, n: u32) -> u32 {
        let n = n.min(32);
        let r = shr(self.bits >> 1, 31u32.wrapping_sub(n));
        self.bits = shl(self.bits, n);
        self.bitpos += n as i32;
        r
    }

    /// Port of `BitReader_ReadMoreThan24Bits` (kraken.cpp:269): `n` bits, 0 to 32.
    pub fn read_more_than_24_bits(&mut self, n: u32) -> u32 {
        self.more_than_24_bits(n, Self::refill)
    }

    /// Port of `BitReader_ReadMoreThan24BitsB` (kraken.cpp:282).
    pub fn read_more_than_24_bits_b(&mut self, n: u32) -> u32 {
        self.more_than_24_bits(n, Self::refill_backwards)
    }

    fn more_than_24_bits(&mut self, n: u32, refill: fn(&mut Self)) -> u32 {
        let rv = if n <= 24 {
            self.read_bits_no_refill_zero(n)
        } else {
            let n = n.min(32);
            let high = self.read_bits_no_refill(24) << (n - 24);
            refill(self);
            high.wrapping_add(self.read_bits_no_refill(n - 24))
        };
        refill(self);
        rv
    }

    /// Port of `BitReader_ReadDistance` (kraken.cpp:338): the offset code for the byte `v`.
    pub fn read_distance(&mut self, v: u32) -> u32 {
        self.distance(v, Self::refill)
    }

    /// Port of `BitReader_ReadDistanceB` (kraken.cpp:365).
    pub fn read_distance_b(&mut self, v: u32) -> u32 {
        self.distance(v, Self::refill_backwards)
    }

    fn distance(&mut self, v: u32, refill: fn(&mut Self)) -> u32 {
        // The `| 1` is the code's leading 1: rotated in at bit n, it lands in the value, and the
        // mask of n + 1 bits clears it from what's left.
        let rv = if v < 0xF0 {
            let n = (v >> 4) + 4;
            let w = (self.bits | 1).rotate_left(n);
            self.bitpos += n as i32;
            let m = shl(2, n).wrapping_sub(1);
            self.bits = w & !m;
            ((w & m) << 4).wrapping_add(v & 0xF).wrapping_sub(248)
        } else {
            let n = v.wrapping_sub(0xF0).wrapping_add(4).min(32);
            let w = (self.bits | 1).rotate_left(n);
            self.bitpos += n as i32;
            let m = shl(2, n).wrapping_sub(1);
            self.bits = w & !m;
            let high = 8_322_816u32.wrapping_add((w & m) << 12);
            refill(self);
            let rv = high.wrapping_add(self.bits >> 20);
            self.bitpos += 12;
            self.bits <<= 12;
            rv
        };
        refill(self);
        rv
    }

    /// Port of `BitReader_ReadLength` (kraken.cpp:391): `None` where ooz returns false, for more
    /// than 12 leading zeros.
    pub fn read_length(&mut self) -> Option<u32> {
        self.length(Self::refill)
    }

    /// Port of `BitReader_ReadLengthB` (kraken.cpp:411).
    pub fn read_length_b(&mut self) -> Option<u32> {
        self.length(Self::refill_backwards)
    }

    fn length(&mut self, refill: fn(&mut Self)) -> Option<u32> {
        // ooz's `_BitScanReverse` of 0 is undefined; no set bit is more than 12 zeros here.
        let n = self.bits.leading_zeros();
        if n > 12 {
            return None;
        }
        self.bitpos += n as i32;
        self.bits <<= n;
        refill(self);
        let n = n + 7;
        self.bitpos += n as i32;
        let rv = (self.bits >> (32 - n)).wrapping_sub(64);
        self.bits <<= n;
        refill(self);
        Some(rv)
    }

    /// The byte reader ooz starts where these bits end (kraken.cpp:1057–1059 and 1667–1669); `None`
    /// before the start of `src`.
    pub fn byte_reader(&self) -> Option<BitReader2<'a>> {
        let back = (24 - self.bitpos + 7) >> 3;
        Some(BitReader2 {
            src: self.src,
            p: usize::try_from(self.p - back as isize).ok()?,
            bitpos: ((self.bitpos - 24) & 7).unsigned_abs(),
        })
    }

    /// Restarts at a byte reader's position, as ooz does after one (kraken.cpp:1068–1073 and
    /// 1676–1681).
    pub fn resume_at(&mut self, br: &BitReader2<'_>) {
        self.bitpos = 24;
        self.p = isize::try_from(br.p).unwrap_or(isize::MAX);
        self.bits = 0;
        self.refill();
        self.bits = shl(self.bits, br.bitpos);
        self.bitpos += br.bitpos as i32;
    }
}

/// `src.len()` as an `isize`, which a slice's length always fits.
pub(crate) fn len(src: &[u8]) -> isize {
    isize::try_from(src.len()).unwrap_or(isize::MAX)
}

/// The byte at `at`, or 0 outside `src`. ooz's cores read whole words near their streams' ends,
/// past them into its padding; the port reads the same bytes inside `src` and zeros outside it.
pub(crate) fn byte_at(src: &[u8], at: isize) -> u32 {
    usize::try_from(at)
        .ok()
        .and_then(|i| src.get(i))
        .map_or(0, |&b| u32::from(b))
}

/// The little-endian `u32` at `at`, its bytes outside `src` read as 0.
pub(crate) fn le32_at(src: &[u8], at: isize) -> u32 {
    let whole = usize::try_from(at)
        .ok()
        .and_then(|i| src.get(i..i.checked_add(4)?))
        .and_then(|b| <[u8; 4]>::try_from(b).ok());
    match whole {
        Some(b) => u32::from_le_bytes(b),
        None => (0..4).fold(0, |v, i| v | byte_at(src, at + i) << (8 * i)),
    }
}

/// The little-endian `u16` at `at`, its bytes outside `src` read as 0.
pub(crate) fn le16_at(src: &[u8], at: isize) -> u32 {
    byte_at(src, at) | byte_at(src, at + 1) << 8
}

/// Port of ooz's `BitReader2` (kraken.cpp:848): the byte `src[p]`, of which the `bitpos` highest
/// bits are read. Its end, ooz's `p_end`, is the end of `src`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BitReader2<'a> {
    pub src: &'a [u8],
    pub p: usize,
    pub bitpos: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwards_reads_bits_first_bit_highest_then_zeros() {
        // 1011_0001 0100_0000 1111_1111: the first refill holds all three bytes.
        let mut b = BitReader::new(&[0b1011_0001, 0b0100_0000, 0xFF]);
        assert_eq!((b.bits, b.bitpos, b.p), (0xB140_FF00, 0, 3));
        assert_eq!(b.read_bit_no_refill(), 1);
        assert_eq!(b.read_bits_no_refill(3), 0b011);
        assert_eq!(b.read_bits_no_refill_zero(0), 0);
        assert_eq!(b.read_bits_no_refill_zero(4), 0b0001);
        assert_eq!(b.bitpos, 8);
        b.refill(); // past the end: a zero byte
        assert_eq!((b.bits, b.bitpos, b.p), (0x40FF_0000, 0, 4));
        assert_eq!(b.read_bits_no_refill(8), 0x40);
        assert_eq!(b.read_bits_no_refill(8), 0xFF);
        assert_eq!(b.read_bits_no_refill(8), 0);
        assert_eq!(b.read_bit(), 0);
    }

    #[test]
    fn backwards_reads_from_the_last_byte() {
        let mut b = BitReader::new_backwards(&[0x00, 0x12, 0x80]);
        assert_eq!((b.bits, b.bitpos, b.p), (0x8012_0000, 0, 0));
        assert_eq!(b.read_bit_no_refill(), 1);
        assert_eq!(b.read_bits_no_refill(7), 0);
        assert_eq!(b.read_bits_no_refill(8), 0x12);
        b.refill_backwards(); // before the start: zero bytes, and p goes below 0
        assert_eq!((b.bitpos, b.p), (0, -2));
        assert_eq!(b.read_bits_no_refill(8), 0);
    }

    #[test]
    fn distances_by_hand() {
        // v < 0xF0: n = (v >> 4) + 4 bits after a leading 1, times 16, plus v & 15, minus 248.
        // 1011 -> 0b1_1011 = 27; 27 * 16 + 0 - 248 = 184.
        let mut b = BitReader::new(&[0b1011_0000, 0, 0, 0]);
        assert_eq!(b.read_distance(0x00), 184);
        assert_eq!((b.bits, b.bitpos, b.p), (0, -4, 4));
        // v = 0x2A: n = 6; 100001 -> 0b1_100001 = 97; 97 * 16 + 10 - 248 = 1314.
        let mut b = BitReader::new(&[0b1000_0100, 0, 0, 0]);
        assert_eq!(b.read_distance(0x2A), 1314);
        // v >= 0xF0: n = v - 0xF0 + 4; 8322816 + (0b1_0101 << 12) + the next 12 bits (0xAC3).
        let mut b = BitReader::new(&[0x5A, 0xC3, 0xF0, 0x00]);
        assert_eq!(b.read_distance(0xF0), 8_322_816 + (21 << 12) + 0xAC3);
        assert_eq!((b.bits, b.bitpos), (0xF000_0000, 0));
        // Backwards, the same bits from the other end.
        let mut b = BitReader::new_backwards(&[0, 0, 0, 0b1011_0000]);
        assert_eq!(b.read_distance_b(0x00), 184);
    }

    #[test]
    fn lengths_by_hand() {
        // 0001_0110 1000_0000: three zeros, then 3 + 7 = 10 bits from the leading 1:
        // 1011010000 = 720, minus 64.
        let mut b = BitReader::new(&[0x16, 0x80, 0x00, 0x00]);
        assert_eq!(b.read_length(), Some(656));
        let mut b = BitReader::new_backwards(&[0x00, 0x00, 0x80, 0x16]);
        assert_eq!(b.read_length_b(), Some(656));
        // Twelve leading zeros are the most a length has; thirteen, or none set, are corrupt.
        let mut b = BitReader::new(&[0x00, 0x08, 0x00, 0x00]);
        assert_eq!(b.read_length(), Some((1 << 18) - 64));
        assert_eq!(BitReader::new(&[0x00, 0x04, 0x00]).read_length(), None);
        assert_eq!(BitReader::new(&[]).read_length(), None);
    }

    #[test]
    fn more_than_24_bits_by_hand() {
        let mut b = BitReader::new(&[0xFF, 0xFF, 0xFF, 0xC0, 0x00]);
        assert_eq!(b.read_more_than_24_bits(26), (1 << 26) - 1);
        let mut b = BitReader::new(&[0b1010_1000, 0, 0]);
        assert_eq!(b.read_more_than_24_bits(0), 0);
        assert_eq!(b.read_more_than_24_bits(5), 0b10101);
        let mut b = BitReader::new_backwards(&[0x00, 0xC0, 0xFF, 0xFF, 0xFF]);
        assert_eq!(b.read_more_than_24_bits_b(26), (1 << 26) - 1);
    }

    #[test]
    fn a_byte_reader_takes_over_where_the_bits_end() {
        let src = [0xAB, 0xCD, 0xEF, 0x01];
        let mut b = BitReader::new(&src);
        b.read_bits_no_refill(10); // one byte and two bits
        let br = b.byte_reader().unwrap();
        assert_eq!((br.p, br.bitpos), (1, 2));
        // And back: refill from that byte, skipping the two bits read.
        b.resume_at(&br);
        assert_eq!((b.bits, b.bitpos, b.p), (0x37BC_0400, 2, 4));
        assert_eq!(b.read_bits_no_refill(6), 0b00_1101);
    }
}
