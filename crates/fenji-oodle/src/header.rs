//! Part of fenji-oodle, a Rust port of the Oodle decoders in ooz's `kraken.cpp`
//! (github.com/powzix/ooz, commit 05038060aa68), Copyright (C) 2016 Powzix.
//! Changes Copyright (C) 2026 the Fenji contributors.
//! GPL-3.0-or-later: see COPYING and NOTICE.
//!
//! The stream and quantum headers: ports of `Kraken_ParseHeader` (kraken.cpp:465) and
//! `Kraken_ParseQuantumHeader` (kraken.cpp:482).
use crate::Error;

/// The two-byte header before every 256 KiB of output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StreamHeader {
    /// 6 Kraken, 10 Mermaid and Selkie, 12 Leviathan, 5 LZNA, 11 Bitknit.
    pub decoder: u8,
    pub uncompressed: bool,
    pub checksums: bool,
}

/// Port of `Kraken_ParseHeader` (kraken.cpp:465): the header at `at`, and where what follows it
/// starts.
pub(crate) fn stream_header(src: &[u8], at: usize) -> Result<(StreamHeader, usize), Error> {
    let Some(&[b0, b1]) = src.get(at..at + 2) else {
        return Err(Error::Truncated { at });
    };
    if b0 & 0x0F != 0x0C || (b0 >> 4) & 3 != 0 {
        return Err(Error::BadStreamHeader { at, byte: b0 });
    }
    let decoder = b1 & 0x7F;
    if !matches!(decoder, 5 | 6 | 10 | 11 | 12) {
        return Err(Error::Unsupported { decoder });
    }
    let h = StreamHeader {
        decoder,
        uncompressed: (b0 >> 6) & 1 == 1,
        checksums: b1 >> 7 == 1,
    };
    Ok((h, at + 2))
}

/// A Kraken-family quantum header (decoders 6, 10 and 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Quantum {
    /// `size` bytes of quantum data follow. As long as the quantum's output, they're stored as is.
    Data { size: usize, checksum: Option<u32> },
    /// The output is one byte, repeated.
    Memset { byte: u8 },
}

/// Port of `Kraken_ParseQuantumHeader` (kraken.cpp:482).
pub(crate) fn quantum_header(
    src: &[u8],
    at: usize,
    checksums: bool,
) -> Result<(Quantum, usize), Error> {
    let b = src.get(at..at + 3).ok_or(Error::Truncated { at })?;
    let v = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
    let size = v & 0x3FFFF;
    if size != 0x3FFFF {
        let size = size as usize + 1;
        if !checksums {
            return Ok((
                Quantum::Data {
                    size,
                    checksum: None,
                },
                at + 3,
            ));
        }
        let c = src
            .get(at + 3..at + 6)
            .ok_or(Error::Truncated { at: at + 3 })?;
        let checksum = u32::from(c[0]) << 16 | u32::from(c[1]) << 8 | u32::from(c[2]);
        return Ok((
            Quantum::Data {
                size,
                checksum: Some(checksum),
            },
            at + 6,
        ));
    }
    if v >> 18 == 1 {
        let byte = *src.get(at + 3).ok_or(Error::Truncated { at: at + 3 })?;
        return Ok((Quantum::Memset { byte }, at + 4));
    }
    Err(Error::BadQuantumHeader { at })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_header_fields() {
        assert_eq!(
            stream_header(&[0x4C, 0x86], 0),
            Ok((
                StreamHeader {
                    decoder: 6,
                    uncompressed: true,
                    checksums: true
                },
                2
            ))
        );
        assert_eq!(stream_header(&[0x8C], 0), Err(Error::Truncated { at: 0 }));
    }

    #[test]
    fn quantum_header_sizes_and_memset() {
        assert_eq!(
            quantum_header(&[0x03, 0xFF, 0xFE], 0, false),
            Ok((
                Quantum::Data {
                    size: 0x3FFFF,
                    checksum: None
                },
                3
            ))
        );
        assert_eq!(
            quantum_header(&[0x00, 0x00, 0x00, 0xA, 0xB, 0xC], 0, true),
            Ok((
                Quantum::Data {
                    size: 1,
                    checksum: Some(0x0A0B0C)
                },
                6
            ))
        );
        assert_eq!(
            quantum_header(&[0x07, 0xFF, 0xFF, 0x5A], 0, true),
            Ok((Quantum::Memset { byte: 0x5A }, 4))
        );
        assert_eq!(
            quantum_header(&[0x07, 0xFF, 0xFF], 0, false),
            Err(Error::Truncated { at: 3 })
        );
    }
}
