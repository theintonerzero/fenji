//! Part of fenji-oodle, a Rust port of the Oodle decoders in ooz's `kraken.cpp`
//! (github.com/powzix/ooz, commit 05038060aa68), Copyright (C) 2016 Powzix.
//! Changes Copyright (C) 2026 the Fenji contributors.
//! GPL-3.0-or-later: see COPYING and NOTICE.
//!
//! Recursive blocks (type 5, kraken.cpp:1518–1554): the output in parts, each a block of its own
//! or a multi-array.
use super::multi_array::decode_multi_array;
use crate::Error;

/// The error for ooz's `return -1` in this file.
fn bad() -> Error {
    Error::corrupt("a bad recursive block")
}

/// Port of `Krak_DecodeRecursive` (kraken.cpp:1518): returns the bytes read.
pub(super) fn decode(
    src: &[u8],
    out: &mut [u8],
    scratch: &mut [u8],
    depth: u32,
) -> Result<usize, Error> {
    if src.len() < 6 {
        return Err(bad());
    }
    let n = src[0] & 0x7F;
    if n < 2 {
        return Err(bad());
    }
    if src[0] & 0x80 == 0 {
        let mut at = 1;
        let mut done = 0;
        for _ in 0..n {
            let (used, decoded) = super::decode(
                src.get(at..).ok_or_else(bad)?,
                &mut out[done..],
                scratch,
                depth + 1,
            )?;
            done += decoded;
            at += used;
        }
        if done != out.len() {
            return Err(bad());
        }
        Ok(at)
    } else {
        let mut lens = [0usize; 1];
        let (used, decoded) = decode_multi_array(src, out, &mut lens, scratch, depth + 1)?;
        if decoded != out.len() {
            return Err(bad());
        }
        Ok(used)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recursive(src: &[u8], n: usize) -> Result<(usize, Vec<u8>), Error> {
        let mut out = vec![0u8; n];
        let mut scratch = vec![0u8; 256];
        let used = decode(src, &mut out, &mut scratch, 0)?;
        Ok((used, out))
    }

    /// Two stored entropy arrays, "hello" and " world", for a multi-array.
    const ARRAYS: &[u8] = b"\x80\x05hello\x80\x06 world";

    #[test]
    fn blocks_one_after_another() {
        let src = b"\x02\x80\x03abc\x80\x02de";
        assert_eq!(recursive(src, 5).unwrap(), (src.len(), b"abcde".to_vec()));
        assert!(recursive(src, 6).is_err());
    }

    #[test]
    fn a_multi_array_of_no_entropy_arrays_holds_its_array_as_a_block() {
        // 0xC0: a multi-array (the top bit), of 0x40 parts as a recursive block sees it (2 at
        // least), and of no entropy arrays (the low 6 bits). Its one array is the block after.
        let src = b"\xC0\x80\x05hello";
        assert_eq!(recursive(src, 5).unwrap(), (src.len(), b"hello".to_vec()));
        assert!(recursive(src, 6).is_err());
    }

    #[test]
    fn a_multi_array_with_its_lengths_beside_its_indexes() {
        // Q = 0x8000 | 2 bytes of interval bits. Each index byte holds a length's bit count
        // above the array it names: 0x21 is 2 bits from array 1, 0x22 2 bits from array 2, and
        // 0x00 ends the output array, its 0-bit length read and skipped. The bits, each value
        // under a leading 1: forwards 01 (5), backwards from the end 10 (6), then forwards
        // nothing (1).
        let mut src = vec![0x82];
        src.extend(ARRAYS);
        src.extend([0x02, 0x80]);
        src.extend([0x80, 0x03, 0x21, 0x22, 0x00]);
        src.extend([0x40, 0x80]);
        assert_eq!(
            recursive(&src, 11).unwrap(),
            (src.len(), b"hello world".to_vec())
        );
    }

    #[test]
    fn a_multi_array_with_its_lengths_apart() {
        // Q = 2 bytes of interval bits: the indexes (1, 2, end), then the bit counts of the two
        // lengths, 2 and 2, each a stored block.
        let mut src = vec![0x82];
        src.extend(ARRAYS);
        src.extend([0x02, 0x00]);
        src.extend([0x80, 0x03, 0x01, 0x02, 0x00]);
        src.extend([0x80, 0x02, 0x02, 0x02]);
        src.extend([0x40, 0x80]);
        assert_eq!(
            recursive(&src, 11).unwrap(),
            (src.len(), b"hello world".to_vec())
        );
        // The intervals have to use up the entropy arrays.
        let last = src.len() - 2;
        src[last] = 0x00; // forwards 00: a length of 4, not 5
        assert!(recursive(&src, 11).is_err());
    }
}
