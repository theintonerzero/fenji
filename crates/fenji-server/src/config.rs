//! `fenji.toml` (M3 spec D16; M4 world spec §3): read whole, checked, then used. Unknown keys and
//! impossible values are refused with the place they were found. Changes need a restart. A first
//! run without one writes it, and `init` writes one to edit (M4 hosting spec §3.6).
use std::io::Write;
use std::net::{IpAddr, SocketAddr};
use std::ops::RangeInclusive;
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
    #[serde(default)]
    pub world: WorldConfig,
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
    /// The address players reach the account API at, which invite links name:
    /// `https://<host>[:<port>]`, or `http://` to a loopback address (fenji-launch refuses plain
    /// http anywhere else). Unset, links name `api_bind` when it is a loopback address, and are
    /// left out otherwise.
    #[serde(default)]
    pub public_api: Option<String>,
    #[serde(default = "twenty")]
    pub max_players: u32,
    /// Peers whose `X-Forwarded-For` names the client (M3 spec D12).
    #[serde(default)]
    pub trusted_proxies: Vec<IpAddr>,
    /// The world's ticks per second (Fenji design spec §6.4).
    #[serde(default = "twenty")]
    pub tick_hz: u32,
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

/// The operator's own copy of the pinned client. Nothing reads it before M5 (#180).
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

/// The world (M4 world spec §3): where new characters enter, the speed check's leniency, and how
/// often places are saved.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldConfig {
    /// The scene new characters enter (world spec §3.1).
    #[serde(default = "start_scene")]
    pub start_scene: i64,
    /// Where in it: x, y, z (up) and a yaw in degrees.
    #[serde(default = "start_position")]
    pub start_position: [f64; 4],
    /// The speed check's tolerance (Fenji design spec §6.4: `max_speed × dt × tolerance`).
    #[serde(default = "one_and_a_half")]
    pub move_tolerance: f64,
    /// How often everyone's place is saved, besides when they leave.
    #[serde(default = "five_minutes")]
    pub save_every_secs: u64,
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            start_scene: start_scene(),
            start_position: start_position(),
            move_tolerance: one_and_a_half(),
            save_every_secs: five_minutes(),
        }
    }
}

/// The farthest a coordinate (x, y or z) may be from 0: 100 km. The world takes no point beyond
/// it, and `[world] start_position` may name none.
pub const COORDINATE_LIMIT: f64 = 10_000_000.0;

/// The largest yaw, either way, in degrees: for every position the world takes, the start's too.
pub const YAW_LIMIT: f64 = 360.0;

