//! Writing under re/ only (ground rule 3).
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

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
            std::fs::create_dir_all(dir)?;
        }
        Ok(Some(target))
    }

    /// Writes `bytes` to [`Out::target`]; returns where, or `None` for an unsafe path.
    pub fn write(&mut self, path: &str, suffix: &str, bytes: &[u8]) -> Result<Option<PathBuf>> {
        let Some(target) = self.target(path, suffix)? else {
            return Ok(None);
        };
        std::fs::write(&target, bytes).with_context(|| format!("writing {}", target.display()))?;
        Ok(Some(target))
    }
}
