//! Stub server for the protocol-discovery loop (design spec §4 B).
//!
//! Accepts connections from the real client and writes what it receives to a per-connection
//! hex-dump log (all of it, unless `--no-auth-capture` keeps the auth out). By default it never
//! answers; with `--speak` it speaks the client's transport through `fenji-proto` and logs the
//! decoded events too (M2 spec §5).

use std::path::{Path, PathBuf};

pub mod events;
pub mod hexdump;
pub mod server;
pub mod session;
pub mod speak;

/// Where logs go without `--out`: `re/stub-logs` in the git checkout that holds `start` (the
/// nearest ancestor with a `.git`), so a stub started from a subdirectory still writes into the
/// git-ignored `re/` (ground rule 3: the client's auth carries the machine's MAC and fingerprint).
/// `None` outside a checkout.
pub fn default_out_dir(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .map(|root| root.join("re").join("stub-logs"))
}