/// `[world] move_tolerance`'s range. The speed check holds any tolerance to it.
pub const MOVE_TOLERANCE: RangeInclusive<f64> = 1.0..=5.0;

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
/// The start point of vocations 0–5 in the client's tables (M4 world spec §2.5; Inferred until
/// the world's first run checks it).
fn start_scene() -> i64 {
    5
}
fn start_position() -> [f64; 4] {
    [2366.0, -71735.0, 64854.0, 345.0]
}
fn one_and_a_half() -> f64 {
    1.5
}
fn five_minutes() -> u64 {
    300
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

    /// What invite links start with (they end `/#invite=<code>`): `public_api`, or `api_bind`
    /// over http when that is a loopback address, or none.
    pub fn invite_base(&self) -> Option<String> {
        if let Some(api) = &self.server.public_api {
            return api_origin(api);
        }
        let a = self.server.api_bind;
        a.ip().is_loopback().then(|| format!("http://{a}"))
    }

    fn check(&self) -> anyhow::Result<()> {
        let s = &self.server;
        if s.name.trim().is_empty() || s.name.chars().count() > 32 {
            bail!("[server] name: 1 to 32 characters");
        }
        if s.name.contains(':') {
            bail!("[server] name: no colon (the client splits gj_uinfo on colons)");
        }
        if s.name.chars().any(char::is_control) {
            bail!("[server] name: no control characters (it goes into gj_uinfo)");
        }
        if s.login_bind == s.api_bind && s.login_bind.port() != 0 {
            bail!("[server] api_bind: the same address as login_bind");
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
        if s.public_api
            .as_deref()
            .is_some_and(|api| api_origin(api).is_none())
        {
            bail!(
                "[server] public_api: https://<host>[:<port>], or http:// to a loopback address, with no path"
            );
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
        if !(1..=60).contains(&s.tick_hz) {
            bail!("[server] tick_hz: 1 to 60");
        }
        let w = &self.world;
        if !(1..=i64::from(i32::MAX)).contains(&w.start_scene) {
            bail!("[world] start_scene: 1 to {}", i32::MAX);
        }
        let [x, y, z, yaw] = w.start_position;
        let beyond = |v: f64, limit: f64| !v.is_finite() || v.abs() > limit;
        if [x, y, z].into_iter().any(|v| beyond(v, COORDINATE_LIMIT)) || beyond(yaw, YAW_LIMIT) {
            bail!(
                "[world] start_position: [x, y, z, yaw], with x, y and z from -{COORDINATE_LIMIT} to {COORDINATE_LIMIT} and the yaw from -{YAW_LIMIT} to {YAW_LIMIT} degrees"
            );
        }
        if !MOVE_TOLERANCE.contains(&w.move_tolerance) {
            bail!(
                "[world] move_tolerance: {:?} to {:?}",
                MOVE_TOLERANCE.start(),
                MOVE_TOLERANCE.end()
            );
        }
        if !(30..=3600).contains(&w.save_every_secs) {
            bail!("[world] save_every_secs: 30 to 3600");
        }
        Ok(())
    }
}

/// `api` as an origin, `scheme://host[:port]` without a trailing slash, if it is one players can use:
/// https anywhere, or http to a loopback address, as fenji-launch accepts them.
fn api_origin(api: &str) -> Option<String> {
    let api = api.strip_suffix('/').unwrap_or(api);
    if api.chars().any(|c| c.is_whitespace() || c.is_control()) || api.contains(['#', '?']) {
        return None;
    }
    let (https, rest) = match (api.strip_prefix("https://"), api.strip_prefix("http://")) {
        (Some(r), _) => (true, r),
        (None, Some(r)) => (false, r),
        (None, None) => return None,
    };
    if rest.is_empty() || rest.contains('/') || rest.contains('@') {
        return None;
    }
    if !https {
        let host = match rest.strip_prefix('[') {
            Some(v6) => v6.split_once(']')?.0,
            None => rest.split(':').next()?,
        };
        let loopback =
            host == "localhost" || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback());
        if !loopback {
            return None;
        }
    }
    Some(api.to_string())
}

/// The example configuration, built into the binary: the one place every key is described.
pub const EXAMPLE: &str = include_str!("../fenji.example.toml");

/// Where the configuration is without `--config`: in the working directory.
pub const DEFAULT_PATH: &str = "fenji.toml";

/// What `run` logs at `info` once a first run has started, `<folder>` being the working directory.
pub const FIRST_RUN: [&str; 3] = [
    "First run: wrote fenji.toml and data/ in <folder>.",
    "Type `invite` to make your first invite link.",
    "Only this computer can join until you set public_host: see docs/hosting.md.",
];

// `[store] path` in the example, and in the template: as wide as each other, so that the comment
// after it stays in line.
const EXAMPLE_STORE_PATH: &str = "\npath = \"fenji.db\"     ";
const TEMPLATE_STORE_PATH: &str = "\npath = \"data/fenji.db\"";

/// What a first run and `init` write: the example, with the database in `data/`, the data folder.
pub fn template() -> String {
    EXAMPLE.replacen(EXAMPLE_STORE_PATH, TEMPLATE_STORE_PATH, 1)
}

/// Writes the [`template`] at `path` for a host to edit (`fenji-server init`), making any folders
/// above it that are missing. Never over a file that's there.
pub fn init(path: &Path) -> anyhow::Result<()> {
    if let Some(folder) = path.parent().filter(|f| !f.as_os_str().is_empty()) {
        std::fs::create_dir_all(folder)
            .with_context(|| format!("can't make the folder {}", folder.display()))?;
    }
    write_new(path, template().as_bytes()).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            anyhow::anyhow!(
                "{} already exists; init doesn't replace a config",
                path.display()
            )
        } else {
            anyhow::Error::new(e).context(format!("can't write {}", path.display()))
        }
    })
}

/// Writes `bytes` to `path` as a new file, failing if anything is there. A file left part-written
/// is deleted, so that no later run reads it as a whole one.
fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    if written.is_err() {
        drop(file);
        let _ = std::fs::remove_file(path);
    }
    written
}

/// Makes `dir` and any folders above it that are missing, owner-only (0700) on Unix: the data
/// folder and the folders in it hold the database, its backups and the server's key.
pub(crate) fn private_folder(dir: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}

