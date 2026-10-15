//! The account API's client (M3 spec D12, D14). It speaks only https, or http to this machine,
//! never follows a redirect, and never shows a password, session token or ticket in `Debug`. What
//! a server says in its own words (maintenance, a ban's reason, its MOTD, the names in its world)
//! reaches the player with hidden characters shown as escapes (M4 hosting spec §3.8, §3.9, §8).
use std::net::IpAddr;
use std::sync::OnceLock;
use std::time::Duration;

use reqwest::{Method, StatusCode, Url};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

/// How long one request may take.
pub const TIMEOUT: Duration = Duration::from_secs(15);
/// The largest answer read; the API's own are a few hundred bytes.
pub const MAX_ANSWER: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("the server address must start with https:// (http:// only for this machine)")]
    Insecure,
    #[error("that isn't a server address: give https://host or https://host:port")]
    BadUrl,
    #[error("the server answered {code} (HTTP {status}){wait}", wait = wait_hint(.retry_after_secs))]
    Server {
        status: u16,
        code: String,
        /// With `try_later`: how long to wait.
        retry_after_secs: Option<u64>,
    },
    #[error("can't reach the server: {0}")]
    Http(String),
    #[error("the server's answer didn't make sense: {0}")]
    Decode(String),
    /// The server is in maintenance (503): the host's message, and when they expect to be back
    /// (Unix ms), if they said. Only the host's admins can sign in.
    #[error("the server is in maintenance{}{}", words(.message), back(.until))]
    Maintenance { message: String, until: Option<i64> },
    /// The host banned the account (403), for this reason.
    #[error("this server has banned your account{}", words(.reason))]
    Banned { reason: String },
}

/// "; try again in N s" when the server said how long to wait.
fn wait_hint(secs: &Option<u64>) -> String {
    secs.map(|n| format!("; try again in {n} s"))
        .unwrap_or_default()
}

/// ": <text>", or nothing for no text.
fn words(text: &str) -> String {
    if text.is_empty() {
        String::new()
    } else {
        format!(": {text}")
    }
}

/// " (expected back around <date and time> UTC)" when the host said when.
fn back(until: &Option<i64>) -> String {
    until
        .map(|ms| format!(" (expected back around {} UTC)", utc_minute(ms)))
        .unwrap_or_default()
}

/// Unix milliseconds as a date and a time of day in UTC, to the minute: `2027-01-15 08:30`.
fn utc_minute(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm), as the server's clock.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let day = doy - (153 * mp + 2) / 5 + 1;
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        rem / 3600,
        rem % 3600 / 60
    )
}

/// Whether `c` would act on a terminal, or change how the text around it reads: controls (ESC
/// among them), the line and paragraph separators, the bidirectional marks, overrides and
/// isolates, and the zero-width characters.
fn hidden(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061C}'
                | '\u{200B}'..='\u{200F}'
                | '\u{2028}'..='\u{202E}'
                | '\u{2060}'..='\u{2069}'
                | '\u{FEFF}'
        )
}

/// A server's words as the launcher shows them: each [`hidden`] character written as its escape,
/// such as `\u{1b}`, so a hostile server can't put terminal sequences on the player's screen.
fn escape_hidden(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if hidden(c) {
            out.extend(c.escape_unicode());
        } else {
            out.push(c);
        }
    }
    out
}

/// Whether `url` is one a launcher may offer to open: `https://` or `http://`, then something,
/// with no whitespace or hidden character.
fn web_address(url: &str) -> bool {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .is_some_and(|rest| !rest.is_empty())
        && !url.chars().any(|c| c.is_whitespace() || hidden(c))
}

