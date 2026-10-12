//! Writing under re/ only (ground rule 3).
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// An output directory inside the repository's re/.
pub struct Out {
    root: PathBuf,
    used: HashMap<PathBuf, u32>,
}

impl Out {
    /// `requested` is resolved from the current directory and must lie inside re/.
    pub fn new(requested: &Path) -> Result<Self> {
        let repo = fenji_data::re_dir::find_repo_root(&std::env::current_dir()?)?;
        let root = fenji_data::re_dir::resolve_out_dir(&repo, requested)?;
        Ok(Self {
            root,
            used: HashMap::new(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where the file `path`, plus `suffix`, goes. `None` if the path could leave the output
    /// directory. A path met twice gets `~2`, `~3` … so nothing is overwritten.
    pub fn target(&mut self, path: &str, suffix: &str) -> Result<Option<PathBuf>> {
        let Some(rel) = fenji_data::re_dir::safe_relative(path) else {
            return Ok(None);
        };
        let mut name = rel.into_os_string();
        name.push(suffix);
        let mut target = self.root.join(name);
        let seen = self.used.entry(target.clone()).or_insert(0);
        *seen += 1;
        if *seen > 1 {
            let mut s = target.into_os_string();
            s.push(format!("~{seen}"));
            target = s.into();
        }
        if let Some(dir) = target.parent() {
            make_dirs_inside(&self.root, dir)?;
        }
        remove_existing(&target)?;
        Ok(Some(target))
    }

    /// Writes `bytes` to [`Out::target`]; returns where, or `None` for an unsafe path.
    pub fn write(&mut self, path: &str, suffix: &str, bytes: &[u8]) -> Result<Option<PathBuf>> {
        let Some(target) = self.target(path, suffix)? else {
            return Ok(None);
        };
        create_fresh(&target, bytes)?;
        Ok(Some(target))
    }

    /// Writes one of a command's own files (a summary or a failure list) at the top of the output
    /// directory, the same way as [`Out::write`]: never through a link already there.
    pub fn write_summary(&self, name: &str, bytes: &[u8]) -> Result<PathBuf> {
        let target = self.root.join(name);
        remove_existing(&target)?;
        create_fresh(&target, bytes)?;
        Ok(target)
    }
}

/// Removes whatever is at `path` (a symlink or a hard link included) so that a write replaces it
/// instead of writing through it.
fn remove_existing(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => {
            std::fs::remove_file(path).with_context(|| format!("replacing {}", path.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Creates `path` anew and writes `bytes`; fails if anything appeared there since it was removed.
fn create_fresh(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    std::fs::File::options()
        .write(true)
        .create_new(true)
        .open(path)
        .and_then(|mut f| f.write_all(bytes))
        .with_context(|| format!("writing {}", path.display()))
}

/// Creates `dir` (inside `root`, which exists) one component at a time, refusing to pass through a
/// link: a link planted inside the output directory could otherwise lead a write out of re/.
fn make_dirs_inside(root: &Path, dir: &Path) -> Result<()> {
    let rel = dir
        .strip_prefix(root)
        .context("an output path outside the output directory")?;
    let mut at = root.to_path_buf();
    for part in rel.components() {
        at.push(part);
        match std::fs::symlink_metadata(&at) {
            Ok(m) if m.file_type().is_symlink() => {
                bail!("{} is a link; luatool won't write through it", at.display())
            }
            Ok(m) if m.is_dir() => {}
            Ok(_) => bail!("{} is in the way: it isn't a directory", at.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => std::fs::create_dir(&at)?,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
