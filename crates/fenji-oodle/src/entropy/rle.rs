//! Part of fenji-oodle, a Rust port of the Oodle decoders in ooz's `kraken.cpp`
//! (github.com/powzix/ooz, commit 05038060aa68), Copyright (C) 2016 Powzix.
//! Changes Copyright (C) 2026 the Fenji contributors.
//! GPL-3.0-or-later: see COPYING and NOTICE.
//!
//! RLE blocks (type 3, kraken.cpp:1556–1634).
use crate::Error;

/// The error for ooz's `return -1` in this file.
fn bad() -> Error {
    Error::corrupt("a bad RLE block")
}

/// Port of `Krak_DecodeRLE` (kraken.cpp:1556): returns the bytes read.
///
/// The commands run backwards from the end of the command bytes, and the bytes they copy
/// forwards from the start. A two-byte command with one byte left is corrupt: ooz reads the byte
/// before its commands there, then fails its end checks.
pub(super) fn decode(
    src: &[u8],
    out: &mut [u8],
    scratch: &mut [u8],
    depth: u32,
) -> Result<usize, Error> {
    let &[first, ..] = src else {
        return Err(bad());
    };
    if src.len() == 1 {
        out.fill(first);
        return Ok(1);
    }
    // The command bytes: the block's own, or a block decoded into the scratch first, then the
    // rest of this one after it.
    let cmds: &[u8] = if first != 0 {
        let (n, dec_size) = super::decode_to_scratch(src, scratch.len(), scratch, depth + 1)?;
        if n == 0 {
            return Err(bad());
        }
        let cmd_len = src.len() - n + dec_size;
        let tail = src.get(n..).ok_or_else(bad)?;
        scratch
            .get_mut(dec_size..cmd_len)
            .ok_or_else(bad)?
            .copy_from_slice(tail);
        &scratch[..cmd_len]
    } else {
        &src[1..]
    };

    let (mut lo, mut hi) = (0usize, cmds.len());
    let mut dst = 0usize;
    let mut rle_byte = 0u8;
    // Copies, then repeats `rle_byte`, checking both against what's left as ooz does.
    let mut run = |lo: &mut usize, hi: usize, copy: usize, rle: usize, rle_byte: u8| {
        if out.len() - dst < copy + rle || hi - *lo < copy {
            return Err(bad());
        }
        out[dst..dst + copy].copy_from_slice(&cmds[*lo..*lo + copy]);
        *lo += copy;
        dst += copy;
        out[dst..dst + rle].fill(rle_byte);
        dst += rle;
        Ok(())
    };

    while lo < hi {
        let cmd = u32::from(cmds[hi - 1]);
        if cmd.wrapping_sub(1) >= 0x2F {
            hi -= 1;
            let bytes_to_copy = (!cmd & 0xF) as usize;
            let bytes_to_rle = (cmd >> 4) as usize;
            run(&mut lo, hi, bytes_to_copy, bytes_to_rle, rle_byte)?;
        } else if cmd == 1 {
            rle_byte = cmds[lo];
            lo += 1;
            hi -= 1;
        } else {
            // The two-byte commands: the command is the high byte.
            if hi - lo < 2 {
                return Err(bad());
            }
            let word = u32::from(cmds[hi - 2]) | cmd << 8;
            hi -= 2;
            if cmd >= 0x10 {
                let data = word - 4096;
                let bytes_to_copy = (data & 0x3F) as usize;
                let bytes_to_rle = (data >> 6) as usize;
                run(&mut lo, hi, bytes_to_copy, bytes_to_rle, rle_byte)?;
            } else if cmd >= 9 {
                let bytes_to_rle = ((word - 0x8FF) * 128) as usize;
                run(&mut lo, hi, 0, bytes_to_rle, rle_byte)?;
            } else {
                let bytes_to_copy = ((word - 511) * 64) as usize;
                run(&mut lo, hi, bytes_to_copy, 0, rle_byte)?;
            }
        }
    }
    if hi != lo || dst != out.len() {
        return Err(bad());
    }
    Ok(src.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rle(src: &[u8], n: usize) -> Result<Vec<u8>, Error> {
        let mut out = vec![0u8; n];
        let mut scratch = vec![0u8; 256];
        let used = decode(src, &mut out, &mut scratch, 0)?;
        assert_eq!(used, src.len());
        Ok(out)
    }

    #[test]
    fn commands_run_from_the_back_and_bytes_from_the_front() {
        // 0x01 takes 'X' as the byte to repeat; 0x5C copies !0x5C & 15 = 3 bytes, then repeats
        // 0x5C >> 4 = 5 times.
        assert_eq!(
            rle(&[0x00, b'X', b'a', b'b', b'c', 0x5C, 0x01], 8).unwrap(),
            b"abcXXXXX"
        );
        // Two-byte commands, the command byte high: 0x29_02 - 4096 copies 2 and repeats 100;
        // 0x09_00 repeats (0x900 - 0x8FF) * 128; 0x02_00 copies (0x200 - 511) * 64.
        let mut src = vec![0x00, b'Y', b'p', b'q'];
        src.extend(0..64u8);
        src.extend([0x00, 0x02, 0x00, 0x09, 0x02, 0x29, 0x01]);
        let mut want = b"pq".to_vec();
        want.extend([b'Y'; 100 + 128]);
        want.extend(0..64u8);
        assert_eq!(rle(&src, want.len()).unwrap(), want);
    }

    #[test]
    fn a_coded_prefix_comes_before_the_rest_of_the_commands() {
        // The block opens with a stored block of 3 bytes, "Xab"; the commands follow it.
        assert_eq!(
            rle(&[0x80, 0x03, b'X', b'a', b'b', 0x4D, 0x01], 6).unwrap(),
            b"abXXXX"
        );
    }

    #[test]
    fn one_byte_fills_and_bad_counts_are_corrupt() {
        assert_eq!(rle(&[7], 3).unwrap(), [7, 7, 7]);
        assert!(rle(&[], 3).is_err());
        // One output byte short, one too many, and a two-byte command with one byte left.
        assert!(rle(&[0x00, b'X', b'a', b'b', b'c', 0x5C, 0x01], 9).is_err());
        assert!(rle(&[0x00, b'X', b'a', b'b', b'c', 0x5C, 0x01], 7).is_err());
        assert!(rle(&[0x00, 0x29], 4).is_err());
    }
}
