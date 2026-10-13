//! The SoLO client's transport, without I/O (M2 spec §4; docs/protocol/transport.md). Bytes go in;
//! bytes to send and events come out. Nothing here panics on peer input.
pub mod client;
pub mod codec;
mod error;
pub mod frame;
pub mod handshake;
pub mod header;
pub mod hello;
pub mod lobby;
pub mod opcode;
pub mod packet;
pub mod pickle;
pub mod scramble;
pub mod variant;

pub use error::{Error, Result};