/// What `GET /api/v1/server` says. A server before v0.5 sends only the fields up to
/// `api_version`, and the rest take their defaults: such a server is online.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    pub pinned_build: String,
    pub gate_host: String,
    pub gate_port: u16,
    pub server_id: u32,
    pub api_version: u32,
    /// The players connected, in the lobby or the world, of the server's cap.
    #[serde(default)]
    pub players: Option<Players>,
    #[serde(default)]
    pub state: ServerState,
    /// While the server is in maintenance: the host's message, and its times.
    #[serde(default)]
    pub maintenance: Option<MaintenanceInfo>,
    /// Where the server's source code is, which the AGPL's §13 has the host offer: an `https://`
    /// or `http://` address.
    #[serde(default)]
    pub source: Option<String>,
    /// The host's notice for players; empty for none.
    #[serde(default)]
    pub motd: String,
}

/// How many players are connected, of how many the server takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Players {
    pub online: u32,
    pub max: u32,
}

/// Whether the server takes players.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerState {
    /// It takes players; a server that doesn't say is online.
    #[default]
    Online,
    /// Only the host's admins can sign in.
    Maintenance,
    /// A state this launcher doesn't know.
    #[serde(other)]
    Unknown,
}

/// Maintenance mode, as `GET /api/v1/server` describes it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MaintenanceInfo {
    pub message: String,
    /// Since when, in Unix milliseconds.
    pub since: i64,
    /// When the host expects to be back, in Unix milliseconds, if they said.
    #[serde(default)]
    pub until: Option<i64>,
}

/// A signed-in session. Its `Debug` doesn't show the token.
#[derive(Clone, PartialEq, Eq, Deserialize)]
pub struct Session {
    token: String,
    pub expires_at: i64,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("token", &"…")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// A single-use game token and where to use it. Its `Debug` doesn't show the ticket.
#[derive(Clone, PartialEq, Eq, Deserialize)]
pub struct GameGrant {
    pub ticket: String,
    pub expires_in_secs: u64,
    pub gate_host: String,
    pub gate_port: u16,
    pub server_id: u32,
    pub server_name: String,
    pub account: String,
    pub user_id: i64,
}

impl std::fmt::Debug for GameGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GameGrant")
            .field("ticket", &"…")
            .field("expires_in_secs", &self.expires_in_secs)
            .field("gate_host", &self.gate_host)
            .field("gate_port", &self.gate_port)
            .field("server_id", &self.server_id)
            .field("server_name", &self.server_name)
            .field("account", &self.account)
            .field("user_id", &self.user_id)
            .finish()
    }
}

pub struct ApiClient {
    base: reqwest::Url,
    http: reqwest::Client,
}

