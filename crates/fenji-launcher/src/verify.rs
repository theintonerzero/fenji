//! Checks a client folder against a manifest (M3 spec D15): every size, and the executables' hashes
//! (quick), or every hash (full).
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::manifest::{Manifest, hash_files};

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

impl Report {
    pub fn ok(&self) -> bool {
        self.missing.is_empty() && self.wrong_size.is_empty() && self.wrong_hash.is_empty()
    }
}

/// An executable the quick check hashes: `bin64/*.exe` or `bin64/*.dll`.
fn executable(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    p.starts_with("bin64/") && (p.ends_with(".exe") || p.ends_with(".dll"))
}

/// Compares `root` with `m`. Files the manifest doesn't list are ignored.
pub fn verify(root: &Path, m: &Manifest, level: Level) -> anyhow::Result<Report> {
    if !root.is_dir() {
        anyhow::bail!("there is no folder at {}", root.display());
    }
    let mut report = Report::default();
    let mut to_hash: Vec<(usize, PathBuf)> = Vec::new();
    for (i, e) in m.entries.iter().enumerate() {
        let full = root.join(&e.path);
        let meta = match std::fs::metadata(&full) {
            Ok(meta) if meta.is_file() => meta,
            Ok(_) => {
                report.missing.push(e.path.clone());
                continue;
            }
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                report.missing.push(e.path.clone());
                continue;
            }
            Err(err) => return Err(err).with_context(|| format!("reading {}", full.display())),
        };
        if meta.len() != e.size {
            report.wrong_size.push(e.path.clone());
        } else if level == Level::Full || executable(&e.path) {
            to_hash.push((i, full));
        }
    }
    let files: Vec<PathBuf> = to_hash.iter().map(|(_, f)| f.clone()).collect();
    for ((i, full), got) in to_hash.iter().zip(hash_files(&files)) {
        let (_, sha) = got.with_context(|| format!("hashing {}", full.display()))?;
        if sha != m.entries[*i].sha256 {
            report.wrong_hash.push(m.entries[*i].path.clone());
        }
    }
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
}
