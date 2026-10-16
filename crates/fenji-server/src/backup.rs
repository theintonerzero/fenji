//! Backups (M4 hosting spec §3.12, H8): `fenji-server backup` and the console's `backup` copy the
//! database into `backups` in the data folder with SQLite's `VACUUM INTO`, which is consistent
//! while the server writes. Each copy is made under a hidden name, checked, and only then given its
//! backup's name (the hosting plan's final review, Minor 4), before older ones are deleted (Ruling
//! HP5): a copy cut short, or one that fails its check, never passes for a backup, and a bad copy
//! never costs a good one.
use std::fs::TryLockError;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
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
/// if it's missing: under the hidden [`part_name`] until the copy has passed its check, then under
/// its own; then deletes the oldest backups beyond `keep`, never the copy just made. It holds
/// `dir`'s lock throughout, so that it can remove what killed backups left. Returns the new file.
///
/// # Errors
/// If another backup is being made in `dir`, if the copy can't be made or fails its check, or if
/// the oldest can't be deleted.
pub async fn run(store: &Store, dir: &Path, keep: usize, now_ms: i64) -> anyhow::Result<PathBuf> {
    // Owner-only on Unix, as backups hold the password hashes.
    crate::config::private_folder(dir)
        .with_context(|| format!("can't make the backups folder {}", dir.display()))?;
    let _held = hold(dir)?;
    remove_leftovers(dir).with_context(|| {
        format!(
            "can't remove what a killed backup left in {}",
            dir.display()
        )
    })?;
    let name = name(now_ms);
    let made = dir.join(&name);
    if made.try_exists()? {
        bail!(
            "{} exists already, and a backup never overwrites a file",
            made.display()
        );
    }
    let part = dir.join(part_name(&name));
    store
        .backup_into(&part)
        .await
        .context("can't back up the database")?;
    check(&part, &made).await?;
    std::fs::rename(&part, &made)
        .with_context(|| format!("can't name the checked copy {}", made.display()))?;
    prune(dir, keep, &name).with_context(|| {
        format!(
            "{} is made, but the oldest backups beside it couldn't be deleted",
            made.display()
        )
    })?;
    Ok(made)
}

/// Takes the backups folder `dir`'s lock: a file of its own, which SQLite never opens, so that its
/// lock and SQLite's on the copies never meet.
fn hold(dir: &Path) -> anyhow::Result<std::fs::File> {
    let lock = crate::lock::open_lock_file(&dir.join(LOCK))?;
    match lock.try_lock() {
        Ok(()) => Ok(lock),
        Err(TryLockError::WouldBlock) => bail!(
            "another backup is being made in {} now; try again once it's done",
            dir.display()
        ),
        Err(TryLockError::Error(e)) => {
            Err(e).with_context(|| format!("can't lock {}", dir.join(LOCK).display()))
        }
    }
}

/// Removes from `dir` what backups killed before their copy passed its check left ([`is_part`]).
/// Only for a backup that holds the folder's lock: no other is being made.
fn remove_leftovers(dir: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let leftover = entry.file_name().to_str().is_some_and(is_part);
        if leftover && entry.file_type()?.is_file() {
            match std::fs::remove_file(entry.path()) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
    }
    Ok(())
}

/// `fenji-YYYYMMDD-HHMMSS.db`, for `now_ms` in UTC.
fn name(now_ms: i64) -> String {
    let t = Utc::from_ms(now_ms);
    format!(
        "fenji-{:04}{:02}{:02}-{:02}{:02}{:02}.db",
        t.year, t.month, t.day, t.hour, t.minute, t.second
    )
}

/// The lock a backup holds in the backups folder while it's made, so that two are never made at
/// once: each removes the leftovers of killed ones, and would take another's copy for one.
const LOCK: &str = ".lock";

/// The hidden name a backup's copy has until it has passed its check, for the backup `name`:
/// `.<name>.part` (the hosting plan's final review, Minor 4).
fn part_name(name: &str) -> String {
    format!(".{name}.part")
}

