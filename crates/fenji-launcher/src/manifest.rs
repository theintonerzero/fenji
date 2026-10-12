//! The pinned client's file list (M3 spec D15): each file's path, size and SHA-256, as
//! tab-separated lines sorted by path, with `#` comments. Hashes only, no game content.
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, bail};
use sha2::{Digest, Sha256};

/// The comment `fenji-launch manifest` writes first.
pub const HEADER: &str = "Fenji client manifest: path, size and SHA-256 of each file; hashes only, no game \
                          content (M3 spec D15)";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Relative, with `/` separators and no `..`.
    pub path: String,
    pub size: u64,
    pub sha256: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Manifest {
    pub entries: Vec<Entry>,
}

/// A manifest path: relative, `/`-separated, with no empty, `.` or `..` component, no backslash
/// and no drive.
fn check_path(path: &str) -> anyhow::Result<()> {
    if path.is_empty() || path.starts_with('/') || path.contains(['\\', ':', '\0']) {
        bail!("{path:?} is not a relative path with / separators");
    }
    if path
        .split('/')
        .any(|c| c.is_empty() || c == "." || c == "..")
    {
        bail!("{path:?} has an empty, . or .. component");
    }
    Ok(())
}

fn unhex(h: &str) -> anyhow::Result<[u8; 32]> {
    if h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("{h:?} is not 64 hex digits");
    }
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&h[2 * i..2 * i + 2], 16)?;
    }
    Ok(out)
}

impl Manifest {
    /// Reads `path\tsize\tsha256` lines, sorted by path with no repeats; blank lines and `#`
    /// comments are skipped.
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let mut entries: Vec<Entry> = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let n = i + 1;
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split('\t').collect();
            let [path, size, hash] = fields[..] else {
                bail!("line {n}: path, size and SHA-256, separated by tabs");
            };
            check_path(path).with_context(|| format!("line {n}"))?;
            if size.is_empty() || !size.bytes().all(|b| b.is_ascii_digit()) {
                bail!("line {n}: the size {size:?} is not a number");
            }
            let size = size.parse().with_context(|| format!("line {n}: size"))?;
            let sha256 = unhex(hash).with_context(|| format!("line {n}"))?;
            if entries.last().is_some_and(|e| e.path.as_str() >= path) {
                bail!("line {n}: {path:?} is out of order or repeated");
            }
            entries.push(Entry {
                path: path.into(),
                size,
                sha256,
            });
        }
        Ok(Self { entries })
    }

    /// The entries as lines, without a header.
    pub fn to_tsv(&self) -> String {
        let mut out = String::new();
        for e in &self.entries {
            let hash: String = e.sha256.iter().map(|b| format!("{b:02x}")).collect();
            out.push_str(&format!("{}\t{}\t{hash}\n", e.path, e.size));
        }
        out
    }

    /// The pinned build's manifest (docs/re/pinned-build.md), committed with the code.
    pub fn pinned() -> Self {
        Self::parse(include_str!("../manifests/10015727.tsv")).expect("the committed manifest")
    }

    /// Lists and hashes every file under `root`. A symbolic link or other non-file is an error.
    pub fn generate(root: &Path) -> anyhow::Result<Self> {
        let mut found = Vec::new();
        walk(root, root, &mut found)?;
        found.sort();
        let files: Vec<PathBuf> = found.iter().map(|(_, full)| full.clone()).collect();
        let hashes = hash_files(&files);
        let mut entries = Vec::with_capacity(found.len());
        for ((path, full), hash) in found.into_iter().zip(hashes) {
            let (size, sha256) = hash.with_context(|| format!("hashing {}", full.display()))?;
            entries.push(Entry { path, size, sha256 });
        }
        Ok(Self { entries })
    }
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> anyhow::Result<()> {
    for item in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let item = item?;
        let full = item.path();
        let kind = item.file_type()?;
        if kind.is_dir() {
            walk(root, &full, out)?;
        } else if kind.is_file() {
            let rel = full.strip_prefix(root)?;
            let mut parts = Vec::new();
            for c in rel.components() {
                let c = c.as_os_str().to_str();
                parts.push(c.with_context(|| format!("{} is not UTF-8", full.display()))?);
            }
            let path = parts.join("/");
            check_path(&path)?;
            out.push((path, full));
        } else {
            bail!("{} is neither a file nor a directory", full.display());
        }
    }
    Ok(())
}

