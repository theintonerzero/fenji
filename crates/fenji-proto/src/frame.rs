//! Frames (FUN_14029B850 receives, FUN_14029C600 and FUN_14029A5B0 send;
//! docs/protocol/transport.md §4). A frame is a `u16` LE length XOR the direction's key, then the
//! body. A length of 0 is a time-sync frame: four more `u16`s, the 64-bit millisecond clock low word
//! first, each XOR the key. Bodies are opaque here: the variant transform and the Oodle wrapper are
//! the caller's.
use crate::scramble::{DownKey, UpKey};
use crate::{Error, Result};

/// The most a `u16` length can say.
pub const MAX_FRAME_BODY: usize = 0xffff;
/// The client copies each server frame body, and decodes each one's raw bytes, into 0x4080-byte
/// buffers without a length check (FUN_14029B850, FUN_14029A900; transport.md 4.9, 5.4), so the
/// server never sends a larger body (ruling R10).
pub const CLIENT_RECV_MAX: usize = 0x4080;
/// The client sends data frames of at most this many bytes, and so does our server.
pub const SEND_CHUNK: usize = 0x800;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    TimeSync(u64),
    Data(Vec<u8>),
}

/// A data frame's body must be 1..=`max` bytes: a length of 0 would read as a time sync.
fn check_body(body: &[u8], max: usize) -> Result<()> {
    if body.is_empty() || body.len() > max {
        return Err(Error::FrameLength(body.len()));
    }
    Ok(())
}

fn put_time(millis: u64, key: u16, out: &mut Vec<u8>) {
    out.extend_from_slice(&key.to_le_bytes()); // a length of 0, XOR the key
    for i in 0..4 {
        out.extend_from_slice(&(((millis >> (16 * i)) as u16) ^ key).to_le_bytes());
    }
}

/// Reads one frame from `buf` with `key`, without stepping it: `(frame, bytes used)`.
fn take(buf: &[u8], key: u16) -> Option<(Frame, usize)> {
    let word = |i: usize| -> Option<u16> {
        Some(u16::from_le_bytes(buf.get(2 * i..2 * i + 2)?.try_into().ok()?) ^ key)
    };
    let len = usize::from(word(0)?);
    if len == 0 {
        let mut millis = 0u64;
        for i in 0..4 {
            millis |= u64::from(word(1 + i)?) << (16 * i);
        }
        return Some((Frame::TimeSync(millis), 10));
    }
    let body = buf.get(2..2 + len)?;
    Some((Frame::Data(body.to_vec()), 2 + len))
}

/// Server side: writes frames to the client.
#[derive(Debug, Default)]
pub struct DownWriter {
    key: DownKey,
}

impl DownWriter {
    pub fn new() -> Self {
        Self::default()
    }

    /// One data frame. Refuses an empty body and one over `CLIENT_RECV_MAX`.
    pub fn data(&mut self, body: &[u8], out: &mut Vec<u8>) -> Result<()> {
        check_body(body, CLIENT_RECV_MAX)?;
        out.extend_from_slice(&(body.len() as u16 ^ self.key.0).to_le_bytes());
        out.extend_from_slice(body);
        self.key.step();
        Ok(())
    }

    pub fn time_sync(&mut self, millis: u64, out: &mut Vec<u8>) {
        put_time(millis, self.key.0, out);
        self.key.step();
    }
}

/// Client side: reads the server's frames. `None` means more bytes are needed; the key moves only
/// when a whole frame is read.
#[derive(Debug, Default)]
pub struct DownReader {
    key: DownKey,
}

impl DownReader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read(&mut self, buf: &[u8]) -> Option<(Frame, usize)> {
        let got = take(buf, self.key.0)?;
        self.key.step();
        Some(got)
    }
}

/// Client side: writes frames to the server.
#[derive(Debug, Default)]
pub struct UpWriter {
    key: UpKey,
}

impl UpWriter {
    pub fn new() -> Self {
        Self::default()
    }

    /// One data frame. Refuses an empty body and one over `MAX_FRAME_BODY`.
    pub fn data(&mut self, body: &[u8], out: &mut Vec<u8>) -> Result<()> {
        check_body(body, MAX_FRAME_BODY)?;
        out.extend_from_slice(&(body.len() as u16 ^ self.key.0).to_le_bytes());
        out.extend_from_slice(body);
        self.key.step_data();
        Ok(())
    }

    pub fn time_sync(&mut self, millis: u64, out: &mut Vec<u8>) {
        put_time(millis, self.key.0, out);
        self.key.step_time_sync();
    }
}

