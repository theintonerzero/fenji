//! `fenji.toml` (M3 spec D16; M4 world spec §3): read whole, checked, then used. Unknown keys and
//! impossible values are refused with the place they were found. Changes need a restart. A first
//! run without one writes it, and `init` writes one to edit (M4 hosting spec §3.6). Two variables
//! of the environment move the listeners over the file's (Ruling HP7).
use std::ffi::OsString;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde::Deserialize;

use crate::invite::{InviteBase, THIS_COMPUTER};

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
    /// The address players' game clients connect to (the gate): an IPv4 address, or a name that
    /// has one. The account API's address is `public_api`. Unset, each player's is the one they
    /// reached the account API at (M4 hosting spec §8, HA2).
    #[serde(default)]
    pub public_host: Option<String>,
    /// The gate: game connections.
    #[serde(default = "login_bind")]
    pub login_bind: SocketAddr,
    /// The account API.
    #[serde(default = "api_bind")]
    pub api_bind: SocketAddr,
    /// `"self"`: the account API speaks TLS with the server's own key, which invite links pin (M4
    /// hosting spec §3.10). Unset, plain http: for loopback, or behind a proxy that holds the
    /// certificate.
    #[serde(default)]
    pub tls: Option<Tls>,
    /// The address players reach the account API at, which invite links name:
    /// `https://<host>[:<port>]`, or `http://` to a loopback address (fenji-launch refuses plain
    /// http anywhere else). Unset, links start as [`Config::invite_base`] and `invite::bases` say.
    #[serde(default)]
    pub public_api: Option<String>,
    /// How many accounts may be connected at once, 1 to [`MAX_PLAYERS`].
    #[serde(default = "twenty")]
    pub max_players: u32,
    /// Peers whose `X-Forwarded-For` names the client (M3 spec D12): proxies that add it, such as
    /// Caddy, never one that passes the players' TLS through, which can't.
    #[serde(default)]
    pub trusted_proxies: Vec<IpAddr>,
    /// The world's ticks per second (Fenji design spec §6.4).
    #[serde(default = "twenty")]
    pub tick_hz: u32,
    /// Where the source code of the server that runs is (M4 hosting spec §3.8), which launchers
    /// offer players: the AGPL's §13 asks a host who changes Fenji to offer their changes. By
    /// default, this repository at this version's tag.
    #[serde(default = "source_url")]
    pub source_url: String,
    /// The host's notice for players, which launchers show (M4 hosting spec §8, HA1); empty for
    /// none. The console can set one over it.
    #[serde(default)]
    pub motd: String,
    /// Who hosts this server, as players' launchers show it ("hosted by Wen"; launcher spec L9):
    /// plain text of at most [`HOST_MAX`] characters. Unset, or blank, the launcher names no one.
    #[serde(default)]
    pub host: Option<String>,
    /// Where this server is, as players' launchers show it ("Germany"; M4 hosting spec Amendment
    /// C, HC1): plain text of at most [`REGION_MAX`] characters. Unset, or blank, they show none.
    /// A first run guesses it from this computer's timezone ([`crate::region::guess`]).
    #[serde(default)]
    pub region: Option<String>,
    /// Whether the file sets `region` blank: its host chose to show none, so a start never hints
    /// at a guess (HC3).
    #[serde(skip)]
    pub region_blank: bool,
}

pub use crate::region::REGION_MAX;

/// The longest `[server] host`, in characters.
pub const HOST_MAX: usize = 32;

/// The most `[server] max_players` may be (#281). With everyone in one place, each tick sends
/// every player a move of each of the others, and a connection's queue (`link::OUTBOUND`, 256)
/// must hold those and the tick's other messages, or a player who reads all it is sent is let go
/// as a slow reader. Fenji is sized for 20 (design spec §6.7).
pub const MAX_PLAYERS: u32 = 200;

/// How the account API serves TLS itself (M4 hosting spec §3.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tls {
    /// `tls = "self"`: with the server's own key, made on its first start in `data/tls/`.
    #[serde(rename = "self")]
    OwnKey,
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
/// The start point of vocations 0–5 in the client's tables (M4 world spec §2.5), where Task 11's
/// runs entered and walked (world.md §13).
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
fn login_bind() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 7800))
}
fn api_bind() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 7801))
}
fn source_url() -> String {
    format!(
        "https://github.com/theintonerzero/fenji/tree/v{}",
        env!("CARGO_PKG_VERSION")
    )
}

/// The hosts' guide for this version, on the web, as the archive's README.txt names it: the
/// archive holds no `docs/hosting.md`, so the server's lines point here (the hosting plan's final
/// review B, Minor 7).
pub fn guide() -> String {
    format!(
        "https://github.com/theintonerzero/fenji/blob/v{}/docs/hosting.md",
        env!("CARGO_PKG_VERSION")
    )
}

/// The longest `[server] source_url`, in characters.
pub const SOURCE_URL_MAX: usize = 200;

/// Whether `url` can be `[server] source_url`: an `https://` or `http://` address of at most
/// [`SOURCE_URL_MAX`] characters, without whitespace or hidden characters, since launchers show it
/// and offer it to open.
fn source_url_ok(url: &str) -> bool {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"));
    rest.is_some_and(|r| !r.is_empty())
        && url.chars().count() <= SOURCE_URL_MAX
        && !url
            .chars()
            .any(|c| c.is_whitespace() || crate::text::hidden(c))
}

