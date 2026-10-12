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
    /// Opens `path`, creating it if needed, and brings its schema up to date.
    pub async fn open(path: &Path) -> Result<Self> {
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
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

    async fn begin(&self) -> Result<Transaction<'static, Sqlite>> {
        Ok(self.pool.begin_with("BEGIN IMMEDIATE").await?)
    }
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
}
