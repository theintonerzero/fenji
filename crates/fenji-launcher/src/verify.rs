//! Checks a client folder against a manifest (M3 spec D15): every size, and the executables' hashes
//! (quick), or every hash (full).
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Context;

use crate::manifest::{Manifest, hash_files_with};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Every size, and the hashes of `bin64/*.exe` and `bin64/*.dll`.
    Quick,
    /// Every hash.
    Full,
}

/// What differs, by manifest path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub missing: Vec<String>,
    pub wrong_size: Vec<String>,
    pub wrong_hash: Vec<String>,
}

/// What is wrong with one file (launcher spec §3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum Problem {
    Missing,
    WrongSize,
    WrongHash,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.missing.is_empty() && self.wrong_size.is_empty() && self.wrong_hash.is_empty()
    }

    /// Every file that failed, sorted and once each: what a repair fetches again.
    pub fn failing(&self) -> Vec<String> {
        let mut all: Vec<String> = self
            .missing
            .iter()
            .chain(&self.wrong_size)
            .chain(&self.wrong_hash)
            .cloned()
            .collect();
        all.sort();
        all.dedup();
        all
    }

    /// Adds what `other` found, as one check of both: a file in both is in [`Report::failing`]
    /// once.
    pub fn merge(&mut self, other: Report) {
        self.missing.extend(other.missing);
        self.wrong_size.extend(other.wrong_size);
        self.wrong_hash.extend(other.wrong_hash);
    }

    /// What is wrong with `path`, if anything: missing first, then the size, then the hash.
    pub fn problem(&self, path: &str) -> Option<Problem> {
        let has = |list: &[String]| list.iter().any(|p| p == path);
        if has(&self.missing) {
            Some(Problem::Missing)
        } else if has(&self.wrong_size) {
            Some(Problem::WrongSize)
        } else if has(&self.wrong_hash) {
            Some(Problem::WrongHash)
        } else {
            None
        }
    }
}

/// How far a check has come: files and bytes, by the manifest's sizes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Progress {
    pub files_done: usize,
    pub files_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

impl Progress {
    /// The bytes done, as 0.0 to 100.0; 100.0 when there is nothing to check.
    pub fn percent(&self) -> f32 {
        if self.bytes_total == 0 {
            100.0
        } else {
            (self.bytes_done as f64 * 100.0 / self.bytes_total as f64) as f32
        }
    }
}

/// A check that ended because it was asked to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stopped;

impl std::fmt::Display for Stopped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the check was stopped")
    }
}

impl std::error::Error for Stopped {}

/// An executable the quick check hashes: `bin64/*.exe` or `bin64/*.dll`.
fn executable(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    p.starts_with("bin64/") && (p.ends_with(".exe") || p.ends_with(".dll"))
}

/// Whether a read failed because the file, or a folder on its way, is no longer there.
fn gone(kind: std::io::ErrorKind) -> bool {
    matches!(
        kind,
        std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
    )
}

/// Compares `root` with `m`. Files the manifest doesn't list are ignored.
pub fn verify(root: &Path, m: &Manifest, level: Level) -> anyhow::Result<Report> {
    verify_with(root, m, level, &|_| {}, &AtomicBool::new(false))
}

