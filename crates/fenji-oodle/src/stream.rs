//! The stream driver and the chunk walk: ports of `Kraken_DecodeStep` (kraken.cpp:4020) and
//! `Kraken_Decompress` (kraken.cpp:4130), for the Kraken family only.
use crate::Error;
use crate::header::{Quantum, StreamHeader, quantum_header, stream_header};

/// Output bytes per quantum in the Kraken family; a stream header precedes each.
pub const QUANTUM: usize = 0x40000;

/// Scratch for a codec's first phase, as ooz sizes it (kraken.cpp:452).
pub(crate) const SCRATCH: usize = 0x6C000;

/// Port of `Kraken_Decompress` (kraken.cpp:4130): `src`, one whole Oodle stream, decoded into
/// exactly `raw_len` bytes.
pub fn decompress(src: &[u8], raw_len: usize) -> Result<Vec<u8>, Error> {
    let mut out = vec![0u8; raw_len];
    let mut scratch = Vec::new();
    let mut at = 0;
    let mut done = 0;
    let mut h = StreamHeader {
        decoder: 0,
        uncompressed: false,
        checksums: false,
    };
    while done < raw_len {
        if done % QUANTUM == 0 {
            (h, at) = stream_header(src, at)?;
        }
        let n = (raw_len - done).min(QUANTUM);
        if h.uncompressed {
            let bytes = src.get(at..at + n).ok_or(Error::Truncated { at })?;
            out[done..done + n].copy_from_slice(bytes);
            at += n;
            done += n;
            continue;
        }
        if !matches!(h.decoder, 6 | 10 | 12) {
            return Err(Error::Unsupported { decoder: h.decoder });
        }
        let q;
        (q, at) = quantum_header(src, at, h.checksums)?;
        match q {
            Quantum::Memset { byte } => out[done..done + n].fill(byte),
            Quantum::Data { size, .. } => {
                if size > n {
                    return Err(Error::QuantumTooLarge { at, size, room: n });
                }
                let data = src.get(at..at + size).ok_or(Error::Truncated { at })?;
                if size == n {
                    out[done..done + n].copy_from_slice(data);
                } else {
                    if scratch.is_empty() {
                        scratch.resize(SCRATCH, 0);
                    }
                    let used =
                        decode_quantum(h.decoder, &mut out[..done + n], done, data, &mut scratch)?;
                    if used != size {
                        return Err(Error::Corrupt {
                            at,
                            what: "the quantum's decoder read a different length than its header gives",
                        });
                    }
                }
                at += size;
            }
        }
        done += n;
    }
    if at != src.len() {
        return Err(Error::TrailingBytes {
            left: src.len() - at,
        });
    }
    Ok(out)
}

/// The quantum decoders, by stream decoder number. Each fills `dst[start..]` from `src`, reading
/// earlier output in `dst[..start]` for matches, and returns how many bytes of `src` it read.
/// Tasks 4–6 add one arm each, for the codecs Task 2's census finds.
pub(crate) fn decode_quantum(
    decoder: u8,
    dst: &mut [u8],
    start: usize,
    src: &[u8],
    scratch: &mut Vec<u8>,
) -> Result<usize, Error> {
    let _ = (dst, start, src, scratch);
    Err(Error::Unsupported { decoder })
}

/// How a chunk's output is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkMode {
    /// The stream header says uncompressed: the bytes follow as they are.
    Uncompressed,
    /// One byte, repeated.
    Memset,
    /// Quantum data as long as its output: stored as is.
    Stored,
    /// Quantum data for the codec.
    Compressed,
}

/// One quantum's headers, read without decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkInfo {
    pub decoder: u8,
    pub mode: ChunkMode,
    pub checksums: bool,
    pub raw_len: usize,
    pub compressed_len: usize,
}

/// Walks `src`'s headers as [`decompress`] would, one quantum at a time, without decoding any.
/// It yields one error and stops at the first problem, leftover bytes included.
pub fn chunks(src: &[u8], raw_len: usize) -> Chunks<'_> {
    Chunks {
        src,
        raw_len,
        at: 0,
        done: 0,
        h: StreamHeader {
            decoder: 0,
            uncompressed: false,
            checksums: false,
        },
        over: false,
    }
}

/// The iterator [`chunks`] returns.
pub struct Chunks<'a> {
    src: &'a [u8],
    raw_len: usize,
    at: usize,
    done: usize,
    h: StreamHeader,
    over: bool,
}