/// Server side: reads the client's frames.
#[derive(Debug, Default)]
pub struct UpReader {
    key: UpKey,
}

impl UpReader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read(&mut self, buf: &[u8]) -> Option<(Frame, usize)> {
        let got = take(buf, self.key.0)?;
        match got.0 {
            Frame::TimeSync(_) => self.key.step_time_sync(),
            Frame::Data(_) => self.key.step_data(),
        }
        Some(got)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_down_frames_bytes() {
        let mut w = DownWriter::new();
        let mut out = Vec::new();
        w.data(b"abc", &mut out).unwrap(); // key 0: length 3 as is
        w.data(b"x", &mut out).unwrap(); // key 0xD736: 1 ^ 0xD736 = 0xD737
        assert_eq!(out, [0x03, 0x00, b'a', b'b', b'c', 0x37, 0xd7, b'x']);
    }

    #[test]
    fn down_round_trip_with_time_sync() {
        let mut w = DownWriter::new();
        let mut out = Vec::new();
        w.data(b"one", &mut out).unwrap();
        w.time_sync(0x0123_4567_89ab_cdef, &mut out);
        w.data(&[7; 0x800], &mut out).unwrap();
        let mut r = DownReader::new();
        let mut at = 0;
        let mut frames = Vec::new();
        while let Some((f, used)) = r.read(&out[at..]) {
            frames.push(f);
            at += used;
        }
        assert_eq!(at, out.len());
        assert_eq!(
            frames,
            [
                Frame::Data(b"one".to_vec()),
                Frame::TimeSync(0x0123_4567_89ab_cdef),
                Frame::Data(vec![7; 0x800]),
            ]
        );
    }

    #[test]
    fn up_round_trip_with_time_sync() {
        let mut w = UpWriter::new();
        let mut out = Vec::new();
        w.time_sync(42, &mut out);
        w.data(b"hello", &mut out).unwrap();
        w.time_sync(43, &mut out);
        let mut r = UpReader::new();
        let mut at = 0;
        let mut frames = Vec::new();
        while let Some((f, used)) = r.read(&out[at..]) {
            frames.push(f);
            at += used;
        }
        assert_eq!(
            frames,
            [
                Frame::TimeSync(42),
                Frame::Data(b"hello".to_vec()),
                Frame::TimeSync(43)
            ]
        );
    }

    #[test]
    fn up_time_sync_bytes() {
        // Key 0: the length word is the key itself (0), then the clock's four words.
        let mut out = Vec::new();
        UpWriter::new().time_sync(0x0004_0003_0002_0001, &mut out);
        assert_eq!(out, [0, 0, 1, 0, 2, 0, 3, 0, 4, 0]);
    }

    #[test]
    fn partial_frames_wait_and_keep_the_key() {
        let mut w = UpWriter::new();
        let mut out = Vec::new();
        w.data(b"first", &mut out).unwrap();
        w.data(b"second", &mut out).unwrap();
        let mut r = UpReader::new();
        for cut in 0..7 {
            assert!(r.read(&out[..cut]).is_none(), "cut {cut}");
        }
        let (f, used) = r.read(&out).unwrap();
        assert_eq!(f, Frame::Data(b"first".to_vec()));
        let (f, _) = r.read(&out[used..]).unwrap();
        assert_eq!(f, Frame::Data(b"second".to_vec()));
    }

    #[test]
    fn frame_body_limits() {
        let mut out = Vec::new();
        assert_eq!(
            DownWriter::new().data(b"", &mut out),
            Err(Error::FrameLength(0))
        );
        let big = vec![0; MAX_FRAME_BODY + 1];
        assert_eq!(
            UpWriter::new().data(&big, &mut out),
            Err(Error::FrameLength(MAX_FRAME_BODY + 1))
        );
        assert!(out.is_empty());
    }

    #[test]
    fn down_body_over_client_buffer_is_refused() {
        // Ruling R10: the client copies each server frame body into a 0x4080-byte buffer without a
        // length check (transport.md 4.9), so the server never sends a larger one.
        let mut out = Vec::new();
        assert_eq!(
            DownWriter::new().data(&vec![0; 0x4081], &mut out),
            Err(Error::FrameLength(0x4081))
        );
        assert!(out.is_empty());
        DownWriter::new().data(&vec![0; 0x4080], &mut out).unwrap();
        assert_eq!(out.len(), 2 + 0x4080);
    }
}
