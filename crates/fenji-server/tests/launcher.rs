//! The launcher's API client against a running server, then the gate with the ticket it got
//! (M3 spec §6; M3 plan Task 7).
mod common;

use common::*;
use fenji_launcher::api::{ApiClient, ApiError};
use fenji_server::server::GateLimits;

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
