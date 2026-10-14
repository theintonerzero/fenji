//! A Rust port of the Oodle decoders in ooz's `kraken.cpp`
//! (github.com/powzix/ooz, commit 05038060aa68), Copyright (C) 2016 Powzix.
//! Changes Copyright (C) 2026 the Fenji contributors.
//! GPL-3.0-or-later: see COPYING and NOTICE.
//!
//! [`decompress`] turns one Oodle stream into exactly `raw_len` bytes. [`chunks`] walks a
//! stream's headers without decoding, for the census in tools/archive. Only the Kraken family
//! (decoders 6, 10 and 12) is decoded. Checksums are parsed but not verified, because ooz's CRC
//! (`Kraken_GetCrc`, kraken.cpp:567) is an unimplemented stub.
mod error;
mod header;
mod stream;

pub use error::Error;
pub use stream::{ChunkInfo, ChunkMode, Chunks, QUANTUM, chunks, decompress};
