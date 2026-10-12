//! The SoLO client's transport, without I/O (M2 spec §4; docs/protocol/transport.md). Bytes go in;
//! bytes to send and events come out. Nothing here panics on peer input.
pub mod codec;
mod error;
pub mod frame;
pub mod header;
pub mod scramble;

pub use error::{Error, Result};
