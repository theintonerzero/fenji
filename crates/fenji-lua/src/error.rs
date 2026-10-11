use crate::Dialect;

/// What can go wrong with a chunk.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("not a {0:?} Lua 5.3 header")]
    Header(Dialect),
    #[error("truncated at byte {offset}: {needed} more bytes needed")]
    Truncated { offset: usize, needed: usize },
    #[error("implausible count {count} at byte {offset}")]
    BadCount { offset: usize, count: i64 },
    #[error("unknown constant type {tag} at byte {offset}")]
    ConstantType { offset: usize, tag: u8 },
    #[error("NULL string constant at byte {offset}")]
    NullString { offset: usize },
    #[error("{extra} bytes after the main function (from byte {offset})")]
    Trailing { offset: usize, extra: usize },
    #[error("functions nested deeper than {limit} (at byte {offset})")]
    TooDeep { offset: usize, limit: usize },
    #[error("opcode {code} is not in the client's table")]
    UnknownOpcode { code: u8 },
    #[error("{op} has no established stock equivalent (docs/re/m1-script-format.md §4)")]
    Unmapped { op: &'static str },
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
