//! The per-frame Oodle wrapper (FUN_14029B0A0; docs/protocol/transport.md §5): every data frame the
//! server sends carries a raw-length prefix, then one packet for `OodleNetwork1TCP_Decode`. The
//! client has no encoder, so the server's codec decides what that packet is.
use crate::{Error, Result};

/// Appends `n` in the shortest prefix form.
pub fn write_raw_len(n: usize, out: &mut Vec<u8>) -> Result<()> {
    match n {
        0..=0xfb => out.push(n as u8),
        0xfc..=0xffff => {
            out.push(0xfc);
            out.extend_from_slice(&(n as u16).to_le_bytes());
        }
        0x1_0000..=0xff_ffff => {
            out.push(0xfd);
            out.extend_from_slice(&(n as u32).to_le_bytes()[..3]);
        }
        _ => {
            let n = u32::try_from(n).map_err(|_| Error::BadRawLength("over 32 bits"))?;
            out.push(0xfe);
            out.extend_from_slice(&n.to_le_bytes());
        }
    }
    Ok(())
}

/// Reads a prefix: `(raw length, prefix length)`. `FF` is refused: the client reads it as no
/// prefix and a raw length of 0, which no packet of ours uses.
pub fn read_raw_len(buf: &[u8]) -> Result<(usize, usize)> {
    let take = |n: usize| buf.get(1..1 + n).ok_or(Error::BadRawLength("truncated"));
    match *buf.first().ok_or(Error::BadRawLength("truncated"))? {
        n @ 0..=0xfb => Ok((usize::from(n), 1)),
        0xfc => {
            let b = take(2)?;
            Ok((usize::from(u16::from_le_bytes([b[0], b[1]])), 3))
        }
        0xfd => {
            let b = take(3)?;
            Ok((u32::from_le_bytes([b[0], b[1], b[2], 0]) as usize, 4))
        }
        0xfe => {
            let b = take(4)?;
            Ok((u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize, 5))
        }
        0xff => Err(Error::BadRawLength("0xff is not a prefix")),
    }
}

/// What a frame's payload holds after the prefix.
pub trait StreamCodec {
    /// Appends the payload for `raw`: the prefix, then the packet.
    fn encode(&self, raw: &[u8], out: &mut Vec<u8>) -> Result<()>;
    /// The raw bytes of one payload.
    fn decode(&self, payload: &[u8]) -> Result<Vec<u8>>;
}

/// The raw bytes themselves, with a raw length equal to the packet's length. The client's decoder
/// accepts it, one-byte and `FC` prefixes alike (transport.md 5.8: runs m2-r3 to m2-r6).
#[derive(Debug, Default, Clone, Copy)]
pub struct StoredCodec;

impl StreamCodec for StoredCodec {
    fn encode(&self, raw: &[u8], out: &mut Vec<u8>) -> Result<()> {
        write_raw_len(raw.len(), out)?;
        out.extend_from_slice(raw);
        Ok(())
    }

    fn decode(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let (raw, used) = read_raw_len(payload)?;
        let rest = &payload[used..];
        if rest.len() != raw {
            return Err(Error::NotStored {
                raw,
                got: rest.len(),
            });
        }
        Ok(rest.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefix(n: usize) -> Vec<u8> {
        let mut out = Vec::new();
        write_raw_len(n, &mut out).unwrap();
        out
    }

    #[test]
    fn prefix_forms() {
        // F4: < 0xFC in one byte; FC + u16; FD + 24 bits; FE + u32, all little-endian.
        assert_eq!(prefix(0), [0x00]);
        assert_eq!(prefix(0xfb), [0xfb]);
        assert_eq!(prefix(0xfc), [0xfc, 0xfc, 0x00]);
        assert_eq!(prefix(0x1234), [0xfc, 0x34, 0x12]);
        assert_eq!(prefix(0x1_0000), [0xfd, 0x00, 0x00, 0x01]);
        assert_eq!(prefix(0xff_ffff), [0xfd, 0xff, 0xff, 0xff]);
        assert_eq!(prefix(0x100_0000), [0xfe, 0x00, 0x00, 0x00, 0x01]);
    }

    #[test]
    fn prefix_round_trips() {
        for n in [
            0,
            1,
            0xfb,
            0xfc,
            0xffff,
            0x1_0000,
            0xff_ffff,
            0x100_0000,
            u32::MAX as usize,
        ] {
            let p = prefix(n);
            assert_eq!(read_raw_len(&p), Ok((n, p.len())), "{n:#x}");
        }
    }

    #[test]
    fn raw_len_ff_is_refused() {
        assert!(matches!(
            read_raw_len(&[0xff, 1, 2, 3, 4]),
            Err(Error::BadRawLength(_))
        ));
    }

    #[test]
    fn truncated_prefix_is_an_error() {
        for bad in [&[][..], &[0xfc, 1], &[0xfd, 1, 2], &[0xfe, 1, 2, 3]] {
            assert!(
                matches!(read_raw_len(bad), Err(Error::BadRawLength(_))),
                "{bad:02x?}"
            );
        }
    }

    #[test]
    fn stored_codec_round_trips_and_checks_length() {
        let mut out = Vec::new();
        StoredCodec.encode(b"\x00\x01\x02\x00ok", &mut out).unwrap();
        assert_eq!(out, [0x06, 0x00, 0x01, 0x02, 0x00, b'o', b'k']);
        assert_eq!(StoredCodec.decode(&out).unwrap(), b"\x00\x01\x02\x00ok");
        assert_eq!(
            StoredCodec.decode(&[0x05, 1, 2]),
            Err(Error::NotStored { raw: 5, got: 2 })
        );
    }
}
