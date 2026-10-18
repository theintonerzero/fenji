//! The early check's loader (retranslation spec §6.5, step 4; RT16), for a sandboxed run only
//! (rule 4). It loads new English into a run workspace's copy of the game the way the engine
//! reads its files (docs/re/retranslation-locale.md §4):
//! - a patch part holding the rebuilt locale cache, which the engine loads after the data parts;
//! - one more line at the end of `_index`, which points the cache's path at that part's record.
//!
//! The copy's own `_index` waits in the work folder until [`revert`] puts it back. Only a copy
//! inside `re/` is ever touched, never the pinned copy ([`run_copy`]).
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use fenji_data::archive::write_part;
use fenji_data::client::GameCopy;
use fenji_data::{BlobHeader, BuiltinCodecs, decode_blob, stored_blob};
use fenji_pack::Fingerprint;
use sha1::{Digest, Sha1};

use crate::extract::LOCALE_CACHE;
use crate::rebuild::rebuild;

/// The patch part the loader adds to the run's copy.
pub const PATCH_PART: &str = "patch/fenji-early";

/// Where the copy's own `_index` waits, in the work folder, while a load is in.
pub const BACKUP: &str = "early/_index.original";

/// What a load put in the copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// The new cache's key: its SHA-1.
    pub key: [u8; 20],
    /// The lines given new English.
    pub replaced: usize,
    /// The lines added to the main table.
    pub added: usize,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Writes `bytes` to a temporary file beside `path`, then renames it into place.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("fenji-tmp");
    std::fs::write(&tmp, bytes).with_context(|| tmp.display().to_string())?;
    std::fs::rename(&tmp, path).with_context(|| path.display().to_string())?;
    Ok(())
}

/// Loads `english` (lines replaced by fingerprint) and `added` (lines put in the main table)
/// into `copy`'s locale cache: [`PATCH_PART`] holds the rebuilt cache as a stored record keyed by
/// its SHA-1, and `data/_index` gets one more line, which re-points the cache's path at it, as a
/// stored blob. Refused while an earlier load is still in.
pub fn apply(
    copy: &Path,
    work: &Path,
    english: &HashMap<Fingerprint, String>,
    added: &[(String, String)],
) -> Result<Applied> {
    let backup = work.join(BACKUP);
    if backup.exists() {
        bail!(
            "{}: a load is already in; revert it first",
            backup.display()
        );
    }
    let index_path = copy.join("data").join("_index");
    let index_blob =
        std::fs::read(&index_path).with_context(|| index_path.display().to_string())?;
    let cache = GameCopy::open(copy)
        .with_context(|| copy.display().to_string())?
        .read(LOCALE_CACHE)
        .context(LOCALE_CACHE)?;
    let rebuilt = rebuild(&cache, english, added)?;
    let key: [u8; 20] = Sha1::digest(&rebuilt.bytes).into();

    let header = BlobHeader::parse(&index_blob, 0)?;
    let mut lines = decode_blob(&index_blob, 0, &BuiltinCodecs)?;
    if !lines.is_empty() && !lines.ends_with(b"\n") {
        lines.push(b'\n');
    }
    lines.extend_from_slice(format!("{}\t{LOCALE_CACHE}\n", hex(&key)).as_bytes());
    let new_index = stored_blob(&lines, header.timestamp);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let part = write_part(&[(key, stored_blob(&rebuilt.bytes, now))])?;

    // The copy's own index goes aside first, so a failure later leaves it there to put back.
    std::fs::create_dir_all(backup.parent().expect("a folder"))?;
    std::fs::write(&backup, &index_blob).with_context(|| backup.display().to_string())?;
    std::fs::create_dir_all(copy.join("patch"))?;
    write_atomic(&copy.join(PATCH_PART), &part)?;
    write_atomic(&index_path, &new_index)?;
    Ok(Applied {
        key,
        replaced: rebuilt.replaced,
        added: rebuilt.added,
    })
}

/// Puts `copy`'s own `_index` back and takes [`PATCH_PART`] out, and the `patch` folder if
/// that leaves it empty. Whether there was anything to put back.
pub fn revert(copy: &Path, work: &Path) -> Result<bool> {
    let backup = work.join(BACKUP);
    let part = copy.join(PATCH_PART);
    let mut any = false;
    if backup.exists() {
        let original = std::fs::read(&backup).with_context(|| backup.display().to_string())?;
        write_atomic(&copy.join("data").join("_index"), &original)?;
        std::fs::remove_file(&backup)?;
        any = true;
    }
    if part.exists() {
        std::fs::remove_file(&part)?;
        // Only if empty: the copy's own patch folder, if it has one, stays.
        let _ = std::fs::remove_dir(copy.join("patch"));
        any = true;
    }
    Ok(any)
}

/// `copy`, made absolute, if the loader may touch it: inside `repo`'s `re/`, where the run
/// workspace keeps its copy, and neither inside nor around `pinned`, the pinned copy.
pub fn run_copy(repo: &Path, copy: &Path, pinned: Option<&Path>) -> Result<PathBuf> {
    let copy = copy
        .canonicalize()
        .with_context(|| format!("{}: no copy there", copy.display()))?;
    let re = repo.join("re").canonicalize()?;
    if copy == re || !copy.starts_with(&re) {
        bail!(
            "{} isn't inside {}: the loader touches only a run workspace's copy",
            copy.display(),
            re.display()
        );
    }
    if let Some(pinned) = pinned.and_then(|p| p.canonicalize().ok())
        && (copy.starts_with(&pinned) || pinned.starts_with(&copy))
    {
        bail!(
            "{} is the pinned copy, which the loader never touches",
            copy.display()
        );
    }
    Ok(copy)
}