/// Whether `host` is one players can reach (`[server] public_host`, or the host a request names):
/// an IPv4 address that isn't 0.0.0.0, a broadcast or a multicast one, or a host name, whose
/// dot-separated labels of 1 to 63 letters, digits and hyphens neither start nor end with a hyphen,
/// at most 253 characters in all, the last not all digits (RFC 1123), so that no IPv4 address in
/// another spelling passes as a name.
pub(crate) fn reachable_host(host: &str) -> bool {
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        return !(ip.is_unspecified() || ip.is_broadcast() || ip.is_multicast());
    }
    let label_ok = |l: &str| {
        (1..=63).contains(&l.len())
            && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && !l.starts_with('-')
            && !l.ends_with('-')
    };
    host.len() <= 253
        && host.split('.').all(label_ok)
        && host
            .rsplit('.')
            .next()
            .is_some_and(|last| !last.bytes().all(|b| b.is_ascii_digit()))
}

/// Whether `host`, a [`reachable_host`], is this computer's own.
fn loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<Ipv4Addr>().is_ok_and(|ip| ip.is_loopback())
}

/// The variable of the environment that moves `[server] login_bind` over the file's (Ruling HP7).
pub const LOGIN_BIND_VAR: &str = "FENJI_LOGIN_BIND";

/// The variable of the environment that moves `[server] api_bind` over the file's (Ruling HP7).
pub const API_BIND_VAR: &str = "FENJI_API_BIND";

