//! The account API (M3 spec D12; M4 hosting spec §3.8, §3.9, §8), driven in process.
mod common;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, StatusCode};
use common::NOW;
use fenji_auth::invite::InviteCode;
use fenji_server::accounts::Accounts;
use fenji_server::api::{AppState, ServerInfo, router};
use fenji_server::clock::TestClock;
use fenji_server::config::AuthConfig;
use fenji_server::online::Online;
use fenji_server::server::{GateLimits, RunningServer};
use fenji_server::status::{Maintenance, Status};
use fenji_store::Store;
use serde_json::{Value, json};
use tower::ServiceExt;

struct Api {
    _dir: tempfile::TempDir,
    app: axum::Router,
    invite: String,
    /// A second open invite in the same store.
    invite2: String,
}

async fn api_with(trusted: Vec<std::net::IpAddr>, peer: &str) -> Api {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("f.db")).await.unwrap();
    let code = InviteCode::generate().unwrap();
    let code2 = InviteCode::generate().unwrap();
    for c in [&code, &code2] {
        store.create_invite(&c.hash(), None, 1, None).await.unwrap();
    }
    let status = Status::load(&store, NOW, "").await.unwrap();
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
        source: "https://example.net/fenji".into(),
    };
    let app = router(AppState {
        accounts,
        info,
        trusted_proxies: trusted,
        status,
        online: Arc::new(Online::new(20)),
    })
    .layer(MockConnectInfo(peer.parse::<SocketAddr>().unwrap()));
    Api {
        _dir: dir,
        app,
        invite: code.display(),
        invite2: code2.display(),
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
    // An invite that isn't in this store is invalid: the taken name stays unrevealed.
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
    // With a valid invite the name is reported taken, and the invite stays open.
    let (s, v) = call(
        &a.app,
        "POST",
        "/api/v1/accounts",
        None,
        Some(reg(&a.invite2, "TESTER", "a long password")),
        None,
    )
    .await;
    assert_eq!(
        (s, &v["error"]),
        (StatusCode::BAD_REQUEST, &json!("username_taken"))
    );
    let (s, v) = call(
        &a.app,
        "POST",
        "/api/v1/accounts",
        None,
        Some(reg(&a.invite2, "second", "a long password")),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
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
async fn a_trusted_proxy_is_recognised_as_an_ipv4_mapped_peer() {
    // A dual-stack listener sees a local IPv4 proxy as ::ffff:127.0.0.1.
    for (trusted, peer) in [
        ("127.0.0.1", "[::ffff:127.0.0.1]:4000"),
        ("::ffff:127.0.0.1", "127.0.0.1:4000"),
    ] {
        let a = api_with(vec![trusted.parse().unwrap()], peer).await;
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
            Some("198.51.100.1"),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{trusted} vs {peer}");
    }
}

#[tokio::test]
async fn bad_bodies_get_json_errors() {
    let a = api().await;
    let raw = |body: &'static str, content_type: Option<&'static str>| {
        let mut req = Request::builder().method("POST").uri("/api/v1/sessions");
        if let Some(ct) = content_type {
            req = req.header("content-type", ct);
        }
        req.body(Body::from(body)).unwrap()
    };
    for (req, status) in [
        (
            raw("not json", Some("application/json")),
            StatusCode::BAD_REQUEST,
        ),
        (
            raw("{\"username\": \"x\"}", Some("application/json")),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (raw("{}", None), StatusCode::UNSUPPORTED_MEDIA_TYPE),
    ] {
        let res = a.app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), status);
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "bad_request", "{status}");
    }
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
    let (s, v) = call(&a.app, "POST", "/api/v1/sessions", None, Some(big), None).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(v["error"], "too_large");
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

// --- The server's facts, maintenance, who is online, the MOTD, ops and bans (M4 hosting spec
// §3.8, §3.9 and §8; Task 3).

/// A server started on port 0 in a temp folder. Its account API is asked in process, through a
/// router over the server's own state, from a peer at 10.0.0.1.
struct Started {
    _dir: tempfile::TempDir,
    server: RunningServer,
    store: Store,
    app: axum::Router,
}

impl Started {
    fn status(&self) -> Arc<Status> {
        self.server.status()
    }

    fn store(&self) -> &Store {
        &self.store
    }

    async fn stop(self) {
        self.server.shutdown().await.unwrap();
    }
}

/// A server with `extra` after its `[server]` keys (`common::config`).
async fn started_with(extra: &str) -> Started {
    let dir = tempfile::tempdir().unwrap();
    let (server, _clock) = common::server(dir.path(), extra, GateLimits::default()).await;
    let store = server.accounts().store().clone();
    let app = router(server.api_state()).layer(MockConnectInfo(
        "10.0.0.1:5000".parse::<SocketAddr>().unwrap(),
    ));
    Started {
        _dir: dir,
        server,
        store,
        app,
    }
}

async fn started() -> Started {
    started_with("").await
}

/// A server whose `[access].admins` are `admins`.
async fn started_with_admins(admins: &[&str]) -> Started {
    let list: Vec<String> = admins.iter().map(|a| format!("\"{a}\"")).collect();
    started_with(&format!("[access]\nadmins = [{}]\n", list.join(", "))).await
}

/// An account that has signed in.
struct SignedIn {
    session: String,
}

/// The status and JSON body of one request to `s`.
async fn ask(
    s: &Started,
    method: &str,
    uri: &str,
    bearer: Option<&str>,
    body: Option<Value>,
) -> (u16, Value) {
    let (status, v) = call(&s.app, method, uri, bearer, body, None).await;
    (status.as_u16(), v)
}

/// A new open invite's code.
async fn invite(s: &Started) -> String {
    let code = InviteCode::generate().unwrap();
    s.store
        .create_invite(&code.hash(), None, NOW, None)
        .await
        .unwrap();
    code.display()
}

async fn invite_is_unused(s: &Started, code: &str) -> bool {
    let code = InviteCode::parse(code).unwrap();
    s.store.invite_is_open(&code.hash(), NOW).await.unwrap()
}

async fn register(s: &Started, invite: &str, username: &str) -> (u16, Value) {
    let body = reg(invite, username, "a long password");
    ask(s, "POST", "/api/v1/accounts", None, Some(body)).await
}

async fn sign_in_as(s: &Started, username: &str, password: &str) -> (u16, Value) {
    let body = creds(username, password);
    ask(s, "POST", "/api/v1/sessions", None, Some(body)).await
}

async fn sign_in(s: &Started, username: &str) -> (u16, Value) {
    sign_in_as(s, username, "a long password").await
}

async fn game_token(s: &Started, session: &str) -> (u16, Value) {
    ask(s, "POST", "/api/v1/game-tokens", Some(session), None).await
}

/// A new account, through a new invite, signed in.
async fn account(s: &Started, username: &str) -> SignedIn {
    let code = invite(s).await;
    let (status, v) = register(s, &code, username).await;
    assert_eq!(status, 201, "{v}");
    let (status, v) = sign_in(s, username).await;
    assert_eq!(status, 200, "{v}");
    SignedIn {
        session: v["token"].as_str().unwrap().to_owned(),
    }
}

async fn account_id(s: &Started, username: &str) -> i64 {
    s.store
        .account_by_username(username)
        .await
        .unwrap()
        .unwrap()
        .id
}

async fn get_status(s: &Started, uri: &str, session: Option<&str>) -> u16 {
    ask(s, "GET", uri, session, None).await.0
}

async fn get_json(s: &Started, uri: &str) -> Value {
    let (status, v) = ask(s, "GET", uri, None, None).await;
    assert_eq!(status, 200, "{v}");
    v
}

async fn get_json_as(s: &Started, uri: &str, session: &str) -> Value {
    let (status, v) = ask(s, "GET", uri, Some(session), None).await;
    assert_eq!(status, 200, "{v}");
    v
}

#[tokio::test]
async fn the_server_facts_carry_players_state_and_source() {
    let s = started().await; // the file's helper: a server on port 0
    let info: Value = get_json(&s, "/api/v1/server").await;
    assert_eq!(info["players"], json!({"online": 0, "max": 20}));
    assert_eq!(info["state"], "online");
    assert_eq!(info["maintenance"], Value::Null);
    assert_eq!(
        info["source"],
        format!(
            "https://github.com/theintonerzero/fenji/tree/v{}",
            env!("CARGO_PKG_VERSION")
        )
    );
    s.stop().await;
}

#[tokio::test]
async fn maintenance_holds_every_way_in_but_lets_admins_check() {
    // Review Focus 4.
    let s = started_with_admins(&["host"]).await;
    let player = account(&s, "player").await; // signed in before maintenance
    let host = account(&s, "host").await;
    let unused = invite(&s).await;
    s.status()
        .set_maintenance(
            s.store(),
            Some(Maintenance {
                message: "Updating".into(),
                since_ms: NOW,
                until_ms: Some(NOW + 1_800_000),
            }),
        )
        .await
        .unwrap();

    let refused = json!({"error": "maintenance", "message": "Updating", "until": NOW + 1_800_000});
    assert_eq!(
        register(&s, &unused, "newcomer").await,
        (503, refused.clone())
    );
    assert!(
        invite_is_unused(&s, &unused).await,
        "the invite isn't spent"
    );
    assert_eq!(sign_in(&s, "player").await, (503, refused.clone()));
    assert_eq!(
        game_token(&s, &player.session).await,
        (503, refused.clone())
    );
    assert_eq!(sign_in(&s, "host").await.0, 200);
    assert_eq!(game_token(&s, &host.session).await.0, 200);
    // A request can't claim to be an admin: the exemption is the signed-in account's.
    assert_eq!(sign_in_as(&s, "HOST", "wrong password").await.0, 401);

    let info: Value = get_json(&s, "/api/v1/server").await;
    assert_eq!(info["state"], "maintenance");
    assert_eq!(
        info["maintenance"],
        json!({"message": "Updating", "since": NOW, "until": NOW + 1_800_000})
    );
    s.stop().await;
}

#[tokio::test]
async fn maintenance_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("fenji.db")).await.unwrap();
    let status = Status::load(&store, NOW, "").await.unwrap();
    let m = Maintenance {
        message: "Back soon".into(),
        since_ms: NOW,
        until_ms: None,
    };
    status
        .set_maintenance(&store, Some(m.clone()))
        .await
        .unwrap();
    drop(status);
    assert_eq!(
        Status::load(&store, NOW + 5, "")
            .await
            .unwrap()
            .maintenance(),
        Some(m)
    );
}

