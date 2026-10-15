//! The account API (M3 spec D12; M4 hosting spec §3.8–§3.10, §8): JSON over HTTP at `/api/v1`.
//! TLS comes from Caddy in front of it (parent spec §8), or from the server itself with
//! `tls = "self"` (`crate::tls`). Every error from a route is `{"error": <code>}`, with one code
//! for every failed sign-in, and the refusals for maintenance and for a ban carry the host's
//! words; only the time limit's 408 has no body. Nothing here logs a password, token or invite
//! code.
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::connect_info::Connected;
use axum::extract::rejection::JsonRejection;
use axum::extract::{ConnectInfo, DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::serve::IncomingStream;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tower_http::timeout::TimeoutLayer;

use crate::accounts::{Accounts, AccountsError};
use crate::listen::PlainListener;
use crate::online::Online;
use crate::status::Status;
use crate::tls::TlsListener;

/// A connection's two ends, which the routes extract as `ConnectInfo<Addrs>`: the peer's address,
/// which the limits count, and the address it came in on, which names the gate when
/// `public_host` is unset (Amendment A, HA2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Addrs {
    pub peer: SocketAddr,
    pub local: SocketAddr,
}

impl Connected<IncomingStream<'_, PlainListener>> for Addrs {
    fn connect_info(stream: IncomingStream<'_, PlainListener>) -> Self {
        Addrs {
            peer: *stream.remote_addr(),
            local: stream.io().local_addr(),
        }
    }
}

impl Connected<IncomingStream<'_, TlsListener>> for Addrs {
    fn connect_info(stream: IncomingStream<'_, TlsListener>) -> Self {
        Addrs {
            peer: *stream.remote_addr(),
            local: stream.io().local_addr(),
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub accounts: Arc<Accounts>,
    pub info: ServerInfo,
    pub trusted_proxies: Vec<IpAddr>,
    /// Maintenance, the MOTD, and who is in the world.
    pub status: Arc<Status>,
    /// The accounts connected, in the lobby or the world: the gate's.
    pub online: Arc<Online>,
}

/// What `GET /api/v1/server` and the game-token reply say about this server that doesn't change
/// while it runs.
#[derive(Debug, Clone, Serialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    pub pinned_build: String,
    /// `[server] public_host`, the gate for everyone; unset, each request's own ([`gate_host`]).
    #[serde(skip)]
    pub gate_host: Option<String>,
    pub gate_port: u16,
    pub server_id: u32,
    /// Where the running server's source code is (`[server] source_url`), which the AGPL's §13
    /// asks a host to offer its players.
    pub source: String,
}

/// The API's version in `GET /api/v1/server`.
pub const API_VERSION: u32 = 1;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/accounts", post(register))
        .route("/api/v1/sessions", post(sign_in).delete(sign_out))
        .route("/api/v1/game-tokens", post(game_token))
        .route("/api/v1/server", get(server_info))
        .route("/api/v1/players", get(players))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(10),
        ))
        .with_state(state)
}

/// The client's address: the peer's, or, when the peer is a trusted proxy, the last entry of
/// `X-Forwarded-For` (the one that proxy added). An entry that doesn't parse gives the peer's.
/// Addresses compare in canonical form, since a dual-stack listener sees an IPv4 proxy as
/// `::ffff:a.b.c.d`.
fn client_addr(peer: SocketAddr, headers: &HeaderMap, trusted: &[IpAddr]) -> IpAddr {
    let peer = peer.ip().to_canonical();
    if !trusted.iter().any(|t| t.to_canonical() == peer) {
        return peer;
    }
    headers
        .get_all("x-forwarded-for")
        .iter()
        .next_back()
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit(',').next())
        .and_then(|last| last.trim().parse::<IpAddr>().ok())
        .unwrap_or(peer)
}

/// The gate's address for a request (Amendment A, HA2): `public_host` when it's set; otherwise the
/// host the request's `Host` names, if that's an IPv4 address players can reach or a host name
/// (`config::reachable_host`), as players play on the address they typed; otherwise the IPv4
/// address the request came in on, `local`. A request over IPv6 loopback gets 127.0.0.1. One over
/// IPv6 otherwise has no IPv4 address to name, and gets its IPv6 one, which launchers refuse: the
/// game client takes IPv4 only.
fn gate_host(public_host: Option<&str>, headers: &HeaderMap, local: SocketAddr) -> String {
    if let Some(host) = public_host {
        return host.to_owned();
    }
    if let Some(host) = typed_host(headers) {
        return host;
    }
    match local.ip().to_canonical() {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) if v6.is_loopback() => Ipv4Addr::LOCALHOST.to_string(),
        IpAddr::V6(v6) => v6.to_string(),
    }
}

