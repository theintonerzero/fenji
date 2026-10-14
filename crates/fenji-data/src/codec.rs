//! Block decompression. Fenji decodes stored and LZ4 blocks itself. Oodle (codec 4) goes through
//! an [`OodleSpool`](crate::oodle_spool::OodleSpool) and the sandboxed helper (gate #39, route c).
use crate::{Codec, Error, Result};

/// The most an LZ4 block can expand: each extra length byte adds at most 255 output bytes.
const LZ4_MAX_RATIO: usize = 255;

/// Decodes one block of a blob.
pub trait Decompressor {
    /// Decodes `input`, one block of `codec` data, into exactly `raw_len` bytes. `index` is the
    /// block's position in its blob, for messages.
    fn decompress_block(
        &self,
        codec: Codec,
        input: &[u8],
        raw_len: usize,
        index: usize,
    ) -> Result<Vec<u8>>;
}

/// Stored and LZ4.
#[derive(Debug, Default, Clone, Copy)]
pub struct BuiltinCodecs;

impl Decompressor for BuiltinCodecs {
    fn decompress_block(
        &self,
        codec: Codec,
        input: &[u8],
        raw_len: usize,
        index: usize,
    ) -> Result<Vec<u8>> {
        let out = match codec {
            Codec::Stored => input.to_vec(),
            Codec::Lz4 => {
                // The decoder allocates `raw_len` up front, so refuse sizes no block can reach.
                if raw_len > input.len().saturating_mul(LZ4_MAX_RATIO).saturating_add(64) {
                    let reason = format!(
                        "{raw_len} bytes can't come from {} compressed bytes",
                        input.len()
                    );
                    return Err(Error::Lz4 { index, reason });
                }
                lz4_flex::block::decompress(input, raw_len).map_err(|e| Error::Lz4 {
                    index,
                    reason: e.to_string(),
                })?
            }
            other => return Err(Error::CodecUnavailable(other.as_u16())),
        };
        if out.len() != raw_len {
            return Err(Error::SizeMismatch {
                got: out.len() as u64,
                want: raw_len as u64,
            });
        }
        Ok(out)
    }
}

/// An Oodle block's stream, once its leading raw length (`m1-archives.md` 3.4) matches the
/// blob header's `raw_len` for that block.
pub fn oodle_payload(input: &[u8], raw_len: usize, index: usize) -> Result<&[u8]> {
    let Some((prefix, stream)) = input.split_first_chunk::<4>() else {
        return Err(Error::Oodle {
            index,
            reason: "shorter than its 4-byte raw length".into(),
        });
    };
    let said = u32::from_le_bytes(*prefix) as usize;
    if said != raw_len {
        return Err(Error::Oodle {
            index,
            reason: format!("it says {said} raw bytes where the blob header says {raw_len}"),
        });
    }
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prefix_that_disagrees_with_the_blob_is_refused() {
        // Review Focus 1: m1-archives 3.4's u32 raw length must match the blob header's.
        let mut block = 100u32.to_le_bytes().to_vec();
        block.extend_from_slice(&[0x4C, 0x06]);
        assert_eq!(crate::oodle_payload(&block, 100, 0).unwrap(), &[0x4C, 0x06]);
        let e = crate::oodle_payload(&block, 99, 3).unwrap_err();
        assert!(
            matches!(&e, Error::Oodle { index: 3, reason } if reason.contains("100") && reason.contains("99")),
            "{e}"
        );
        assert!(matches!(
            crate::oodle_payload(&[1, 2, 3], 1, 0),
            Err(Error::Oodle { .. })
        ));
    }
}
