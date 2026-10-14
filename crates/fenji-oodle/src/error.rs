use std::fmt;

/// Why a stream didn't decode. `at` is a byte offset into the stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The stream ended inside a header or a quantum.
    Truncated { at: usize },
    /// A stream header whose first byte isn't `0x_C` with bits 4–5 clear.
    BadStreamHeader { at: usize, byte: u8 },
    /// A quantum header with the reserved size and a kind other than memset.
    BadQuantumHeader { at: usize },
    /// A decoder this crate doesn't decode: LZNA (5), Bitknit (11), a number ooz doesn't know, or
    /// one not ported (a codec the pinned client doesn't use).
    Unsupported { decoder: u8 },
    /// A quantum claiming more bytes than its output holds.
    QuantumTooLarge { at: usize, size: usize, room: usize },
    /// Malformed quantum data; `what` says how.
    Corrupt { at: usize, what: &'static str },
    /// Bytes left over once the output is complete.
    TrailingBytes { left: usize },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated { at } => write!(f, "the stream ends early, at byte {at}"),
            Self::BadStreamHeader { at, byte } => {
                write!(f, "not an Oodle stream header at byte {at} ({byte:#04x})")
            }
            Self::BadQuantumHeader { at } => write!(f, "a bad quantum header at byte {at}"),
            Self::Unsupported { decoder } => write!(f, "decoder {decoder} isn't one Fenji decodes"),
            Self::QuantumTooLarge { at, size, room } => write!(
                f,
                "the quantum at byte {at} claims {size} bytes where its output holds {room}"
            ),
            Self::Corrupt { at, what } => write!(f, "corrupt data at byte {at}: {what}"),
            Self::TrailingBytes { left } => {
                write!(f, "{left} bytes are left over after the output is complete")
            }
        }
    }
}

impl std::error::Error for Error {}
