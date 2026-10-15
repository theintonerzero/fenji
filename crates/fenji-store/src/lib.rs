//! Fenji's SQLite store (M3 spec D7, §4; M4 hosting spec §3.9, §8): accounts, invites, sessions,
//! game tokens, characters, ops and bans, and the server's own state, in one file in WAL mode with
//! foreign keys on. Every write that must be atomic is one `BEGIN IMMEDIATE` transaction, so two
//! writers queue on the busy timeout instead of failing halfway. Times are Unix milliseconds;
//! secrets arrive here already hashed.
use std::io;
use std::path::Path;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Sqlite, SqlitePool, Transaction};

mod access;
mod accounts;
mod characters;
mod error;
mod invites;
mod tokens;

pub use access::Ban;
pub use accounts::{Account, AccountSummary};
pub use characters::{CREATE_LEVEL, Character, NewCharacter, Place};
pub use error::{Error, Result};
pub use invites::InviteRow;

/// The database, shared by every task of the server (cloning shares the pool).
#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
}

impl Store {
    /// Opens `path`, creating it if needed, and brings its schema up to date. The file is kept
    /// owner-only, since it holds password hashes, and SQLite overwrites deleted rows
    /// (`secure_delete`), so an account deleted is gone from the file too. Older copies in the
    /// write-ahead log go with [`Store::checkpoint`], or when SQLite next restarts the log, which
    /// it then cuts back to nothing (`journal_size_limit`), or when the last connection closes.
    pub async fn open(path: &Path) -> Result<Self> {
        owner_only(path)?;
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .pragma("secure_delete", "ON")
            .pragma("journal_size_limit", "0")
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    /// Folds the write-ahead log into the database and empties it (`wal_checkpoint(TRUNCATE)`), so
    /// nothing deleted lingers there. False when another connection still had the log in use at
    /// the end of the busy timeout: the log then empties when SQLite next restarts it.
    pub async fn checkpoint(&self) -> Result<bool> {
        let (busy, _, _): (i64, i64, i64) = sqlx::query_as("PRAGMA wal_checkpoint(TRUNCATE)")
            .fetch_one(&self.pool)
            .await?;
        Ok(busy == 0)
    }

    /// Writes a consistent copy of the database to `dest`, a new file, with `VACUUM INTO`: it reads
    /// one snapshot while writers carry on, so it's safe while the server runs. Refuses a `dest`
    /// that exists, or whose name isn't UTF-8. The copy is owner-only from the start, as the
    /// database is, and a copy that fails partway is deleted.
    pub async fn backup_into(&self, dest: &Path) -> Result<()> {
        let name = dest.to_str().ok_or_else(|| {
            Error::File(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{}: not UTF-8, which SQLite needs", dest.display()),
            ))
        })?;
        create_new(dest).map_err(|e| {
            let what = match e.kind() {
                io::ErrorKind::AlreadyExists => format!(
                    "{} exists already, and a backup never overwrites a file",
                    dest.display()
                ),
                _ => format!("{}: {e}", dest.display()),
            };
            Error::File(io::Error::new(e.kind(), what))
        })?;
        if let Err(e) = sqlx::query("VACUUM INTO ?")
            .bind(name)
            .execute(&self.pool)
            .await
        {
            // Best effort: SQLite's error is the one to report.
            let _ = std::fs::remove_file(dest);
            return Err(e.into());
        }
        Ok(())
    }

    /// `PRAGMA integrity_check`'s first line: `ok` when SQLite finds the whole file sound,
    /// otherwise the first problem it found.
    pub async fn integrity_check(&self) -> Result<String> {
        Ok(sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_one(&self.pool)
            .await?)
    }

    /// Closes every connection; later calls fail.
    pub async fn close(&self) {
        self.pool.close().await;
    }

