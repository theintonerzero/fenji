//! The 4-byte inner message header (spike-0 4.29; written by FUN_14029AC60 and, for the auth message,
//! FUN_14029C330; read by FUN_14029ABE0; docs/protocol/transport.md 7.1). `len` counts the body only.
use crate::{Error, Result};

/// Bytes in a header.
pub const HEADER_LEN: usize = 4;
/// The largest body a 24-bit length can carry.
pub const MAX_BODY: usize = 0xFF_FFFF;

/// A message's type and body length. Built only by [`Header::new`], which refuses a length over
/// [`MAX_BODY`], or read from the wire, so [`Header::to_bytes`] never cuts one short:
///
/// ```compile_fail
/// let h = fenji_proto::header::Header { ty: 0, len: 1 << 25 };
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    ty: u8,
    len: usize,
}

impl Header {
    pub fn new(ty: u8, len: usize) -> Result<Self> {
        if len > MAX_BODY {
            return Err(Error::BodyTooLong(len));
        }
        Ok(Self { ty, len })
    }

    pub fn ty(self) -> u8 {
        self.ty
    }

    /// The body's length in bytes, at most [`MAX_BODY`].
    pub fn body_len(self) -> usize {
        self.len
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

/// The first whole message in `buf`: its header, its body and the bytes it takes; `None` until all
/// of it is there. A header that announces a body over `max` bytes is refused as soon as it is
/// there, so a caller never buffers more than `max` bytes waiting for one message.
pub fn split_message(buf: &[u8], max: usize) -> Result<Option<(Header, &[u8], usize)>> {
    let Some(&[a, b, c, d]) = buf.get(..HEADER_LEN) else {
        return Ok(None);
    };
    let h = Header::from_bytes([a, b, c, d]);
    if h.len > max {
        return Err(Error::BodyTooLong(h.len));
    }
    let end = HEADER_LEN + h.len;
    Ok(buf.get(HEADER_LEN..end).map(|body| (h, body, end)))
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
    fn split_refuses_a_body_over_its_bound_from_the_header() {
        // Refused once the header is there, so the caller never waits for, or buffers, the body.
        let mut buf = Vec::new();
        write_message(1, &[0; 100], &mut buf).unwrap();
        assert_eq!(split_message(&buf[..4], 99), Err(Error::BodyTooLong(100)));
        assert_eq!(split_message(&buf[..4], 100), Ok(None));
        assert_eq!(split_message(&buf[..3], 99), Ok(None));
        let (h, body, used) = split_message(&buf, 100).unwrap().unwrap();
        assert_eq!(
            (h, body.len(), used),
            (Header::new(1, 100).unwrap(), 100, 104)
        );
    }

    #[test]
    fn split_waits_for_a_whole_message() {
        let mut buf = Vec::new();
        write_message(7, b"abc", &mut buf).unwrap();
        write_message(8, b"", &mut buf).unwrap();
        for cut in 0..7 {
            assert_eq!(split_message(&buf[..cut], MAX_BODY), Ok(None), "cut {cut}");
        }
        let (h, body, used) = split_message(&buf, MAX_BODY).unwrap().unwrap();
        assert_eq!((h.ty(), body, used), (7, &b"abc"[..], 7));
        let (h, body, used) = split_message(&buf[used..], MAX_BODY).unwrap().unwrap();
        assert_eq!((h.ty(), body, used), (8, &b""[..], 4));
    }
}
