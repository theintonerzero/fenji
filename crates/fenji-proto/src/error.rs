/// What can go wrong reading or writing the transport.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("a message body of {0} bytes is over the limit")]
    BodyTooLong(usize),
    #[error(
        "a frame body of {0} bytes is out of range (1..=65535 on the wire, at most 0x4080 to the client)"
    )]
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
    #[error("pickle: {0}")]
    Pickle(String),
    #[error("script message {0} is never sent (ground rule 6; docs/protocol/packets/lobby.md §2)")]
    NeverSend(i64),
    #[error("can't {action} in state {state}")]
    WrongState {
        action: &'static str,
        state: &'static str,
    },
    #[error("a type-{opcode} body of {len} bytes doesn't fit its layout")]
    BadBody { opcode: u8, len: usize },
    #[error("property blob: {0}")]
    Blob(String),
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
