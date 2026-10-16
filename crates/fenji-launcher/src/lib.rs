//! Fenji's launcher (M3 spec D14): the account API client, client verification against the pinned
//! build's manifest, the launch command as data, and the login path behind a trait. `fenji-launch`
//! is a small CLI over it; `apps/launcher` is the desktop shell.
pub mod api;
pub mod app;
pub mod appimage;
pub mod depot;
pub mod invite_link;
pub mod launch;
pub mod login_path;
pub mod manifest;
pub mod pin;
pub mod probe;
pub mod servers;
pub mod settings;
pub mod start;
pub mod steam;
#[cfg(test)]
mod test_support;
pub mod tools;
pub mod verify;
