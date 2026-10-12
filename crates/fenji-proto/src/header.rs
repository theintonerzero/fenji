//! The 4-byte inner message header (spike-0 4.29; written by FUN_14029AC60 and, for the auth message,
//! FUN_14029C330; read by FUN_14029ABE0; docs/protocol/transport.md 7.1). `len` counts the body only.
use crate::{Error, Result};

/// Bytes in a header.
pub const HEADER_LEN: usize = 4;
/// The largest body a 24-bit length can carry.
pub const MAX_BODY: usize = 0xFF_FFFF;

/// A message's type and body length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub ty: u8,
    pub len: usize,
}

impl Header {
    pub fn new(ty: u8, len: usize) -> Result<Self> {
        if len > MAX_BODY {
            return Err(Error::BodyTooLong(len));
        }
        Ok(Self { ty, len })
    }

    /// `[len >> 16, ty ^ low byte of (len >> 1), len, len >> 8]`.
    pub fn to_bytes(self) -> [u8; HEADER_LEN] {
        let l = self.len as u32;
        [
            (l >> 16) as u8,
            self.ty ^ (l >> 1) as u8,
            l as u8,
            (l >> 8) as u8,
        ]
    }

    pub fn from_bytes(b: [u8; HEADER_LEN]) -> Self {
        let len = (u32::from(b[0]) << 16) | (u32::from(b[3]) << 8) | u32::from(b[2]);
        Self {
            ty: b[1] ^ (len >> 1) as u8,
            len: len as usize,
        }
    }
}

/// Appends one message, header then body, to `out`.
pub fn write_message(ty: u8, body: &[u8], out: &mut Vec<u8>) -> Result<()> {
    out.extend_from_slice(&Header::new(ty, body.len())?.to_bytes());
    out.extend_from_slice(body);
    Ok(())
}

/// The first whole message in `buf`: its header, its body and the bytes it takes. `None` until all
/// of it is there.
pub fn split_message(buf: &[u8]) -> Option<(Header, &[u8], usize)> {
    let h = Header::from_bytes(buf.get(..HEADER_LEN)?.try_into().ok()?);
    let body = buf.get(HEADER_LEN..HEADER_LEN + h.len)?;
    Some((h, body, HEADER_LEN + h.len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_reply_bytes() {
        // F5/F6: a 2-byte body "ok" of type 0: [0, 0 ^ (2 >> 1), 2, 0].
        let mut out = Vec::new();
        write_message(0, b"ok", &mut out).unwrap();
        assert_eq!(out, [0x00, 0x01, 0x02, 0x00, b'o', b'k']);
    }

    #[test]
    fn auth_header_for_a_100_byte_payload() {
        // F5: length 22 + 100 = 122 = 0x7a; byte 1 = 0 ^ (122 >> 1) = 0x3d.
        assert_eq!(
            Header::new(0, 122).unwrap().to_bytes(),
            [0x00, 0x3d, 0x7a, 0x00]
        );
    }

    #[test]
    fn round_trips_lengths_and_types() {
        for len in [0, 1, 2, 255, 256, 0x7ff, 0x800, 65_535, 65_536, MAX_BODY] {
            for ty in [0u8, 1, 0x7f, 0x80, 0xfe, 0xff] {
                let h = Header::new(ty, len).unwrap();
                assert_eq!(Header::from_bytes(h.to_bytes()), h, "ty {ty} len {len}");
            }
        }
    }

    #[test]
    fn a_length_over_255_uses_all_four_bytes() {
        // transport.md §7: len 0x012345 -> byte 0 = 0x01, byte 2 = 0x45, byte 3 = 0x23;
        // (len >> 1) = 0x0091a2, so byte 1 = 0x2a ^ 0xa2 = 0x88.
        let h = Header::new(0x2a, 0x01_2345).unwrap();
        assert_eq!(h.to_bytes(), [0x01, 0x88, 0x45, 0x23]);
        assert_eq!(Header::from_bytes([0x01, 0x88, 0x45, 0x23]), h);
    }

    #[test]
    fn length_over_24_bits_is_refused() {
        assert_eq!(
            Header::new(0, MAX_BODY + 1),
            Err(Error::BodyTooLong(MAX_BODY + 1))
        );
    }

    #[test]
    fn split_waits_for_a_whole_message() {
        let mut buf = Vec::new();
        write_message(7, b"abc", &mut buf).unwrap();
        write_message(8, b"", &mut buf).unwrap();
        for cut in 0..7 {
            assert!(split_message(&buf[..cut]).is_none(), "cut {cut}");
        }
        let (h, body, used) = split_message(&buf).unwrap();
        assert_eq!((h.ty, body, used), (7, &b"abc"[..], 7));
        let (h, body, used) = split_message(&buf[used..]).unwrap();
        assert_eq!((h.ty, body, used), (8, &b""[..], 4));
    }
}
