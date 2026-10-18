//! A copy of the game on disk, read by path: `data/_index` maps each path to its file's key, and
//! the data parts' live records hold the files (docs/re/m1-archives.md). It decodes natively
//! only; the archive tool's spool goes through its own `_index` and these places.
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::archive::{read_blob_header, read_part_header};
use crate::index::{self, PathMap};
use crate::{BuiltinCodecs, Error, Result, decode_blob};

/// Where a live record is: its part, its offset and size there, and its blob's codec
/// (`bad-header` where the header can't be read).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    pub part: u32,
    pub offset: u32,
    pub size: u32,
    pub codec: &'static str,
}

/// Every live record's place by key, the first part's winning, with the counts the archive
/// tool's `paths` reports.
#[derive(Debug, Default)]
pub struct Places {
    pub by_key: HashMap<[u8; 20], Place>,
    pub live: usize,
    pub repeated: usize,
    pub unreadable: usize,
}

/// Part `n` of the copy at `root`: `data/data000` and on.
pub fn part_path(root: &Path, n: u32) -> PathBuf {
    root.join("data").join(format!("data{n:03}"))
}

/// Every live record's place in the copy at `root`, part by part until one is missing.
pub fn live_places(root: &Path) -> Result<Places> {
    let mut p = Places::default();
    for n in 0..1000 {
        let path = part_path(root, n);
        if !path.exists() {
            break;
        }
        let named = |e: Error| Error::BadArchive(format!("{}: {e}", path.display()));
        let h = read_part_header(&path).map_err(named)?;
        let mut f = File::open(&path).map_err(|e| named(e.into()))?;
        for r in h.live() {
            p.live += 1;
            if p.by_key.contains_key(&r.key) {
                p.repeated += 1;
                continue;
            }
            let codec = match read_blob_header(&mut f, u64::from(r.offset)) {
                Ok(b) => b.codec.name(),
                Err(_) => {
                    p.unreadable += 1;
                    "bad-header"
                }
            };
            p.by_key.insert(
                r.key,
                Place {
                    part: n,
                    offset: r.offset,
                    size: r.size,
                    codec,
                },
            );
        }
    }
    Ok(p)
}

/// A live record's blob, read whole once it is known to lie inside its part.
pub fn read_record(root: &Path, p: &Place) -> Result<Vec<u8>> {
    let part = part_path(root, p.part);
    let mut f =
        File::open(&part).map_err(|e| Error::BadArchive(format!("{}: {e}", part.display())))?;
    let len = f.metadata()?.len();
    if u64::from(p.offset) + u64::from(p.size) > len {
        return Err(Error::BadArchive(format!(
            "the record at {:#x} ({} bytes) runs past the end of {} ({len} bytes)",
            p.offset,
            p.size,
            part.display()
        )));
    }
    f.seek(SeekFrom::Start(u64::from(p.offset)))?;
    let mut blob = vec![0; p.size as usize];
    f.read_exact(&mut blob)?;
    Ok(blob)
}

/// A copy of the game, its files read by path.
#[derive(Debug)]
pub struct GameCopy {
    root: PathBuf,
    index: PathMap,
    places: Places,
}

impl GameCopy {
    /// Opens the copy at `root`: decodes `data/_index`, and finds its parts' live records.
    pub fn open(root: &Path) -> Result<GameCopy> {
        let bytes = std::fs::read(root.join("data").join("_index"))?;
        let index = index::build(&decode_blob(&bytes, 0, &BuiltinCodecs)?);
        Ok(GameCopy {
            root: root.to_path_buf(),
            index,
            places: live_places(root)?,
        })
    }

    /// The file `_index` names at `path`, decoded: [`Error::NotFound`] if `_index` doesn't name
    /// it, or no live record holds its key.
    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        let missing = || Error::NotFound(path.to_string());
        let key = self.index.by_path.get(path).ok_or_else(missing)?;
        let place = self.places.by_key.get(key).ok_or_else(missing)?;
        let blob = read_record(&self.root, place)?;
        decode_blob(&blob, u64::from(place.offset), &BuiltinCodecs)
    }

    /// Every path `_index` names, in its order.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.index.by_path.keys().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use sha1::{Digest, Sha1};

    use super::*;
    use crate::test_util::{oodle_blob, stored_blob, zfs_part};

    fn key(bytes: &[u8]) -> [u8; 20] {
        Sha1::digest(bytes).into()
    }

    #[test]
    fn a_copy_reads_a_file_by_path_through_its_index() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        // Invented files: one stored, one Oodle-packed.
        let plain = b"an invented table\n".to_vec();
        let packed = b"an invented cache, Oodle-packed".to_vec();
        std::fs::write(
            data.join("data000"),
            zfs_part(&[vec![(1, key(&plain), stored_blob(&plain))]]),
        )
        .unwrap();
        std::fs::write(
            data.join("data001"),
            zfs_part(&[vec![(1, key(&packed), oodle_blob(&packed, 512 * 1024))]]),
        )
        .unwrap();
        let index = format!(
            "{}\tdata/tables/x.tab\n{}\tdata/caches/y.new_cache\n{}\tdata/gone.tab\n",
            crate::hex(&key(&plain)),
            crate::hex(&key(&packed)),
            crate::hex(&[7; 20])
        );
        std::fs::write(
            data.join("_index"),
            oodle_blob(index.as_bytes(), 512 * 1024),
        )
        .unwrap();

        let copy = GameCopy::open(root.path()).unwrap();
        assert_eq!(copy.read("data/tables/x.tab").unwrap(), plain);
        assert_eq!(copy.read("data/caches/y.new_cache").unwrap(), packed);
        assert!(
            matches!(copy.read("data/nowhere.tab"), Err(Error::NotFound(p)) if p == "data/nowhere.tab")
        );
        // _index names it, but no live record holds its key.
        assert!(matches!(
            copy.read("data/gone.tab"),
            Err(Error::NotFound(_))
        ));
        let mut paths: Vec<&str> = copy.paths().collect();
        paths.sort_unstable();
        assert_eq!(
            paths,
            [
                "data/caches/y.new_cache",
                "data/gone.tab",
                "data/tables/x.tab"
            ]
        );
    }
}
