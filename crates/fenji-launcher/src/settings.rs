//! The launcher's settings (M3 spec D14; launcher spec §3.11): the servers, the active one, the
//! client folder and the launcher's own options, as JSON in the platform's config directory, found
//! without the `dirs` crate (D20). No password or session token is ever stored.
use std::path::{Path, PathBuf};

use fenji_console::text::escape_hidden;
use serde::{Deserialize, Serialize};

use crate::pin::{Pin, ServerAddr};
use crate::servers::SavedServer;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub servers: Vec<SavedServer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<u32>,
    /// The id the next server gets; ids are never reused.
    #[serde(default)]
    pub next_id: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<PathBuf>,
    #[serde(default = "yes")]
    pub auto_update: bool,
    /// GameMode while the game runs, where it's installed (launcher spec Amendment B, LB2).
    #[serde(default = "yes")]
    pub gamemode: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub umu_run: Option<PathBuf>,
    /// Where Start download saves the game, when the player changed it ("Change" beside the
    /// folder); `~/Games/SoLO` otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_folder: Option<PathBuf>,
    /// The launcher version whose update dialog was shown (launcher spec §3.8: once a version).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_seen: Option<String>,
    // M3's single server, and hosting Task 5's pin for it: read from an older file, moved into
    // `servers` by `load`, and never written again.
    #[serde(default, skip_serializing)]
    server: Option<String>,
    #[serde(default, skip_serializing)]
    username: Option<String>,
    #[serde(default, skip_serializing)]
    pin: Option<String>,
}

fn yes() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            servers: Vec::new(),
            active: None,
            next_id: 1,
            client: None,
            auto_update: true,
            gamemode: true,
            umu_run: None,
            download_folder: None,
            update_seen: None,
            server: None,
            username: None,
            pin: None,
        }
    }
}

/// What adding a server did to the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddOutcome {
    /// A new server, with the address's pin if it had one.
    New(u32),
    /// A server already in the list; its key is unchanged.
    Known(u32),
    /// A server already in the list whose saved key isn't the one the link offers. Nothing changed:
    /// [`Settings::set_pin`] replaces it once the player confirms.
    KeyDiffers {
        id: u32,
        saved: Option<String>,
        offered: String,
    },
}

impl AddOutcome {
    /// The server's id, whatever happened.
    pub fn id(&self) -> u32 {
        match *self {
            AddOutcome::New(id) | AddOutcome::Known(id) | AddOutcome::KeyDiffers { id, .. } => id,
        }
    }
}

const FILE: &str = "settings.json";

impl Settings {
    /// The saved settings, or the defaults when none were saved. An older file's single server
    /// becomes the list's first, and the active one.
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let mut s: Settings = match std::fs::read(dir.join(FILE)) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        s.next_id = s
            .next_id
            .max(1)
            .max(s.servers.iter().map(|v| v.id).max().map_or(1, |m| m + 1));
        if let Some(origin) = s.server.take()
            && s.servers.is_empty()
        {
            let id = s.next_id;
            s.next_id += 1;
            s.servers.push(SavedServer {
                id,
                origin,
                pin: s.pin.take(),
                name: None,
                username: s.username.take(),
                last_seen_ms: None,
            });
            s.active = Some(id);
        }
        (s.username, s.pin) = (None, None);
        // A name a launcher before 0.5.1 cached as the server sent it, hidden characters and all,
        // shows as the server's answers now do (#367); one already escaped stays as it is.
        for v in &mut s.servers {
            if let Some(name) = &mut v.name {
                *name = escape_hidden(name);
            }
        }
        Ok(s)
    }

    /// [`Settings::load`], or, when the file doesn't parse, the defaults and a notice: the bad file
    /// is renamed `settings.json.bad`, so the player loses nothing and the launcher still starts.
    pub fn load_or_reset(dir: &Path) -> (Settings, Option<String>) {
        match Self::load(dir) {
            Ok(s) => (s, None),
            Err(_) => {
                let moved = std::fs::rename(dir.join(FILE), dir.join(format!("{FILE}.bad")));
                let notice = match moved {
                    Ok(()) => format!(
                        "Fenji couldn't read its settings, so it started fresh. The old file is \
                         settings.json.bad in {}.",
                        dir.display()
                    ),
                    Err(_) => format!(
                        "Fenji couldn't read its settings in {}, so it started fresh.",
                        dir.display()
                    ),
                };
                (Self::default(), Some(notice))
            }
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

    /// Adds a server, or finds it by its origin. A saved key never changes here: a link whose key
    /// differs from the saved one (or that brings one to a server saved without) changes nothing
    /// and says so, since a pasted link could come from anyone; the player confirms first, and
    /// [`Settings::set_pin`] makes the change. An address without a pin keeps the saved one.
    pub fn add_server(&mut self, addr: &ServerAddr) -> AddOutcome {
        if let Some(found) = self.servers.iter().find(|s| s.origin == addr.origin) {
            let offered = addr.pin.map(|p| p.to_string());
            return match offered {
                Some(offered) if found.pin.as_ref() != Some(&offered) => AddOutcome::KeyDiffers {
                    id: found.id,
                    saved: found.pin.clone(),
                    offered,
                },
                _ => AddOutcome::Known(found.id),
            };
        }
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        self.servers.push(SavedServer {
            id,
            origin: addr.origin.clone(),
            pin: addr.pin.map(|p| p.to_string()),
            name: None,
            username: None,
            last_seen_ms: None,
        });
        AddOutcome::New(id)
    }

    /// Sets a server's key, once the player has confirmed the change.
    pub fn set_pin(&mut self, id: u32, pin: Option<Pin>) -> bool {
        match self.server_mut(id) {
            Some(server) => {
                server.pin = pin.map(|p| p.to_string());
                true
            }
            None => false,
        }
    }

    /// Forgets a server. The active server falls back to the first one left, or none.
    pub fn remove_server(&mut self, id: u32) -> bool {
        let before = self.servers.len();
        self.servers.retain(|s| s.id != id);
        if self.servers.len() == before {
            return false;
        }
        if self.active == Some(id) {
            self.active = self.servers.first().map(|s| s.id);
        }
        true
    }

    pub fn server(&self, id: u32) -> Option<&SavedServer> {
        self.servers.iter().find(|s| s.id == id)
    }

    pub fn server_mut(&mut self, id: u32) -> Option<&mut SavedServer> {
        self.servers.iter_mut().find(|s| s.id == id)
    }

    pub fn active_server(&self) -> Option<&SavedServer> {
        self.server(self.active?)
    }

    pub fn set_active(&mut self, id: u32) -> bool {
        let known = self.server(id).is_some();
        if known {
            self.active = Some(id);
        }
        known
    }
}

/// Which platform's convention `config_dir_for` follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    MacOs,
    Windows,
    /// Any other system; it keeps Linux's folder conventions.
    Other,
}

