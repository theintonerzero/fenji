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
/// Unix, if it's missing. `given` is the `--config` this run was given, which the command a
/// refusal suggests repeats.
///
/// # Errors
/// `another fenji-server holds this data folder (<path>)` while another process holds it
/// (`in_use`), or why the folder or its lock can't be made or taken.
pub fn take(data: &Path, given: Option<&Path>) -> anyhow::Result<Lock> {
    match try_take(data)? {
        Some(lock) => Ok(lock),
        None => bail!(in_use(data, given)),
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

/// The second of the two holders a refusal over the data folder's lock names, after a server: an
/// admin command holds the lock for a moment, while it brings the database up to date (#359).
pub(crate) const OR_UPDATING: &str =
    "or a command bringing its database up to date (try again in a moment)";

/// Why a server can't start on the data folder `data`, naming both holders the lock may have, and
/// what to do about each (the hosting plan's final review B, Minor 4; #359): a server, which a
/// console attaches to with the `--config` this run was `given`, or which is stopped first; or a
/// command bringing the database up to date ([`OR_UPDATING`]).
pub(crate) fn in_use(data: &Path, given: Option<&Path>) -> String {
    let server = if cfg!(unix) {
        format!(
            "a server, in another window, in tmux or as a service (attach to it with `{}`, or \
             stop it first)",
            crate::cli::command_line(given, "console")
        )
    } else {
        "a server, in another window (use that one, or stop it first)".to_owned()
    };
    format!(
        "another fenji-server holds this data folder ({}): {server}, {OR_UPDATING}",
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
        let held = take(&data, None).unwrap();
        let e = take(&data, None).unwrap_err();
        assert!(
            format!("{e:#}").starts_with("another fenji-server holds this data folder ("),
            "{e:#}"
        );
        // The two it may be, up front, and what to do about each: a server (the hosting plan's
        // final review B, Minor 4), or a command bringing the database up to date (#359, Task 2's
        // review, Minor 3, and Task 7's review, Minor 1).
        let server = if cfg!(unix) {
            "): a server, in another window, in tmux or as a service (attach to it with \
             `fenji-server console`, or stop it first), "
        } else {
            "): a server, in another window (use that one, or stop it first), "
        };
        assert!(
            format!("{e:#}").ends_with(&format!("{server}{OR_UPDATING}")),
            "{e:#}"
        );
        assert_eq!(
            OR_UPDATING,
            "or a command bringing its database up to date (try again in a moment)"
        );
        drop(held);
        let _again = take(&data, None).unwrap();
        assert!(data.join(NAME).is_file());
    }

    #[cfg(unix)]
    #[test]
    fn a_refused_server_is_told_to_attach_with_the_config_it_was_given() {
        // #359 (Task 2's review, Minor 2): a server started with `--config` was told to attach
        // with `fenji-server console` alone, which finds no fenji.toml outside the server's folder.
        let data = Path::new("/var/lib/fenji");
        let etc = Path::new("/etc/fenji/fenji.toml");
        let text = in_use(data, Some(etc));
        assert!(
            text.contains(
                "attach to it with `fenji-server --config /etc/fenji/fenji.toml console`"
            ),
            "{text}"
        );
        let spaced = Path::new("/home/wen/my fenji/fenji.toml");
        let text = in_use(data, Some(spaced));
        assert!(
            text.contains("`fenji-server --config '/home/wen/my fenji/fenji.toml' console`"),
            "{text}"
        );
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
        let server = take(&data, None).unwrap();
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
        let held = take(dir.path(), None).unwrap();
        assert!(!is_free(&path).unwrap());
        drop(held);
        assert!(is_free(&path).unwrap());
        // Seeing doesn't keep it.
        let _again = take(dir.path(), None).unwrap();
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
        let e = format!("{:#}", take(&data, None).unwrap_err());
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
        let _held = take(dir.path(), None).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