    /// The server's state under `key` (M4 hosting spec §3.9), such as maintenance mode, if it has
    /// any.
    pub async fn state(&self, key: &str) -> Result<Option<String>> {
        Ok(
            sqlx::query_scalar("SELECT value FROM server_state WHERE key = ?")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    /// Keeps `value` under `key`, in place of what was there.
    pub async fn set_state(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO server_state (key, value) VALUES (?, ?) \
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Drops what is under `key`, if anything is.
    pub async fn clear_state(&self, key: &str) -> Result<()> {
        sqlx::query("DELETE FROM server_state WHERE key = ?")
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Holds the write lock for `d`, then lets go without writing: for tests of what other writers
    /// see while a server's transaction is open. `held` is sent once the lock is taken, so a test
    /// knows the lock is held rather than guessing with a sleep.
    #[cfg(feature = "test-util")]
    pub async fn hold_write_lock(
        &self,
        d: Duration,
        held: tokio::sync::oneshot::Sender<()>,
    ) -> Result<()> {
        let tx = self.begin().await?;
        let _ = held.send(());
        tokio::time::sleep(d).await;
        tx.rollback().await?;
        Ok(())
    }

    async fn begin(&self) -> Result<Transaction<'static, Sqlite>> {
        Ok(self.pool.begin_with("BEGIN IMMEDIATE").await?)
    }
}

/// Creates `path` owner-only (0600) if it is missing, and takes group and other access away from it
/// and from its `-wal` and `-shm` files if they have any (a database from v0.4.0 was 0644). SQLite
/// gives the files it creates later the main file's mode.
#[cfg(unix)]
fn owner_only(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)?;
    for suffix in ["", "-wal", "-shm"] {
        let mut p = path.as_os_str().to_owned();
        p.push(suffix);
        let p = std::path::PathBuf::from(p);
        match std::fs::metadata(&p) {
            Ok(m) if m.permissions().mode() & 0o077 != 0 => {
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600))?
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn owner_only(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Creates `path` empty, and owner-only (0600) on Unix, failing if it exists, for `VACUUM INTO` to
/// fill: SQLite writes into an empty file, though it refuses one with anything in it.
fn create_new(path: &Path) -> io::Result<()> {
    let mut file = std::fs::OpenOptions::new();
    file.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut file, 0o600);
    file.open(path).map(drop)
}

#[cfg(test)]
pub(crate) mod test_util {
    pub async fn store() -> (tempfile::TempDir, super::Store) {
        let dir = tempfile::tempdir().unwrap();
        let s = super::Store::open(&dir.path().join("fenji.db"))
            .await
            .unwrap();
        (dir, s)
    }

    pub const NOW: i64 = 1_800_000_000_000;

    pub fn h(n: u8) -> [u8; 32] {
        [n; 32]
    }

    /// Opens all four of the pool's connections and gives them back, so that the futures a test
    /// then joins each get a ready connection and really run at the same time (otherwise one
    /// waits for a new connection and the other has finished first).
    pub async fn warm(s: &super::Store) {
        let mut held = Vec::new();
        for _ in 0..4 {
            held.push(s.pool.acquire().await.unwrap());
        }
        drop(held);
    }

    /// A new account through a fresh invite whose hash is `h(n)`.
    pub async fn account(s: &super::Store, n: u8, name: &str) -> i64 {
        s.create_invite(&h(n), None, NOW, None).await.unwrap();
        s.redeem_invite(&h(n), name, "$phc", NOW).await.unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn open_uses_wal_and_keeps_data_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fenji.db");
        let s = Store::open(&path).await.unwrap();
        let mode: (String,) = sqlx::query_as("PRAGMA journal_mode")
            .fetch_one(&s.pool)
            .await
            .unwrap();
        assert_eq!(mode.0, "wal");
        let fk: (i64,) = sqlx::query_as("PRAGMA foreign_keys")
            .fetch_one(&s.pool)
            .await
            .unwrap();
        assert_eq!(fk.0, 1);
        s.create_invite(&[7; 32], Some("for bob"), 1, None)
            .await
            .unwrap();
        s.close().await;
        let s = Store::open(&path).await.unwrap();
        assert_eq!(s.list_invites().await.unwrap().len(), 1);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_missing_directory_is_reported_as_the_files_not_as_its_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let e = Store::open(&dir.path().join("missing").join("fenji.db"))
            .await
            .err()
            .expect("no such directory")
            .to_string();
        assert!(e.starts_with("the database file: "), "{e}");
        assert!(!e.contains("permissions"), "{e}");
    }

    #[tokio::test]
    async fn deleted_rows_are_overwritten() {
        let (_dir, s) = test_util::store().await;
        let on: (i64,) = sqlx::query_as("PRAGMA secure_delete")
            .fetch_one(&s.pool)
            .await
            .unwrap();
        assert_eq!(on.0, 1);
    }

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_new_database_and_its_wal_are_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fenji.db");
        let s = Store::open(&path).await.unwrap();
        s.create_invite(&[7; 32], None, 1, None).await.unwrap();
        assert_eq!(mode(&path), 0o600);
        let wal = dir.path().join("fenji.db-wal");
        assert!(
            wal.exists(),
            "a write in WAL mode leaves the -wal file while the pool is open"
        );
        assert_eq!(mode(&wal), 0o600);
        assert_eq!(mode(&dir.path().join("fenji.db-shm")), 0o600);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_existing_world_readable_database_is_made_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fenji.db");
        let s = Store::open(&path).await.unwrap();
        s.create_invite(&[7; 32], None, 1, None).await.unwrap();
        s.close().await;
        for f in ["fenji.db", "fenji.db-wal", "fenji.db-shm"] {
            let p = dir.path().join(f);
            if p.exists() {
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
            }
        }
        let s = Store::open(&path).await.unwrap();
        assert_eq!(s.list_invites().await.unwrap().len(), 1);
        for f in ["fenji.db", "fenji.db-wal", "fenji.db-shm"] {
            let p = dir.path().join(f);
            if p.exists() {
                assert_eq!(mode(&p), 0o600, "{f}");
            }
        }
    }

    /// The `-wal` file next to `dir`'s database (SQLite appends the suffix to the whole name).
    fn wal(dir: &Path) -> std::path::PathBuf {
        dir.join("fenji.db-wal")
    }

    #[tokio::test]
    async fn the_log_is_cut_back_whenever_sqlite_restarts_it() {
        // A checkpoint alone doesn't shrink the write-ahead log, so frames that copied a deleted
        // row would stay in the file until overwritten (PRIVACY.md, "Deleting your data").
        let (_d, s) = test_util::store().await;
        let limit: i64 = sqlx::query_scalar("PRAGMA journal_size_limit")
            .fetch_one(&s.pool)
            .await
            .unwrap();
        assert_eq!(limit, 0);
    }

    #[tokio::test]
    async fn a_checkpoint_empties_the_log() {
        let (d, s) = test_util::store().await;
        test_util::account(&s, 1, "bob").await;
        assert!(std::fs::metadata(wal(d.path())).unwrap().len() > 0);
        assert!(s.checkpoint().await.unwrap());
        assert_eq!(std::fs::metadata(wal(d.path())).unwrap().len(), 0);
    }

    #[tokio::test]
    async fn a_backup_taken_while_a_writer_works_is_consistent() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("fenji.db")).await.unwrap();
        let writer = {
            let store = store.clone();
            tokio::spawn(async move {
                for n in 0..300u32 {
                    let mut code = [0u8; 32];
                    code[..4].copy_from_slice(&n.to_le_bytes());
                    store.create_invite(&code, None, 1, None).await.unwrap();
                }
            })
        };
        let dest = dir.path().join("copy.db");
        store.backup_into(&dest).await.unwrap();
        writer.await.unwrap();
        let copy = Store::open(&dest).await.unwrap();
        assert_eq!(copy.integrity_check().await.unwrap(), "ok");
        assert!(
            store.backup_into(&dest).await.is_err(),
            "an existing file is never overwritten"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_backup_is_owner_only_and_needs_a_utf_8_name() {
        use std::os::unix::ffi::OsStrExt;
        let (d, s) = test_util::store().await;
        let dest = d.path().join("copy.db");
        s.backup_into(&dest).await.unwrap();
        assert_eq!(mode(&dest), 0o600, "it holds the password hashes");
        let odd = d.path().join(std::ffi::OsStr::from_bytes(b"copy-\xff.db"));
        assert!(s.backup_into(&odd).await.is_err());
        let names: Vec<_> = std::fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert!(
            names
                .iter()
                .all(|n| n == "copy.db" || n.as_bytes().starts_with(b"fenji.db")),
            "nothing written under that name or a lossy one: {names:?}"
        );
    }

    #[tokio::test]
    async fn server_state_keeps_one_value_per_key_across_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fenji.db");
        let s = Store::open(&path).await.unwrap();
        assert_eq!(s.state("maintenance").await.unwrap(), None);
        s.set_state("maintenance", "{\"message\":\"a\"}")
            .await
            .unwrap();
        s.set_state("maintenance", "{\"message\":\"b\"}")
            .await
            .unwrap();
        s.set_state("motd", "Welcome").await.unwrap();
        s.close().await;
        let s = Store::open(&path).await.unwrap();
        assert_eq!(
            s.state("maintenance").await.unwrap().as_deref(),
            Some("{\"message\":\"b\"}"),
            "the latest value, kept"
        );
        s.clear_state("maintenance").await.unwrap();
        s.clear_state("maintenance").await.unwrap();
        assert_eq!(s.state("maintenance").await.unwrap(), None);
        assert_eq!(s.state("motd").await.unwrap().as_deref(), Some("Welcome"));
    }

    #[tokio::test]
    async fn a_checkpoint_a_reader_holds_up_says_so() {
        // A reader still on an older snapshot keeps its frames in use: the log can't be emptied
        // (after the busy timeout), and the caller hears it.
        let (_d, s) = test_util::store().await;
        test_util::account(&s, 1, "bob").await;
        let mut reader = s.pool.acquire().await.unwrap();
        sqlx::query("BEGIN").execute(&mut *reader).await.unwrap();
        let _: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts")
            .fetch_one(&mut *reader)
            .await
            .unwrap();
        test_util::account(&s, 2, "eve").await;
        assert!(!s.checkpoint().await.unwrap());
        sqlx::query("ROLLBACK").execute(&mut *reader).await.unwrap();
    }
}
