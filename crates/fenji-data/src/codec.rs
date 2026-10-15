//! Block decompression. Fenji decodes stored, LZ4 and Oodle (codec 4) blocks itself, Oodle with
//! fenji-oodle, its port of ooz's Kraken decoder, under the default `oodle` feature. The
//! [`OodleSpool`](crate::oodle_spool::OodleSpool) and the sandboxed helper with the client's own DLL
//! stay, as a dev-only cross-check (gate #39); the helper builds without the feature.
use crate::{Codec, Error, Result};

/// The most an LZ4 block can expand: each extra length byte adds at most 255 output bytes.
const LZ4_MAX_RATIO: usize = 255;

/// The most raw bytes an Oodle block holds: the engine's blocks are at most 512 KiB
/// (m1-archives.md 2.4, 3.1).
pub const MAX_OODLE_BLOCK: usize = 512 * 1024;

/// Whether this build decodes Oodle: the `oodle` feature, which links fenji-oodle
/// (GPL-3.0-or-later). tools/oodle-helper, the program that loads the client's own Oodle DLL,
/// refuses to build for Windows with it (Ruling O16). It turns the feature off, but Cargo turns a
/// feature on for every package that one command builds, so `-p fenji-archive` or `--workspace`
/// beside it would link fenji-oodle into it all the same.
pub const OODLE_DECODER: bool = cfg!(feature = "oodle");

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

/// Stored, LZ4 and, with the `oodle` feature, Oodle.
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
            #[cfg(feature = "oodle")]
            Codec::Oodle => decode_oodle_block(input, raw_len, index)?.0,
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

/// Decodes one Oodle block as [`BuiltinCodecs`] does, and counts the chunks in it where
/// fenji-oodle's output departs from ooz's, for `archive verify-oodle` (Rulings O13 to O15).
///
/// A block of more than [`MAX_OODLE_BLOCK`] raw bytes is refused before any decoding (the Oodle
/// decoder plan's Review Focus 5): fenji-oodle's output grows only as the stream backs it, but a
/// memset quantum backs 256 KiB with 6 bytes, so a tampered blob header could still ask for
/// gigabytes. Then the block's `u32` raw length must match the blob header's ([`oodle_payload`]).
#[cfg(feature = "oodle")]
pub fn decode_oodle_block(
    input: &[u8],
    raw_len: usize,
    index: usize,
) -> Result<(Vec<u8>, fenji_oodle::Departures)> {
    if raw_len > MAX_OODLE_BLOCK {
        return Err(Error::Oodle {
            index,
            reason: format!("{raw_len} raw bytes, more than a block holds"),
        });
    }
    let stream = oodle_payload(input, raw_len, index)?;
    fenji_oodle::decompress_counting(stream, raw_len).map_err(|e| Error::Oodle {
        index,
        reason: e.to_string(),
    })
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

    #[cfg(feature = "oodle")]
    #[test]
    fn builtin_codecs_decode_oodle() {
        // A memset stream (fenji-oodle's own format test), behind the archive's raw-length prefix.
        let mut block = 1000u32.to_le_bytes().to_vec();
        block.extend_from_slice(&[0x8C, 0x06, 0x07, 0xFF, 0xFF, 0x2A]);
        assert_eq!(
            BuiltinCodecs
                .decompress_block(Codec::Oodle, &block, 1000, 0)
                .unwrap(),
            vec![0x2A; 1000]
        );
    }

    #[cfg(feature = "oodle")]
    #[test]
    fn oversized_oodle_blocks_are_refused() {
        // Review Focus 5: a tampered header can't make the decoder allocate more than a block.
        // The stream backs all 600 KiB (a memset quantum is 256 KiB from 6 bytes), so only the
        // bound refuses it.
        use crate::test_util::{oodle_block, oodle_memset_stream};
        let block = oodle_block(600 * 1024, &oodle_memset_stream(600 * 1024, 0x2A));
        let got = BuiltinCodecs.decompress_block(Codec::Oodle, &block, 600 * 1024, 2);
        assert!(
            matches!(&got, Err(Error::Oodle { index: 2, reason }) if reason.contains("614400")),
            "{:?}",
            got.map(|raw| raw.len())
        );
        // A whole block is fine.
        let block = oodle_block(512 * 1024, &oodle_memset_stream(512 * 1024, 0x2A));
        assert_eq!(
            BuiltinCodecs
                .decompress_block(Codec::Oodle, &block, 512 * 1024, 0)
                .unwrap(),
            vec![0x2A; 512 * 1024]
        );
    }

    #[cfg(not(feature = "oodle"))]
    #[test]
    fn without_the_oodle_feature_oodle_is_unavailable() {
        // tools/oodle-helper's build: no fenji-oodle, so no Oodle.
        let block = crate::test_util::oodle_block(6, &[0xCC, 0x06, 1, 2, 3, 4, 5, 6]);
        let e = BuiltinCodecs
            .decompress_block(Codec::Oodle, &block, 6, 0)
            .unwrap_err();
        assert!(matches!(e, Error::CodecUnavailable(4)), "{e}");
    }

    #[cfg(feature = "oodle")]
    #[test]
    fn decode_oodle_block_counts_where_the_port_departs_from_ooz() {
        // Ruling O14: an entropy-only chunk of one stored block, after an RLE one (as in
        // fenji-oodle's Kraken tests).
        use crate::test_util::{oodle_block, oodle_memset_stream};
        let mut stream = vec![0x8C, 0x06, 0x00, 0x00, 0x70];
        stream.extend([0x37, 0xFF, 0xFC, 0x00, 0x05, 0x00, b'X', 0xFF, 0x0C, 0x01]);
        stream.extend([0x00, 0x00, 100]);
        stream.extend(0..100u8);
        let raw_len = 0x2_0000 + 100;
        let block = oodle_block(raw_len, &stream);
        let (raw, departures) = crate::decode_oodle_block(&block, raw_len, 0).unwrap();
        assert!(
            raw == BuiltinCodecs
                .decompress_block(Codec::Oodle, &block, raw_len, 0)
                .unwrap()
        );
        assert_eq!(
            (
                departures.stored_entropy_chunks,
                departures.near_match_chunks
            ),
            (1, 0)
        );
        // BuiltinCodecs' checks: the raw length its prefix gives, and the bound.
        assert!(matches!(
            crate::decode_oodle_block(&block, raw_len - 1, 4),
            Err(Error::Oodle { index: 4, .. })
        ));
        let over = crate::MAX_OODLE_BLOCK + 1;
        let big = oodle_block(over, &oodle_memset_stream(over, 0));
        assert!(matches!(
            crate::decode_oodle_block(&big, over, 0),
            Err(Error::Oodle { reason, .. }) if reason.contains("more than a block holds")
        ));
    }
}