impl Os {
    /// The system this build runs on.
    pub fn this() -> Os {
        if cfg!(target_os = "linux") {
            Os::Linux
        } else if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Other
        }
    }
}

/// The launcher's config directory on this machine.
pub fn config_dir() -> Option<PathBuf> {
    config_dir_for(Os::this(), |k| std::env::var_os(k).map(PathBuf::from))
}

/// The launcher's data directory on this machine (its Wine prefix lives there on Linux).
pub fn data_dir() -> Option<PathBuf> {
    data_dir_for(Os::this(), |k| std::env::var_os(k).map(PathBuf::from))
}

/// `$XDG_DATA_HOME/fenji-launcher` or `$HOME/.local/share/fenji-launcher` (Linux), the config
/// directory elsewhere. Relative or empty variables are ignored.
pub fn data_dir_for(os: Os, var: impl Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    match os {
        Os::Linux | Os::Other => {
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
        Os::Linux | Os::Other => {
            abs("XDG_CONFIG_HOME").or_else(|| abs("HOME").map(|h| h.join(".config")))?
        }
        Os::MacOs => abs("HOME")?.join("Library").join("Application Support"),
        Os::Windows => abs("APPDATA")?,
    };
    Some(base.join("fenji-launcher"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pin::ServerAddr;

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
        let other = data_dir_for(Os::Other, vars(&[("HOME", "/home/p")]));
        assert_eq!(
            other,
            Some(PathBuf::from("/home/p/.local/share/fenji-launcher"))
        );
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
        let mut s = Settings {
            client: Some("/games/solo".into()),
            ..Settings::default()
        };
        let id = s
            .add_server(
                &ServerAddr::parse(&format!("https://play.example.net/#pin={PIN}")).unwrap(),
            )
            .id();
        s.server_mut(id).unwrap().username = Some("tester".into());
        s.save(&dir).unwrap();
        assert_eq!(Settings::load(&dir).unwrap(), s);
        let text = std::fs::read_to_string(dir.join(FILE)).unwrap();
        assert!(!text.contains("password") && !text.contains("token"));
    }

    const PIN: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    #[test]
    fn gamemode_is_on_unless_the_player_turned_it_off() {
        // Launcher spec Amendment B, LB2: on by default, in a file saved before it too.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), r#"{"next_id": 1}"#).unwrap();
        assert!(Settings::load(dir.path()).unwrap().gamemode);
        assert!(Settings::default().gamemode);
        let off = Settings {
            gamemode: false,
            ..Settings::default()
        };
        off.save(dir.path()).unwrap();
        assert!(!Settings::load(dir.path()).unwrap().gamemode);
    }

    #[test]
    fn an_m3_settings_file_becomes_the_lists_first_server() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            format!(
                r#"{{"server":"https://203.0.113.5:7801","username":"lin_yue","client":"/games/solo","pin":"{PIN}"}}"#
            ),
        )
        .unwrap();
        let s = Settings::load(dir.path()).unwrap();
        assert_eq!(s.servers.len(), 1);
        let first = &s.servers[0];
        assert_eq!(
            (
                first.origin.as_str(),
                first.username.as_deref(),
                first.pin.as_deref()
            ),
            ("https://203.0.113.5:7801", Some("lin_yue"), Some(PIN))
        );
        assert_eq!(s.active, Some(first.id));
        assert_eq!(s.client.as_deref(), Some(Path::new("/games/solo")));
        assert!(s.auto_update);
        s.save(dir.path()).unwrap();
        let text = std::fs::read_to_string(dir.path().join("settings.json")).unwrap();
        assert!(
            !text.contains("\"server\""),
            "the old field goes once saved: {text}"
        );
        assert_eq!(Settings::load(dir.path()).unwrap(), s);
    }

    /// A link never changes a saved key on its own (a pasted link could come from anyone): the
    /// player confirms first, and `set_pin` does it.
    #[test]
    fn a_link_never_changes_a_saved_key_without_the_player_saying_so() {
        let mut s = Settings::default();
        let plain = ServerAddr::parse("https://203.0.113.5:7801").unwrap();
        let AddOutcome::New(id) = s.add_server(&plain) else {
            panic!("a new server")
        };
        assert_eq!(s.add_server(&plain), AddOutcome::Known(id));
        let pinned = ServerAddr::parse(&format!("https://203.0.113.5:7801/#pin={PIN}")).unwrap();
        assert_eq!(
            s.add_server(&pinned),
            AddOutcome::KeyDiffers {
                id,
                saved: None,
                offered: PIN.into()
            }
        );
        assert_eq!(s.server(id).unwrap().pin, None, "nothing changed yet");
        assert!(s.set_pin(id, pinned.pin));
        assert_eq!(s.server(id).unwrap().pin.as_deref(), Some(PIN));
        assert_eq!(s.add_server(&pinned), AddOutcome::Known(id));
        assert_eq!(
            s.add_server(&plain),
            AddOutcome::Known(id),
            "an address without a pin keeps the saved one"
        );
        let other = format!("{}A", "B".repeat(42));
        let changed = ServerAddr::parse(&format!("https://203.0.113.5:7801/#pin={other}")).unwrap();
        assert_eq!(
            s.add_server(&changed),
            AddOutcome::KeyDiffers {
                id,
                saved: Some(PIN.into()),
                offered: other
            }
        );
        assert_eq!(
            s.server(id).unwrap().pin.as_deref(),
            Some(PIN),
            "still the saved key"
        );
        assert_eq!(s.servers.len(), 1);
    }

    #[test]
    fn ids_are_never_reused_and_removing_the_active_server_moves_it_on() {
        let mut s = Settings::default();
        let a = ServerAddr::parse("https://203.0.113.5:7801").unwrap();
        let id = s.add_server(&a).id();
        let other = s
            .add_server(&ServerAddr::parse("https://play.example.net").unwrap())
            .id();
        assert_ne!(other, id);
        assert!(s.set_active(other));
        assert!(s.remove_server(other));
        assert_eq!(
            s.active,
            Some(id),
            "the active server falls back to the first left"
        );
        assert!(s.remove_server(id));
        assert_eq!((s.active, s.servers.len()), (None, 0));
        assert!(!s.remove_server(id));
        assert!(!s.set_active(id) && !s.set_pin(id, None));
        assert!(s.add_server(&a).id() > other, "ids are never reused");
    }

    #[test]
    fn a_server_name_an_older_launcher_cached_is_escaped_when_it_loads() {
        // #367: 0.5.0 cached a server's name as the server sent it, and a later launcher showed it
        // as cached until the server answered again.
        let dir = tempfile::tempdir().unwrap();
        let json = serde_json::json!({
            "servers": [
                { "id": 1, "origin": "https://moon.example", "name": "Moon\u{202e}ward\u{0007}" },
                { "id": 2, "origin": "https://jade.example", "name": "Jade Hall" },
            ],
            "next_id": 3,
        });
        std::fs::write(dir.path().join("settings.json"), json.to_string()).unwrap();
        let s = Settings::load(dir.path()).unwrap();
        let names: Vec<_> = s.servers.iter().map(|v| v.name.clone().unwrap()).collect();
        assert_eq!(names[0], escape_hidden("Moon\u{202e}ward\u{0007}"));
        assert!(!names[0].contains('\u{202e}') && !names[0].contains('\u{7}'));
        assert_eq!(names[1], "Jade Hall");
    }

    #[test]
    fn a_damaged_settings_file_is_moved_aside_and_defaults_load() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("settings.json"), b"{ not json").unwrap();
        let (s, notice) = Settings::load_or_reset(dir.path());
        assert_eq!(s, Settings::default());
        assert!(notice.unwrap().contains("settings.json.bad"));
        assert_eq!(
            std::fs::read(dir.path().join("settings.json.bad")).unwrap(),
            b"{ not json"
        );
        assert!(!dir.path().join("settings.json").exists());
    }
}