impl Iterator for Chunks<'_> {
    type Item = Result<ChunkInfo, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.over {
            return None;
        }
        if self.done >= self.raw_len {
            self.over = true;
            return (self.at != self.src.len()).then(|| {
                Err(Error::TrailingBytes {
                    left: self.src.len() - self.at,
                })
            });
        }
        let r = self.step();
        if r.is_err() {
            self.over = true;
        }
        Some(r)
    }
}

impl Chunks<'_> {
    fn step(&mut self) -> Result<ChunkInfo, Error> {
        if self.done.is_multiple_of(QUANTUM) {
            (self.h, self.at) = stream_header(self.src, self.at)?;
        }
        let h = self.h;
        let n = (self.raw_len - self.done).min(QUANTUM);
        let info = |mode, compressed_len| ChunkInfo {
            decoder: h.decoder,
            mode,
            checksums: h.checksums,
            raw_len: n,
            compressed_len,
        };
        let chunk = if h.uncompressed {
            self.src
                .get(self.at..self.at + n)
                .ok_or(Error::Truncated { at: self.at })?;
            self.at += n;
            info(ChunkMode::Uncompressed, n)
        } else {
            if !matches!(h.decoder, 6 | 10 | 12) {
                return Err(Error::Unsupported { decoder: h.decoder });
            }
            let q;
            (q, self.at) = quantum_header(self.src, self.at, h.checksums)?;
            match q {
                Quantum::Memset { .. } => info(ChunkMode::Memset, 0),
                Quantum::Data { size, .. } => {
                    if size > n {
                        return Err(Error::QuantumTooLarge {
                            at: self.at,
                            size,
                            room: n,
                        });
                    }
                    self.src
                        .get(self.at..self.at + size)
                        .ok_or(Error::Truncated { at: self.at })?;
                    self.at += size;
                    let mode = if size == n {
                        ChunkMode::Stored
                    } else {
                        ChunkMode::Compressed
                    };
                    info(mode, size)
                }
            }
        };
        self.done += n;
        Ok(chunk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stream header: the 0x_C magic with bits 4–5 clear, the restart bit, and the decoder.
    fn hdr(uncompressed: bool, decoder: u8, checksums: bool) -> [u8; 2] {
        [
            0x8C | (u8::from(uncompressed) << 6),
            decoder | (u8::from(checksums) << 7),
        ]
    }

    /// A quantum header for `size` bytes of data: the 18-bit field holds size − 1.
    fn data_q(size: usize) -> [u8; 3] {
        let v = (size - 1) as u32;
        [(v >> 16) as u8, (v >> 8) as u8, v as u8]
    }

    /// The memset quantum: the reserved size 0x3FFFF, kind 1, then the byte.
    fn memset_q(byte: u8) -> [u8; 4] {
        [0x07, 0xFF, 0xFF, byte]
    }

    fn bytes(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn an_uncompressed_stream_is_its_bytes() {
        let data = bytes(100);
        let src = [&hdr(true, 6, false)[..], &data].concat();
        assert_eq!(decompress(&src, 100).unwrap(), data);
    }

    #[test]
    fn every_quantum_starts_with_a_stream_header() {
        let data = bytes(QUANTUM + 50);
        let src = [
            &hdr(true, 6, false)[..],
            &data[..QUANTUM],
            &hdr(true, 12, false),
            &data[QUANTUM..],
        ]
        .concat();
        assert_eq!(decompress(&src, data.len()).unwrap(), data);
    }

    #[test]
    fn a_memset_quantum_repeats_its_byte() {
        let src = [&hdr(false, 6, false)[..], &memset_q(0xAB)].concat();
        assert_eq!(decompress(&src, 1000).unwrap(), vec![0xAB; 1000]);
    }

    #[test]
    fn a_quantum_as_long_as_its_output_is_stored() {
        let data = bytes(100);
        let src = [&hdr(false, 10, false)[..], &data_q(100), &data].concat();
        assert_eq!(decompress(&src, 100).unwrap(), data);
    }

    #[test]
    fn checksums_add_three_bytes_and_are_not_checked() {
        // ooz's Kraken_GetCrc is unimplemented (kraken.cpp:567); the port parses the checksum
        // and never checks it.
        let data = bytes(10);
        let src = [&hdr(false, 12, true)[..], &data_q(10), &[1, 2, 3], &data].concat();
        assert_eq!(decompress(&src, 10).unwrap(), data);
    }

    #[test]
    fn two_quanta() {
        let tail = bytes(10);
        let src = [
            &hdr(false, 6, false)[..],
            &memset_q(7),
            &hdr(false, 6, false),
            &data_q(10),
            &tail,
        ]
        .concat();
        let mut want = vec![7u8; QUANTUM];
        want.extend_from_slice(&tail);
        assert_eq!(decompress(&src, QUANTUM + 10).unwrap(), want);
    }

    #[test]
    fn bad_headers_are_errors() {
        assert_eq!(
            decompress(&[0x00, 0x06], 10),
            Err(Error::BadStreamHeader { at: 0, byte: 0 })
        );
        assert_eq!(
            decompress(&[0x9C, 0x06], 10), // bits 4–5 set
            Err(Error::BadStreamHeader { at: 0, byte: 0x9C })
        );
        for d in [5u8, 11, 7, 0] {
            let src = [&hdr(false, d, false)[..], &memset_q(1)].concat();
            assert_eq!(
                decompress(&src, 10),
                Err(Error::Unsupported { decoder: d }),
                "{d}"
            );
        }
        // The reserved size with a kind other than memset.
        let src = [&hdr(false, 6, false)[..], &[0x0B, 0xFF, 0xFF, 0]].concat();
        assert_eq!(decompress(&src, 10), Err(Error::BadQuantumHeader { at: 2 }));
    }

    #[test]
    fn cut_streams_are_truncated() {
        // Review Focus 2.
        let full = [&hdr(false, 6, false)[..], &data_q(10), &bytes(10)].concat();
        for cut in 0..full.len() {
            let r = decompress(&full[..cut], 10);
            assert!(
                matches!(r, Err(Error::Truncated { .. })),
                "cut at {cut}: {r:?}"
            );
        }
        let unc = [&hdr(true, 6, false)[..], &bytes(9)].concat();
        assert_eq!(decompress(&unc, 10), Err(Error::Truncated { at: 2 }));
    }

    #[test]
    fn oversized_quanta_are_refused() {
        // Review Focus 3.
        let src = [&hdr(false, 6, false)[..], &data_q(11), &bytes(11)].concat();
        assert_eq!(
            decompress(&src, 10),
            Err(Error::QuantumTooLarge {
                at: 5,
                size: 11,
                room: 10
            })
        );
    }

    #[test]
    fn leftover_bytes_are_an_error() {
        let src = [&hdr(true, 6, false)[..], &bytes(11)].concat();
        assert_eq!(decompress(&src, 10), Err(Error::TrailingBytes { left: 1 }));
        assert_eq!(
            decompress(&[0x8C], 0),
            Err(Error::TrailingBytes { left: 1 })
        );
        assert_eq!(decompress(&[], 0), Ok(Vec::new()));
    }

    #[test]
    fn a_compressed_quantum_needs_its_decoder() {
        // Tasks 4–6 each remove the decoder they port from this list and add its fixtures.
        for d in [6u8, 10, 12] {
            let src = [&hdr(false, d, false)[..], &data_q(5), &bytes(5)].concat();
            assert_eq!(
                decompress(&src, 10),
                Err(Error::Unsupported { decoder: d }),
                "{d}"
            );
        }
    }

    #[test]
    fn the_chunk_walk_reads_headers_only() {
        let tail = bytes(10);
        let src = [
            &hdr(false, 6, false)[..],
            &memset_q(7),
            &hdr(false, 6, true),
            &data_q(10),
            &[0, 0, 0],
            &tail,
        ]
        .concat();
        let got: Vec<_> = chunks(&src, QUANTUM + 10).collect();
        assert_eq!(
            got,
            vec![
                Ok(ChunkInfo {
                    decoder: 6,
                    mode: ChunkMode::Memset,
                    checksums: false,
                    raw_len: QUANTUM,
                    compressed_len: 0
                }),
                Ok(ChunkInfo {
                    decoder: 6,
                    mode: ChunkMode::Stored,
                    checksums: true,
                    raw_len: 10,
                    compressed_len: 10
                }),
            ]
        );
        // A compressed quantum is walked over, not decoded, and a walk ends at its first error.
        let src = [&hdr(false, 12, false)[..], &data_q(5), &bytes(5), &[9]].concat();
        let got: Vec<_> = chunks(&src, 10).collect();
        assert_eq!(
            got,
            vec![
                Ok(ChunkInfo {
                    decoder: 12,
                    mode: ChunkMode::Compressed,
                    checksums: false,
                    raw_len: 10,
                    compressed_len: 5
                }),
                Err(Error::TrailingBytes { left: 1 }),
            ]
        );
    }
}
