//! Opening the client: the exe's path, the pinned-build check, and decoding one entry.
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use fenji_data::{BuiltinCodecs, ExeEntry, ExeFs, PeImage};
use fenji_lua::Dialect;

/// `--exe`, else `$FENJI_CLIENT_PATH/bin64/gujianol.exe`.
pub fn exe_path(arg: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(p) = arg {
        return Ok(p);
    }
    let root = std::env::var_os("FENJI_CLIENT_PATH").context(
        "pass --exe, or set FENJI_CLIENT_PATH to the pinned client copy (docs/re/pinned-build.md)",
    )?;
    Ok(Path::new(&root).join("bin64").join("gujianol.exe"))
}

/// Reads and parses the exe. Refuses any build but the pinned one unless `any_build` is set.
pub fn load(path: &Path, any_build: bool) -> Result<PeImage> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let hash = fenji_data::sha256_hex(&bytes);
    if hash != fenji_data::PINNED_EXE_SHA256 && !any_build {
        bail!(
            "{} is not the pinned build: its SHA-256 is {hash}, the pinned one is {} \
             (docs/re/pinned-build.md); pass --any-build to read it anyway",
            path.display(),
            fenji_data::PINNED_EXE_SHA256
        );
    }
    Ok(PeImage::parse(bytes)?)
}

/// What decoding one embedded file gave.
pub enum Decoded {
    /// A Lua chunk, and its dialect (the client's, or stock).
    Lua(Vec<u8>, Dialect),
    /// Any other file: XML, TOC, config, C source …
    Other(Vec<u8>),
    /// The file doesn't decode. None does in the pinned build (docs/re/m1-script-format.md §5).
    Failed(String),
}

pub fn decode(fs: &ExeFs, e: &ExeEntry) -> Decoded {
    match fs.read(e, &BuiltinCodecs) {
        Ok(bytes) => match Dialect::detect(&bytes) {
            Some(dialect) => Decoded::Lua(bytes, dialect),
            None => Decoded::Other(bytes),
        },
        Err(err) => Decoded::Failed(err.to_string()),
    }
}

/// The files whose engine path (`data/<dir>/<path>`) starts with `prefix`.
pub fn selected<'f>(fs: &'f ExeFs<'_>, prefix: &'f str) -> impl Iterator<Item = &'f ExeEntry> {
    fs.entries()
        .iter()
        .filter(move |e| e.engine_path().starts_with(prefix))
}

/// The area an engine path belongs to, for per-area counts: its directories, at most four, such
/// as `data/scripts/combat/skill` or `data/interface/addons/Login`.
pub fn area(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    let dirs = &parts[..parts.len() - 1];
    if dirs.is_empty() {
        "(top level)".into()
    } else {
        dirs[..dirs.len().min(4)].join("/")
    }
}

#[cfg(test)]
mod tests {
    use super::area;

    #[test]
    fn areas_are_at_most_four_directories_deep() {
        assert_eq!(
            area("data/scripts/combat/skill/3007163.luac"),
            "data/scripts/combat/skill"
        );
        assert_eq!(
            area("data/scripts/combat/skill/sub/x.luac"),
            "data/scripts/combat/skill"
        );
        assert_eq!(area("data/locale/load.config"), "data/locale");
        assert_eq!(area("x.luac"), "(top level)");
    }
}