impl Config {
    /// The configuration at `path`, with the binds the environment gives over the file's
    /// ([`Config::override_binds`]): what `fenji-server` runs with, for `run` and the admin
    /// commands alike.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("can't read {}", path.display()))?;
        let base = path.parent().unwrap_or(Path::new("."));
        Self::parse(&text, base)
            .with_context(|| format!("in {}", path.display()))?
            .override_binds(|name| std::env::var_os(name))
    }

    /// Parses `text`, taking a relative store path from `base`.
    pub fn parse(text: &str, base: &Path) -> anyhow::Result<Self> {
        let mut c: Config = toml::from_str(text)?;
        if c.server
            .host
            .as_deref()
            .is_some_and(|h| h.trim().is_empty())
        {
            c.server.host = None;
        }
        if c.server
            .region
            .as_deref()
            .is_some_and(|r| r.trim().is_empty())
        {
            c.server.region = None;
            c.server.region_blank = true;
        }
        if c.store.path.is_relative() {
            c.store.path = base.join(&c.store.path);
        }
        c.check()?;
        Ok(c)
    }

    /// `login_bind` and `api_bind` from [`LOGIN_BIND_VAR`] and [`API_BIND_VAR`] as `var` reads
    /// them, each where it's set (Ruling HP7): a container, a service or a test moves the
    /// listeners without editing the file. A value that isn't an address and port is an error that
    /// names its variable, and the binds are checked as the file's are.
    pub fn override_binds(
        mut self,
        var: impl Fn(&str) -> Option<OsString>,
    ) -> anyhow::Result<Self> {
        let mut moved = false;
        for (name, bind, example) in [
            (LOGIN_BIND_VAR, &mut self.server.login_bind, "0.0.0.0:7800"),
            (API_BIND_VAR, &mut self.server.api_bind, "0.0.0.0:7801"),
        ] {
            let Some(value) = var(name) else {
                continue;
            };
            *bind = value
                .to_str()
                .and_then(|v| v.parse().ok())
                .with_context(|| {
                    format!("{name}: {value:?} isn't an address and port, such as {example}")
                })?;
            moved = true;
        }
        if moved {
            self.check()
                .with_context(|| format!("with {LOGIN_BIND_VAR} and {API_BIND_VAR}"))?;
        }
        Ok(self)
    }

    /// Where invite links start (M4 hosting spec §3.11), `pin` being the server's own key's:
    /// 1. `public_api`'s origin;
    /// 2. with `tls = "self"`, `https://<public_host>:<api_bind's port>`;
    /// 3. `api_bind` itself, when it's a loopback address and `public_host` is one too or unset;
    /// 4. otherwise none, and without `tls = "self"` `run` warns with
    ///    [`crate::invite::no_links`].
    ///
    /// Under `tls = "self"` each carries the pin, and there's none without it: a launcher couldn't
    /// trust the server's key. With neither `public_api` nor `public_host` set, `invite::bases`
    /// gives one for each of the machine's addresses besides (Amendment A, HA4).
    pub fn invite_base(&self, pin: Option<&str>) -> Option<InviteBase> {
        self.labelled_invite_base(pin, self.server.api_bind.port())
            .map(|(_, base)| base)
    }

    /// [`Config::invite_base`], for an API listening on `api_port`, with the rule's label for
    /// `invite`: `public_api`, `public_host` or [`THIS_COMPUTER`].
    pub(crate) fn labelled_invite_base(
        &self,
        pin: Option<&str>,
        api_port: u16,
    ) -> Option<(&'static str, InviteBase)> {
        let s = &self.server;
        let own_key = s.tls == Some(Tls::OwnKey);
        let pin = match (own_key, pin) {
            (true, None) => return None,
            (true, Some(pin)) => Some(pin.to_owned()),
            (false, _) => None,
        };
        if let Some(api) = &s.public_api {
            let origin = api_origin(api)?;
            return Some(("public_api", InviteBase { origin, pin }));
        }
        if let (true, Some(host)) = (own_key, &s.public_host) {
            let origin = format!("https://{host}:{api_port}");
            return Some(("public_host", InviteBase { origin, pin }));
        }
        let api = s.api_bind.ip().to_canonical();
        if api.is_loopback() && s.public_host.as_deref().is_none_or(loopback_host) {
            let scheme = if own_key { "https" } else { "http" };
            let origin = format!("{scheme}://{}", SocketAddr::new(api, api_port));
            return Some((THIS_COMPUTER, InviteBase { origin, pin }));
        }
        None
    }

    fn check(&self) -> anyhow::Result<()> {
        let s = &self.server;
        if s.name.trim().is_empty() || s.name.chars().count() > 32 {
            bail!("[server] name: 1 to 32 characters");
        }
        if s.name.contains(':') {
            bail!("[server] name: no colon (the client splits gj_uinfo on colons)");
        }
        // Plain as `host` and `motd` are: the console's title bar, every launcher's list and the
        // game's gj_uinfo show it.
        if !crate::text::plain(&s.name) {
            bail!("[server] name: no control or invisible characters (it goes into gj_uinfo)");
        }
        if s.login_bind == s.api_bind && s.login_bind.port() != 0 {
            bail!("[server] api_bind: the same address as login_bind");
        }
        if s.id == 0 || s.id > i32::MAX as u32 {
            bail!("[server] id: 1 to {}", i32::MAX);
        }
        if !(1..=MAX_PLAYERS).contains(&s.max_players) {
            bail!("[server] max_players: 1 to {MAX_PLAYERS}");
        }
        if s.public_host.as_deref().is_some_and(|h| !reachable_host(h)) {
            bail!(
                "[server] public_host: a host name or an IPv4 address players can reach, without a port"
            );
        }
        if s.public_api
            .as_deref()
            .is_some_and(|api| api_origin(api).is_none())
        {
            bail!(
                "[server] public_api: https://<host>[:<port>], or http:// to a loopback address, with no path"
            );
        }
        if s.tls == Some(Tls::OwnKey)
            && s.public_api
                .as_deref()
                .is_some_and(|api| !api.starts_with("https://"))
        {
            bail!(
                "[server] public_api: https://, as with tls = \"self\" the account API speaks TLS"
            );
        }
        if !source_url_ok(&s.source_url) {
            bail!(
                "[server] source_url: an https:// or http:// address of at most {SOURCE_URL_MAX} characters, without spaces or control characters"
            );
        }
        if let Some(host) = &s.host
            && (host.chars().count() > HOST_MAX || crate::status::check_message(host).is_err())
        {
            bail!(
                "[server] host: plain text of at most {HOST_MAX} characters, without control or invisible characters"
            );
        }
        if let Some(region) = &s.region
            && (region.chars().count() > REGION_MAX
                || crate::status::check_message(region).is_err())
        {
            bail!(
                "[server] region: plain text of at most {REGION_MAX} characters, without control or invisible characters"
            );
        }
        if crate::status::check_message(&s.motd).is_err() {
            bail!(
                "[server] motd: plain text of at most {} characters, without control or invisible characters",
                crate::status::MESSAGE_MAX
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

/// What a first run says, at `info`, `<folder>` being where it wrote and `<guide>` this version's
/// hosts' guide ([`guide`]): the first line before the server starts, so that a first run that
/// can't start still explains its new files, and the others once it runs (Amendment A, HA3).
pub const FIRST_RUN: [&str; 3] = [
    "First run: wrote fenji.toml and data/ in <folder>.",
    "Friends on this network can join now: type `invite` for a link.",
    "To let in friends over the internet, open ports 7800 and 7801 on your router, or use a VPN: \
     see <guide>.",
];

/// [`FIRST_RUN`]'s lines for a first run in `folder`, for `run` and the host console to show.
pub fn first_run_lines(folder: &Path) -> [String; 3] {
    let (folder, guide) = (folder.display().to_string(), guide());
    FIRST_RUN.map(|line| line.replace("<folder>", &folder).replace("<guide>", &guide))
}

/// The lines the template changes in the example, each pair as wide as each other so that the
/// comments after them stay in line: the database in `data/`, and the LAN-ready start of Amendment
/// A's HA3, listening on every interface with the server's own key.
const TEMPLATE_EDITS: [(&str, &str); 4] = [
    ("\npath = \"fenji.db\"     ", "\npath = \"data/fenji.db\""),
    (
        "\nlogin_bind = \"127.0.0.1:7800\"",
        "\nlogin_bind = \"0.0.0.0:7800\"  ",
    ),
    (
        "\napi_bind = \"127.0.0.1:7801\"",
        "\napi_bind = \"0.0.0.0:7801\"  ",
    ),
    ("\n# tls = \"self\"", "\ntls = \"self\"  "),
];

/// What a first run and `init` write: the example, with the database in `data/`, the data folder,
/// and ready for the LAN (HA3): both listeners on every interface, and `tls = "self"`. With the
/// region this computer's timezone names, if it names one (Amendment C, HC2).
pub fn template() -> String {
    template_with(crate::region::guess().as_deref())
}

/// The example's commented-out region, which a guess takes the place of.
const REGION_LINE: &str = "\n# region = \"Your country\"       #";

/// [`template`] with `region` in place of the example's commented-out one, if there is one, its
/// comment kept in line where the name allows.
pub fn template_with(region: Option<&str>) -> String {
    let text = TEMPLATE_EDITS
        .iter()
        .fold(EXAMPLE.to_owned(), |text, (from, to)| {
            text.replacen(from, to, 1)
        });
    let Some(region) = region else {
        return text;
    };
    let value = format!(
        "region = \"{}\"",
        region.replace('\\', "\\\\").replace('"', "\\\"")
    );
    // The comment starts where the line it replaces had it, counted in characters, as the
    // padding is.
    let width = REGION_LINE.len() - "\n #".len();
    let width = width.max(value.chars().count());
    text.replacen(REGION_LINE, &format!("\n{value:<width$} #"), 1)
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
/// folder and the folders in it hold the database, its backups and the server's key. An empty
/// `dir`, a bare file name's parent, is the working directory, which is there.
pub(crate) fn private_folder(dir: &Path) -> std::io::Result<()> {
    if dir.as_os_str().is_empty() {
        return Ok(());
    }
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}

/// The data folder of a database at `store_path`: its directory, or the working directory for a
/// database named alone. It holds the database, its backups, the server's key, the full-screen
/// console's logs and the console socket.
pub fn data_folder(store_path: &Path) -> PathBuf {
    match store_path.parent() {
        Some(folder) if !folder.as_os_str().is_empty() => folder.to_owned(),
        _ => PathBuf::from("."),
    }
}

/// A configuration for `run`, and whether this run wrote it.
#[derive(Debug)]
pub enum Loaded {
    /// A file that was there.
    Existing(Config),
    /// The [`template`], which this run wrote with its data folder, as there was no config.
    FirstRun(Config),
}

impl Loaded {
    /// The configuration, either way.
    pub fn config(&self) -> &Config {
        match self {
            Loaded::Existing(cfg) | Loaded::FirstRun(cfg) => cfg,
        }
    }
}

/// The configuration the admin commands and `console` use: `path`, `--config`'s, or without it
/// [`DEFAULT_PATH`], which must be there: they make no first run. Without `--config`, outside the
/// server's folder, the error says how to find it (the hosting plan's final review B, Minor 4).
pub fn load_existing(path: Option<&Path>) -> anyhow::Result<Config> {
    if let Some(path) = path {
        return Config::load(path);
    }
    let here = Path::new(DEFAULT_PATH);
    if matches!(here.try_exists(), Ok(false)) {
        let folder = std::env::current_dir().unwrap_or_else(|_| ".".into());
        bail!(
            "can't read {DEFAULT_PATH}: there's none in this folder ({}); run fenji-server in the \
             server's own folder, or give --config with its {DEFAULT_PATH}",
            folder.display()
        );
    }
    Config::load(here)
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
    use crate::invite::InviteBase;

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
        assert_eq!(c.server.motd, "");
        assert_eq!(c.server.source_url, source_url());
        // The hosting plan's final review, Minor 3: trusted_proxies is for a proxy that adds
        // X-Forwarded-For, such as Caddy, never one that passes TLS through.
        let proxies = EXAMPLE
            .lines()
            .find(|l| l.starts_with("trusted_proxies"))
            .unwrap();
        assert!(proxies.contains("X-Forwarded-For"), "{proxies}");
    }

    #[test]
    fn the_template_is_ready_for_the_lan_with_its_own_key_and_keeps_the_database_in_data() {
        // Amendment A, HA3: listening on every interface, with the server's own key.
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::parse(&template_with(None), dir.path()).unwrap();
        assert_eq!(cfg.store.path, dir.path().join("data/fenji.db"));
        assert_eq!(cfg.server.login_bind, "0.0.0.0:7800".parse().unwrap());
        assert_eq!(cfg.server.api_bind, "0.0.0.0:7801".parse().unwrap());
        assert_eq!(cfg.server.tls, Some(Tls::OwnKey));
        assert_eq!(
            cfg.server.public_host, None,
            "HA2: players play where they came in"
        );
        assert_eq!(cfg.server.public_api, None);
        // The example keeps the defaults: loopback, no TLS of the server's own.
        let example = Config::parse(EXAMPLE, dir.path()).unwrap();
        assert!(example.server.login_bind.ip().is_loopback());
        assert!(example.server.api_bind.ip().is_loopback());
        assert_eq!(example.server.tls, None);
        assert_eq!(example.server.public_host, None);
        // Only those lines differ, and the comments after them stay in line.
        let written = template_with(None);
        let changed: Vec<(&str, &str)> = EXAMPLE
            .lines()
            .zip(written.lines())
            .filter(|(a, b)| a != b)
            .collect();
        assert_eq!(changed.len(), 4, "{changed:#?}");
        for (example, template) in changed {
            assert_eq!(example.find(" # "), template.find(" # "), "{template}");
        }
    }

    #[test]
    fn a_guessed_region_takes_the_place_of_the_commented_out_one() {
        // Amendment C, HC2: a first run writes the timezone's country where the host reads it.
        let dir = tempfile::tempdir().unwrap();
        let written = template_with(Some("Germany"));
        let cfg = Config::parse(&written, dir.path()).unwrap();
        assert_eq!(cfg.server.region.as_deref(), Some("Germany"));
        let base = template_with(None);
        assert_eq!(
            Config::parse(&base, dir.path()).unwrap().server.region,
            None
        );
        let changed: Vec<(&str, &str)> = base
            .lines()
            .zip(written.lines())
            .filter(|(a, b)| a != b)
            .collect();
        assert_eq!(changed.len(), 1, "{changed:#?}");
        // The comment's column, in characters.
        let column = |line: &str| line[..line.find(" # ").unwrap()].chars().count();
        assert_eq!(column(changed[0].0), column(changed[0].1), "{changed:?}");
        // Likewise for a name of many accented letters, whose bytes outnumber its characters.
        let accented = template_with(Some("Éàéèêëîïôöùûüç Åäö"));
        let line = accented.lines().find(|l| l.starts_with("region")).unwrap();
        assert_eq!(column(line), column(changed[0].0), "{line}");
        // A name the shell or TOML would read otherwise stays the name.
        let odd = template_with(Some(r#"Côte d'Ivoire "\" x"#));
        assert_eq!(
            Config::parse(&odd, dir.path())
                .unwrap()
                .server
                .region
                .as_deref(),
            Some(r#"Côte d'Ivoire "\" x"#)
        );
    }

    #[test]
    fn the_servers_region_is_short_plain_text() {
        let parse = |t: &str| Config::parse(t, Path::new("/"));
        let c = parse("[server]\nname = \"M\"\nregion = \"Germany\"\n").unwrap();
        assert_eq!(c.server.region.as_deref(), Some("Germany"));
        assert!(!c.server.region_blank);
        assert_eq!(
            parse("[server]\nname = \"M\"\n").unwrap().server.region,
            None
        );
        let blank = parse("[server]\nname = \"M\"\nregion = \"  \"\n").unwrap();
        assert_eq!(blank.server.region, None, "blank is none");
        assert!(blank.server.region_blank, "and the host's choice");
        assert!(
            !parse("[server]\nname = \"M\"\n")
                .unwrap()
                .server
                .region_blank
        );
        let long = parse(&format!(
            "[server]\nname = \"M\"\nregion = \"{}\"\n",
            "x".repeat(33)
        ))
        .unwrap_err();
        assert!(long.to_string().contains("[server] region"), "{long}");
        assert!(parse("[server]\nname = \"M\"\nregion = \"A\\u0007u\"\n").is_err());
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
    fn the_first_run_lines_name_the_folder_and_say_who_can_join() {
        // Amendment A, HA3. The guide is this version's, on the web: the archive holds no
        // docs/hosting.md (the hosting plan's final review B, Minor 7).
        let lines = first_run_lines(Path::new("/srv/fenji"));
        assert_eq!(
            lines,
            [
                "First run: wrote fenji.toml and data/ in /srv/fenji.".to_owned(),
                "Friends on this network can join now: type `invite` for a link.".to_owned(),
                format!(
                    "To let in friends over the internet, open ports 7800 and 7801 on your \
                     router, or use a VPN: see {}.",
                    guide()
                ),
            ]
        );
    }

    #[test]
    fn the_guide_is_the_one_the_archives_readme_names() {
        let guide = guide();
        assert_eq!(
            guide,
            format!(
                "https://github.com/theintonerzero/fenji/blob/v{}/docs/hosting.md",
                env!("CARGO_PKG_VERSION")
            )
        );
        let readme = include_str!("../../../deploy/archive/README.txt")
            .replace("@VERSION@", env!("CARGO_PKG_VERSION"));
        assert!(readme.contains(&guide), "{readme}");
    }

    #[test]
    fn the_environment_moves_the_binds_and_a_bad_value_names_its_variable() {
        // Ruling HP7: FENJI_LOGIN_BIND and FENJI_API_BIND, over the file's.
        let file = Config::parse("[server]\nname = \"F\"\n", Path::new("/")).unwrap();
        let env = |pairs: &[(&str, &str)]| {
            let pairs: Vec<(String, String)> = pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| k == name)
                    .map(|(_, v)| std::ffi::OsString::from(v))
            }
        };
        let moved = file
            .clone()
            .override_binds(env(&[
                ("FENJI_LOGIN_BIND", "127.0.0.1:0"),
                ("FENJI_API_BIND", "[::1]:9001"),
            ]))
            .unwrap();
        assert_eq!(moved.server.login_bind, "127.0.0.1:0".parse().unwrap());
        assert_eq!(moved.server.api_bind, "[::1]:9001".parse().unwrap());
        let one = file
            .clone()
            .override_binds(env(&[("FENJI_API_BIND", "0.0.0.0:9001")]))
            .unwrap();
        assert_eq!(one.server.login_bind, file.server.login_bind);
        assert_eq!(one.server.api_bind, "0.0.0.0:9001".parse().unwrap());
        let none = file.clone().override_binds(|_| None).unwrap();
        assert_eq!(
            (none.server.login_bind, none.server.api_bind),
            (file.server.login_bind, file.server.api_bind)
        );
        for (name, value) in [
            ("FENJI_LOGIN_BIND", "7800"),
            ("FENJI_LOGIN_BIND", ""),
            ("FENJI_API_BIND", "localhost:7801"),
            ("FENJI_API_BIND", "0.0.0.0"),
        ] {
            let e = file
                .clone()
                .override_binds(env(&[(name, value)]))
                .unwrap_err();
            let e = format!("{e:#}");
            assert!(
                e.contains(name) && e.contains("an address and port"),
                "{name}={value:?}: {e}"
            );
        }
        // They're checked as the file's are: the two can't share an address.
        let e = file
            .override_binds(env(&[("FENJI_LOGIN_BIND", "127.0.0.1:7801")]))
            .unwrap_err();
        assert!(format!("{e:#}").contains("api_bind"), "{e:#}");
    }

    #[cfg(unix)]
    #[test]
    fn a_bind_that_isnt_text_names_its_variable_too() {
        use std::os::unix::ffi::OsStringExt;
        let file = Config::parse("[server]\nname = \"F\"\n", Path::new("/")).unwrap();
        let e = file
            .override_binds(|name| {
                (name == "FENJI_API_BIND").then(|| std::ffi::OsString::from_vec(vec![0xff]))
            })
            .unwrap_err();
        assert!(format!("{e:#}").contains("FENJI_API_BIND"), "{e:#}");
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
    fn max_players_is_at_most_what_a_crowd_can_be_sent_each_tick() {
        // #281: with everyone in one view, each tick sends every player a move of each of the
        // others, and a connection's queue holds 256: past about 250 players, ones reading as fast
        // as they can were let go as slow readers. A config over the cap is refused, as every
        // value out of its range is.
        let parse = |n: u32| {
            let text = format!("[server]\nname = \"F\"\nmax_players = {n}\n");
            Config::parse(&text, Path::new("/"))
        };
        assert_eq!(parse(200).unwrap().server.max_players, 200);
        assert_eq!(parse(1).unwrap().server.max_players, 1);
        for n in [0, 201, 1_000] {
            let e = format!("{:#}", parse(n).unwrap_err());
            assert!(e.contains("[server] max_players: 1 to 200"), "{n}: {e}");
        }
    }

    #[test]
    fn a_name_is_plain_text_as_the_host_and_the_motd_are() {
        // The hosting plan's final review, Minor 7: the name is the most visible host text (the
        // console's title bar, every launcher's list, the game's `gj_uinfo`), so the format
        // characters `host` and `motd` refuse are refused here too: bidirectional overrides and
        // isolates, zero-width characters, the byte order mark and the Hangul fillers.
        for name in [
            "Fen\\u202Eji",
            "Fen\\u2066ji",
            "Fen\\u200Bji",
            "Fen\\uFEFFji",
            "Fen\\u3164ji",
        ] {
            let text = format!("[server]\nname = \"{name}\"\n");
            let e = format!("{:#}", Config::parse(&text, Path::new("/")).unwrap_err());
            assert!(e.contains("[server] name"), "{name}: {e}");
        }
        let c = Config::parse("[server]\nname = \"Moonward é 你好\"\n", Path::new("/")).unwrap();
        assert_eq!(c.server.name, "Moonward é 你好");
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
        assert_eq!((c.server.public_host, c.server.tls), (None, None));
        assert!(c.server.login_bind.ip().is_loopback() && c.server.api_bind.ip().is_loopback());
        assert_eq!((c.server.tick_hz, c.world.start_scene), (20, 5));
        // The start of vocations 0 to 5 in the client's tables, which Task 11's runs walked from
        // (world.md §13).
        assert_eq!(c.world.start_position, [2366.0, -71735.0, 64854.0, 345.0]);
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
    fn the_worlds_start_takes_the_edges_of_its_limits() {
        // Each limit is inclusive: its ends are taken, and just past them is refused.
        let parse = |extra: &str| {
            Config::parse(&format!("[server]\nname = \"F\"\n{extra}"), Path::new("/"))
        };
        let low =
            parse("[world]\nstart_scene = 1\nstart_position = [-1e7, 1e7, -1e7, -360]\n").unwrap();
        assert_eq!(low.world.start_scene, 1);
        assert_eq!(
            low.world.start_position,
            [
                -COORDINATE_LIMIT,
                COORDINATE_LIMIT,
                -COORDINATE_LIMIT,
                -YAW_LIMIT
            ]
        );
        let high = parse("[world]\nstart_scene = 2147483647\nstart_position = [0, 0, 1e7, 360]\n")
            .unwrap();
        assert_eq!(high.world.start_scene, i64::from(i32::MAX));
        assert_eq!(
            high.world.start_position,
            [0.0, 0.0, COORDINATE_LIMIT, YAW_LIMIT]
        );
        for past in [
            "[10000000.5, 0, 0, 0]",
            "[0, -10000000.5, 0, 0]",
            "[0, 0, 0, -360.5]",
        ] {
            let e = parse(&format!("[world]\nstart_position = {past}\n")).unwrap_err();
            assert!(format!("{e:#}").contains("start_position"), "{past}: {e:#}");
        }
    }

    #[test]
    fn the_example_gives_each_range_the_parser_holds() {
        // fenji.example.toml names each range beside its setting, and that is the range
        // `validate` holds: both ends are taken, and just past either is refused.
        let line = |key: &str| {
            EXAMPLE
                .lines()
                .find(|l| l.starts_with(&format!("{key} = ")))
                .unwrap_or_else(|| panic!("no {key} in the example"))
        };
        for (section, key, range, ends) in [
            (
                "server",
                "max_players",
                "1 to 200",
                ["1", "200", "0", "201"],
            ),
            ("server", "tick_hz", "1 to 60", ["1", "60", "0", "61"]),
            (
                "world",
                "move_tolerance",
                "1.0 to 5.0",
                ["1.0", "5.0", "0.99", "5.01"],
            ),
            (
                "world",
                "save_every_secs",
                "30 to 3600",
                ["30", "3600", "29", "3601"],
            ),
        ] {
            assert!(line(key).contains(range), "{key}: {}", line(key));
            for (value, taken) in ends.into_iter().zip([true, true, false, false]) {
                let setting = match section {
                    "server" => format!("{key} = {value}\n"),
                    _ => format!("[{section}]\n{key} = {value}\n"),
                };
                let parsed = Config::parse(
                    &format!("[server]\nname = \"F\"\n{setting}"),
                    Path::new("/"),
                );
                assert_eq!(parsed.is_ok(), taken, "{key} = {value}: {parsed:?}");
            }
        }
    }

    #[test]
    fn the_hosts_name_is_short_plain_text() {
        let parse = |t: &str| Config::parse(t, Path::new("/"));
        let c = parse("[server]\nname = \"Moonward\"\nhost = \"Wen\"\n").unwrap();
        assert_eq!(c.server.host.as_deref(), Some("Wen"));
        assert_eq!(parse("[server]\nname = \"M\"\n").unwrap().server.host, None);
        let long = parse(&format!(
            "[server]\nname = \"M\"\nhost = \"{}\"\n",
            "x".repeat(33)
        ))
        .unwrap_err();
        assert!(long.to_string().contains("[server] host"), "{long}");
        assert!(parse("[server]\nname = \"M\"\nhost = \"W\\u0007en\"\n").is_err());
        assert!(
            parse(&format!(
                "[server]\nname = \"M\"\nhost = \"{}\"\n",
                "é".repeat(32)
            ))
            .is_ok()
        );
    }

    #[test]
    fn the_source_defaults_to_this_versions_tag_and_the_motd_to_none() {
        let c = Config::parse("[server]\nname = \"F\"\n", Path::new("/")).unwrap();
        assert_eq!(
            c.server.source_url,
            format!(
                "https://github.com/theintonerzero/fenji/tree/v{}",
                env!("CARGO_PKG_VERSION")
            )
        );
        assert_eq!(c.server.motd, "");
        let c = Config::parse(
            "[server]\nname = \"F\"\nsource_url = \"http://git.example.net/my-fenji\"\n\
             motd = \"Welcome, é! Events on Fridays.\"\n",
            Path::new("/"),
        )
        .unwrap();
        assert_eq!(c.server.source_url, "http://git.example.net/my-fenji");
        assert_eq!(c.server.motd, "Welcome, é! Events on Fridays.");
    }

    #[test]
    fn a_source_is_a_web_address_and_an_motd_plain_text_each_of_200_characters_at_most() {
        let parse = |extra: &str| {
            Config::parse(
                &format!("[server]\nname = \"F\"\n{extra}\n"),
                Path::new("/"),
            )
        };
        // "https://example.net/" is 20 characters.
        parse(&format!(
            "source_url = \"https://example.net/{}\"",
            "x".repeat(180)
        ))
        .unwrap();
        parse(&format!("motd = \"{}\"", "é".repeat(200))).unwrap();
        let long_source = format!("source_url = \"https://example.net/{}\"", "x".repeat(181));
        let long_motd = format!("motd = \"{}\"", "é".repeat(201));
        for (extra, place) in [
            ("source_url = \"ftp://example.net/fenji\"", "source_url"),
            ("source_url = \"github.com/me/fenji\"", "source_url"),
            ("source_url = \"https://\"", "source_url"),
            (
                "source_url = \"https://example.net/my fenji\"",
                "source_url",
            ),
            (
                "source_url = \"https://example.net/\\u001b[2J\"",
                "source_url",
            ),
            ("source_url = \"https://example.net/\\u202e\"", "source_url"),
            (long_source.as_str(), "source_url"),
            ("motd = \"\\u001b[31mred\"", "motd"),
            ("motd = \"two\\nlines\"", "motd"),
            ("motd = \"back \\u202esoon\"", "motd"),
            (long_motd.as_str(), "motd"),
        ] {
            let e = format!("{:#}", parse(extra).unwrap_err());
            assert!(e.contains(place), "{extra}: {e}");
        }
    }

    /// What `invite_base` gives without a pin: its origin.
    fn origin(c: &Config) -> Option<String> {
        c.invite_base(None).map(|b| b.origin)
    }

    #[test]
    fn invite_links_start_where_players_can_reach() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = |extra: &str| {
            Config::parse(&format!("[server]\nname = \"F\"\n{extra}"), dir.path()).unwrap()
        };
        let at = |o: &str, p: Option<&str>| {
            Some(InviteBase {
                origin: o.into(),
                pin: p.map(Into::into),
            })
        };

        let local = cfg("api_bind = \"127.0.0.1:7801\"\npublic_host = \"127.0.0.1\"\n");
        assert_eq!(local.invite_base(None), at("http://127.0.0.1:7801", None));

        // #174: a public gate behind a loopback API with no public_api has no usable link.
        let proxied = cfg("api_bind = \"127.0.0.1:7801\"\npublic_host = \"203.0.113.5\"\n");
        assert_eq!(proxied.invite_base(None), None);

        let named =
            cfg("public_api = \"https://play.example.net\"\npublic_host = \"203.0.113.5\"\n");
        assert_eq!(
            named.invite_base(None),
            at("https://play.example.net", None)
        );

        let own =
            cfg("tls = \"self\"\napi_bind = \"0.0.0.0:7801\"\npublic_host = \"203.0.113.5\"\n");
        assert_eq!(
            own.invite_base(Some("PIN")),
            at("https://203.0.113.5:7801", Some("PIN"))
        );

        let own_named = cfg(
            "tls = \"self\"\npublic_api = \"https://play.example.net:7801\"\npublic_host = \"203.0.113.5\"\n",
        );
        assert_eq!(
            own_named.invite_base(Some("PIN")),
            at("https://play.example.net:7801", Some("PIN"))
        );
    }

    #[test]
    fn with_its_own_key_every_link_carries_the_pin() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = |extra: &str| {
            Config::parse(&format!("[server]\nname = \"F\"\n{extra}"), dir.path()).unwrap()
        };
        // A launcher can't trust the server's own key without its pin: no link goes without it.
        let own = cfg("tls = \"self\"\napi_bind = \"127.0.0.1:7801\"\n");
        assert_eq!(own.invite_base(None), None);
        let pinned = own.invite_base(Some("PIN")).unwrap();
        assert_eq!(
            (pinned.origin.as_str(), pinned.pin.as_deref()),
            ("https://127.0.0.1:7801", Some("PIN")),
            "a loopback API speaks TLS too"
        );
        let named = cfg("tls = \"self\"\npublic_api = \"https://play.example.net\"\n");
        assert_eq!(named.invite_base(None), None);
        // Without tls the server has no key of its own, and a pin would mean nothing.
        let plain = cfg("api_bind = \"127.0.0.1:7801\"\n");
        let base = plain.invite_base(Some("PIN")).unwrap();
        assert_eq!(
            (base.origin.as_str(), base.pin),
            ("http://127.0.0.1:7801", None)
        );
    }

    #[test]
    fn tls_is_self_or_unset_and_takes_an_https_public_api() {
        let parse = |extra: &str| {
            Config::parse(
                &format!("[server]\nname = \"F\"\n{extra}\n"),
                Path::new("/"),
            )
        };
        assert_eq!(parse("").unwrap().server.tls, None);
        assert_eq!(
            parse("tls = \"self\"").unwrap().server.tls,
            Some(Tls::OwnKey)
        );
        for (extra, place) in [
            ("tls = \"acme\"", "tls"),
            ("tls = true", "tls"),
            (
                "tls = \"self\"\npublic_api = \"http://127.0.0.1:7801\"",
                "public_api",
            ),
        ] {
            let e = format!("{:#}", parse(extra).unwrap_err());
            assert!(e.contains(place), "{extra}: {e}");
        }
    }

    #[test]
    fn public_host_is_optional_and_an_address_players_can_reach() {
        // Amendment A, HA2: unset by default. #174: it names the gate.
        let parse = |host: &str| {
            Config::parse(
                &format!("[server]\nname = \"F\"\npublic_host = \"{host}\"\n"),
                Path::new("/"),
            )
        };
        let c = Config::parse("[server]\nname = \"F\"\n", Path::new("/")).unwrap();
        assert_eq!(c.server.public_host, None);
        for good in ["203.0.113.5", "play.example.net", "localhost", "my-pc.lan"] {
            let c = parse(good).unwrap();
            assert_eq!(c.server.public_host.as_deref(), Some(good));
        }
        for bad in [
            "",
            " ",
            "a:1",
            "play example.net",
            "play/x",
            "play.example.net#x",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.1",
            "127.1",
            "-play.example.net",
            "play..example.net",
            "pläy.example.net",
            "play.example.net\\u001b",
        ] {
            let e = format!("{:#}", parse(bad).unwrap_err());
            assert!(e.contains("public_host"), "{bad:?}: {e}");
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
        assert_eq!(origin(&c).as_deref(), Some("https://play.example.net"));
        let c = parse("https://play.example.net:8443").unwrap();
        assert_eq!(origin(&c).as_deref(), Some("https://play.example.net:8443"));
        let c = parse("http://127.0.0.1:7801").unwrap();
        assert_eq!(origin(&c).as_deref(), Some("http://127.0.0.1:7801"));
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
        assert_eq!(origin(&c).as_deref(), Some("http://127.0.0.1:7801"));
        let c = Config::parse(
            "[server]\nname = \"F\"\napi_bind = \"[::1]:7801\"\nlogin_bind = \"[::1]:7800\"\n",
            base,
        )
        .unwrap();
        assert_eq!(origin(&c).as_deref(), Some("http://[::1]:7801"));
        let c = Config::parse(
            "[server]\nname = \"F\"\napi_bind = \"0.0.0.0:7801\"\n",
            base,
        )
        .unwrap();
        assert_eq!(c.invite_base(None), None);
    }
}
