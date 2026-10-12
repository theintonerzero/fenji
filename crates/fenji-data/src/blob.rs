//! The engine's file blob. The same header fronts every file in the data archives and in the
//! file system embedded in gujianol.exe (docs/re/m1-script-format.md §2).
//!
//! Layout (little-endian): raw_size u64, timestamp u64 (Unix seconds), stored_size u64,
//! block_size u32, codec u16, check u16; then one u32 compressed size per block for compressed
//! codecs; then the data. stored_size counts from the header's first byte to the data's last.
use crate::{Decompressor, Error, Result};

/// Length of the fixed part of the header.
pub const FIXED_LEN: usize = 32;
/// No client file comes near this; larger raw sizes mean a corrupt header.
const MAX_RAW: u64 = 1 << 32;
/// The most `decode` reserves up front; larger outputs grow block by block.
const MAX_PREALLOC: u64 = 64 << 20;

/// How a blob's data is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    Stored,
    Lz4,
    Oodle,
    Other(u16),
}

impl Codec {
    pub fn from_u16(v: u16) -> Self {
        match v {
            0 => Self::Stored,
            1 => Self::Lz4,
            4 => Self::Oodle,
            other => Self::Other(other),
        }
    }

    pub fn as_u16(self) -> u16 {
        match self {
            Self::Stored => 0,
            Self::Lz4 => 1,
            Self::Oodle => 4,
            Self::Other(v) => v,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Stored => "stored",
            Self::Lz4 => "lz4",
            Self::Oodle => "oodle",
            Self::Other(_) => "unknown",
        }
    }
}

/// A parsed blob header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobHeader {
    pub raw_size: u64,
    pub timestamp: u64,
    pub stored_size: u64,
    pub block_size: u32,
    pub codec: Codec,
    /// Meaning unknown (docs/re/m1-script-format.md §2).
    pub check: u16,
    /// Compressed size of each block; empty for stored blobs.
    pub block_sizes: Vec<u32>,
}

impl BlobHeader {
    /// Parses the header at the start of `bytes`. `at` only labels errors. Refuses a header whose
    /// stored size disagrees with its own block list.
    pub fn parse(bytes: &[u8], at: u64) -> Result<Self> {
        let bad = |reason: String| Error::BadBlob { at, reason };
        let fixed = bytes.get(..FIXED_LEN).ok_or_else(|| {
            bad(format!(
                "{} bytes, the header needs {FIXED_LEN}",
                bytes.len()
            ))
        })?;
        let u64_at = |o: usize| u64::from_le_bytes(fixed[o..o + 8].try_into().expect("8 bytes"));
        let (raw_size, timestamp, stored_size) = (u64_at(0), u64_at(8), u64_at(16));
        let block_size = u32::from_le_bytes(fixed[24..28].try_into().expect("4 bytes"));
        let codec = Codec::from_u16(u16::from_le_bytes([fixed[28], fixed[29]]));
        let check = u16::from_le_bytes([fixed[30], fixed[31]]);
        if raw_size > MAX_RAW {
            return Err(bad(format!("raw size {raw_size} is implausible")));
        }
        let blocks = match codec {
            Codec::Stored => 0,
            _ if raw_size == 0 => 0,
            _ if block_size == 0 => return Err(bad("block size 0 with data".into())),
            _ => raw_size.div_ceil(u64::from(block_size)) as usize,
        };
        let list = bytes
            .get(FIXED_LEN..FIXED_LEN + 4 * blocks)
            .ok_or_else(|| bad(format!("the list of {blocks} block sizes is truncated")))?;
        let block_sizes = list
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().expect("4 bytes")))
            .collect();
        let header = Self {
            raw_size,
            timestamp,
            stored_size,
            block_size,
            codec,
            check,
            block_sizes,
        };
        let expected = header.header_len() as u64 + header.data_len();
        if expected != stored_size {
            return Err(bad(format!(
                "stored size {stored_size}, but header and blocks add up to {expected}"
            )));
        }
        Ok(header)
    }

    /// Length of the header including the block list.
    pub fn header_len(&self) -> usize {
        FIXED_LEN + 4 * self.block_sizes.len()
    }

    /// Length of the data after the header.
    pub fn data_len(&self) -> u64 {
        match self.codec {
            Codec::Stored => self.raw_size,
            _ => self.block_sizes.iter().map(|&s| u64::from(s)).sum(),
        }
    }
}

