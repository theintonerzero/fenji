//! `fenji.toml` (M3 spec D16): read whole, checked, then used. Unknown keys and impossible values
//! are refused with the place they were found. Changes need a restart in M3.
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub server: ServerConfig,
    #[serde(default)]
    pub store: StoreConfig,
    #[serde(default)]
    pub data: DataConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub lobby: LobbyConfig,
    #[serde(default)]
    pub access: AccessConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    /// Sent to the client as `gj_uinfo`'s first field.
    #[serde(default = "one")]
    pub id: u32,
    /// Shown by the client; it splits `gj_uinfo` on colons, so none here.
    pub name: String,
    /// What launchers connect to: an IPv4 address, or a name that has one.
    #[serde(default = "localhost")]
    pub public_host: String,
    /// The gate: game connections.
    #[serde(default = "login_bind")]
    pub login_bind: SocketAddr,
    /// The account API.
    #[serde(default = "api_bind")]
    pub api_bind: SocketAddr,
    #[serde(default = "twenty")]
    pub max_players: u32,
    /// Peers whose `X-Forwarded-For` names the client (M3 spec D12).
    #[serde(default)]
    pub trusted_proxies: Vec<IpAddr>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreConfig {
    /// The database file; a relative path is taken from the config file's directory.
    pub path: PathBuf,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::from("fenji.db"),
        }
    }
}

/// The operator's own copy of the pinned client. M3 reads nothing from it (ruling P4).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataConfig {
    pub client_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthConfig {
    #[serde(default = "sixty")]
    pub game_token_ttl_secs: u64,
    #[serde(default = "a_day")]
    pub session_ttl_secs: u64,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            game_token_ttl_secs: sixty(),
            session_ttl_secs: a_day(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LobbyConfig {
    /// Characters per account; the select screen shows nine places and scrolls past them.
    #[serde(default = "nine")]
    pub max_characters: u32,
}

impl Default for LobbyConfig {
    fn default() -> Self {
        Self {
            max_characters: nine(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccessConfig {
    #[serde(default = "yes")]
    pub invite_only: bool,
    #[serde(default)]
    pub admins: Vec<String>,
}

impl Default for AccessConfig {
    fn default() -> Self {
        Self {
            invite_only: true,
            admins: Vec::new(),
        }
    }
}

fn one() -> u32 {
    1
}
fn nine() -> u32 {
    9
}
fn twenty() -> u32 {
    20
}
fn sixty() -> u64 {
    60
}
fn a_day() -> u64 {
    86_400
}
fn yes() -> bool {
    true
}
fn localhost() -> String {
    "127.0.0.1".into()
}
fn login_bind() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 7800))
}
fn api_bind() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 7801))
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("can't read {}", path.display()))?;
        let base = path.parent().unwrap_or(Path::new("."));
        Self::parse(&text, base).with_context(|| format!("in {}", path.display()))
    }

    /// Parses `text`, taking a relative store path from `base`.
    pub fn parse(text: &str, base: &Path) -> anyhow::Result<Self> {
        let mut c: Config = toml::from_str(text)?;
        if c.store.path.is_relative() {
            c.store.path = base.join(&c.store.path);
        }
        c.check()?;
        Ok(c)
    }

    fn check(&self) -> anyhow::Result<()> {
        let s = &self.server;
        if s.name.trim().is_empty() || s.name.chars().count() > 32 {
            bail!("[server] name: 1 to 32 characters");
        }
        if s.name.contains(':') {
            bail!("[server] name: no colon (the client splits gj_uinfo on colons)");
        }
        if s.id == 0 || s.id > i32::MAX as u32 {
            bail!("[server] id: 1 to {}", i32::MAX);
        }
        if s.max_players == 0 {
            bail!("[server] max_players: at least 1");
        }
        if s.public_host.trim().is_empty() || s.public_host.contains([':', '/', ' ']) {
            bail!("[server] public_host: a host name or an IPv4 address, without a port");
        }
        if !(10..=3600).contains(&self.auth.game_token_ttl_secs) {
            bail!("[auth] game_token_ttl_secs: 10 to 3600");
        }
        if !(60..=30 * 86_400).contains(&self.auth.session_ttl_secs) {
            bail!("[auth] session_ttl_secs: 60 to 2592000");
        }
        if !(1..=crate::MAX_CHARACTER_SLOTS).contains(&self.lobby.max_characters) {
            bail!(
                "[lobby] max_characters: 1 to {}",
                crate::MAX_CHARACTER_SLOTS
            );
        }
        if !self.access.invite_only {
            bail!("[access] invite_only: M3 servers are invite-only");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const EXAMPLE: &str = include_str!("../fenji.example.toml");

    #[test]
    fn the_example_parses_and_resolves_the_store_path() {
        let c = Config::parse(EXAMPLE, Path::new("/etc/fenji")).unwrap();
        assert_eq!(c.server.login_bind.port(), 7800);
        assert_eq!(c.server.api_bind.port(), 7801);
        assert_eq!(c.store.path, Path::new("/etc/fenji/fenji.db"));
        assert_eq!(c.auth.game_token_ttl_secs, 60);
        assert_eq!(c.lobby.max_characters, 9);
    }

    #[test]
    fn unknown_keys_and_bad_values_are_refused_with_their_place() {
        let e = Config::parse("[server]\nname = \"F\"\nbogus = 1\n", Path::new("/"))
            .unwrap_err()
            .to_string();
        assert!(e.contains("bogus"), "{e}");
        let e = Config::parse("[server]\nname = \"Fen:ji\"\n", Path::new("/"))
            .unwrap_err()
            .to_string();
        assert!(e.contains("colon"), "{e}");
        let e = Config::parse("[server]\nname = \"F\"\nmax_players = 0\n", Path::new("/"))
            .unwrap_err()
            .to_string();
        assert!(e.contains("max_players"), "{e}");
        let e = Config::parse(
            "[server]\nname = \"F\"\n[access]\ninvite_only = false\n",
            Path::new("/"),
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("invite_only"), "{e}");
        let e = Config::parse(
            "[server]\nname = \"F\"\n[lobby]\nmax_characters = 21\n",
            Path::new("/"),
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("max_characters"), "{e}");
        let e = Config::parse(
            "[server]\nname = \"F\"\npublic_host = \"a:1\"\n",
            Path::new("/"),
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("public_host"), "{e}");
    }

    #[test]
    fn defaults_fill_everything_but_the_name() {
        let c = Config::parse("[server]\nname = \"Fenji\"\n", Path::new("/srv")).unwrap();
        assert_eq!(
            (c.server.id, c.server.max_players, c.lobby.max_characters),
            (1, 20, 9)
        );
        assert!(c.server.trusted_proxies.is_empty());
        assert_eq!(c.store.path, Path::new("/srv/fenji.db"));
        assert!(c.data.client_path.is_none());
        assert!(
            Config::parse("", Path::new("/")).is_err(),
            "a name is required"
        );
    }
}