/// rustls with the `ring` provider (M3 spec D20), installed once per process.
pub(crate) fn crypto() {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| {
        // Another part of the process may have installed one already; either is fine.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// A loopback host: `localhost`, 127.0.0.0/8 or `::1`.
pub(crate) fn loopback(url: &Url) -> bool {
    match url.host_str() {
        Some("localhost") => true,
        Some(h) => h
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback()),
        None => false,
    }
}

impl ApiClient {
    /// A client for the server at `server` (`https://host[:port]`, or `http://` on this machine).
    pub fn new(server: &str) -> Result<Self, ApiError> {
        let base = Url::parse(server).map_err(|_| ApiError::BadUrl)?;
        match base.scheme() {
            "https" => {}
            "http" if loopback(&base) => {}
            "http" => return Err(ApiError::Insecure),
            _ => return Err(ApiError::BadUrl),
        }
        if base.host_str().is_none() {
            return Err(ApiError::BadUrl);
        }
        crypto();
        let mut builder = reqwest::Client::builder()
            .timeout(TIMEOUT)
            // A redirect could carry a password to another place, or to http://.
            .redirect(reqwest::redirect::Policy::none());
        if loopback(&base) {
            // Plain http stays on this machine: never through a proxy from the environment.
            builder = builder.no_proxy();
        }
        let http = builder.build().map_err(describe)?;
        Ok(Self { base, http })
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        bearer: Option<&str>,
        body: Option<Value>,
    ) -> Result<Option<T>, ApiError> {
        let url = self.base.join(path).map_err(|_| ApiError::BadUrl)?;
        let mut req = self.http.request(method, url);
        if let Some(t) = bearer {
            req = req.bearer_auth(t);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        let mut res = req.send().await.map_err(describe)?;
        let status = res.status();
        let mut bytes = Vec::new();
        while let Some(chunk) = res.chunk().await.map_err(describe)? {
            if bytes.len() + chunk.len() > MAX_ANSWER {
                return Err(ApiError::Decode("the server's answer is too large".into()));
            }
            bytes.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            return Err(server_error(status.as_u16(), &bytes));
        }
        if status == StatusCode::NO_CONTENT || bytes.is_empty() {
            return Ok(None);
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| ApiError::Decode(e.to_string()))
    }

    async fn expect<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        bearer: Option<&str>,
        body: Option<Value>,
    ) -> Result<T, ApiError> {
        self.call(method, path, bearer, body)
            .await?
            .ok_or_else(|| ApiError::Decode("an empty answer".into()))
    }

    /// Refused unless the gate is a plain host name or address (`ServerInfo::check`). The host's
    /// words come with their hidden characters as escapes, and the source only if it's a web
    /// address (`ServerInfo::shown`).
    pub async fn server_info(&self) -> Result<ServerInfo, ApiError> {
        let info: ServerInfo = self
            .expect(Method::GET, "/api/v1/server", None, None)
            .await?;
        info.check()?;
        Ok(info.shown())
    }

    /// The names of the characters in the server's world, for a signed-in session.
    pub async fn players(&self, session: &Session) -> Result<Vec<String>, ApiError> {
        #[derive(Deserialize)]
        struct List {
            players: Vec<Named>,
        }
        #[derive(Deserialize)]
        struct Named {
            name: String,
        }
        let list: List = self
            .expect(Method::GET, "/api/v1/players", Some(&session.token), None)
            .await?;
        Ok(list
            .players
            .iter()
            .map(|p| escape_hidden(&p.name))
            .collect())
    }

    /// Creates an account with an invite code.
    pub async fn register(
        &self,
        invite: &str,
        username: &str,
        password: &str,
    ) -> Result<(), ApiError> {
        let body = json!({"invite": invite, "username": username, "password": password});
        self.call::<Value>(Method::POST, "/api/v1/accounts", None, Some(body))
            .await
            .map(|_| ())
    }

    pub async fn sign_in(&self, username: &str, password: &str) -> Result<Session, ApiError> {
        let body = json!({"username": username, "password": password});
        self.expect(Method::POST, "/api/v1/sessions", None, Some(body))
            .await
    }

    /// Ends the session; the token is spent either way.
    pub async fn sign_out(&self, session: Session) -> Result<(), ApiError> {
        self.call::<Value>(
            Method::DELETE,
            "/api/v1/sessions",
            Some(&session.token),
            None,
        )
        .await
        .map(|_| ())
    }

    /// A single-use game token, valid for about a minute.
    /// Refused unless every field is one the login path can pass on unchanged (`GameGrant::check`).
    pub async fn game_token(&self, session: &Session) -> Result<GameGrant, ApiError> {
        let grant: GameGrant = self
            .expect(
                Method::POST,
                "/api/v1/game-tokens",
                Some(&session.token),
                None,
            )
            .await?;
        grant.check()?;
        Ok(grant)
    }
}

/// A network error with its causes ("error sending request: client error (Connect): tcp connect
/// error: Connection refused"), without the URL, so nothing a user typed is shown.
fn describe(e: reqwest::Error) -> ApiError {
    let e = e.without_url();
    let mut text = e.to_string();
    let mut cause = std::error::Error::source(&e);
    while let Some(c) = cause {
        text.push_str(": ");
        text.push_str(&c.to_string());
        cause = c.source();
    }
    ApiError::Http(text)
}

/// A host name or IPv4 address as a gate needs it: letters, digits, dots and hyphens. Nothing
/// else, so a server's value can't carry terminal sequences into an error message.
fn plain_host(h: &str) -> bool {
    (1..=253).contains(&h.len())
        && h.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
}

impl ServerInfo {
    /// The gate is a plain host name or address, and the server's name holds no control character.
    pub fn check(&self) -> Result<(), ApiError> {
        if !plain_host(&self.gate_host) || self.name.chars().any(char::is_control) {
            return Err(ApiError::Decode(
                "the server's info names a gate the client can't use".into(),
            ));
        }
        Ok(())
    }

    /// The facts with the host's words ready to show: the MOTD and the maintenance message with
    /// their hidden characters as escapes, and a source only if it's a web address.
    fn shown(mut self) -> Self {
        self.motd = escape_hidden(&self.motd);
        if let Some(m) = &mut self.maintenance {
            m.message = escape_hidden(&m.message);
        }
        self.source = self.source.filter(|url| web_address(url));
        self
    }
}

impl GameGrant {
    /// The ticket is `fenji1-` and 64 hex digits, and the names hold no colon (the client splits
    /// `gj_uinfo` on colons) and no control character: a server can't bend the login fields.
    pub fn check(&self) -> Result<(), ApiError> {
        let ticket_ok = self
            .ticket
            .strip_prefix("fenji1-")
            .is_some_and(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()));
        let plain = |s: &str| !s.is_empty() && !s.chars().any(|c| c == ':' || c.is_control());
        if !ticket_ok
            || !plain(&self.server_name)
            || !plain(&self.account)
            || !plain_host(&self.gate_host)
        {
            return Err(ApiError::Decode(
                "the game token's fields aren't ones the client can take".into(),
            ));
        }
        Ok(())
    }
}

