//! Stub server for the protocol-discovery loop (design spec §4 B).
//!
//! Accepts connections from the real client, never answers, and writes every received byte
//! to a per-connection hex-dump log so we can see what the client sends first.

pub mod hexdump;
pub mod server;
pub mod session;
