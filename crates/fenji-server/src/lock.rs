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
    match try_take(data)? {
        Some(lock) => Ok(lock),
        None => bail!(in_use(data)),
    }
}

/// [`take`], or `None` while another process holds the lock: a server runs on the folder, or an
/// admin command is bringing its database up to date (#359).
///
/// # Errors
/// Why the folder or its lock can't be made or taken.
pub(crate) fn try_take(data: &Path) -> anyhow::Result<Option<Lock>> {
    crate::config::private_folder(data)
        .with_context(|| format!("can't make the data folder {}", data.display()))?;
    let path = data.join(NAME);
    let file = open_lock_file(&path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(Lock { _file: file })),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => {
            Err(e).with_context(|| format!("can't lock {}", path.display()))
        }
    }
}

/// Opens the lock file at `path` to take its lock, making it if it's missing. On Linux and macOS
/// it's never opened through a link in its place, which would make or open the link's target
/// wherever it points, and it's made owner-only (0600), however it was left (the hosting plan's
/// final review, Minor 1).
///
/// # Errors
/// If it can't be opened, a link in its place saying so, or its mode set.
pub(crate) fn open_lock_file(path: &Path) -> anyhow::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let no_link = rustix::fs::OFlags::NOFOLLOW.bits() as i32;
        options.mode(0o600).custom_flags(no_link);
    }
    let file = options.open(path).map_err(|e| {
        #[cfg(unix)]
        if rustix::io::Errno::from_io_error(&e) == Some(rustix::io::Errno::LOOP) {
            return anyhow::anyhow!(
                "can't open {}: it's a symbolic link, which the server never follows; delete it",
                path.display()
            );
        }
        anyhow::Error::new(e).context(format!("can't open {}", path.display()))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("can't make {} owner-only", path.display()))?;
    }
    Ok(file)
}

/// Whether the lock at `path` is free, as a server starting would find it: there's none, or no
/// process holds it. It's taken and let go at once, to see, and never opened through a link in its
/// place.
///
/// # Errors
/// If it can't be opened or locked for another reason.
#[cfg(unix)]
pub(crate) fn is_free(path: &Path) -> std::io::Result<bool> {
    use std::os::unix::fs::OpenOptionsExt;
    let no_link = rustix::fs::OFlags::NOFOLLOW.bits() as i32;
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(no_link)
        .open(path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(e) => return Err(e),
    };
    match file.try_lock() {
        Ok(()) => Ok(true),
        Err(TryLockError::WouldBlock) => Ok(false),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

/// Why a server can't start on the data folder `data`, and what to do (the hosting plan's final
/// review B, Minor 4).
pub(crate) fn in_use(data: &Path) -> String {
    let todo = if cfg!(unix) {
        "it runs in another window, in tmux or as a service; attach to it with \
         `fenji-server console`, or stop it first"
    } else {
        "it runs in another window; use that one, or stop it first"
    };
    format!(
        "a server is already using this data folder ({}): {todo}",
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
        // And what to do (the hosting plan's final review B, Minor 4).
        let todo = if cfg!(unix) {
            "): it runs in another window, in tmux or as a service; attach to it with \
             `fenji-server console`, or stop it first"
        } else {
            "): it runs in another window; use that one, or stop it first"
        };
        assert!(format!("{e:#}").ends_with(todo), "{e:#}");
        drop(held);
        let _again = take(&data).unwrap();
        assert!(data.join(NAME).is_file());
    }

    #[test]
    fn an_admin_command_takes_the_lock_only_while_no_one_holds_it() {
        // #359: an admin command holds the lock while it brings the database up to date, so that a
        // server starting meanwhile is refused; while a server holds it, the command sees so.
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let held = try_take(&data).unwrap().expect("free: made and taken");
        assert!(try_take(&data).unwrap().is_none(), "taken twice");
        drop(held);
        let server = take(&data).unwrap();
        assert!(try_take(&data).unwrap().is_none(), "a server holds it");
        drop(server);
        assert!(try_take(&data).unwrap().is_some());
    }

    #[cfg(unix)]
    #[test]
    fn the_lock_is_free_once_its_server_lets_go_of_it() {
        // The fix pass's re-review, NB2: what `--run stop` waits for.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(NAME);
        assert!(is_free(&path).unwrap(), "none yet");
        let held = take(dir.path()).unwrap();
        assert!(!is_free(&path).unwrap());
        drop(held);
        assert!(is_free(&path).unwrap());
        // Seeing doesn't keep it.
        let _again = take(dir.path()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_the_locks_place_is_never_followed() {
        // The hosting plan's final review, Minor 1: a dangling link at server.lock made the server
        // create its target, wherever it pointed.
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        std::fs::create_dir(&data).unwrap();
        let target = dir.path().join("elsewhere");
        std::os::unix::fs::symlink(&target, data.join(NAME)).unwrap();
        let e = format!("{:#}", take(&data).unwrap_err());
        assert!(e.contains("link"), "{e}");
        assert!(!target.exists(), "nothing was made where it pointed");
    }

    #[cfg(unix)]
    #[test]
    fn a_lock_left_open_to_others_is_made_owner_only() {
        // The hosting plan's final review, Minor 1: an existing 0644 server.lock stayed 0644.
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(NAME);
        std::fs::write(&path, b"").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let _held = take(dir.path()).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
