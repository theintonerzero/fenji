//! The launcher's settings (M3 spec D14): the server address, the username and the client folder,
//! as JSON in the platform's config directory, found without the `dirs` crate (D20). No password
//! or session token is ever stored.
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub server: Option<String>,
    pub username: Option<String>,
    pub client: Option<PathBuf>,
}

const FILE: &str = "settings.json";

impl Settings {
    /// The saved settings, or the defaults when none were saved.
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        match std::fs::read(dir.join(FILE)) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// Writes through a temporary file and a rename, so a crash never leaves half a file.
    pub fn save(&self, dir: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join(format!("{FILE}.tmp"));
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(&serde_json::to_vec_pretty(self)?)?;
            // On disk before the rename, so a power cut can't leave an empty file in its place.
            f.sync_all()?;
        }
        std::fs::rename(&tmp, dir.join(FILE))?;
        Ok(())
    }
}

/// Which platform's convention `config_dir_for` follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    MacOs,
    Windows,
}

/// The launcher's config directory on this machine.
pub fn config_dir() -> Option<PathBuf> {
    let os = if cfg!(windows) {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::MacOs
    } else {
        Os::Linux
    };
    config_dir_for(os, |k| std::env::var_os(k).map(PathBuf::from))
}

/// The launcher's data directory on this machine (its Wine prefix lives there on Linux).
pub fn data_dir() -> Option<PathBuf> {
    let os = if cfg!(windows) {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::MacOs
    } else {
        Os::Linux
    };
    data_dir_for(os, |k| std::env::var_os(k).map(PathBuf::from))
}

/// `$XDG_DATA_HOME/fenji-launcher` or `$HOME/.local/share/fenji-launcher` (Linux), the config
/// directory elsewhere. Relative or empty variables are ignored.
pub fn data_dir_for(os: Os, var: impl Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    match os {
        Os::Linux => {
            let abs = |k: &str| var(k).filter(|p| p.is_absolute() || p.has_root());
            let base =
                abs("XDG_DATA_HOME").or_else(|| abs("HOME").map(|h| h.join(".local/share")))?;
            Some(base.join("fenji-launcher"))
        }
        other => config_dir_for(other, var),
    }
}

/// `$XDG_CONFIG_HOME/fenji-launcher` or `$HOME/.config/fenji-launcher` (Linux),
/// `$HOME/Library/Application Support/fenji-launcher` (macOS), `%APPDATA%\fenji-launcher`
/// (Windows). Relative or empty variables are ignored.
pub fn config_dir_for(os: Os, var: impl Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    let abs = |k: &str| var(k).filter(|p| p.is_absolute() || p.has_root());
    let base = match os {
        Os::Linux => abs("XDG_CONFIG_HOME").or_else(|| abs("HOME").map(|h| h.join(".config")))?,
        Os::MacOs => abs("HOME")?.join("Library").join("Application Support"),
        Os::Windows => abs("APPDATA")?,
    };
    Some(base.join("fenji-launcher"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<PathBuf> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k| {
            pairs
                .iter()
                .find(|(kk, _)| kk == k)
                .map(|(_, v)| PathBuf::from(v))
        }
    }

    #[test]
    fn each_platform_has_its_place() {
        let linux = config_dir_for(Os::Linux, vars(&[("HOME", "/home/p")]));
        assert_eq!(linux, Some(PathBuf::from("/home/p/.config/fenji-launcher")));
        let xdg = config_dir_for(
            Os::Linux,
            vars(&[("HOME", "/home/p"), ("XDG_CONFIG_HOME", "/cfg")]),
        );
        assert_eq!(xdg, Some(PathBuf::from("/cfg/fenji-launcher")));
        let relative_xdg = config_dir_for(
            Os::Linux,
            vars(&[("HOME", "/home/p"), ("XDG_CONFIG_HOME", "cfg")]),
        );
        assert_eq!(
            relative_xdg,
            Some(PathBuf::from("/home/p/.config/fenji-launcher"))
        );
        let mac = config_dir_for(Os::MacOs, vars(&[("HOME", "/Users/p")]));
        assert_eq!(
            mac,
            Some(PathBuf::from(
                "/Users/p/Library/Application Support/fenji-launcher"
            ))
        );
        let win = config_dir_for(Os::Windows, vars(&[("APPDATA", "/appdata")]));
        assert_eq!(win, Some(PathBuf::from("/appdata/fenji-launcher")));
        assert_eq!(config_dir_for(Os::Linux, vars(&[])), None);
    }

    #[test]
    fn the_data_folder_follows_xdg_on_linux() {
        let d = data_dir_for(Os::Linux, vars(&[("HOME", "/home/p")]));
        assert_eq!(
            d,
            Some(PathBuf::from("/home/p/.local/share/fenji-launcher"))
        );
        let x = data_dir_for(
            Os::Linux,
            vars(&[("HOME", "/home/p"), ("XDG_DATA_HOME", "/d")]),
        );
        assert_eq!(x, Some(PathBuf::from("/d/fenji-launcher")));
        let w = data_dir_for(Os::Windows, vars(&[("APPDATA", "/appdata")]));
        assert_eq!(w, Some(PathBuf::from("/appdata/fenji-launcher")));
    }

    #[test]
    fn settings_round_trip_and_default_when_absent() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("fenji-launcher");
        assert_eq!(Settings::load(&dir).unwrap(), Settings::default());
        let s = Settings {
            server: Some("https://play.example.net".into()),
            username: Some("tester".into()),
            client: Some("/games/solo".into()),
        };
        s.save(&dir).unwrap();
        assert_eq!(Settings::load(&dir).unwrap(), s);
        let text = std::fs::read_to_string(dir.join(FILE)).unwrap();
        assert!(!text.contains("password") && !text.contains("token"));
    }
}
