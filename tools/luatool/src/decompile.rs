//! Running unluac (MIT; an external tool, never linked; docs/re/sources.md) on normalised chunks.
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

/// How one decompilation went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    Failed,
    TimedOut,
}

/// The java binary, the pinned unluac jar, and a per-chunk time limit.
pub struct Unluac {
    java: PathBuf,
    jar: PathBuf,
    timeout: Duration,
}

impl Unluac {
    /// `java`: `--java`, else `$JAVA_HOME/bin/java`, else `java` on PATH. `jar`: `--jar`, else the
    /// pinned jar that scripts/bootstrap.sh installs (`UNLUAC_VERSION` in scripts/versions.env).
    pub fn locate(
        java: Option<PathBuf>,
        jar: Option<PathBuf>,
        repo: &Path,
        timeout: Duration,
    ) -> Result<Self> {
        let java = java
            .or_else(|| {
                std::env::var_os("JAVA_HOME").map(|h| Path::new(&h).join("bin").join("java"))
            })
            .unwrap_or_else(|| PathBuf::from("java"));
        let jar = match jar {
            Some(j) => j,
            None => pinned_jar(
                repo,
                Path::new(&std::env::var_os("HOME").context("HOME is not set")?),
            )?,
        };
        if !jar.is_file() {
            bail!(
                "unluac jar not found at {} (run scripts/bootstrap.sh, or pass --jar)",
                jar.display()
            );
        }
        Ok(Self { java, jar, timeout })
    }

    /// Decompiles `input` into `output`. On failure, unluac's stderr is kept in `<output>.err`.
    pub fn run(&self, input: &Path, output: &Path) -> Result<Outcome> {
        let err_path = with_suffix(output, ".err");
        let mut child = Command::new(&self.java)
            .arg("-jar")
            .arg(&self.jar)
            .arg(input)
            .stdin(Stdio::null())
            .stdout(std::fs::File::create(output)?)
            .stderr(std::fs::File::create(&err_path)?)
            .spawn()
            .with_context(|| format!("starting {}", self.java.display()))?;
        let start = Instant::now();
        loop {
            if let Some(status) = child.try_wait()? {
                if status.success() {
                    std::fs::remove_file(&err_path).ok();
                    return Ok(Outcome::Ok);
                }
                return Ok(Outcome::Failed);
            }
            if start.elapsed() > self.timeout {
                child.kill().ok();
                child.wait().ok();
                return Ok(Outcome::TimedOut);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Runs every (input, output) pair on `workers` threads. Outcomes come back in input order.
    pub fn run_all(&self, jobs: &[(PathBuf, PathBuf)], workers: usize) -> Vec<Outcome> {
        let next = AtomicUsize::new(0);
        let results = Mutex::new(vec![Outcome::Failed; jobs.len()]);
        std::thread::scope(|s| {
            for _ in 0..workers.max(1) {
                s.spawn(|| {
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some((input, output)) = jobs.get(i) else {
                            break;
                        };
                        let outcome = self.run(input, output).unwrap_or(Outcome::Failed);
                        results
                            .lock()
                            .expect("no worker panics while holding the lock")[i] = outcome;
                    }
                });
            }
        });
        results.into_inner().expect("all workers finished")
    }
}

fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(suffix);
    s.into()
}

/// `<home>/.local/share/fenji/tools/unluac/unluac_<UNLUAC_VERSION>.jar`, refused unless its
/// SHA-256 is `UNLUAC_SHA256` (scripts/versions.env): java runs it, so a jar that doesn't match the
/// pin never runs.
fn pinned_jar(repo: &Path, home: &Path) -> Result<PathBuf> {
    let env = std::fs::read_to_string(repo.join("scripts").join("versions.env"))
        .context("reading scripts/versions.env")?;
    let version = pin(&env, "UNLUAC_VERSION")?;
    let sha = pin(&env, "UNLUAC_SHA256")?;
    let jar = home.join(format!(
        ".local/share/fenji/tools/unluac/unluac_{version}.jar"
    ));
    let bytes = std::fs::read(&jar).with_context(|| {
        format!(
            "unluac jar not found at {} (run scripts/bootstrap.sh, or pass --jar)",
            jar.display()
        )
    })?;
    let got = fenji_data::sha256_hex(&bytes);
    if got != sha {
        bail!(
            "the unluac jar at {} has SHA-256 {got}, not the pinned {sha}; delete it and re-run scripts/bootstrap.sh",
            jar.display()
        );
    }
    Ok(jar)
}

/// The value of `NAME="value"` in versions.env, without quotes or a trailing comment.
fn pin(env: &str, name: &str) -> Result<String> {
    env.lines()
        .find_map(|l| l.strip_prefix(name)?.strip_prefix('='))
        .map(|v| {
            v.split('#')
                .next()
                .unwrap_or_default()
                .trim()
                .trim_matches('"')
                .to_string()
        })
        .filter(|v| !v.is_empty())
        .with_context(|| format!("{name} is missing from scripts/versions.env"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A checkout whose versions.env pins `b"the real jar"`, and a home holding `jar` at the pinned
    /// path.
    fn fixture(jar: &[u8]) -> (tempfile::TempDir, tempfile::TempDir) {
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir(repo.path().join("scripts")).unwrap();
        let sha = fenji_data::sha256_hex(b"the real jar");
        let env =
            format!("# pins\nUNLUAC_VERSION=\"1_2\"   # a comment\nUNLUAC_SHA256=\"{sha}\"\n");
        std::fs::write(repo.path().join("scripts/versions.env"), env).unwrap();
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join(".local/share/fenji/tools/unluac");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("unluac_1_2.jar"), jar).unwrap();
        (repo, home)
    }

    #[test]
    fn the_pinned_jar_runs_only_if_its_checksum_matches() {
        let (repo, home) = fixture(b"the real jar");
        let jar = pinned_jar(repo.path(), home.path()).unwrap();
        assert!(jar.ends_with("unluac_1_2.jar"));
        let (repo, home) = fixture(b"a tampered jar");
        let err = pinned_jar(repo.path(), home.path())
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("SHA-256") && err.contains("bootstrap"),
            "{err}"
        );
    }
}