/// The host a request's `Host` header names, lower-cased and without its port or a final dot, if
/// it's an IPv4 address players can reach or a host name: never an IPv6 literal, which the game
/// can't use.
fn typed_host(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::HOST)?.to_str().ok()?;
    let host = match value.split_once(':') {
        Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => host,
        Some(_) => return None,
        None => value,
    };
    let host = host.strip_suffix('.').unwrap_or(host).to_ascii_lowercase();
    crate::config::reachable_host(&host).then_some(host)
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

/// The JSON error answer for an account error.
fn fail(e: AccountsError) -> Response {
    let (status, code) = match &e {
        AccountsError::InvalidInvite => (StatusCode::BAD_REQUEST, "invalid_invite"),
        AccountsError::UsernameTaken => (StatusCode::BAD_REQUEST, "username_taken"),
        AccountsError::Rule(fenji_auth::rules::RuleError::PasswordLength) => {
            (StatusCode::BAD_REQUEST, "bad_password")
        }
        AccountsError::Rule(_) => (StatusCode::BAD_REQUEST, "bad_username"),
        AccountsError::BadCredentials => (StatusCode::UNAUTHORIZED, "bad_credentials"),
        AccountsError::BadSession => (StatusCode::UNAUTHORIZED, "bad_session"),
        AccountsError::Maintenance(m) => {
            let body = json!({"error": "maintenance", "message": m.message, "until": m.until_ms});
            return (StatusCode::SERVICE_UNAVAILABLE, Json(body)).into_response();
        }
        AccountsError::Banned(reason) => {
            let body = json!({"error": "banned", "reason": reason});
            return (StatusCode::FORBIDDEN, Json(body)).into_response();
        }
        AccountsError::TryLater(after) => {
            let secs = after.as_secs() + u64::from(after.subsec_nanos() > 0);
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({"error": "try_later", "retry_after_secs": secs.max(1)})),
            )
                .into_response();
        }
        AccountsError::Internal(detail) => {
            tracing::error!(%detail, "account API");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal")
        }
    };
    (status, Json(json!({ "error": code }))).into_response()
}

/// A body that isn't the route's JSON: the rejection's own status, with a JSON error.
fn bad_body(r: JsonRejection) -> Response {
    let status = r.status();
    let code = if status == StatusCode::PAYLOAD_TOO_LARGE {
        "too_large"
    } else {
        "bad_request"
    };
    (status, Json(json!({ "error": code }))).into_response()
}

#[derive(Deserialize)]
struct RegisterBody {
    invite: String,
    username: String,
    password: String,
}

async fn register(
    State(s): State<AppState>,
    ConnectInfo(conn): ConnectInfo<Addrs>,
    headers: HeaderMap,
    body: Result<Json<RegisterBody>, JsonRejection>,
) -> Response {
    let b = match body {
        Ok(Json(b)) => b,
        Err(r) => return bad_body(r),
    };
    let addr = client_addr(conn.peer, &headers, &s.trusted_proxies);
    match s
        .accounts
        .register(&b.invite, &b.username, &b.password, addr)
        .await
    {
        Ok(_) => (StatusCode::CREATED, Json(json!({ "username": b.username }))).into_response(),
        Err(e) => fail(e),
    }
}

#[derive(Deserialize)]
struct SignInBody {
    username: String,
    password: String,
}

async fn sign_in(
    State(s): State<AppState>,
    ConnectInfo(conn): ConnectInfo<Addrs>,
    headers: HeaderMap,
    body: Result<Json<SignInBody>, JsonRejection>,
) -> Response {
    let b = match body {
        Ok(Json(b)) => b,
        Err(r) => return bad_body(r),
    };
    let addr = client_addr(conn.peer, &headers, &s.trusted_proxies);
    match s.accounts.sign_in(&b.username, &b.password, addr).await {
        Ok(g) => Json(json!({ "token": g.token, "expires_at": g.expires_at })).into_response(),
        Err(e) => fail(e),
    }
}

async fn sign_out(State(s): State<AppState>, headers: HeaderMap) -> Response {
    let Some(token) = bearer(&headers) else {
        return fail(AccountsError::BadSession);
    };
    match s.accounts.sign_out(token).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => fail(e),
    }
}

async fn game_token(
    State(s): State<AppState>,
    ConnectInfo(conn): ConnectInfo<Addrs>,
    headers: HeaderMap,
) -> Response {
    let Some(token) = bearer(&headers) else {
        return fail(AccountsError::BadSession);
    };
    match s.accounts.issue_game_token(token).await {
        Ok(g) => Json(json!({
            "ticket": g.ticket,
            "expires_in_secs": g.expires_in_secs,
            "gate_host": gate_host(s.info.gate_host.as_deref(), &headers, conn.local),
            "gate_port": s.info.gate_port,
            "server_id": s.info.server_id,
            "server_name": s.info.name,
            "account": g.username,
            "user_id": g.account_id,
        }))
        .into_response(),
        Err(e) => fail(e),
    }
}