/// A configuration for `run`, and whether this run wrote it.
#[derive(Debug)]
pub enum Loaded {
    /// A file that was there.
    Existing(Config),
    /// The [`template`], which this run wrote with its data folder, as there was no config.
    FirstRun(Config),
}

/// The configuration `run` serves with (M4 hosting spec §3.6, H9):
/// - `path`, which must exist, so that a mistyped path under systemd never starts a server with a
///   fresh, empty database;
/// - without one, [`DEFAULT_PATH`]; if there's no file there, a first run writes the [`template`]
///   and makes the data folder, owner-only on Unix.
pub fn load_or_first_run(path: Option<&Path>) -> anyhow::Result<Loaded> {
    let given = path.is_some();
    let path = path.unwrap_or(Path::new(DEFAULT_PATH));
    // Unless it's known to be missing: when that can't be told, `load` says why.
    if !matches!(path.try_exists(), Ok(false)) {
        return Config::load(path).map(Loaded::Existing);
    }
    if given {
        bail!(
            "there's no config at {0}; write one with `fenji-server --config {0} init`",
            path.display()
        );
    }
    init(path)?;
    let cfg = Config::load(path)?;
    if let Some(data) = cfg.store.path.parent() {
        private_folder(data)
            .with_context(|| format!("can't make the data folder {}", data.display()))?;
    }
    Ok(Loaded::FirstRun(cfg))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn the_example_parses_and_resolves_the_store_path() {
        let c = Config::parse(EXAMPLE, Path::new("/etc/fenji")).unwrap();
        assert_eq!(c.server.login_bind.port(), 7800);
        assert_eq!(c.server.api_bind.port(), 7801);
        assert_eq!(c.store.path, Path::new("/etc/fenji/fenji.db"));
        assert_eq!(c.auth.game_token_ttl_secs, 60);
        assert_eq!(c.lobby.max_characters, 9);
        assert_eq!(c.server.tick_hz, 20);
        assert_eq!(c.world.start_scene, 5);
        assert_eq!(c.world.start_position, [2366.0, -71735.0, 64854.0, 345.0]);
        assert_eq!(c.world.move_tolerance, 1.5);
        assert_eq!(c.world.save_every_secs, 300);
    }

    #[test]
    fn the_template_parses_binds_loopback_and_keeps_the_database_in_data() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::parse(&template(), dir.path()).unwrap();
        assert_eq!(cfg.store.path, dir.path().join("data/fenji.db"));
        assert!(cfg.server.login_bind.ip().is_loopback());
        assert!(cfg.server.api_bind.ip().is_loopback());
    }

    #[test]
    fn init_writes_the_template_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("etc/fenji/fenji.toml");
        init(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), template());
        let e = init(&path).unwrap_err();
        assert!(format!("{e:#}").contains("already exists"), "{e:#}");
    }

    #[test]
    fn an_explicit_config_that_is_missing_is_an_error_with_the_init_hint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.toml");
        let e = load_or_first_run(Some(&path)).unwrap_err();
        let text = format!("{e:#}");
        assert!(
            text.contains("missing.toml") && text.contains("init"),
            "{text}"
        );
        assert!(!path.exists(), "nothing is written for an explicit path");
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
    fn a_name_has_no_control_characters_and_the_binds_differ() {
        for name in ["Fen\\u0000ji", "Fen\\nji", "Fen\\u007fji"] {
            let text = format!("[server]\nname = \"{name}\"\n");
            let e = Config::parse(&text, Path::new("/"))
                .unwrap_err()
                .to_string();
            assert!(e.contains("name"), "{name}: {e}");
        }
        let same = "[server]\nname = \"F\"\nlogin_bind = \"127.0.0.1:7800\"\napi_bind = \"127.0.0.1:7800\"\n";
        let e = Config::parse(same, Path::new("/")).unwrap_err().to_string();
        assert!(e.contains("api_bind"), "{e}");
        // Port 0 asks the system for a free port each time, so two of them don't clash.
        let zero =
            "[server]\nname = \"F\"\nlogin_bind = \"127.0.0.1:0\"\napi_bind = \"127.0.0.1:0\"\n";
        Config::parse(zero, Path::new("/")).unwrap();
    }

    #[test]
    fn defaults_fill_everything_but_the_name() {
        let c = Config::parse("[server]\nname = \"Fenji\"\n", Path::new("/srv")).unwrap();
        assert_eq!(
            (c.server.id, c.server.max_players, c.lobby.max_characters),
            (1, 20, 9)
        );
        assert!(c.server.trusted_proxies.is_empty());
        assert_eq!((c.server.tick_hz, c.world.start_scene), (20, 5));
        assert_eq!(
            (c.world.move_tolerance, c.world.save_every_secs),
            (1.5, 300)
        );
        assert_eq!(c.store.path, Path::new("/srv/fenji.db"));
        assert!(c.data.client_path.is_none());
        assert!(
            Config::parse("", Path::new("/")).is_err(),
            "a name is required"
        );
    }

    #[test]
    fn the_world_takes_whole_numbers_for_its_start_and_refuses_impossible_values() {
        let parse = |extra: &str| {
            Config::parse(&format!("[server]\nname = \"F\"\n{extra}"), Path::new("/"))
        };
        let c = parse("[world]\nstart_scene = 3006\nstart_position = [52626, 9004, 1450, -169]\n")
            .unwrap();
        assert_eq!(c.world.start_scene, 3006);
        assert_eq!(c.world.start_position, [52626.0, 9004.0, 1450.0, -169.0]);
        for (extra, place) in [
            ("tick_hz = 0", "tick_hz"),
            ("tick_hz = 61", "tick_hz"),
            ("[world]\nstart_scene = 0", "start_scene"),
            ("[world]\nstart_scene = 2147483648", "start_scene"),
            (
                "[world]\nstart_position = [1.0, 2.0, 3.0]",
                "start_position",
            ),
            (
                "[world]\nstart_position = [nan, 0.0, 0.0, 0.0]",
                "start_position",
            ),
            (
                "[world]\nstart_position = [0.0, inf, 0.0, 0.0]",
                "start_position",
            ),
            (
                "[world]\nstart_position = [0.0, 0.0, 1e8, 0.0]",
                "start_position",
            ),
            (
                "[world]\nstart_position = [0.0, 0.0, 0.0, 361.0]",
                "start_position",
            ),
            ("[world]\nmove_tolerance = 0.5", "move_tolerance"),
            ("[world]\nmove_tolerance = nan", "move_tolerance"),
            ("[world]\nsave_every_secs = 29", "save_every_secs"),
            ("[world]\nsave_every_secs = 3601", "save_every_secs"),
            ("[world]\nbogus = 1", "bogus"),
        ] {
            let e = format!("{:#}", parse(extra).unwrap_err());
            assert!(e.contains(place), "{extra}: {e}");
        }
    }

    #[test]
    fn public_api_is_https_or_loopback_http() {
        let base = std::path::Path::new("/");
        let parse = |api: &str| {
            Config::parse(
                &format!("[server]\nname = \"F\"\npublic_api = \"{api}\"\n"),
                base,
            )
        };
        let c = parse("https://play.example.net/").unwrap();
        assert_eq!(c.invite_base().as_deref(), Some("https://play.example.net"));
        let c = parse("https://play.example.net:8443").unwrap();
        assert_eq!(
            c.invite_base().as_deref(),
            Some("https://play.example.net:8443")
        );
        let c = parse("http://127.0.0.1:7801").unwrap();
        assert_eq!(c.invite_base().as_deref(), Some("http://127.0.0.1:7801"));
        for bad in [
            "http://play.example.net",
            "ftp://play.example.net",
            "https://",
            "https://play.example.net/api",
            "https://play.example.net#x",
            "https://play example.net",
            "play.example.net",
        ] {
            let e = parse(bad).unwrap_err().to_string();
            assert!(e.contains("public_api"), "{bad}: {e}");
        }
    }

    #[test]
    fn without_public_api_invite_links_use_a_loopback_api_bind_only() {
        let base = std::path::Path::new("/");
        let c = Config::parse("[server]\nname = \"F\"\n", base).unwrap();
        assert_eq!(c.invite_base().as_deref(), Some("http://127.0.0.1:7801"));
        let c = Config::parse(
            "[server]\nname = \"F\"\napi_bind = \"[::1]:7801\"\nlogin_bind = \"[::1]:7800\"\n",
            base,
        )
        .unwrap();
        assert_eq!(c.invite_base().as_deref(), Some("http://[::1]:7801"));
        let c = Config::parse(
            "[server]\nname = \"F\"\napi_bind = \"0.0.0.0:7801\"\n",
            base,
        )
        .unwrap();
        assert_eq!(c.invite_base(), None);
    }
}
