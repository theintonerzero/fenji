//! Fenji's own accounts, without storage (M3 spec D6): password hashes, tokens and invite codes,
//! the rules for usernames and passwords, and the sign-in limiter. `fenji-server` runs the flows
//! against `fenji-store`; nothing here does I/O apart from reading the OS's random source.
mod error;
pub mod invite;
pub mod limiter;
pub mod password;
pub mod rules;
pub mod token;

pub use error::{Error, Result};