/// Whether `name` is a copy's that never passed its check, as a killed backup leaves it: a
/// [`part_name`], or one of the files SQLite keeps beside it as it's checked.
fn is_part(name: &str) -> bool {
    name.strip_prefix('.')
        .and_then(|rest| rest.split_once(".part"))
        .is_some_and(|(backup, beside)| {
            is_backup(backup) && ["", "-wal", "-shm", "-journal"].contains(&beside)
        })
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

/// Requires `copy`, which is to be the backup `made`, to open as a store and pass SQLite's
/// integrity check (Ruling HP5). A copy that doesn't is deleted, and the error says so.
async fn check(copy: &Path, made: &Path) -> anyhow::Result<()> {
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
        "the copy for {} failed its check, {fate}; the older backups stay",
        made.display()
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
        let made = run(&store, &backups, 14, ON_THE_14TH).await.unwrap();
        assert_eq!(made.file_name().unwrap(), "fenji-20261014-040000.db");
        let left = listed(&backups);
        assert_eq!(
            left.len(),
            16,
            "14 backups, the backups' lock and the unrelated file: {left:?}"
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
        let made = dir.path().join("fenji-20261014-040000.db");
        let bad = dir.path().join(part_name("fenji-20261014-040000.db"));
        std::fs::write(&bad, b"not a database").unwrap();
        let e = check(&bad, &made).await.unwrap_err();
        assert!(!bad.exists(), "{e:#}");
        assert!(!made.exists(), "it never had a backup's name");
        let says = format!(
            "the copy for {} failed its check, so it was deleted; the older backups stay",
            made.display()
        );
        assert!(format!("{e:#}").contains(&says), "{e:#}");
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
        let made = run(&store, &backups, 2, ON_THE_14TH).await.unwrap();
        assert!(made.is_file());
        assert_eq!(
            listed(&backups),
            [LOCK, "fenji-20261014-040000.db", "fenji-20270103-040000.db"]
        );
    }

    /// 2026-10-14 04:00:00 UTC.
    const ON_THE_14TH: i64 = 1_791_950_400_000;

    /// The names in `dir`, sorted.
    fn listed(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    /// A store in `dir`, and its backups folder with an older backup in it, of the 2nd.
    async fn with_a_backup(dir: &Path) -> (Store, PathBuf) {
        let store = Store::open(&dir.join("fenji.db")).await.unwrap();
        let backups = folder(&dir.join("fenji.db"));
        std::fs::create_dir_all(&backups).unwrap();
        std::fs::write(backups.join("fenji-20261002-040000.db"), b"old").unwrap();
        (store, backups)
    }

    #[test]
    fn a_copy_has_a_hidden_name_until_it_is_checked() {
        // The hosting plan's final review, Minor 4.
        let part = part_name("fenji-20261014-040000.db");
        assert_eq!(part, ".fenji-20261014-040000.db.part");
        assert!(!is_backup(&part));
        for left in ["", "-wal", "-shm", "-journal"] {
            assert!(is_part(&format!("{part}{left}")), "{left}");
        }
        for other in [
            LOCK,
            "fenji-20261014-040000.db",
            ".fenji.db.part",
            ".x-20261014-040000.db.part",
            ".fenji-20261014-040000.db.partial",
            ".fenji-20261014-040000.db.part-x",
            "fenji-20261014-040000.db.part",
        ] {
            assert!(!is_part(other), "{other}");
        }
    }

    #[tokio::test]
    async fn a_leftover_of_a_killed_backup_never_counts_and_the_next_one_removes_it() {
        // The hosting plan's final review, Minor 4: a backup killed as it copied (its timer's
        // service stopped, a reboot, Ctrl-C) left a part-written copy under a backup's name, which
        // later prunes counted, and which a host might restore as the newest. A copy has a hidden
        // name until it has passed its check: a killed one's never counts, and the next backup
        // removes it.
        let dir = tempfile::tempdir().unwrap();
        let (store, backups) = with_a_backup(dir.path()).await;
        std::fs::write(backups.join("fenji-20261001-040000.db"), b"old").unwrap();
        // What a backup killed on the 13th left: its copy, and the copy's log as its check had it.
        for left in [
            ".fenji-20261013-040000.db.part",
            ".fenji-20261013-040000.db.part-wal",
        ] {
            std::fs::write(backups.join(left), b"cut short").unwrap();
        }
        let made = run(&store, &backups, 2, ON_THE_14TH).await.unwrap();
        assert_eq!(made, backups.join("fenji-20261014-040000.db"));
        // Two kept, the new one among them, as the leftovers were never counted; and they're gone.
        assert_eq!(
            listed(&backups),
            [LOCK, "fenji-20261002-040000.db", "fenji-20261014-040000.db"]
        );
    }

    #[tokio::test]
    async fn a_backup_that_fails_leaves_nothing_that_counts_as_one() {
        let dir = tempfile::tempdir().unwrap();
        let (store, backups) = with_a_backup(dir.path()).await;
        store.close().await;
        run(&store, &backups, 14, ON_THE_14TH).await.unwrap_err();
        let left = listed(&backups);
        let counted: Vec<&String> = left.iter().filter(|n| is_backup(n)).collect();
        assert_eq!(counted, ["fenji-20261002-040000.db"], "{left:?}");
        assert!(left.iter().all(|n| !is_part(n)), "{left:?}");
    }

    #[tokio::test]
    async fn a_backup_made_while_another_is_says_so_and_takes_nothing() {
        // Two at once, the nightly timer's and the console's: the second mustn't take the first's
        // copy for a leftover.
        let dir = tempfile::tempdir().unwrap();
        let (store, backups) = with_a_backup(dir.path()).await;
        let held = crate::lock::open_lock_file(&backups.join(LOCK)).unwrap();
        held.try_lock().unwrap();
        let e = run(&store, &backups, 14, ON_THE_14TH).await.unwrap_err();
        assert!(format!("{e:#}").contains("another backup"), "{e:#}");
        assert_eq!(listed(&backups), [LOCK, "fenji-20261002-040000.db"]);
        drop(held);
        run(&store, &backups, 14, ON_THE_14TH).await.unwrap();
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
