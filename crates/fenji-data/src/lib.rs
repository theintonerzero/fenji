//! Reads the SoLO client's own data at run time (design spec §6.3; M1 spec §2–§5). Nothing it
//! reads is ever committed: tools write only under `re/` ([`re_dir`]).
pub mod archive;
mod blob;
pub mod cache;
pub mod client;
mod codec;
mod error;
mod exe_fs;
pub mod index;
pub mod oodle_spool;
mod pe;
pub mod re_dir;
#[cfg(any(test, feature = "test-util"))]
pub mod test_util;

pub use blob::{
    BlobHeader, Block, Codec, FIXED_LEN, blocks, decode as decode_blob, header_check,
    stored as stored_blob,
};
#[cfg(feature = "oodle")]
pub use codec::decode_oodle_block;
pub use codec::{BuiltinCodecs, Decompressor, MAX_OODLE_BLOCK, OODLE_DECODER, oodle_payload};
pub use error::{Error, Result};
pub use exe_fs::{ExeEntry, ExeFs, PIECE_LEN};
pub use pe::{PeImage, Section};

/// SHA-256 of the pinned `bin64/gujianol.exe` (docs/re/pinned-build.md).
pub const PINNED_EXE_SHA256: &str =
    "3fdc6394de85c1224b91312d62bf2e57251625b49e8d0f3960accf8dfe5f0678";

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex(&Sha256::digest(bytes))
}

/// `bytes` in lowercase hex, as the archives' keys are written.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn sha256_hex_matches_the_standard_vector() {
        assert_eq!(
            super::sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
