//! Part of fenji-oodle, a Rust port of the Oodle decoders in ooz's `kraken.cpp`
//! (github.com/powzix/ooz, commit 05038060aa68), Copyright (C) 2016 Powzix.
//! Changes Copyright (C) 2026 the Fenji contributors.
//! GPL-3.0-or-later: see COPYING and NOTICE.
//!
//! The entropy blocks Kraken's streams are made of. A block holds its bytes as they are (type 0),
//! or coded with tANS (1), Huffman (2 and 4), RLE (3), or as more blocks (5).
//!
//! ooz can leave a stored block where it is (`*output = src`, kraken.cpp:2149) and hand back a
//! pointer into the input; the port always copies it, so `force_memmove` goes. The bytes are the
//! same, and Kraken reserves the room either way (kraken.cpp:2359).
mod huffman;
mod multi_array;
mod recursive;
mod rle;
mod tans;

use crate::Error;

/// How deeply blocks may nest, in recursive blocks, multi-arrays and RLE command prefixes. ooz
/// recurses without a limit, so a hostile stream could exhaust the stack; Oodle's own streams nest
/// a level or two.
const MAX_DEPTH: u32 = 16;

/// A block's header (kraken.cpp:2126–2176).
struct Header {
    kind: u8,
    /// The header's own length.
    len: usize,
    src_size: usize,
    dst_size: usize,
}

/// The header parse `Kraken_DecodeBytes` (kraken.cpp:2122) and `Kraken_GetBlockSize`
/// (kraken.cpp:2063) share: the block at the start of `src`, decoding to at most `cap` bytes.
fn header(src: &[u8], cap: usize) -> Result<Header, Error> {
    let short = || Error::corrupt("an entropy block's header is cut short");
    let &[b0, b1, ..] = src else {
        return Err(short());
    };
    let kind = (b0 >> 4) & 7;
    if kind == 0 {
        let (src_size, len) = if b0 >= 0x80 {
            // Stored, with a 12-bit size.
            ((usize::from(b0) << 8 | usize::from(b1)) & 0xFFF, 2)
        } else {
            let b2 = *src.get(2).ok_or_else(short)?;
            let size = usize::from(b0) << 16 | usize::from(b1) << 8 | usize::from(b2);
            if size & !0x3FFFF != 0 {
                return Err(Error::corrupt("a stored block's size sets reserved bits"));
            }
            (size, 3)
        };
        if src_size > cap || src.len() - len < src_size {
            return Err(Error::corrupt("a stored block is larger than its room"));
        }
        return Ok(Header {
            kind,
            len,
            src_size,
            dst_size: src_size,
        });
    }
    let (src_size, dst_size, len) = if b0 >= 0x80 {
        // Short: 10-bit sizes.
        let b2 = *src.get(2).ok_or_else(short)?;
        let bits = u32::from(b0) << 16 | u32::from(b1) << 8 | u32::from(b2);
        let src_size = (bits & 0x3FF) as usize;
        (src_size, src_size + ((bits >> 10) & 0x3FF) as usize + 1, 3)
    } else {
        // Long: 18-bit sizes.
        let &[_, b1, b2, b3, b4, ..] = src else {
            return Err(short());
        };
        let bits = u32::from_be_bytes([b1, b2, b3, b4]);
        let src_size = (bits & 0x3FFFF) as usize;
        let dst_size = (((bits >> 18) | u32::from(b0) << 14) & 0x3FFFF) as usize + 1;
        if src_size >= dst_size {
            return Err(Error::corrupt(
                "an entropy block no smaller than its output",
            ));
        }
        (src_size, dst_size, 5)
    };
    if src.len() - len < src_size || dst_size > cap {
        return Err(Error::corrupt("an entropy block is larger than its room"));
    }
    Ok(Header {
        kind,
        len,
        src_size,
        dst_size,
    })
}

/// Port of `Kraken_GetBlockSize` (kraken.cpp:2063): how many bytes the block at the start of
/// `src` decodes to, at most `cap`. ooz refuses types 6 and 7 before reading the sizes, and the
/// port after; the same blocks fail.
fn block_size(src: &[u8], cap: usize) -> Result<usize, Error> {
    let h = header(src, cap)?;
    if h.kind >= 6 {
        return Err(Error::corrupt("an entropy block of a reserved type"));
    }
    Ok(h.dst_size)
}

/// Port of `Kraken_DecodeBytes` (kraken.cpp:2122): decodes the block at the start of `src` into
/// `out`, at most `out.len()` bytes, and returns the bytes it read and the bytes it decoded.
pub(crate) fn decode_bytes(
    src: &[u8],
    out: &mut [u8],
    scratch: &mut [u8],
) -> Result<(usize, usize), Error> {
    decode(src, out, scratch, 0)
}

/// [`decode_bytes`] with its output at the start of `scratch`, at most `cap` bytes, as ooz calls
/// it with `*output == scratch`: the block's own decoders get the scratch past its output
/// (kraken.cpp:2179–2183).
pub(crate) fn decode_bytes_to_scratch(
    src: &[u8],
    cap: usize,
    scratch: &mut [u8],
) -> Result<(usize, usize), Error> {
    decode_to_scratch(src, cap, scratch, 0)
}

fn decode(
    src: &[u8],
    out: &mut [u8],
    scratch: &mut [u8],
    depth: u32,
) -> Result<(usize, usize), Error> {
    let h = header(src, out.len())?;
    let out = out
        .get_mut(..h.dst_size)
        .ok_or(Error::corrupt("an entropy block is larger than its room"))?;
    body(&h, src, out, scratch, depth)
}

fn decode_to_scratch(
    src: &[u8],
    cap: usize,
    scratch: &mut [u8],
    depth: u32,
) -> Result<(usize, usize), Error> {
    let h = header(src, cap)?;
    if scratch.len() < h.dst_size {
        return Err(Error::corrupt(
            "an entropy block is larger than the scratch",
        ));
    }
    let (out, rest) = scratch.split_at_mut(h.dst_size);
    body(&h, src, out, rest, depth)
}

/// The rest of `Kraken_DecodeBytes` (kraken.cpp:2146–2206): `out` is exactly the block's output.
fn body(
    h: &Header,
    src: &[u8],
    out: &mut [u8],
    scratch: &mut [u8],
    depth: u32,
) -> Result<(usize, usize), Error> {
    if depth > MAX_DEPTH {
        return Err(Error::corrupt("entropy blocks nest too deeply"));
    }
    let end = h.len + h.src_size;
    let block = src
        .get(h.len..end)
        .ok_or(Error::corrupt("an entropy block is larger than its room"))?;
    let used = match h.kind {
        0 => {
            out.copy_from_slice(block);
            h.src_size
        }
        1 => tans::decode(block, out, scratch)?,
        2 | 4 => huffman::decode(block, out, h.kind >> 1)?,
        3 => rle::decode(block, out, scratch, depth)?,
        5 => recursive::decode(block, out, scratch, depth)?,
        _ => return Err(Error::corrupt("an entropy block of a reserved type")),
    };
    if used != h.src_size {
        return Err(Error::corrupt(
            "an entropy block's decoder read a different length than its header gives",
        ));
    }
    Ok((end, h.dst_size))
}