/// [`verify`], calling `progress` as each file is sized or hashed, and once at the end. Once
/// `stop` is set it takes no new file, and it ends with an error that downcasts to [`Stopped`].
/// A file that disappears while it is checked is missing.
pub fn verify_with(
    root: &Path,
    m: &Manifest,
    level: Level,
    progress: &(dyn Fn(Progress) + Sync),
    stop: &AtomicBool,
) -> anyhow::Result<Report> {
    if !root.is_dir() {
        anyhow::bail!("there is no folder at {}", root.display());
    }
    let state = Mutex::new(Progress {
        files_total: m.entries.len(),
        bytes_total: m.entries.iter().map(|e| e.size).sum(),
        ..Progress::default()
    });
    // The lock is released before `progress` runs.
    let tick = |size: u64| {
        let now = {
            let mut p = state.lock().unwrap_or_else(|e| e.into_inner());
            p.files_done += 1;
            p.bytes_done += size;
            *p
        };
        progress(now);
    };
    let stopped = || stop.load(Ordering::Relaxed);
    let mut report = Report::default();
    let mut to_hash: Vec<(usize, PathBuf)> = Vec::new();
    for (i, e) in m.entries.iter().enumerate() {
        if stopped() {
            return Err(anyhow::Error::new(Stopped));
        }
        let full = root.join(&e.path);
        let meta = match std::fs::metadata(&full) {
            Ok(meta) if meta.is_file() => meta,
            Ok(_) => {
                report.missing.push(e.path.clone());
                tick(e.size);
                continue;
            }
            Err(err) if gone(err.kind()) => {
                report.missing.push(e.path.clone());
                tick(e.size);
                continue;
            }
            Err(err) => return Err(err).with_context(|| format!("reading {}", full.display())),
        };
        if meta.len() != e.size {
            report.wrong_size.push(e.path.clone());
            tick(e.size);
        } else if level == Level::Full || executable(&e.path) {
            to_hash.push((i, full));
        } else {
            tick(e.size);
        }
    }
    let files: Vec<PathBuf> = to_hash.iter().map(|(_, f)| f.clone()).collect();
    let hashed = hash_files_with(&files, &|k, _| tick(m.entries[to_hash[k].0].size), stop);
    if stopped() {
        return Err(anyhow::Error::new(Stopped));
    }
    for ((i, full), got) in to_hash.iter().zip(hashed) {
        match got {
            Ok((_, sha)) => {
                if sha != m.entries[*i].sha256 {
                    report.wrong_hash.push(m.entries[*i].path.clone());
                }
            }
            Err(err) if gone(err.kind()) => report.missing.push(m.entries[*i].path.clone()),
            Err(err) => return Err(err).with_context(|| format!("hashing {}", full.display())),
        }
    }
    progress(*state.lock().unwrap_or_else(|e| e.into_inner()));
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_folder_is_an_error_and_a_file_in_the_way_is_missing() {
        let (d, m) = tree();
        assert!(verify(&d.path().join("nope"), &m, Level::Quick).is_err());
        std::fs::remove_dir_all(d.path().join("data")).unwrap();
        std::fs::write(d.path().join("data"), b"a file where a folder was").unwrap();
        let r = verify(d.path(), &m, Level::Quick).unwrap();
        assert_eq!(r.missing, vec!["data/data000".to_string()]);
    }

    fn tree() -> (tempfile::TempDir, Manifest) {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("bin64")).unwrap();
        std::fs::create_dir_all(d.path().join("data")).unwrap();
        std::fs::write(d.path().join("bin64/gujianol.exe"), b"exe!").unwrap();
        std::fs::write(d.path().join("data/data000"), b"archive").unwrap();
        let m = Manifest::generate(d.path()).unwrap();
        (d, m)
    }

    #[test]
    fn an_intact_tree_passes_both_levels() {
        let (d, m) = tree();
        assert!(verify(d.path(), &m, Level::Quick).unwrap().ok());
        assert!(verify(d.path(), &m, Level::Full).unwrap().ok());
    }

    #[test]
    fn quick_catches_sizes_and_executables_full_catches_the_rest() {
        let (d, m) = tree();
        std::fs::write(d.path().join("bin64/gujianol.exe"), b"exe?").unwrap(); // same size
        let r = verify(d.path(), &m, Level::Quick).unwrap();
        assert_eq!(r.wrong_hash, vec!["bin64/gujianol.exe".to_string()]);
        assert!(!r.ok());
        let (d, m) = tree();
        std::fs::write(d.path().join("data/data000"), b"ARCHIVE").unwrap(); // same size
        assert!(verify(d.path(), &m, Level::Quick).unwrap().ok());
        assert_eq!(
            verify(d.path(), &m, Level::Full).unwrap().wrong_hash,
            vec!["data/data000".to_string()]
        );
        std::fs::write(d.path().join("data/data000"), b"longer archive").unwrap();
        assert_eq!(
            verify(d.path(), &m, Level::Quick).unwrap().wrong_size,
            vec!["data/data000".to_string()]
        );
        std::fs::remove_file(d.path().join("data/data000")).unwrap();
        assert_eq!(
            verify(d.path(), &m, Level::Quick).unwrap().missing,
            vec!["data/data000".to_string()]
        );
    }

    #[test]
    fn extra_files_dont_matter() {
        let (d, m) = tree();
        std::fs::write(d.path().join("bin64/settings.cfg"), b"mine").unwrap();
        assert!(verify(d.path(), &m, Level::Full).unwrap().ok());
    }

    /// Writes each `(path, size)` under `root`, filled with one repeating byte, and returns the
    /// manifest of what it wrote.
    fn folder_with(root: &Path, files: &[(&str, u64)]) -> Manifest {
        for (i, (path, size)) in files.iter().enumerate() {
            let full = root.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(&full, vec![b'a' + (i % 26) as u8; *size as usize]).unwrap();
        }
        Manifest::generate(root).unwrap()
    }

    #[test]
    fn a_full_check_reports_each_file_and_ends_at_one_hundred_percent() {
        let dir = tempfile::tempdir().unwrap();
        let m = folder_with(
            dir.path(),
            &[("a.bin", 10_000), ("bin64/b.dll", 20), ("c/d.txt", 5)],
        );
        let seen = std::sync::Mutex::new(Vec::new());
        let report = verify_with(
            dir.path(),
            &m,
            Level::Full,
            &|p| seen.lock().unwrap().push(p),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(report.ok());
        let seen = seen.into_inner().unwrap();
        let last = *seen.last().unwrap();
        assert_eq!((last.files_done, last.files_total), (3, 3));
        assert_eq!((last.bytes_done, last.bytes_total), (10_025, 10_025));
        assert_eq!(last.percent(), 100.0);
        assert!(
            seen.windows(2).all(|w| w[0].bytes_done <= w[1].bytes_done),
            "progress never goes back"
        );
    }

    #[test]
    fn a_check_stops_within_one_file_of_being_asked() {
        let dir = tempfile::tempdir().unwrap();
        let files: Vec<(String, u64)> = (0..50).map(|i| (format!("f{i:02}.bin"), 1_000)).collect();
        let refs: Vec<(&str, u64)> = files.iter().map(|(n, s)| (n.as_str(), *s)).collect();
        let m = folder_with(dir.path(), &refs);
        let stop = AtomicBool::new(false);
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let err = verify_with(
            dir.path(),
            &m,
            Level::Full,
            &|_| {
                if calls.fetch_add(1, Ordering::SeqCst) == 5 {
                    stop.store(true, Ordering::SeqCst);
                }
            },
            &stop,
        )
        .unwrap_err();
        assert!(err.downcast_ref::<Stopped>().is_some(), "{err:#}");
        assert!(
            calls.load(Ordering::SeqCst) < 50,
            "it went on after the stop"
        );
    }

    #[test]
    fn the_repair_list_names_every_failing_file_once_in_order() {
        let report = Report {
            missing: vec!["z.bin".into(), "a.bin".into()],
            wrong_size: vec!["m.bin".into()],
            wrong_hash: vec!["a.bin".into()],
        };
        assert_eq!(report.failing(), vec!["a.bin", "m.bin", "z.bin"]);
        assert_eq!(report.problem("m.bin"), Some(Problem::WrongSize));
        assert_eq!(report.problem("nothing"), None);
    }

    /// At the quick level these files are only sized, one after another, so the folder goes
    /// after the first: a full check hashes in parallel, and a file already open still reads.
    #[test]
    fn a_folder_removed_mid_check_reports_missing_files_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("game");
        let m = folder_with(&root, &[("a.bin", 100), ("b.bin", 100), ("c.bin", 100)]);
        let gone = std::sync::Once::new();
        let report = verify_with(
            &root,
            &m,
            Level::Quick,
            &|_| gone.call_once(|| std::fs::remove_dir_all(&root).unwrap()),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(report.missing, vec!["b.bin", "c.bin"]);
        assert!(report.wrong_size.is_empty() && report.wrong_hash.is_empty());
    }
}
