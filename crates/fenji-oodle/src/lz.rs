//! Part of fenji-oodle, a Rust port of the Oodle decoders in ooz's `kraken.cpp`
//! (github.com/powzix/ooz, commit 05038060aa68), Copyright (C) 2016 Powzix.
//! Changes Copyright (C) 2026 the Fenji contributors.
//! GPL-3.0-or-later: see COPYING and NOTICE.
//!
//! The match copy every LZ codec shares. ooz copies matches 8 bytes at a time with `COPY_64`
//! (kraken.cpp:441), past the match's end and without checking its source; this copy is exact and
//! checked.
use crate::Error;

/// Copies `len` bytes from `offset` back, to `pos`. Overlapping copies repeat bytes as LZ needs.
///
/// An offset of 0, or one reaching before the start of `dst`, is corrupt (Review Focus 4), and so
/// is a match running past the end of `dst`. For the offsets Kraken writes, 8 or more, the result
/// is the one ooz's 8-byte copies give.
pub(crate) fn copy_match(
    dst: &mut [u8],
    pos: usize,
    offset: usize,
    len: usize,
) -> Result<(), Error> {
    if offset == 0 || offset > pos {
        return Err(Error::Corrupt {
            at: pos,
            what: "a match reaches before the start of the output",
        });
    }
    let end = pos
        .checked_add(len)
        .filter(|&e| e <= dst.len())
        .ok_or(Error::Corrupt {
            at: pos,
            what: "a match runs past the quantum's output",
        })?;
    if offset >= len {
        dst.copy_within(pos - offset..pos - offset + len, pos);
    } else {
        for i in pos..end {
            dst[i] = dst[i - offset];
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_overlapping_match_repeats_its_bytes() {
        let mut dst = *b"ab\0\0\0\0\0";
        copy_match(&mut dst, 2, 2, 5).unwrap();
        assert_eq!(&dst, b"abababa");
        let mut dst = *b"abc\0\0\0";
        copy_match(&mut dst, 3, 3, 3).unwrap();
        assert_eq!(&dst, b"abcabc");
    }

    #[test]
    fn a_match_past_the_end_is_corrupt() {
        let mut dst = [0u8; 8];
        assert!(matches!(
            copy_match(&mut dst, 4, 4, 5),
            Err(Error::Corrupt { .. })
        ));
        assert!(matches!(
            copy_match(&mut dst, 4, 4, usize::MAX),
            Err(Error::Corrupt { .. })
        ));
    }
}
