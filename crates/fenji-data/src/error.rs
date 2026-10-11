/// Everything that can go wrong while reading the client's data.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a PE32+ image: {0}")]
    NotPe(&'static str),
    #[error("blob header at {at:#x}: {reason}")]
    BadBlob { at: u64, reason: String },
    #[error("codec {0} is not available here (Oodle waits on the M1 decision gate)")]
    CodecUnavailable(u16),
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
    #[error("output directory: {0}")]
    OutDir(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
