//! Fenji's server (M3 spec §4): the game gate and the lobby, the account API and the admin
//! commands, in one process.
pub mod accounts;
pub mod api;
pub mod cli;
pub mod clock;
pub mod config;
pub mod gate;
pub mod lobby;
pub mod online;
pub mod server;

/// The most characters `[lobby] max_characters` may allow.
pub const MAX_CHARACTER_SLOTS: u32 = 20;