#[tokio::test]
async fn who_is_online_is_for_signed_in_players_only() {
    let s = started().await;
    let player = account(&s, "player").await;
    s.status()
        .names_sender()
        .send_replace(Arc::from(vec!["Lin".to_string(), "Mai".to_string()]));
    assert_eq!(get_status(&s, "/api/v1/players", None).await, 401);
    assert_eq!(
        get_status(&s, "/api/v1/players", Some("fenji1-00")).await,
        401
    );
    let list: Value = get_json_as(&s, "/api/v1/players", &player.session).await;
    assert_eq!(list, json!({"players": [{"name": "Lin"}, {"name": "Mai"}]}));
    s.stop().await;
}

#[tokio::test]
async fn the_motd_comes_from_the_file_until_the_database_has_one() {
    // Amendment A, HA1.
    let none = started().await;
    assert_eq!(get_json(&none, "/api/v1/server").await["motd"], "");
    none.stop().await;

    let s = started_with("motd = \"Welcome to Fenji!\"\n").await;
    let motd = |v: Value| v["motd"].clone();
    assert_eq!(
        motd(get_json(&s, "/api/v1/server").await),
        "Welcome to Fenji!"
    );
    s.status()
        .set_motd(s.store(), Some("Boss fight at 20:00".into()))
        .await
        .unwrap();
    assert_eq!(
        motd(get_json(&s, "/api/v1/server").await),
        "Boss fight at 20:00"
    );
    // Kept over the file's across a restart.
    let restarted = Status::load(s.store(), NOW, "Welcome to Fenji!")
        .await
        .unwrap();
    assert_eq!(restarted.motd(), "Boss fight at 20:00");
    // An empty one is no MOTD, still over the file's; clearing it gives the file's back.
    s.status()
        .set_motd(s.store(), Some(String::new()))
        .await
        .unwrap();
    assert_eq!(motd(get_json(&s, "/api/v1/server").await), "");
    s.status().set_motd(s.store(), None).await.unwrap();
    assert_eq!(
        motd(get_json(&s, "/api/v1/server").await),
        "Welcome to Fenji!"
    );
    s.stop().await;
}