/// The server's facts (M4 hosting spec §3.8): what doesn't change while it runs, with the gate
/// this request names (HA2), then the players connected of its cap, its state, the maintenance
/// while it's on (times in Unix ms), and its MOTD. A server that doesn't answer is offline: the
/// launcher concludes that itself.
async fn server_info(
    State(s): State<AppState>,
    ConnectInfo(conn): ConnectInfo<Addrs>,
    headers: HeaderMap,
) -> Response {
    let mut v = serde_json::to_value(&s.info).unwrap_or_default();
    v["gate_host"] = json!(gate_host(s.info.gate_host.as_deref(), &headers, conn.local));
    v["api_version"] = json!(API_VERSION);
    v["players"] = json!({"online": s.online.count(), "max": s.online.max()});
    let maintenance = s.status.maintenance();
    v["state"] = json!(if maintenance.is_some() {
        "maintenance"
    } else {
        "online"
    });
    v["maintenance"] = maintenance.map_or(
        Value::Null,
        |m| json!({"message": m.message, "since": m.since_ms, "until": m.until_ms}),
    );
    v["motd"] = json!(s.status.motd());
    Json(v).into_response()
}

/// The characters in the world, by name (M4 hosting spec §3.8): for a signed-in account only, and
/// not for a banned one.
async fn players(State(s): State<AppState>, headers: HeaderMap) -> Response {
    let Some(token) = bearer(&headers) else {
        return fail(AccountsError::BadSession);
    };
    let account = match s.accounts.session_account(token).await {
        Ok(account) => account,
        Err(e) => return fail(e),
    };
    if let Err(e) = s.accounts.check_ban(account).await {
        return fail(e);
    }
    let names: Vec<Value> = s
        .status
        .names()
        .iter()
        .map(|name| json!({ "name": name }))
        .collect();
    Json(json!({ "players": names })).into_response()
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn with_host(host: Option<&[u8]>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(h) = host {
            headers.insert(header::HOST, HeaderValue::from_bytes(h).unwrap());
        }
        headers
    }

    #[test]
    fn the_gate_is_the_host_a_player_typed_or_the_address_they_came_in_on() {
        // Amendment A, HA2.
        let lan: SocketAddr = "192.168.1.10:7801".parse().unwrap();
        for (host, gate) in [
            (Some(&b"203.0.113.5:7801"[..]), "203.0.113.5"),
            (Some(b"203.0.113.5"), "203.0.113.5"),
            (Some(b"Play.Example.NET:7801"), "play.example.net"),
            (Some(b"play.example.net."), "play.example.net"),
            (Some(b"localhost:7801"), "localhost"),
            (Some(b"my-pc"), "my-pc"),
            // Neither an IPv4 address nor a name: the address the request came in on.
            (Some(b"[2001:db8::1]:7801"), "192.168.1.10"),
            (Some(b"[::1]"), "192.168.1.10"),
            (Some(b"0.0.0.0:7801"), "192.168.1.10"),
            (Some(b"224.0.0.1"), "192.168.1.10"),
            (Some(b"127.1"), "192.168.1.10"),
            (Some(b"play_example.net"), "192.168.1.10"),
            (Some(b"play..example.net"), "192.168.1.10"),
            (Some(b"-play.example.net"), "192.168.1.10"),
            (Some(b"play.example.net:x"), "192.168.1.10"),
            (Some(b"play.example.net:7801:1"), "192.168.1.10"),
            (Some(b"user@play.example.net"), "192.168.1.10"),
            (Some(b""), "192.168.1.10"),
            (Some(b"pl\xc3\xa4y.example.net"), "192.168.1.10"),
            (None, "192.168.1.10"),
        ] {
            assert_eq!(
                gate_host(None, &with_host(host), lan),
                gate,
                "{:?}",
                host.map(String::from_utf8_lossy)
            );
        }
        // public_host names it for everyone.
        let typed = with_host(Some(b"play.example.net"));
        assert_eq!(gate_host(Some("203.0.113.5"), &typed, lan), "203.0.113.5");
        // A dual-stack listener sees an IPv4 client's address as IPv6-mapped.
        let mapped: SocketAddr = "[::ffff:192.168.1.10]:7801".parse().unwrap();
        assert_eq!(gate_host(None, &with_host(None), mapped), "192.168.1.10");
        // Over IPv6 loopback, this computer's IPv4 loopback; over IPv6 otherwise there's no IPv4
        // address to name, and launchers refuse the IPv6 one, which the game can't use.
        let v6_local: SocketAddr = "[::1]:7801".parse().unwrap();
        assert_eq!(
            gate_host(None, &with_host(Some(b"[::1]:7801")), v6_local),
            "127.0.0.1"
        );
        let v6: SocketAddr = "[2001:db8::7]:7801".parse().unwrap();
        assert_eq!(gate_host(None, &with_host(None), v6), "2001:db8::7");
    }
}
