//! One server to a data folder (M4 hosting spec §3.5; Review Focus 5): `server.lock` in the data
//! folder, which a running server holds, on every platform. The system lets go of the lock however
//! the process ends, `kill -9` included. `fenji-server run` takes it before it opens the store or
//! listens: on Linux and macOS through the console socket's claim, which also replaces a stale
//! socket; elsewhere alone, so that a start script run twice on Windows doesn't run a second world
//! on one database.
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

/// The lock's name in the data folder.
pub const NAME: &str = "server.lock";

/// The data folder's lock, held until it's dropped.
#[derive(Debug)]
pub struct Lock {
    _file: File,
}

/// Takes the lock of the data folder `data` for this process, making the folder, owner-only on
/// Unix, if it's missing.
///
/// # Errors
/// `a server is already using this data folder (<path>)` while another server holds it, or why the
/// folder or its lock can't be made or taken.
pub fn take(data: &Path) -> anyhow::Result<Lock> {
    crate::config::private_folder(data)
        .with_context(|| format!("can't make the data folder {}", data.display()))?;
    let path = data.join(NAME);
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(false);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let file = options
        .open(&path)
        .with_context(|| format!("can't open {}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(Lock { _file: file }),
        Err(TryLockError::WouldBlock) => bail!(in_use(data)),
        Err(TryLockError::Error(e)) => {
            Err(e).with_context(|| format!("can't lock {}", path.display()))
        }
    }
}

/// Why a server can't start on the data folder `data`.
pub(crate) fn in_use(data: &Path) -> String {
    format!(
        "a server is already using this data folder ({})",
        shown(data).display()
    )
}

/// `data` as a host finds it: from the root, as the working directory makes it.
pub(crate) fn shown(data: &Path) -> PathBuf {
    std::path::absolute(data).unwrap_or_else(|_| data.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_data_folder_is_one_servers_while_it_holds_the_lock() {
        // M4 hosting plan, Task 11's review 1, Minor 3: on every platform, Windows' start script
        // double-clicked twice among them, before a second server opens the database.
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let held = take(&data).unwrap();
        let e = take(&data).unwrap_err();
        assert!(
            format!("{e:#}").contains("a server is already using this data folder ("),
            "{e:#}"
        );
        drop(held);
        let _again = take(&data).unwrap();
        assert!(data.join(NAME).is_file());
    }
}
