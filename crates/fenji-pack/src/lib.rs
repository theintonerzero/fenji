//! The content pack's format (retranslation spec §6.2): what `fenji-content` releases and the
//! launcher fetches. A pack holds Fenji's own new English only, each line keyed by a
//! [`Fingerprint`] of the game's line, never the game's own text (ground rule 3 as amended).
mod fingerprint;
mod pack;
mod tsv;

pub use fingerprint::{Fingerprint, fingerprint};
pub use pack::{FORMAT, MAX_PACK, MAX_UNPACKED, Manifest, Pack, read_pack, verify};
pub use tsv::{Entry, escape, read_tsv, unescape, write_tsv};

/// Why a fingerprint, an entry, a pack or its signature was refused.
#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error("not a fingerprint (32 hex digits): {0:?}")]
    Fingerprint(String),
    #[error("an escape the format doesn't have: {0:?}")]
    Escape(String),
    #[error("line {line}: {why}")]
    Tsv { line: usize, why: String },
    #[error("the pack: {0}")]
    Pack(String),
    #[error("the signature: {0}")]
    Signature(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