/// One block of a blob: its compressed bytes and the raw length it decodes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block<'a> {
    pub index: usize,
    pub input: &'a [u8],
    pub raw_len: usize,
}

/// A blob's header and blocks. `bytes` starts at its header and holds at least `stored_size`
/// bytes. A stored blob is one block of its whole data.
pub fn blocks(bytes: &[u8], at: u64) -> Result<(BlobHeader, Vec<Block<'_>>)> {
    let h = BlobHeader::parse(bytes, at)?;
    let data = bytes
        .get(h.header_len()..h.stored_size as usize)
        .ok_or_else(|| Error::BadBlob {
            at,
            reason: format!(
                "{} bytes available, stored size {}",
                bytes.len(),
                h.stored_size
            ),
        })?;
    if h.codec == Codec::Stored {
        let one = Block {
            index: 0,
            input: data,
            raw_len: h.raw_size as usize,
        };
        return Ok((h, vec![one]));
    }
    let mut out = Vec::with_capacity(h.block_sizes.len());
    let (mut pos, mut left) = (0usize, h.raw_size);
    for (index, &len) in h.block_sizes.iter().enumerate() {
        let raw_len = left.min(u64::from(h.block_size)) as usize;
        // In range: `data` is exactly the sum of the block sizes (checked by `parse`).
        out.push(Block {
            index,
            input: &data[pos..pos + len as usize],
            raw_len,
        });
        pos += len as usize;
        left -= raw_len as u64;
    }
    Ok((h, out))
}

