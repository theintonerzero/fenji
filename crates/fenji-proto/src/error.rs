/// What can go wrong reading or writing the transport.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("a message body of {0} bytes is over the limit")]
    BodyTooLong(usize),
    #[error("a frame body of {0} bytes doesn't fit a u16 length of 1..=65535")]
    FrameLength(usize),
    #[error("raw-length prefix: {0}")]
    BadRawLength(&'static str),
    #[error("stored packet: the prefix says {raw} bytes, but {got} follow")]
    NotStored { raw: usize, got: usize },
    #[error("hello: {0}")]
    BadHello(String),
    #[error("variant {0} is not implemented (docs/protocol/transport.md §3)")]
    Unsupported(u32),
    #[error("auth message: {0}")]
    BadAuth(String),
    #[error("can't {action} in state {state}")]
    WrongState {
        action: &'static str,
        state: &'static str,
    },
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
