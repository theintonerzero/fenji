//! Fenji's SQLite store (M3 spec D7, §4): accounts, invites, sessions, game tokens and characters,
//! in one file in WAL mode with foreign keys on. Every write that must be atomic is one
//! `BEGIN IMMEDIATE` transaction, so two writers queue on the busy timeout instead of failing
//! halfway. Times are Unix milliseconds; secrets arrive here already hashed.
use std::path::Path;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Sqlite, SqlitePool, Transaction};

mod accounts;
mod characters;
mod error;
mod invites;
mod tokens;

pub use accounts::{Account, AccountSummary};
pub use characters::{Character, NewCharacter};
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
    /// (`secure_delete`), so an account deleted is gone from the file too, bar the WAL until its
    /// next checkpoint.
    pub async fn open(path: &Path) -> Result<Self> {
        owner_only(path)?;
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .pragma("secure_delete", "ON")
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    /// Closes every connection; later calls fail.
    pub async fn close(&self) {
        self.pool.close().await;
    }

    /// Holds the write lock for `d`, then lets go without writing: for tests of what other writers
    /// see while a server's transaction is open.
    #[doc(hidden)]
    pub async fn hold_write_lock(&self, d: Duration) -> Result<()> {
        let tx = self.begin().await?;
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
}
