//! The account API (M3 spec D12), driven in process.
use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, StatusCode};
use fenji_auth::invite::InviteCode;
use fenji_server::accounts::Accounts;
use fenji_server::api::{AppState, ServerInfo, router};
use fenji_server::clock::TestClock;
use fenji_server::config::AuthConfig;
use fenji_store::Store;
use serde_json::{Value, json};
use tower::ServiceExt;

struct Api {
    _dir: tempfile::TempDir,
    app: axum::Router,
    invite: String,
}

async fn api_with(trusted: Vec<std::net::IpAddr>, peer: &str) -> Api {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("f.db")).await.unwrap();
    let code = InviteCode::generate().unwrap();
    store
        .create_invite(&code.hash(), None, 1, None)
        .await
        .unwrap();
    let accounts = Arc::new(Accounts::new(
        store,
        AuthConfig::default(),
        Arc::new(TestClock::at(1_800_000_000_000)),
    ));
    let info = ServerInfo {
        name: "Fenji".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        pinned_build: "10015727".into(),
        gate_host: "127.0.0.1".into(),
        gate_port: 7800,
        server_id: 1,
    };
    let app = router(AppState {
        accounts,
        info,
        trusted_proxies: trusted,
    })
    .layer(MockConnectInfo(peer.parse::<SocketAddr>().unwrap()));
    Api {
        _dir: dir,
        app,
        invite: code.display(),
    }
}

