/// Everything that can go wrong while reading the client's data.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a PE32+ image: {0}")]
    NotPe(&'static str),
    #[error("blob header at {at:#x}: {reason}")]
    BadBlob { at: u64, reason: String },
    #[error("codec {0} is not available here (codec 4 needs an OodleSpool: M2 plan Task 11)")]
    CodecUnavailable(u16),
    #[error("Oodle block {0} is spooled: run scripts/spike/run-oodle-helper.sh, then retry")]
    OodlePending(String),
    #[error(
        "Oodle output {path}: {problem}; delete it and run scripts/spike/run-oodle-helper.sh again"
    )]
    OodleBadOutput { path: String, problem: String },
    #[error("LZ4 block {index}: {reason}")]
    Lz4 { index: usize, reason: String },
    #[error("decoded {got} bytes where the header says {want}")]
    SizeMismatch { got: u64, want: u64 },
    #[error("{len} bytes at {va:#x} are not all in one section's file data")]
    OutOfImage { va: u64, len: u64 },
    #[error("no embedded file system (mount table) found in .data")]
    NoRegistry,
    #[error("embedded file at {va:#x}: {reason}")]
    BadEntry { va: u64, reason: String },
    #[error("archive: {0}")]
    BadArchive(String),
    #[error("output directory: {0}")]
    OutDir(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