/// Decodes a whole blob. `bytes` starts at its header and holds at least `stored_size` bytes.
pub fn decode(bytes: &[u8], at: u64, codecs: &dyn Decompressor) -> Result<Vec<u8>> {
    let (h, blocks) = blocks(bytes, at)?;
    let mut out = Vec::with_capacity(h.raw_size.min(MAX_PREALLOC) as usize);
    for b in blocks {
        let decoded = codecs.decompress_block(h.codec, b.input, b.raw_len, b.index)?;
        if decoded.len() != b.raw_len {
            return Err(Error::SizeMismatch {
                got: decoded.len() as u64,
                want: b.raw_len as u64,
            });
        }
        out.extend_from_slice(&decoded);
    }
    if out.len() as u64 != h.raw_size {
        return Err(Error::SizeMismatch {
            got: out.len() as u64,
            want: h.raw_size,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BuiltinCodecs;
    use crate::test_util::{lz4_blob, lz4_blob_blocks, stored_blob};

    #[test]
    fn single_block_lz4_round_trip() {
        let content = b"local x = 1 ".repeat(40);
        let blob = lz4_blob(&content, 1_669_366_433);
        let h = BlobHeader::parse(&blob, 0).unwrap();
        assert_eq!(
            (h.raw_size, h.timestamp, h.codec),
            (content.len() as u64, 1_669_366_433, Codec::Lz4)
        );
        assert_eq!(h.block_sizes.len(), 1);
        assert_eq!(decode(&blob, 0, &BuiltinCodecs).unwrap(), content);
    }

    #[test]
    fn multi_block_lz4() {
        let content: Vec<u8> = (0..10_000u32)
            .flat_map(|i| (i % 251).to_le_bytes())
            .collect();
        let block = 16_384usize;
        let blocks: Vec<Vec<u8>> = content
            .chunks(block)
            .map(lz4_flex::block::compress)
            .collect();
        let b = lz4_blob_blocks(&blocks, content.len() as u64, block as u32);
        assert_eq!(BlobHeader::parse(&b, 0).unwrap().block_sizes.len(), 3);
        assert_eq!(decode(&b, 0, &BuiltinCodecs).unwrap(), content);
    }

    #[test]
    fn blocks_lists_each_block_with_its_raw_length() {
        let content = vec![9u8; 40_000];
        let block = 16_384usize;
        let compressed: Vec<Vec<u8>> = content
            .chunks(block)
            .map(lz4_flex::block::compress)
            .collect();
        let blob = lz4_blob_blocks(&compressed, content.len() as u64, block as u32);
        let (h, blocks) = blocks(&blob, 0).unwrap();
        assert_eq!(h.block_sizes.len(), 3);
        let raws: Vec<usize> = blocks.iter().map(|b| b.raw_len).collect();
        assert_eq!(raws, [16_384, 16_384, 40_000 - 2 * 16_384]);
        assert_eq!(blocks[1].input, &compressed[1][..]);
        assert_eq!(blocks[2].index, 2);
    }

    #[test]
    fn a_stored_blob_is_one_block() {
        let blob = stored_blob(b"plain");
        let (_, blocks) = blocks(&blob, 0).unwrap();
        assert_eq!(
            blocks,
            [Block {
                index: 0,
                input: &b"plain"[..],
                raw_len: 5
            }]
        );
    }

    #[test]
    fn inconsistent_stored_size_is_refused() {
        let mut blob = lz4_blob(b"abcabcabc", 0);
        blob[16] ^= 1;
        assert!(matches!(
            BlobHeader::parse(&blob, 0x10),
            Err(Error::BadBlob { at: 0x10, .. })
        ));
    }

    #[test]
    fn builtin_codecs_refuse_oodle() {
        let mut blob = lz4_blob(b"abcabcabc", 0);
        blob[28] = 4;
        assert!(matches!(
            decode(&blob, 0, &BuiltinCodecs),
            Err(Error::CodecUnavailable(4))
        ));
    }

    #[test]
    fn every_truncation_is_an_error() {
        let blob = lz4_blob(b"abcdefabcdef", 0);
        for n in 0..blob.len() {
            assert!(
                decode(&blob[..n], 0, &BuiltinCodecs).is_err(),
                "prefix of {n} bytes"
            );
        }
    }

    #[test]
    fn corrupting_any_byte_never_panics() {
        let blob = lz4_blob(&b"local t = {1, 2, 3} ".repeat(20), 0);
        for i in 0..blob.len() {
            for flip in [0x01u8, 0x80, 0xFF] {
                let mut b = blob.clone();
                b[i] ^= flip;
                let _ = decode(&b, 0, &BuiltinCodecs); // Ok or Err, never a panic
            }
        }
    }

    #[test]
    fn a_decompressor_that_returns_too_much_is_an_error() {
        // A Decompressor of ours that misbehaves (say, a future Oodle backend) must not make decode
        // panic on the block arithmetic.
        struct TooMuch;
        impl Decompressor for TooMuch {
            fn decompress_block(
                &self,
                _: Codec,
                _: &[u8],
                raw_len: usize,
                _: usize,
            ) -> Result<Vec<u8>> {
                Ok(vec![0; raw_len * 3])
            }
        }
        let content: Vec<u8> = (0..10_000u32)
            .flat_map(|i| (i % 251).to_le_bytes())
            .collect();
        let block = 16_384usize;
        let blocks: Vec<Vec<u8>> = content
            .chunks(block)
            .map(lz4_flex::block::compress)
            .collect();
        let b = lz4_blob_blocks(&blocks, content.len() as u64, block as u32);
        assert!(matches!(
            decode(&b, 0, &TooMuch),
            Err(Error::SizeMismatch { .. })
        ));
    }

    #[test]
    fn implausible_raw_size_is_refused_before_allocating() {
        // A consistent header that claims 3 GB from one 12-byte LZ4 block: no LZ4 block expands
        // that far, so this must fail fast instead of allocating 3 GB.
        let comp = lz4_flex::block::compress(b"abcabcabcabc");
        let raw: u64 = 3_000_000_000;
        let mut b = Vec::new();
        b.extend_from_slice(&raw.to_le_bytes());
        b.extend_from_slice(&0u64.to_le_bytes());
        b.extend_from_slice(&((FIXED_LEN + 4 + comp.len()) as u64).to_le_bytes());
        b.extend_from_slice(&(raw as u32).to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&(comp.len() as u32).to_le_bytes());
        b.extend_from_slice(&comp);
        assert!(matches!(
            decode(&b, 0, &BuiltinCodecs),
            Err(Error::Lz4 { index: 0, .. })
        ));
    }
}
