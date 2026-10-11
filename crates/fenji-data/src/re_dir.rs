//! Where tools may write: only inside the repository's git-ignored `re/` (ground rule 3).
use std::path::{Component, Path, PathBuf};

use crate::{Error, Result};

/// The repository root: the nearest ancestor of `start` holding `.git`, which must have a `re/`
/// directory.
pub fn find_repo_root(start: &Path) -> Result<PathBuf> {
    let start = start.canonicalize()?;
    // The nearest checkout only: a clone or worktree without re/ must not resolve to an outer one.
    let root = start
        .ancestors()
        .find(|d| d.join(".git").exists())
        .ok_or_else(|| {
            Error::OutDir(format!("{} is not inside a git checkout", start.display()))
        })?;
    if !root.join("re").is_dir() {
        return Err(Error::OutDir(format!(
            "{} has no re/ directory (create it: it is git-ignored)",
            root.display()
        )));
    }
    Ok(root.to_path_buf())
}

/// Resolves `requested` (absolute, or relative to the current directory) to a directory strictly
/// inside `<repo_root>/re/`, and creates it. Refuses `..`, `re/` itself, and anything that
/// resolves outside `re/`, including through a symlink. If `re` is itself a symlink (to other
/// storage, say), its target is the boundary.
pub fn resolve_out_dir(repo_root: &Path, requested: &Path) -> Result<PathBuf> {
    let re = repo_root.join("re").canonicalize()?;
    let abs = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        std::env::current_dir()?.join(requested)
    };
    if abs.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(Error::OutDir(format!(
            "{} contains '..'",
            requested.display()
        )));
    }
    let no_ancestor = || Error::OutDir(format!("{} has no existing ancestor", abs.display()));
    // Canonicalise the longest existing ancestor, then add back the parts that don't exist yet.
    let (mut existing, mut rest) = (abs.as_path(), Vec::new());
    while !existing.exists() {
        rest.push(existing.file_name().ok_or_else(no_ancestor)?.to_owned());
        existing = existing.parent().ok_or_else(no_ancestor)?;
    }
    let mut out = existing.canonicalize()?;
    out.extend(rest.iter().rev());
    if out == re || !out.starts_with(&re) {
        return Err(Error::OutDir(format!(
            "{} is not inside {}",
            out.display(),
            re.display()
        )));
    }
    std::fs::create_dir_all(&out)?;
    Ok(out)
}

/// A registry path as a relative path made of plain components only: no empty part, `.`, `..`,
/// `.git`, backslash, drive colon or control character. On its own it can't leave the directory it
/// is joined to; links already inside that directory are the writer's to check.
pub fn safe_relative(path: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in path.split('/') {
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.eq_ignore_ascii_case(".git")
            || part.contains(['\\', ':'])
            || part.chars().any(char::is_control)
        {
            return None;
        }
        out.push(part);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir(t.path().join(".git")).unwrap();
        std::fs::create_dir(t.path().join("re")).unwrap();
        t
    }

    #[test]
    fn out_dir_inside_re_is_created() {
        let t = repo();
        let out = resolve_out_dir(t.path(), &t.path().join("re/m1/disasm")).unwrap();
        assert!(out.is_dir());
        assert!(out.starts_with(t.path().join("re").canonicalize().unwrap()));
    }

    #[test]
    fn out_dir_outside_re_is_refused() {
        let t = repo();
        assert!(resolve_out_dir(t.path(), &t.path().join("docs")).is_err());
        assert!(!t.path().join("docs").exists());
        assert!(resolve_out_dir(t.path(), &t.path().join("re")).is_err()); // a subdirectory, please
    }

    #[test]
    fn out_dir_with_dotdot_is_refused() {
        let t = repo();
        assert!(resolve_out_dir(t.path(), &t.path().join("re/../docs")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn out_dir_through_a_symlink_leaving_re_is_refused() {
        let t = repo();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), t.path().join("re/escape")).unwrap();
        assert!(resolve_out_dir(t.path(), &t.path().join("re/escape/x")).is_err());
        assert!(!outside.path().join("x").exists());
    }

    #[test]
    fn out_dir_without_re_is_refused() {
        let t = tempfile::tempdir().unwrap();
        assert!(resolve_out_dir(t.path(), &t.path().join("re/x")).is_err());
    }

    #[test]
    fn find_repo_root_walks_up() {
        let t = repo();
        std::fs::create_dir_all(t.path().join("a/b")).unwrap();
        assert_eq!(
            find_repo_root(&t.path().join("a/b")).unwrap(),
            t.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn find_repo_root_stops_at_the_nearest_checkout() {
        // A checkout without re/ (a fresh clone, a worktree) inside another that has one must not
        // resolve to the outer one.
        let outer = repo();
        let inner = outer.path().join("inner");
        std::fs::create_dir_all(inner.join("sub")).unwrap();
        std::fs::write(inner.join(".git"), "gitdir: elsewhere\n").unwrap(); // as in a worktree
        assert!(find_repo_root(&inner.join("sub")).is_err());
    }

    #[test]
    fn safe_relative_refuses_git_directories() {
        for bad in [".git/config", "a/.GIT/hooks/post-checkout", ".Git"] {
            assert_eq!(safe_relative(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn safe_relative_accepts_plain_paths() {
        assert_eq!(
            safe_relative("addons/Login/Login.luac"),
            Some(PathBuf::from("addons/Login/Login.luac"))
        );
    }

    #[test]
    fn safe_relative_refuses_escapes() {
        for bad in [
            "",
            "/etc/passwd",
            "a/../b",
            "..",
            "a//b",
            "a\\b",
            "C:/x",
            "a/./b",
            "a\nb",
        ] {
            assert_eq!(safe_relative(bad), None, "{bad:?}");
        }
    }
}
