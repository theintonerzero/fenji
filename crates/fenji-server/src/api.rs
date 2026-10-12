//! The account API (M3 spec D12): JSON over HTTP at `/api/v1`. TLS comes from Caddy in front of it
//! (parent spec §8). Every error from a route is `{"error": <code>}`, with one code for every failed
//! sign-in; only the time limit's 408 has no body. Nothing here logs a password, token or invite
//! code.
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::rejection::JsonRejection;
use axum::extract::{ConnectInfo, DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower_http::timeout::TimeoutLayer;

use crate::accounts::{Accounts, AccountsError};

#[derive(Clone)]
pub struct AppState {
    pub accounts: Arc<Accounts>,
    pub info: ServerInfo,
    pub trusted_proxies: Vec<IpAddr>,
}

/// What `GET /api/v1/server` and the game-token reply say about this server.
#[derive(Debug, Clone, Serialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    pub pinned_build: String,
    pub gate_host: String,
    pub gate_port: u16,
    pub server_id: u32,
}

/// The API's version in `GET /api/v1/server`.
pub const API_VERSION: u32 = 1;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/accounts", post(register))
        .route("/api/v1/sessions", post(sign_in).delete(sign_out))
        .route("/api/v1/game-tokens", post(game_token))
        .route("/api/v1/server", get(server_info))
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
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Result<Json<RegisterBody>, JsonRejection>,
) -> Response {
    let b = match body {
        Ok(Json(b)) => b,
        Err(r) => return bad_body(r),
    };
    let addr = client_addr(peer, &headers, &s.trusted_proxies);
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
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Result<Json<SignInBody>, JsonRejection>,
) -> Response {
    let b = match body {
        Ok(Json(b)) => b,
        Err(r) => return bad_body(r),
    };
    let addr = client_addr(peer, &headers, &s.trusted_proxies);
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

async fn game_token(State(s): State<AppState>, headers: HeaderMap) -> Response {
    let Some(token) = bearer(&headers) else {
        return fail(AccountsError::BadSession);
    };
    match s.accounts.issue_game_token(token).await {
        Ok(g) => Json(json!({
            "ticket": g.ticket,
            "expires_in_secs": g.expires_in_secs,
            "gate_host": s.info.gate_host,
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

async fn server_info(State(s): State<AppState>) -> Response {
    let mut v = serde_json::to_value(&s.info).unwrap_or_default();
    v["api_version"] = json!(API_VERSION);
    Json(v).into_response()
}
