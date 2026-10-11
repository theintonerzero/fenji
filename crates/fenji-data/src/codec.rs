//! Block decompression. Fenji decodes stored and LZ4 blocks itself; Oodle (codec 4) waits on the
//! M1 decision gate and is refused until then.
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