async fn api() -> Api {
    api_with(Vec::new(), "10.0.0.1:5000").await
}

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    bearer: Option<&str>,
    body: Option<Value>,
    xff: Option<&str>,
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = bearer {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    if let Some(x) = xff {
        req = req.header("x-forwarded-for", x);
    }
    let req = match body {
        Some(b) => req
            .header("content-type", "application/json")
            .body(Body::from(b.to_string()))
            .unwrap(),
        None => req.body(Body::empty()).unwrap(),
    };
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn reg(invite: &str, user: &str, pw: &str) -> Value {
    json!({"invite": invite, "username": user, "password": pw})
}

fn creds(user: &str, pw: &str) -> Value {
    json!({"username": user, "password": pw})
}

async fn register_and_sign_in(a: &Api) -> String {
    let (s, v) = call(
        &a.app,
        "POST",
        "/api/v1/accounts",
        None,
        Some(reg(&a.invite, "tester", "a long password")),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v["username"], "tester");
    let (s, v) = call(
        &a.app,
        "POST",
        "/api/v1/sessions",
        None,
        Some(creds("tester", "a long password")),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v["expires_at"].as_i64().is_some());
    v["token"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn the_whole_flow_gives_a_game_token_with_the_gate() {
    let a = api().await;
    let token = register_and_sign_in(&a).await;
    let (s, v) = call(
        &a.app,
        "POST",
        "/api/v1/game-tokens",
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v["ticket"].as_str().unwrap().starts_with("fenji1-"));
    assert_eq!(v["expires_in_secs"], 60);
    assert_eq!(v["gate_host"], "127.0.0.1");
    assert_eq!(v["gate_port"], 7800);
    assert_eq!(v["server_id"], 1);
    assert_eq!(v["server_name"], "Fenji");
    assert_eq!(v["account"], "tester");
    assert!(v["user_id"].as_i64().unwrap() > 0);
}

#[tokio::test]
async fn bad_requests_get_stable_error_codes() {
    let a = api().await;
    let (s, v) = call(
        &a.app,
        "POST",
        "/api/v1/accounts",
        None,
        Some(reg("0000-0000-0000-0000", "tester", "a long password")),
        None,
    )
    .await;
    assert_eq!(
        (s, v["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_invite"))
    );
    let (_, v) = call(
        &a.app,
        "POST",
        "/api/v1/accounts",
        None,
        Some(reg(&a.invite, "a:b", "a long password")),
        None,
    )
    .await;
    assert_eq!(v["error"], "bad_username");
    let (_, v) = call(
        &a.app,
        "POST",
        "/api/v1/accounts",
        None,
        Some(reg(&a.invite, "tester", "short")),
        None,
    )
    .await;
    assert_eq!(v["error"], "bad_password");
    let (s, v) = call(
        &a.app,
        "POST",
        "/api/v1/sessions",
        None,
        Some(creds("nobody", "whatever123")),
        None,
    )
    .await;
    assert_eq!(
        (s, v["error"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("bad_credentials"))
    );
    let (s, v) = call(
        &a.app,
        "POST",
        "/api/v1/game-tokens",
        Some("fenji1-00"),
        None,
        None,
    )
    .await;
    assert_eq!(
        (s, v["error"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("bad_session"))
    );
    let (s, _) = call(&a.app, "POST", "/api/v1/game-tokens", None, None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_taken_username_is_reported_with_a_valid_invite_only() {
    let a = api().await;
    register_and_sign_in(&a).await;
    let code = InviteCode::generate().unwrap();
    // A second invite isn't in this store, so it is invalid: the taken name stays unrevealed.
    let (_, v) = call(
        &a.app,
        "POST",
        "/api/v1/accounts",
        None,
        Some(reg(&code.display(), "tester", "a long password")),
        None,
    )
    .await;
    assert_eq!(v["error"], "invalid_invite");
}

#[tokio::test]
async fn sign_out_ends_the_session() {
    let a = api().await;
    let token = register_and_sign_in(&a).await;
    let (s, _) = call(
        &a.app,
        "DELETE",
        "/api/v1/sessions",
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = call(
        &a.app,
        "POST",
        "/api/v1/game-tokens",
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_limiter_answers_429_with_a_retry_time() {
    let a = api().await;
    register_and_sign_in(&a).await;
    for _ in 0..5 {
        call(
            &a.app,
            "POST",
            "/api/v1/sessions",
            None,
            Some(creds("tester", "wrong password")),
            None,
        )
        .await;
    }
    let (s, v) = call(
        &a.app,
        "POST",
        "/api/v1/sessions",
        None,
        Some(creds("tester", "a long password")),
        None,
    )
    .await;
    assert_eq!(
        (s, v["error"].as_str()),
        (StatusCode::TOO_MANY_REQUESTS, Some("try_later"))
    );
    assert!(v["retry_after_secs"].as_u64().unwrap() >= 1);
}

#[tokio::test]
async fn forwarded_for_counts_only_from_a_trusted_proxy() {
    // Behind a trusted proxy, each X-Forwarded-For address is limited on its own.
    let a = api_with(vec!["127.0.0.1".parse().unwrap()], "127.0.0.1:4000").await;
    register_and_sign_in(&a).await;
    for _ in 0..5 {
        call(
            &a.app,
            "POST",
            "/api/v1/sessions",
            None,
            Some(creds("tester", "wrong password")),
            Some("203.0.113.9"),
        )
        .await;
    }
    let (s, _) = call(
        &a.app,
        "POST",
        "/api/v1/sessions",
        None,
        Some(creds("tester", "a long password")),
        Some("198.51.100.1, 203.0.113.10"),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "another forwarded address isn't locked");
    let (s, _) = call(
        &a.app,
        "POST",
        "/api/v1/sessions",
        None,
        Some(creds("tester", "a long password")),
        Some("203.0.113.10, 203.0.113.9"),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::TOO_MANY_REQUESTS,
        "the last hop is the client"
    );
    // From an untrusted peer, the header is ignored: every attempt counts against the peer.
    let b = api().await;
    register_and_sign_in(&b).await;
    for i in 0..5 {
        call(
            &b.app,
            "POST",
            "/api/v1/sessions",
            None,
            Some(creds("tester", "wrong password")),
            Some(&format!("198.51.100.{i}")),
        )
        .await;
    }
    let (s, _) = call(
        &b.app,
        "POST",
        "/api/v1/sessions",
        None,
        Some(creds("tester", "a long password")),
        Some("198.51.100.99"),
    )
    .await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn a_bad_last_hop_falls_back_to_the_peer() {
    let a = api_with(vec!["127.0.0.1".parse().unwrap()], "127.0.0.1:4000").await;
    register_and_sign_in(&a).await;
    for _ in 0..5 {
        call(
            &a.app,
            "POST",
            "/api/v1/sessions",
            None,
            Some(creds("tester", "wrong password")),
            Some("203.0.113.9, garbage"),
        )
        .await;
    }
    // Those counted against the peer (127.0.0.1), not against 203.0.113.9.
    let (s, _) = call(
        &a.app,
        "POST",
        "/api/v1/sessions",
        None,
        Some(creds("tester", "a long password")),
        Some("203.0.113.9"),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn oversized_bodies_are_refused() {
    let a = api().await;
    let big = json!({"username": "x".repeat(20_000), "password": "y"});
    let (s, _) = call(&a.app, "POST", "/api/v1/sessions", None, Some(big), None).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn server_info_names_the_build() {
    let a = api().await;
    let (s, v) = call(&a.app, "GET", "/api/v1/server", None, None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["name"], "Fenji");
    assert_eq!(v["pinned_build"], "10015727");
    assert_eq!(v["api_version"], 1);
    assert_eq!(v["gate_port"], 7800);
}