#[tokio::test]
async fn an_op_in_the_database_is_exempt_from_maintenance_as_an_admin_is() {
    // Amendment A, HA5: an admin is in [access].admins or in ops, and this server names none.
    let s = started().await;
    let opped = account(&s, "opped").await;
    let player = account(&s, "player").await;
    let id = account_id(&s, "opped").await;
    assert!(s.store().op(id).await.unwrap());
    s.status()
        .set_maintenance(
            s.store(),
            Some(Maintenance {
                message: "Updating".into(),
                since_ms: NOW,
                until_ms: None,
            }),
        )
        .await
        .unwrap();
    let refused = json!({"error": "maintenance", "message": "Updating", "until": null});
    assert_eq!(sign_in(&s, "OPPED").await.0, 200);
    assert_eq!(game_token(&s, &opped.session).await.0, 200);
    assert_eq!(sign_in(&s, "player").await, (503, refused.clone()));
    assert_eq!(
        game_token(&s, &player.session).await,
        (503, refused.clone())
    );
    // Without op, the door holds for them too.
    assert!(s.store().deop(id).await.unwrap());
    assert_eq!(sign_in(&s, "opped").await, (503, refused.clone()));
    assert_eq!(game_token(&s, &opped.session).await, (503, refused));
    s.stop().await;
}

