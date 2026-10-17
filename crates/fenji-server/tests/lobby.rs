//! The gate and the lobby through a real socket (M3 spec §6; M3 plan Task 6).
mod common;

use std::time::Duration;

use common::*;
use fenji_proto::lobby::{self, code};
use fenji_proto::pickle::{Table, Value};
use fenji_server::gate::{
    AUTH_FAILED, CLIENT_EXIT, DUPLICATE_LOGIN, PRE_AUTH_CAP, SERVER_ERROR, SERVER_FULL,
};
use fenji_server::server::GateLimits;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::test]
async fn login_list_create_restart_list_delete() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    account(&s, "tester").await;
    let t = ticket(&s, "tester").await;
    let mut bot = Bot::connect(s.login_addr, payload(&t, "tester", false)).await;
    bot.expect_work().await;
    bot.send(lobby::SERVER_LIST, &[]).await;
    bot.send(lobby::HOTPATCH_INIT, &[Value::Int(0)]).await;
    bot.send(lobby::GM_SCRIPTS, &[]).await;
    bot.send(lobby::QUERY_ACTORS, &[]).await;
    let (id, args) = bot.recv().await;
    assert_eq!(id, lobby::ROLE_LIST);
    assert_eq!(args[1], Value::Table(Table::default()));
    bot.send(lobby::CREATE_ACTOR, &request("Lin", 0, 2)).await;
    let (id, args) = bot.recv().await;
    assert_eq!(id, lobby::ROLE_CREATE);
    let Value::Table(actor) = &args[0] else {
        panic!("{args:?}")
    };
    let pid = field(actor, "pid").cloned().unwrap();
    assert_eq!(field(actor, "slot"), Some(&Value::Int(1)));
    s.shutdown().await.unwrap();

    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    let t = ticket(&s, "tester").await;
    let mut bot = Bot::connect(s.login_addr, payload(&t, "tester", true)).await;
    bot.expect_work().await;
    bot.send(lobby::QUERY_ACTORS, &[]).await;
    let (_, args) = bot.recv().await;
    let Value::Table(list) = &args[1] else {
        panic!("{args:?}")
    };
    assert_eq!(list.array.len(), 1, "the character survived the restart");
    let Value::Table(first) = &list.array[0] else {
        panic!()
    };
    assert_eq!(
        field(first, "prop_base_name"),
        Some(&Value::Str(b"Lin".to_vec()))
    );
    bot.send(lobby::DELETE_ACTOR, std::slice::from_ref(&pid))
        .await;
    let (id, args) = bot.recv().await;
    assert_eq!((id, &args[0]), (lobby::ROLE_DELETE, &pid));
    bot.send(lobby::QUERY_ACTORS, &[]).await;
    let (_, args) = bot.recv().await;
    assert_eq!(args[1], Value::Table(Table::default()));
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_ticket_works_once_and_must_match_its_account() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    account(&s, "tester").await;
    account(&s, "other").await;
    let t = ticket(&s, "tester").await;
    let mut bot = Bot::connect(s.login_addr, payload(&t, "TESTER", false)).await;
    bot.expect_work().await; // the account's case doesn't matter
    let mut again = Bot::connect(s.login_addr, payload(&t, "tester", false)).await;
    assert_eq!(again.expect_rejected().await, AUTH_FAILED.as_bytes());
    let t2 = ticket(&s, "tester").await;
    let mut wrong = Bot::connect(s.login_addr, payload(&t2, "other", false)).await;
    assert_eq!(wrong.expect_rejected().await, AUTH_FAILED.as_bytes());
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn expired_mocked_and_garbage_tickets_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (s, clock) = server(dir.path(), "", GateLimits::default()).await;
    account(&s, "tester").await;
    let t = ticket(&s, "tester").await;
    clock.advance(60_000);
    let mut bot = Bot::connect(s.login_addr, payload(&t, "tester", false)).await;
    assert_eq!(bot.expect_rejected().await, AUTH_FAILED.as_bytes());
    // The mocked SDK's ticket form (payloads.md P.6), in a default build.
    let mocked = "7b226163636f756e745f646973706c61795f6e616d65223a226d6f636b6564227d";
    let mut bot = Bot::connect(s.login_addr, payload(mocked, "account-name", false)).await;
    assert_eq!(bot.expect_rejected().await, AUTH_FAILED.as_bytes());
    let mut bot = Bot::connect(s.login_addr, vec![0xFF, 0, 0, 9, 1, 2, 3]).await;
    assert_eq!(bot.expect_rejected().await, AUTH_FAILED.as_bytes());
    let mut bot = Bot::connect(s.login_addr, vec![0x01, 0x03, 0x07]).await;
    assert_eq!(bot.expect_rejected().await, AUTH_FAILED.as_bytes());
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn refusals_come_back_as_270() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(
        dir.path(),
        "[lobby]\nmax_characters = 1\n",
        GateLimits::default(),
    )
    .await;
    account(&s, "tester").await;
    let t = ticket(&s, "tester").await;
    let mut bot = Bot::connect(s.login_addr, payload(&t, "tester", false)).await;
    bot.expect_work().await;
    bot.send(lobby::CREATE_ACTOR, &request("", 0, 2)).await;
    let refused = |c: i64| (lobby::ERROR_CODE, vec![Value::Int(c)]);
    assert_eq!(bot.recv().await, refused(code::ROLE_NAME_LENERR));
    bot.send(lobby::CREATE_ACTOR, &request("Lin", 0, 2)).await;
    assert_eq!(bot.recv().await.0, lobby::ROLE_CREATE);
    bot.send(lobby::CREATE_ACTOR, &request("Two", 0, 2)).await;
    assert_eq!(bot.recv().await, refused(code::ROLE_MAX_NUMBER));
    bot.send(lobby::DELETE_ACTOR, &[Value::Int(999_999)]).await;
    assert_eq!(bot.recv().await, refused(code::ROLE_NOT_FOUND));
    // 10 enters the world, but only as one of the account's own characters (world_link.rs).
    bot.send(lobby::SELECT_ACTOR, &[Value::Int(999_999)]).await;
    assert_eq!(bot.recv().await, refused(code::ROLE_NOT_FOUND));
    bot.send(lobby::CHANGE_SLOT, &[Value::Int(1), Value::Int(1)])
        .await; // ignored
    bot.send(12_345, &[]).await; // unknown ids are ignored, and the connection stays
    bot.quiet(300).await;
    bot.send(lobby::QUERY_ACTORS, &[]).await;
    assert_eq!(bot.recv().await.0, lobby::ROLE_LIST);
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_taken_name_is_refused_for_everyone() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    account(&s, "aaa").await;
    account(&s, "bbb").await;
    let t = ticket(&s, "aaa").await;
    let mut a = Bot::connect(s.login_addr, payload(&t, "aaa", false)).await;
    a.expect_work().await;
    a.send(lobby::CREATE_ACTOR, &request("Lin", 0, 2)).await;
    assert_eq!(a.recv().await.0, lobby::ROLE_CREATE);
    let t = ticket(&s, "bbb").await;
    let mut b = Bot::connect(s.login_addr, payload(&t, "bbb", false)).await;
    b.expect_work().await;
    b.send(lobby::CREATE_ACTOR, &request("LIN", 0, 2)).await;
    assert_eq!(
        b.recv().await,
        (lobby::ERROR_CODE, vec![Value::Int(code::ROLE_NAME_EXIST)])
    );
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn oversized_appearance_is_refused_and_server_survives() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    account(&s, "tester").await;
    let t = ticket(&s, "tester").await;
    let mut bot = Bot::connect(s.login_addr, payload(&t, "tester", false)).await;
    bot.expect_work().await;
    let mut req = request("Lin", 0, 2);
    if let Value::Table(t) = &mut req[0] {
        t.hash[5].1 = Value::Str(vec![b'x'; 100_000]); // the face string
    }
    bot.send(lobby::CREATE_ACTOR, &req).await;
    assert_eq!(bot.recv().await.0, lobby::ERROR_CODE);
    bot.send(lobby::QUERY_ACTORS, &[]).await;
    assert_eq!(bot.recv().await.0, lobby::ROLE_LIST);
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_player_cap_and_a_second_login() {
    let dir = tempfile::tempdir().unwrap();
    let limits = GateLimits {
        max_players: 1,
        ..GateLimits::default()
    };
    let (s, _c) = server(dir.path(), "", limits).await;
    account(&s, "aaa").await;
    account(&s, "bbb").await;
    let t = ticket(&s, "aaa").await;
    let mut a = Bot::connect(s.login_addr, payload(&t, "aaa", false)).await;
    a.expect_work().await;
    let t = ticket(&s, "bbb").await;
    let mut b = Bot::connect(s.login_addr, payload(&t, "bbb", false)).await;
    assert_eq!(b.expect_rejected().await, SERVER_FULL.as_bytes());
    let t = ticket(&s, "aaa").await;
    let mut a2 = Bot::connect(s.login_addr, payload(&t, "aaa", false)).await;
    a2.expect_work().await;
    assert_eq!(a.expect_closed().await, DUPLICATE_LOGIN.as_bytes());
    // The replaced connection's end doesn't free the newer one's place.
    let t = ticket(&s, "bbb").await;
    let mut b = Bot::connect(s.login_addr, payload(&t, "bbb", false)).await;
    assert_eq!(b.expect_rejected().await, SERVER_FULL.as_bytes());
    a2.send(lobby::QUERY_ACTORS, &[]).await;
    assert_eq!(a2.recv().await.0, lobby::ROLE_LIST);
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_player_who_leaves_frees_the_place() {
    let dir = tempfile::tempdir().unwrap();
    let limits = GateLimits {
        max_players: 1,
        ..GateLimits::default()
    };
    let (s, _c) = server(dir.path(), "", limits).await;
    account(&s, "aaa").await;
    account(&s, "bbb").await;
    let t = ticket(&s, "aaa").await;
    let mut a = Bot::connect(s.login_addr, payload(&t, "aaa", false)).await;
    a.expect_work().await;
    a.send(lobby::CLIENT_EXIT, &[]).await;
    drop(a);
    let mut joined = false;
    for _ in 0..50 {
        let t = ticket(&s, "bbb").await;
        let mut b = Bot::connect(s.login_addr, payload(&t, "bbb", false)).await;
        match b.next().await {
            Some(fenji_proto::client::ClientEvent::Accepted) => {
                joined = true;
                break;
            }
            _ => tokio::time::sleep(Duration::from_millis(20)).await,
        }
    }
    assert!(joined, "the place was never freed");
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn silent_connection_is_closed_after_the_deadline() {
    let dir = tempfile::tempdir().unwrap();
    let limits = GateLimits {
        auth_deadline: Duration::from_millis(300),
        ..GateLimits::default()
    };
    let (s, _c) = server(dir.path(), "", limits).await;
    let mut sock = TcpStream::connect(s.login_addr).await.unwrap();
    let mut buf = vec![0u8; 256];
    let mut got = 0;
    loop {
        match tokio::time::timeout(Duration::from_secs(3), sock.read(&mut buf)).await {
            Ok(Ok(0)) | Ok(Err(_)) => break,
            Ok(Ok(n)) => got += n,
            Err(_) => panic!("not closed after the deadline"),
        }
    }
    assert_eq!(got, 64, "only the hello");
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn oversized_pre_auth_input_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    let mut sock = TcpStream::connect(s.login_addr).await.unwrap();
    assert!(hello(&mut sock).await.is_some());
    // One byte past the cap, never a complete auth message.
    let _ = sock.write_all(&vec![b'a'; PRE_AUTH_CAP + 1]).await;
    let mut buf = [0u8; 64];
    match tokio::time::timeout(Duration::from_secs(3), sock.read(&mut buf)).await {
        Ok(Ok(0)) | Ok(Err(_)) => {}
        Ok(Ok(n)) => panic!("{n} bytes after the hello"),
        Err(_) => panic!("not closed past the cap"),
    }
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn refused_connections_keep_their_waiting_place_until_they_close() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    let mut refused = Vec::new();
    for _ in 0..8 {
        let mut bot = Bot::connect(s.login_addr, vec![0xFF, 0, 0, 9, 1, 2, 3]).await;
        assert_eq!(bot.expect_rejected().await, AUTH_FAILED.as_bytes());
        refused.push(bot); // still open: the server lingers for the client's close
    }
    let mut extra = TcpStream::connect(s.login_addr).await.unwrap();
    assert!(
        hello(&mut extra).await.is_none(),
        "a 9th got a hello while 8 were being refused"
    );
    drop(refused);
    let mut freed = false;
    for _ in 0..50 {
        let mut sock = TcpStream::connect(s.login_addr).await.unwrap();
        if hello(&mut sock).await.is_some() {
            freed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(freed, "the places never came back");
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn replaced_connections_linger_only_within_the_waiting_places() {
    let dir = tempfile::tempdir().unwrap();
    let limits = GateLimits {
        linger: Duration::from_secs(5),
        ..GateLimits::default()
    };
    let (s, _c) = server(dir.path(), "", limits).await;
    account(&s, "aaa").await;
    // Nine logins to one account: each replaces the one before, whose client stays open.
    let mut held = Vec::new();
    for _ in 0..9 {
        let t = ticket(&s, "aaa").await;
        let mut bot = Bot::connect(s.login_addr, payload(&t, "aaa", false)).await;
        bot.expect_work().await;
        held.push(bot);
    }
    let mut extra = TcpStream::connect(s.login_addr).await.unwrap();
    assert!(
        hello(&mut extra).await.is_none(),
        "replaced connections lingered outside the waiting places"
    );
    drop(held);
    let mut freed = false;
    for _ in 0..50 {
        let mut sock = TcpStream::connect(s.login_addr).await.unwrap();
        if hello(&mut sock).await.is_some() {
            freed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(freed, "the places never came back");
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn client_exit_closes_the_connection() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    account(&s, "tester").await;
    let t = ticket(&s, "tester").await;
    let mut bot = Bot::connect(s.login_addr, payload(&t, "tester", false)).await;
    bot.expect_work().await;
    bot.send(lobby::CLIENT_EXIT, &[]).await;
    // The idle timeout is 60 s and the bot's reads give up after 5 s, so an end within 2 s is the
    // server closing on 392 (lobby.md §2), with `client-exit`, which makes the client quit at once
    // (world.md 10.1; #382).
    let end = tokio::time::timeout(Duration::from_secs(2), bot.next()).await;
    assert!(
        matches!(&end, Ok(Some(fenji_proto::client::ClientEvent::Closed(r))) if r == CLIENT_EXIT.as_bytes()),
        "392 didn't close the connection with client-exit: {end:?}"
    );
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_deleted_accounts_session_ends_at_its_next_message() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    account(&s, "tester").await;
    let t = ticket(&s, "tester").await;
    let mut bot = Bot::connect(s.login_addr, payload(&t, "tester", false)).await;
    bot.expect_work().await;
    assert!(s.accounts().store().delete_account("tester").await.unwrap());
    bot.send(lobby::QUERY_ACTORS, &[]).await;
    assert_eq!(bot.expect_closed().await, AUTH_FAILED.as_bytes());
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_idle_client_is_closed() {
    let dir = tempfile::tempdir().unwrap();
    let limits = GateLimits {
        idle: Duration::from_millis(300),
        ..GateLimits::default()
    };
    let (s, _c) = server(dir.path(), "", limits).await;
    account(&s, "tester").await;
    let t = ticket(&s, "tester").await;
    let mut bot = Bot::connect(s.login_addr, payload(&t, "tester", false)).await;
    bot.expect_work().await;
    // The bot's own reads give up only after 5 s, so an end within 2 s is the server's close.
    let end = tokio::time::timeout(Duration::from_secs(2), bot.next()).await;
    assert!(matches!(end, Ok(None)), "not closed when idle: {end:?}");
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_client_that_stops_reading_is_closed() {
    let dir = tempfile::tempdir().unwrap();
    let limits = GateLimits {
        max_players: 1,
        write_deadline: Duration::from_millis(300),
        ..GateLimits::default()
    };
    let (s, _c) = server(dir.path(), "", limits).await;
    account(&s, "aaa").await;
    account(&s, "bbb").await;
    let t = ticket(&s, "aaa").await;
    let mut a = Bot::connect(s.login_addr, payload(&t, "aaa", false)).await;
    a.expect_work().await;
    // Nine characters with a face near the largest in the client's own shape (#391) make each
    // list about 36 KB: every section at its widest, in ten-digit values.
    let big = "2147483647";
    let sliders = format!("{}0,0,2147483647,1073741823", format!("{big},").repeat(61));
    let body = [big; 256].join(",");
    let face = format!(
        "v:5;1:{sliders};2:65543,{big},{big};3:{big},{big},{big},{big},{big};\
         4:31,{big},{big},{big},{big},{big};5:{body};6:{big}"
    );
    assert!(fenji_proto::face::valid(&face, 0), "in the client's shape");
    assert!(
        (3_500..=lobby::FACE_MAX).contains(&face.len()),
        "{}",
        face.len()
    );
    let face = Value::Str(face.into_bytes());
    for name in [
        "Ana", "Bea", "Cid", "Dax", "Eli", "Fay", "Gus", "Hal", "Ivy",
    ] {
        let mut req = request(name, 0, 2);
        if let Value::Table(t) = &mut req[0] {
            t.hash[5].1 = face.clone();
        }
        a.send(lobby::CREATE_ACTOR, &req).await;
        assert_eq!(a.recv().await.0, lobby::ROLE_CREATE);
    }
    // Far more than the socket buffers hold, never read.
    for _ in 0..600 {
        a.send(lobby::QUERY_ACTORS, &[]).await;
    }
    let mut joined = false;
    for _ in 0..100 {
        let t = ticket(&s, "bbb").await;
        let mut b = Bot::connect(s.login_addr, payload(&t, "bbb", false)).await;
        if matches!(
            b.next().await,
            Some(fenji_proto::client::ClientEvent::Accepted)
        ) {
            joined = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(joined, "the stuck connection kept its place");
    drop(a);
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_ticket_from_before_a_ban_is_refused_at_the_gate() {
    // M4 hosting spec §8, HA5: a ticket lives for a minute, so the account banned meanwhile is
    // refused at the gate's door as at the API's: the store doesn't spend its ticket. A store
    // that can't say whether it's banned refuses it with the server's error key: it fails closed.
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    account(&s, "tester").await;
    account(&s, "other").await;
    let t = ticket(&s, "tester").await;
    let other = ticket(&s, "other").await;
    let store = s.accounts().store().clone();
    let id = store
        .account_by_username("tester")
        .await
        .unwrap()
        .unwrap()
        .id;
    assert!(store.ban(id, "Griefing", NOW).await.unwrap());
    let mut bot = Bot::connect(s.login_addr, payload(&t, "tester", false)).await;
    assert_eq!(bot.expect_rejected().await, AUTH_FAILED.as_bytes());
    assert_eq!(s.api_state().online.count(), 0, "it never took a place");
    store.break_bans().await.unwrap();
    let mut bot = Bot::connect(s.login_addr, payload(&other, "other", false)).await;
    assert_eq!(bot.expect_rejected().await, SERVER_ERROR.as_bytes());
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_store_failure_refuses_with_the_server_error_key() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    account(&s, "tester").await;
    let t = ticket(&s, "tester").await;
    s.accounts().store().close().await;
    let mut bot = Bot::connect(s.login_addr, payload(&t, "tester", false)).await;
    assert_eq!(bot.expect_rejected().await, SERVER_ERROR.as_bytes());
    s.shutdown().await.unwrap();
}

/// Reads the 64-byte hello, or `None` when the server closes first.
async fn hello(sock: &mut TcpStream) -> Option<()> {
    let mut buf = [0u8; 64];
    match tokio::time::timeout(Duration::from_secs(3), sock.read_exact(&mut buf)).await {
        Ok(Ok(_)) => Some(()),
        Ok(Err(_)) => None,
        Err(_) => panic!("neither a hello nor a close"),
    }
}

#[tokio::test]
async fn one_network_cannot_take_every_waiting_place() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    assert_eq!(GateLimits::default().max_unauthenticated_per_network, 8);
    let mut waiting = Vec::new();
    for _ in 0..8 {
        let mut sock = TcpStream::connect(s.login_addr).await.unwrap();
        assert!(hello(&mut sock).await.is_some());
        waiting.push(sock);
    }
    let mut extra = TcpStream::connect(s.login_addr).await.unwrap();
    assert!(
        hello(&mut extra).await.is_none(),
        "a 9th from one address got a hello"
    );
    drop(waiting.pop());
    let mut freed = false;
    for _ in 0..50 {
        let mut sock = TcpStream::connect(s.login_addr).await.unwrap();
        if hello(&mut sock).await.is_some() {
            freed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(freed, "the address's place never came back");
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn at_most_64_connections_wait_for_their_auth() {
    let dir = tempfile::tempdir().unwrap();
    // Every test connection comes from 127.0.0.1, so lift the per-network cap here.
    let limits = GateLimits {
        max_unauthenticated_per_network: 64,
        ..GateLimits::default()
    };
    let (s, _c) = server(dir.path(), "", limits).await;
    assert_eq!(GateLimits::default().max_unauthenticated, 64);
    let mut waiting = Vec::new();
    for _ in 0..64 {
        let mut sock = TcpStream::connect(s.login_addr).await.unwrap();
        assert!(hello(&mut sock).await.is_some());
        waiting.push(sock);
    }
    let mut extra = TcpStream::connect(s.login_addr).await.unwrap();
    assert!(hello(&mut extra).await.is_none(), "a 65th got a hello");
    drop(waiting.pop());
    let mut freed = false;
    for _ in 0..50 {
        let mut sock = TcpStream::connect(s.login_addr).await.unwrap();
        if hello(&mut sock).await.is_some() {
            freed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(freed, "a place never came back");
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_server_info_names_the_bound_gate_port() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    let mut sock = TcpStream::connect(s.api_addr).await.unwrap();
    sock.write_all(b"GET /api/v1/server HTTP/1.1\r\nHost: fenji\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut text = String::new();
    sock.read_to_string(&mut text).await.unwrap();
    let body = text.split("\r\n\r\n").nth(1).unwrap();
    let info: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(info["gate_port"], u64::from(s.login_addr.port()));
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_transport_time_sync_is_echoed_in_the_lobby() {
    // #269: the client's ping display reads only what a transport-level time sync from the server
    // sets (transport.md 4.4), so the gate echoes each one with the client's own clock.
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    account(&s, "tester").await;
    let t = ticket(&s, "tester").await;
    let mut bot = Bot::connect(s.login_addr, payload(&t, "tester", false)).await;
    bot.expect_work().await;
    bot.send_transport_time_sync(1_791_950_400_123).await;
    assert_eq!(
        bot.next_transport_time_sync().await,
        Some(1_791_950_400_123)
    );
    // The stream stays in step after an echo: the next message still decodes.
    bot.send(lobby::QUERY_ACTORS, &[]).await;
    let (id, _) = bot.recv().await;
    assert_eq!(id, lobby::ROLE_LIST);
    s.shutdown().await.unwrap();
}
