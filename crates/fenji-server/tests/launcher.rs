//! The launcher's API client against a running server, then the gate with the ticket it got
//! (M3 spec §6; M3 plan Task 7), and what it reads of the server's facts, maintenance and bans
//! (M4 hosting spec §3.8, §3.9 and §8).
mod common;

use std::sync::Arc;

use common::*;
use fenji_launcher::api::{ApiClient, ApiError, MaintenanceInfo, Players, ServerInfo, ServerState};
use fenji_server::server::GateLimits;
use fenji_server::status::Maintenance;
use serde_json::json;

#[tokio::test]
async fn the_launcher_registers_signs_in_and_gets_a_ticket_the_gate_takes() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    let code = fenji_auth::invite::InviteCode::generate().unwrap();
    s.accounts()
        .store()
        .create_invite(&code.hash(), None, NOW, None)
        .await
        .unwrap();
    let api = ApiClient::new(&format!("http://{}", s.api_addr)).unwrap();
    let info = api.server_info().await.unwrap();
    assert_eq!((info.gate_port, info.api_version), (s.login_addr.port(), 1));
    api.register(&code.display(), "tester", "a long password")
        .await
        .unwrap();
    let again = api
        .register(&code.display(), "other", "a long password")
        .await
        .unwrap_err();
    assert!(
        matches!(again, ApiError::Server { status: 400, ref code, .. } if code == "invalid_invite"),
        "{again:?}"
    );
    let wrong = api.sign_in("tester", "wrong password!").await.unwrap_err();
    assert!(
        matches!(wrong, ApiError::Server { status: 401, ref code, .. } if code == "bad_credentials"),
        "{wrong:?}"
    );
    let session = api.sign_in("tester", "a long password").await.unwrap();
    let grant = api.game_token(&session).await.unwrap();
    assert_eq!(
        (
            grant.account.as_str(),
            grant.server_name.as_str(),
            grant.gate_port
        ),
        ("tester", "Fenji", s.login_addr.port())
    );
    // As `fenji-launch play` does: the session ends before the client uses the ticket.
    let held = session.clone();
    api.sign_out(session).await.unwrap();
    let mut bot = Bot::connect(s.login_addr, payload(&grant.ticket, &grant.account, true)).await;
    bot.expect_work().await;
    let gone = api.game_token(&held).await.unwrap_err();
    assert!(
        matches!(gone, ApiError::Server { status: 401, ref code, .. } if code == "bad_session"),
        "{gone:?}"
    );
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_launcher_reads_maintenance_and_the_servers_facts() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    account(&s, "tester").await;
    let api = ApiClient::new(&format!("http://{}", s.api_addr)).unwrap();
    let until = NOW + 1_800_000;
    s.status()
        .set_maintenance(
            s.accounts().store(),
            Some(Maintenance {
                message: "Updating".into(),
                since_ms: NOW,
                until_ms: Some(until),
            }),
        )
        .await
        .unwrap();
    let e = api.sign_in("tester", "a long password").await.unwrap_err();
    assert!(
        matches!(e, ApiError::Maintenance { ref message, until: Some(at) } if message == "Updating" && at == until),
        "{e:?}"
    );
    assert!(
        e.to_string()
            .contains("the server is in maintenance: Updating"),
        "{e}"
    );
    assert_eq!(
        e.to_string(),
        "the server is in maintenance: Updating (expected back around 2027-01-15 08:30 UTC)"
    );
    let info = api.server_info().await.unwrap();
    assert_eq!(info.state, ServerState::Maintenance);
    assert_eq!(info.players, Some(Players { online: 0, max: 20 }));
    assert_eq!(
        info.maintenance,
        Some(MaintenanceInfo {
            message: "Updating".into(),
            since: NOW,
            until: Some(until),
        })
    );
    let source = format!(
        "https://github.com/theintonerzero/fenji/tree/v{}",
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(info.source.as_deref(), Some(source.as_str()));
    assert_eq!(info.motd, "");
    // A v0.4 server's answer has only the old fields: it still parses, as a server that's online.
    let old: ServerInfo = serde_json::from_value(json!({
        "name": "Fenji",
        "version": "0.4.1",
        "pinned_build": "10015727",
        "gate_host": "127.0.0.1",
        "gate_port": 7800,
        "server_id": 1,
        "api_version": 1,
    }))
    .unwrap();
    assert_eq!(
        (
            old.state,
            old.players,
            old.maintenance,
            old.source,
            old.motd
        ),
        (ServerState::Online, None, None, None, String::new())
    );
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_launcher_names_a_ban_with_its_reason_and_lists_who_is_in_the_world() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    account(&s, "tester").await;
    let api = ApiClient::new(&format!("http://{}", s.api_addr)).unwrap();
    let session = api.sign_in("tester", "a long password").await.unwrap();
    assert!(api.players(&session).await.unwrap().is_empty());
    s.status()
        .names_sender()
        .send_replace(Arc::from(vec!["Lin".to_string(), "Mai".to_string()]));
    assert_eq!(api.players(&session).await.unwrap(), ["Lin", "Mai"]);
    let store = s.accounts().store().clone();
    let id = store
        .account_by_username("tester")
        .await
        .unwrap()
        .unwrap()
        .id;
    assert!(store.ban(id, "Griefing", NOW).await.unwrap());
    let e = api.sign_in("tester", "a long password").await.unwrap_err();
    assert!(
        matches!(e, ApiError::Banned { ref reason } if reason == "Griefing"),
        "{e:?}"
    );
    assert_eq!(
        e.to_string(),
        "this server has banned your account: Griefing"
    );
    let e = api.game_token(&session).await.unwrap_err();
    assert!(matches!(e, ApiError::Banned { .. }), "{e:?}");
    let e = api.players(&session).await.unwrap_err();
    assert!(matches!(e, ApiError::Banned { .. }), "{e:?}");
    s.shutdown().await.unwrap();
}
