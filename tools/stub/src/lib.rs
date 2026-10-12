//! Stub server for the protocol-discovery loop (design spec §4 B).
//!
//! Accepts connections from the real client and writes every received byte to a per-connection
//! hex-dump log. By default it never answers; with `--speak` it speaks the client's transport
//! through `fenji-proto` and logs the decoded events too (M2 spec §5).

pub mod events;
pub mod hexdump;
pub mod server;
pub mod session;
pub mod speak;
