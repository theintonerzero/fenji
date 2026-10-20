//! The content pack's format (retranslation spec §6.2): what `fenji-content` releases and the
//! launcher fetches. A pack holds Fenji's own new English only, each line keyed by a
//! [`Fingerprint`] of the game's line, never the game's own text (ground rule 3 as amended).
mod fingerprint;
mod pack;
mod tsv;

pub use fingerprint::{Fingerprint, fingerprint, fingerprint_in};
pub use pack::{FORMAT, MAX_PACK, MAX_UNPACKED, Manifest, Pack, read_pack, verify};
pub use tsv::{Entry, escape, read_tsv, unescape, write_tsv};

/// The content key's public half (RT4), key ID 42B62B67B663DE9B, as `content.pub` in
/// `fenji-content` holds it: the launcher uses a pack only when its signature checks against it
/// ([`verify`]). Its private half signs `fenji-content`'s releases, behind the owner's approval
/// (docs/development.md, "Releasing a content pack").
pub const CONTENT_PUBLIC_KEY: &str = "RWSb3mO2Zyu2QviE1se0lm1X5N+t74cuZwi+34AepltITdZ1GjzpKEy+";

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_content_key_is_the_minisign_key_its_comment_names() {
        minisign_verify::PublicKey::from_base64(CONTENT_PUBLIC_KEY).unwrap();
        let key = minisign::PublicKey::from_base64(CONTENT_PUBLIC_KEY).unwrap();
        let id = u64::from_le_bytes(key.keynum().try_into().unwrap());
        assert_eq!(format!("{id:016X}"), "42B62B67B663DE9B");
    }
}
