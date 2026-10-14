//! A Rust port of the Oodle decoders in ooz (github.com/powzix/ooz, commit 05038060aa68),
//! `kraken.cpp`, Copyright (C) 2016, Powzix; the port's changes Copyright the Fenji contributors.
//! GPL-3.0-or-later: see COPYING and NOTICE.
//!
//! [`decompress`] turns one Oodle stream into exactly `raw_len` bytes. [`chunks`] walks a
//! stream's headers without decoding, for the census in tools/archive. Only the Kraken family
//! (decoders 6, 10 and 12) is decoded; checksums are parsed and never checked, as in ooz.
mod error;
mod header;
mod stream;

pub use error::Error;
pub use stream::{ChunkInfo, ChunkMode, Chunks, QUANTUM, chunks, decompress};