/// A file's size and SHA-256, or why it couldn't be read.
pub(crate) type Hashed = std::io::Result<(u64, [u8; 32])>;

/// Each file's size and SHA-256, in the given order, hashed on every core.
pub(crate) fn hash_files(files: &[PathBuf]) -> Vec<Hashed> {
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let mut done: Vec<(usize, Hashed)> = std::thread::scope(|s| {
        let workers: Vec<_> = (0..threads.min(files.len().max(1)))
            .map(|_| {
                s.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(f) = files.get(i) else { break };
                        mine.push((i, hash_file(f)));
                    }
                    mine
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().expect("a hashing thread panicked"))
            .collect()
    });
    done.sort_by_key(|(i, _)| *i);
    done.into_iter().map(|(_, r)| r).collect()
}

fn hash_file(path: &Path) -> Hashed {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut size = 0u64;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        h.update(&buf[..n]);
    }
    Ok((size, h.finalize().into()))
}

#[cfg(test)]
pub(crate) fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tsv_round_trips() {
        let m = Manifest {
            entries: vec![
                Entry {
                    path: "bin64/a.dll".into(),
                    size: 3,
                    sha256: [1; 32],
                },
                Entry {
                    path: "data/b".into(),
                    size: 0,
                    sha256: [0xAB; 32],
                },
            ],
        };
        assert_eq!(Manifest::parse(&m.to_tsv()).unwrap(), m);
    }

    #[test]
    fn the_pinned_manifest_is_the_pinned_build() {
        let m = Manifest::pinned();
        assert_eq!(m.entries.len(), 210);
        assert_eq!(
            m.entries.iter().map(|e| e.size).sum::<u64>(),
            107_996_004_001
        );
        let exe = m
            .entries
            .iter()
            .find(|e| e.path == "bin64/gujianol.exe")
            .unwrap();
        assert_eq!(
            hex(&exe.sha256),
            "3fdc6394de85c1224b91312d62bf2e57251625b49e8d0f3960accf8dfe5f0678"
        );
        assert_eq!(exe.size, 54_029_984);
        let mut sorted = m.entries.clone();
        sorted.sort_by(|a, b| a.path.cmp(&b.path));
        assert_eq!(sorted, m.entries, "sorted by path");
    }

    #[test]
    fn parse_refuses_damaged_lines() {
        let h = "00".repeat(32);
        for bad in [
            "bin64/a.dll\t3\n".to_string(),
            format!("bin64/a.dll\tx\t{h}\n"),
            format!("bin64/a.dll\t3\t{}\n", &h[..62]),
            format!("bin64/a.dll\t3\t{}zz\n", &h[..62]),
            format!("../a\t3\t{h}\n"),
            format!("bin64/../a\t3\t{h}\n"),
            format!("/etc/a\t3\t{h}\n"),
            format!("bin64\\a.dll\t3\t{h}\n"),
            format!("\t3\t{h}\n"),
            format!("a\t3\t{h}\na\t3\t{h}\n"),
            format!("b\t3\t{h}\na\t3\t{h}\n"),
        ] {
            assert!(Manifest::parse(&bad).is_err(), "{bad:?}");
        }
        let ok = format!("# a comment\n\nbin64/a.dll\t3\t{h}\n");
        assert_eq!(Manifest::parse(&ok).unwrap().entries.len(), 1);
    }

    #[test]
    fn generate_lists_every_file_sorted_with_its_hash() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("bin64/sub")).unwrap();
        std::fs::write(d.path().join("bin64/sub/z"), b"z").unwrap();
        std::fs::write(d.path().join("a.txt"), b"").unwrap();
        let m = Manifest::generate(d.path()).unwrap();
        let paths: Vec<&str> = m.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["a.txt", "bin64/sub/z"]);
        // SHA-256 of the empty string and of "z".
        assert_eq!(
            hex(&m.entries[0].sha256),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&m.entries[1].sha256),
            "594e519ae499312b29433b7dd8a97ff068defcba9755b6d5d00e84c524d67b06"
        );
        assert_eq!(m.entries[1].size, 1);
    }
}
