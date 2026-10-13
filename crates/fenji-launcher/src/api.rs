//! The account API's client (M3 spec D12, D14). It speaks only https, or http to this machine,
//! never follows a redirect, and never shows a password, session token or ticket in `Debug`.
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
}

/// "; try again in N s" when the server said how long to wait.
fn wait_hint(secs: &Option<u64>) -> String {
    secs.map(|n| format!("; try again in {n} s"))
        .unwrap_or_default()
}

/// What `GET /api/v1/server` says.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    pub pinned_build: String,
    pub gate_host: String,
    pub gate_port: u16,
    pub server_id: u32,
    pub api_version: u32,
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
fn crypto() {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| {
        // Another part of the process may have installed one already; either is fine.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// A loopback host: `localhost`, 127.0.0.0/8 or `::1`.
fn loopback(url: &Url) -> bool {
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

    /// Refused unless the gate is a plain host name or address (`ServerInfo::check`).
    pub async fn server_info(&self) -> Result<ServerInfo, ApiError> {
        let info: ServerInfo = self
            .expect(Method::GET, "/api/v1/server", None, None)
            .await?;
        info.check()?;
        Ok(info)
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

/// The error an answer's status and body describe: the body's `error` code when it is a plain
/// code (`[a-z0-9_]`, up to 64), so a hostile server can't put terminal sequences on the screen;
/// otherwise `unknown`.
fn server_error(status: u16, body: &[u8]) -> ApiError {
    let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
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