#[tokio::test]
async fn a_banned_account_hears_why_on_every_route_after_its_password() {
    // Amendment A, HA5.
    let s = started().await;
    let banned = account(&s, "griefer").await;
    let id = account_id(&s, "griefer").await;
    assert!(
        s.store()
            .ban(id, "Griefing at the spawn", NOW)
            .await
            .unwrap()
    );
    let refused = json!({"error": "banned", "reason": "Griefing at the spawn"});
    assert_eq!(sign_in(&s, "griefer").await, (403, refused.clone()));
    assert_eq!(
        game_token(&s, &banned.session).await,
        (403, refused.clone())
    );
    assert_eq!(
        ask(&s, "GET", "/api/v1/players", Some(&banned.session), None).await,
        (403, refused.clone())
    );
    // The ban shows only to someone with the password.
    assert_eq!(
        sign_in_as(&s, "griefer", "wrong password").await,
        (401, json!({"error": "bad_credentials"}))
    );
    // It comes before maintenance.
    let m = Maintenance {
        message: "Updating".into(),
        since_ms: NOW,
        until_ms: None,
    };
    s.status()
        .set_maintenance(s.store(), Some(m))
        .await
        .unwrap();
    assert_eq!(sign_in(&s, "griefer").await, (403, refused.clone()));
    assert_eq!(game_token(&s, &banned.session).await, (403, refused));
    s.status().set_maintenance(s.store(), None).await.unwrap();
    assert_eq!(get_json(&s, "/api/v1/server").await["state"], "online");
    // A pardon lets them back in.
    assert!(s.store().pardon(id).await.unwrap());
    assert_eq!(sign_in(&s, "griefer").await.0, 200);
    assert_eq!(game_token(&s, &banned.session).await.0, 200);
    s.stop().await;
}