/// The error an answer's status and body describe: maintenance and a ban with the host's words
/// ([`escape_hidden`]); otherwise the body's `error` code when it is a plain code (`[a-z0-9_]`, up
/// to 64), so a hostile server can't put terminal sequences on the screen, or else `unknown`.
fn server_error(status: u16, body: &[u8]) -> ApiError {
    let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let text = |key: &str| escape_hidden(v[key].as_str().unwrap_or_default());
    match v["error"].as_str() {
        Some("maintenance") => {
            return ApiError::Maintenance {
                message: text("message"),
                until: v["until"].as_i64(),
            };
        }
        Some("banned") => {
            return ApiError::Banned {
                reason: text("reason"),
            };
        }
        _ => {}
    }
    let code = v["error"]
        .as_str()
        .filter(|c| {
            (1..=64).contains(&c.len())
                && c.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
        .unwrap_or("unknown");
    ApiError::Server {
        status,
        code: code.to_string(),
        retry_after_secs: v["retry_after_secs"].as_u64(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_plain_http_off_loopback() {
        assert!(ApiClient::new("http://127.0.0.1:7801").is_ok());
        assert!(ApiClient::new("http://127.9.9.9:7801/").is_ok());
        assert!(ApiClient::new("http://localhost:7801").is_ok());
        assert!(ApiClient::new("http://[::1]:7801").is_ok());
        assert!(matches!(
            ApiClient::new("http://play.example.net"),
            Err(ApiError::Insecure)
        ));
        assert!(matches!(
            ApiClient::new("http://10.0.0.1:7801"),
            Err(ApiError::Insecure)
        ));
        assert!(ApiClient::new("https://play.example.net").is_ok());
        assert!(ApiClient::new("ftp://play.example.net").is_err());
        assert!(ApiClient::new("play.example.net").is_err());
    }

    #[test]
    fn error_bodies_become_codes() {
        let e = server_error(429, br#"{"error":"try_later","retry_after_secs":7}"#);
        assert!(matches!(
            e,
            ApiError::Server { status: 429, ref code, retry_after_secs: Some(7) } if code == "try_later"
        ));
        let e = server_error(401, br#"{"error":"bad_credentials"}"#);
        assert!(matches!(
            e,
            ApiError::Server { status: 401, ref code, retry_after_secs: None } if code == "bad_credentials"
        ));
        let e = server_error(502, b"<html>Bad Gateway</html>");
        assert!(matches!(e, ApiError::Server { status: 502, ref code, .. } if code == "unknown"));
    }

    #[test]
    fn maintenance_and_bans_carry_the_hosts_words() {
        let e = server_error(
            503,
            br#"{"error":"maintenance","message":"Updating","until":1800001800000}"#,
        );
        assert!(
            matches!(e, ApiError::Maintenance { ref message, until: Some(1_800_001_800_000) } if message == "Updating"),
            "{e:?}"
        );
        assert_eq!(
            e.to_string(),
            "the server is in maintenance: Updating (expected back around 2027-01-15 08:30 UTC)"
        );
        let e = server_error(
            503,
            br#"{"error":"maintenance","message":"Back soon","until":null}"#,
        );
        assert_eq!(e.to_string(), "the server is in maintenance: Back soon");
        let e = server_error(403, br#"{"error":"banned","reason":"Griefing"}"#);
        assert!(
            matches!(e, ApiError::Banned { ref reason } if reason == "Griefing"),
            "{e:?}"
        );
        assert_eq!(
            e.to_string(),
            "this server has banned your account: Griefing"
        );
        let e = server_error(403, br#"{"error":"banned","reason":""}"#);
        assert_eq!(e.to_string(), "this server has banned your account");
        // A proxy's own 503 is no maintenance.
        let e = server_error(503, b"<html>Service Unavailable</html>");
        assert!(matches!(e, ApiError::Server { status: 503, ref code, .. } if code == "unknown"));
    }

    #[test]
    fn a_hostile_servers_words_cant_act_on_the_terminal() {
        let body = json!({
            "error": "maintenance",
            "message": "\u{1b}]0;owned\u{7} \u{202e}evil",
            "until": null,
        });
        let e = server_error(503, body.to_string().as_bytes());
        let text = e.to_string();
        assert!(
            !text.chars().any(|c| c.is_control() || c == '\u{202E}'),
            "{text:?}"
        );
        assert_eq!(
            text,
            "the server is in maintenance: \\u{1b}]0;owned\\u{7} \\u{202e}evil"
        );
        let e = server_error(403, br#"{"error":"banned","reason":"a\nb"}"#);
        assert_eq!(
            e.to_string(),
            "this server has banned your account: a\\u{a}b"
        );
    }

    /// A server's facts as `GET /api/v1/server` gives them, with `extra` fields.
    fn info_with(extra: Value) -> ServerInfo {
        let mut v = json!({
            "name": "Fenji",
            "version": "0.5.0",
            "pinned_build": "10015727",
            "gate_host": "127.0.0.1",
            "gate_port": 7800,
            "server_id": 1,
            "api_version": 1,
        });
        for (k, x) in extra.as_object().unwrap() {
            v[k] = x.clone();
        }
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn the_facts_read_a_state_this_launcher_doesnt_know_as_unknown() {
        let info = info_with(json!({"state": "starting"}));
        assert_eq!(info.state, ServerState::Unknown);
        let info = info_with(json!({
            "players": {"online": 3, "max": 20},
            "state": "maintenance",
            "maintenance": {"message": "Updating", "since": 5, "until": null},
            "source": "https://example.net/fenji",
            "motd": "Welcome",
        }));
        assert_eq!(info.players, Some(Players { online: 3, max: 20 }));
        assert_eq!(info.state, ServerState::Maintenance);
        assert_eq!(
            info.maintenance,
            Some(MaintenanceInfo {
                message: "Updating".into(),
                since: 5,
                until: None
            })
        );
        assert_eq!(
            (info.source.as_deref(), info.motd.as_str()),
            (Some("https://example.net/fenji"), "Welcome")
        );
    }

    #[test]
    fn the_hosts_words_in_the_facts_are_shown_safely_and_a_source_is_a_web_address() {
        let info = info_with(json!({
            "maintenance": {"message": "\u{1b}[2J", "since": 5, "until": null},
            "source": "javascript:alert(1)",
            "motd": "Hi\u{202e}",
        }))
        .shown();
        assert_eq!(info.motd, "Hi\\u{202e}");
        assert_eq!(info.maintenance.unwrap().message, "\\u{1b}[2J");
        assert_eq!(info.source, None);
        for bad in ["https://", "https://a b", "https://a\u{1b}b", "ftp://a"] {
            let info = info_with(json!({ "source": bad })).shown();
            assert_eq!(info.source, None, "{bad:?}");
        }
        let info = info_with(json!({"source": "http://git.example.net/fenji"})).shown();
        assert_eq!(info.source.as_deref(), Some("http://git.example.net/fenji"));
    }

    #[test]
    fn times_show_as_minutes_in_utc() {
        assert_eq!(utc_minute(0), "1970-01-01 00:00");
        assert_eq!(utc_minute(1_800_000_000_000), "2027-01-15 08:00");
        assert_eq!(utc_minute(-1), "1969-12-31 23:59");
        // Whatever a server sends, without overflowing.
        for ms in [i64::MAX, i64::MIN] {
            assert!(!utc_minute(ms).is_empty());
        }
    }

    #[test]
    fn a_rate_limited_player_is_told_how_long_to_wait() {
        let e = server_error(429, br#"{"error":"try_later","retry_after_secs":7}"#);
        assert!(e.to_string().contains("try again in 7 s"), "{e}");
        let e = server_error(401, br#"{"error":"bad_credentials"}"#);
        assert!(!e.to_string().contains("try again"), "{e}");
    }

    fn grant() -> GameGrant {
        GameGrant {
            ticket: format!("fenji1-{}", "0a".repeat(32)),
            expires_in_secs: 60,
            gate_host: "play.example.net".into(),
            gate_port: 7800,
            server_id: 1,
            server_name: "Fenji".into(),
            account: "lin_yue".into(),
            user_id: 7,
        }
    }

    #[test]
    fn a_grant_takes_only_a_well_formed_ticket_and_plain_names() {
        assert!(grant().check().is_ok());
        let bad: Vec<(&str, GameGrant)> = vec![
            (
                "no prefix",
                GameGrant {
                    ticket: "0a".repeat(32),
                    ..grant()
                },
            ),
            (
                "upper-case prefix",
                GameGrant {
                    ticket: format!("FENJI1-{}", "0a".repeat(32)),
                    ..grant()
                },
            ),
            (
                "63 digits",
                GameGrant {
                    ticket: format!("fenji1-{}", "a".repeat(63)),
                    ..grant()
                },
            ),
            (
                "65 digits",
                GameGrant {
                    ticket: format!("fenji1-{}", "a".repeat(65)),
                    ..grant()
                },
            ),
            (
                "not hex",
                GameGrant {
                    ticket: format!("fenji1-{}", "g".repeat(64)),
                    ..grant()
                },
            ),
            (
                "empty account",
                GameGrant {
                    account: String::new(),
                    ..grant()
                },
            ),
            (
                "colon in account",
                GameGrant {
                    account: "a:b".into(),
                    ..grant()
                },
            ),
            (
                "control in account",
                GameGrant {
                    account: "a\u{1b}[2J".into(),
                    ..grant()
                },
            ),
            (
                "colon in server name",
                GameGrant {
                    server_name: "x:y".into(),
                    ..grant()
                },
            ),
            (
                "control in server name",
                GameGrant {
                    server_name: "x\ny".into(),
                    ..grant()
                },
            ),
            (
                "gate with a space",
                GameGrant {
                    gate_host: "play example".into(),
                    ..grant()
                },
            ),
            (
                "gate with a port",
                GameGrant {
                    gate_host: "play.example.net:7800".into(),
                    ..grant()
                },
            ),
        ];
        for (what, g) in bad {
            assert!(matches!(g.check(), Err(ApiError::Decode(_))), "{what}");
        }
    }

    #[test]
    fn grants_never_show_their_secrets() {
        let s = Session {
            token: "fenji1-secret".into(),
            expires_at: 9,
        };
        assert!(!format!("{s:?}").contains("secret"));
        let g = GameGrant {
            ticket: "fenji1-secret".into(),
            expires_in_secs: 60,
            gate_host: "h".into(),
            gate_port: 1,
            server_id: 1,
            server_name: "F".into(),
            account: "a".into(),
            user_id: 1,
        };
        assert!(!format!("{g:?}").contains("secret"));
    }
}
