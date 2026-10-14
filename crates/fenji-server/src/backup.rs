//! Backups (M4 hosting spec §3.12, H8): `fenji-server backup` and the console's `backup` copy the
//! database into `backups` in the data folder with SQLite's `VACUUM INTO`, which is consistent
//! while the server writes. Each copy is checked before older ones are deleted (Ruling HP5), so a
//! bad copy never costs a good one.
use std::path::{Path, PathBuf};

use anyhow::Context;
use fenji_store::Store;

use crate::clock::Utc;
use crate::text::escape_hidden;

/// How many backups are kept unless the host says otherwise.
pub const DEFAULT_KEEP: usize = 14;

/// Where backups go: `backups` in the data folder, the directory of `[store] path`.
pub fn folder(store_path: &Path) -> PathBuf {
    store_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("backups")
}

/// Copies `store` into `dir` as `fenji-YYYYMMDD-HHMMSS.db`, named for `now_ms` in UTC, making `dir`
/// if it's missing; checks the copy; then deletes the oldest backups beyond `keep`, never the copy
/// just made. Returns the new file.
pub async fn run(store: &Store, dir: &Path, keep: usize, now_ms: i64) -> anyhow::Result<PathBuf> {
    make_folder(dir).with_context(|| format!("can't make the backups folder {}", dir.display()))?;
    let name = name(now_ms);
    let made = dir.join(&name);
    store
        .backup_into(&made)
        .await
        .context("can't back up the database")?;
    check(&made).await?;
    prune(dir, keep, &name).with_context(|| {
        format!(
            "{} is made, but the oldest backups beside it couldn't be deleted",
            made.display()
        )
    })?;
    Ok(made)
}

/// `fenji-YYYYMMDD-HHMMSS.db`, for `now_ms` in UTC.
fn name(now_ms: i64) -> String {
    let t = Utc::from_ms(now_ms);
    format!(
        "fenji-{:04}{:02}{:02}-{:02}{:02}{:02}.db",
        t.year, t.month, t.day, t.hour, t.minute, t.second
    )
}

/// Whether `name` is a backup's: `fenji-`, 8 digits, `-`, 6 digits and `.db`, exactly.
fn is_backup(name: &str) -> bool {
    name.strip_prefix("fenji-")
        .and_then(|rest| rest.strip_suffix(".db"))
        .is_some_and(|stamp| {
            stamp.len() == 15
                && stamp.bytes().enumerate().all(|(i, b)| {
                    if i == 8 {
                        b == b'-'
                    } else {
                        b.is_ascii_digit()
                    }
                })
        })
}

/// Makes `dir` if it's missing, owner-only (0700) on Unix, as backups hold the password hashes.
fn make_folder(dir: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}

/// Requires the copy to open as a store and pass SQLite's integrity check (Ruling HP5). A copy that
/// doesn't is deleted, and the error says so.
async fn check(copy: &Path) -> anyhow::Result<()> {
    let problem = match verdict(copy).await {
        Ok(v) if v == "ok" => return Ok(()),
        Ok(v) => anyhow::anyhow!("SQLite's integrity check found: {}", escape_hidden(&v)),
        Err(e) => e,
    };
    let fate = match std::fs::remove_file(copy) {
        Ok(()) => "so it was deleted".to_string(),
        Err(e) => format!("and couldn't be deleted ({e})"),
    };
    Err(problem.context(format!(
        "{} failed its check, {fate}; the older backups stay",
        copy.display()
    )))
}

/// The copy's integrity check, closing it again whatever the answer.
async fn verdict(copy: &Path) -> anyhow::Result<String> {
    let store = Store::open(copy).await?;
    let verdict = store.integrity_check().await;
    store.close().await;
    Ok(verdict?)
}

/// Deletes the oldest backups in `dir` beyond `keep`, counting `new`, the copy just made, which
/// stays whatever its name: a clock that once ran ahead may have left later ones. Only names
/// [`is_backup`] takes are touched; being fixed-width, they sort by time.
fn prune(dir: &Path, keep: usize, new: &str) -> std::io::Result<()> {
    let mut older = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name != new && is_backup(&name) && entry.file_type()?.is_file() {
            older.push(name);
        }
    }
    older.sort_unstable();
    let surplus = older.len().saturating_sub(keep.saturating_sub(1));
    for name in &older[..surplus] {
        match std::fs::remove_file(dir.join(name)) {
            // A backup running at the same time deleted it first.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            done => done?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_backup_lands_in_the_folder_and_the_oldest_beyond_keep_go() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("fenji.db")).await.unwrap();
        let backups = folder(&dir.path().join("fenji.db"));
        std::fs::create_dir_all(&backups).unwrap();
        for day in 1..=15 {
            std::fs::write(
                backups.join(format!("fenji-202601{day:02}-040000.db")),
                b"old",
            )
            .unwrap();
        }
        std::fs::write(backups.join("notes.txt"), b"mine").unwrap();
        // 2026-10-14 04:00:00 UTC
        let made = run(&store, &backups, 14, 1_791_950_400_000).await.unwrap();
        assert_eq!(made.file_name().unwrap(), "fenji-20261014-040000.db");
        let mut left: Vec<_> = std::fs::read_dir(&backups)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(
            left.len(),
            15,
            "14 backups and the unrelated file: {left:?}"
        );
        assert!(!left.contains(&"fenji-20260101-040000.db".to_string()));
        assert!(!left.contains(&"fenji-20260102-040000.db".to_string()));
        assert!(left.contains(&"notes.txt".to_string()));
    }

    #[test]
    fn only_backup_names_are_pruned() {
        for name in [
            "fenji-20261014-040000.db",
            "fenji-19700101-000000.db",
            "fenji-99991231-235959.db",
        ] {
            assert!(is_backup(name), "{name}");
        }
        for name in [
            "fenji.db",
            "fenji-2026.db",
            "notes.txt",
            "fenji-20261014-040000.db-wal",
            "x-20261014-040000.db",
        ] {
            assert!(!is_backup(name), "{name}");
        }
    }

    #[tokio::test]
    async fn a_copy_that_fails_its_check_is_deleted() {
        // Ruling HP5: the copy is checked before anything is pruned, and a bad one goes.
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("fenji-20261014-040000.db");
        std::fs::write(&bad, b"not a database").unwrap();
        let e = check(&bad).await.unwrap_err();
        assert!(!bad.exists(), "{e:#}");
        assert!(
            format!("{e:#}").contains("so it was deleted; the older backups stay"),
            "{e:#}"
        );
    }

    #[tokio::test]
    async fn the_new_backup_stays_when_older_ones_carry_later_names() {
        // A clock that once ran ahead left names later than now: the copy just made still stays.
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("fenji.db")).await.unwrap();
        let backups = folder(&dir.path().join("fenji.db"));
        std::fs::create_dir_all(&backups).unwrap();
        for day in 1..=3 {
            std::fs::write(
                backups.join(format!("fenji-202701{day:02}-040000.db")),
                b"old",
            )
            .unwrap();
        }
        let made = run(&store, &backups, 2, 1_791_950_400_000).await.unwrap();
        assert!(made.is_file());
        assert!(backups.join("fenji-20270103-040000.db").is_file());
        assert_eq!(std::fs::read_dir(&backups).unwrap().count(), 2);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_new_backups_folder_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("fenji.db")).await.unwrap();
        let backups = folder(&dir.path().join("fenji.db"));
        assert_eq!(backups, dir.path().join("backups"));
        run(&store, &backups, DEFAULT_KEEP, 0).await.unwrap();
        let mode = std::fs::metadata(&backups).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }
}
