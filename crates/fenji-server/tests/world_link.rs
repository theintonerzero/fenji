//! Entering the world through the gate (M4 world plan Task 11; Amendment A): the lobby's 10 hands
//! the connection to the link, which speaks for it to the world, with the bot as the client.
mod common;

use std::time::Duration;

use common::*;
use fenji_proto::lobby::{self, code};
use fenji_proto::opcode::{Cts, Stc};
use fenji_proto::pickle::{self, Value};
use fenji_proto::world::{self, BlobValue};
use fenji_server::clock::Clock;
use fenji_server::gate::{AUTH_FAILED, DUPLICATE_LOGIN};
use fenji_server::server::{GateLimits, RunningServer};
use fenji_store::{Place, Store};

/// `[world]` for these tests: a start apart from the defaults, so a test can tell them apart.
const WORLD: &str = "[world]\nstart_scene = 7\nstart_position = [100.0, -200.0, 300.0, 90.0]\n";
const START: [f64; 4] = [100.0, -200.0, 300.0, 90.0];
const START_PLACE: Place = Place {
    scene: 7,
    x: 100.0,
    y: -200.0,
    z: 300.0,
    yaw: 90.0,
};

/// A new account `name` signed in, with one character, Fox Mage Lin's look: the bot in the lobby
/// and the character's pid.
async fn in_lobby(s: &RunningServer, name: &str, character: &str) -> (Bot, i64) {
    account(s, name).await;
    let t = ticket(s, name).await;
    let mut bot = Bot::connect(s.login_addr, payload(&t, name, false)).await;
    bot.expect_work().await;
    bot.send(lobby::CREATE_ACTOR, &request(character, 7, 3))
        .await;
    let (id, args) = bot.recv().await;
    assert_eq!(id, lobby::ROLE_CREATE);
    let Value::Table(actor) = &args[0] else {
        panic!("{args:?}")
    };
    (bot, pid_of(actor))
}

/// 10 for `pid`: 772's arguments.
async fn select(bot: &mut Bot, pid: i64) -> Vec<Value> {
    bot.send(lobby::SELECT_ACTOR, &[Value::Int(pid)]).await;
    let m = bot.recv_message().await;
    assert_eq!(m.ty, Stc::Script.to_u8(), "772 is a script message");
    let mut v = pickle::decode(&m.body).unwrap();
    assert_eq!(v.remove(0), Value::Int(world::LOAD_SCENE));
    v
}

/// 772's arguments for `pid` standing at `at` in `scene`.
fn load_scene(pid: i64, scene: i64, at: [f64; 4]) -> Vec<Value> {
    let mut v = pickle::decode(&world::load_scene(pid, scene, at)).unwrap();
    v.remove(0);
    v
}

/// Type 3, then the player's own 7 and 8, in that order: the body of 7. No type 23: the blob's
/// 1325 sets the speed alone (run m4-e3).
async fn enter(bot: &mut Bot, pid: i64) -> Vec<u8> {
    bot.send_native(Cts::ClientEnter.to_u8(), &CLIENT_ENTER)
        .await;
    let seven = bot.recv_message().await;
    assert_eq!(seven.ty, Stc::SyncEnter.to_u8());
    assert_eq!(seven.body[..8], pid.to_le_bytes());
    let eight = bot.recv_message().await;
    assert_eq!(
        (eight.ty, eight.body),
        (
            Stc::SyncEnterFinish.to_u8(),
            world::sync_enter_finish(pid).to_vec()
        )
    );
    seven.body
}

/// Waits (5 s at most) until the store holds `want` as the character's place: the world's save,
/// which runs off the tick.
async fn saved_at(store: &Store, account: &str, pid: i64, want: Place) {
    let a = store.account_by_username(account).await.unwrap().unwrap();
    let mut place = None;
    for _ in 0..100 {
        place = store.character(a.id, pid).await.unwrap().unwrap().place;
        if place == Some(want) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("not saved at {want:?} within 5 s: {place:?}");
}

#[tokio::test]
async fn selecting_a_character_loads_its_scene_and_the_load_brings_it_in() {
    let dir = tempfile::tempdir().unwrap();
    let (s, clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    let (mut bot, pid) = in_lobby(&s, "tester", "Lin").await;
    assert_eq!(select(&mut bot, pid).await, load_scene(pid, 7, START));
    // The client's syncs start with 772, before the scene has loaded (world.md 7.1).
    bot.send_native(Cts::TimeSync.to_u8(), &time_sync(0xABCD))
        .await;
    let reply = bot.recv_message().await;
    let now = u64::try_from(clock.now_ms()).unwrap();
    assert_eq!(
        (reply.ty, reply.body),
        (
            Stc::TimeSync.to_u8(),
            world::time_sync_reply(0xABCD, now).to_vec()
        )
    );
    enter(&mut bot, pid).await;
    // And they go on in the world.
    bot.send_native(Cts::TimeSync.to_u8(), &time_sync(0xABCE))
        .await;
    assert_eq!(bot.recv_message().await.ty, Stc::TimeSync.to_u8());
    s.shutdown().await;
}

#[tokio::test]
async fn select_actor_answers_load_scene() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    let (mut bot, pid) = in_lobby(&s, "tester", "Lin").await;
    // A character that has stood in the world before comes back where it was saved.
    let place = Place {
        scene: 12,
        x: -1.5,
        y: 2.25,
        z: 30.0,
        yaw: -45.0,
    };
    assert!(s.accounts().store().save_place(pid, &place).await.unwrap());
    let args = select(&mut bot, pid).await;
    assert_eq!(args, load_scene(pid, 12, [-1.5, 2.25, 30.0, -45.0]));
    assert_eq!(args[3], Value::Int(world::PROPS_VERSION));
    s.shutdown().await;
}

#[tokio::test]
async fn selecting_another_accounts_character_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    let (_a, theirs) = in_lobby(&s, "aaa", "Lin").await;
    let (mut b, _) = in_lobby(&s, "bbb", "Mai").await;
    let not_found = (lobby::ERROR_CODE, vec![Value::Int(code::ROLE_NOT_FOUND)]);
    for args in [
        vec![Value::Int(theirs)],
        vec![Value::Int(999_999)],
        vec![],
        vec![Value::Str(b"1".to_vec())],
    ] {
        b.send(lobby::SELECT_ACTOR, &args).await;
        assert_eq!(b.recv().await, not_found, "{args:?}");
    }
    // Still in the lobby.
    b.send(lobby::QUERY_ACTORS, &[]).await;
    assert_eq!(b.recv().await.0, lobby::ROLE_LIST);
    s.shutdown().await;
}

#[tokio::test]
async fn type_2_is_answered_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let (s, clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    let (mut bot, pid) = in_lobby(&s, "tester", "Lin").await;
    select(&mut bot, pid).await;
    bot.send_native(Cts::TimeSync.to_u8(), &time_sync(0x1234_5678))
        .await;
    let reply = bot.recv_message().await;
    assert_eq!(reply.ty, Stc::TimeSync.to_u8());
    assert_eq!(reply.body.len(), 12);
    assert_eq!(reply.body[..4], 0x1234_5678u32.to_le_bytes());
    let server_ms = i64::from_le_bytes(reply.body[4..].try_into().unwrap());
    assert!(
        (server_ms - clock.now_ms()).abs() <= 5_000,
        "{server_ms} is the server's clock"
    );
    // A body that isn't 13 bytes is dropped, and the connection stays.
    bot.send_native(Cts::TimeSync.to_u8(), &[1, 2, 3]).await;
    bot.quiet(300).await;
    s.shutdown().await;
}

#[tokio::test]
async fn client_enter_brings_7_and_8_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    let (mut bot, pid) = in_lobby(&s, "tester", "Lin").await;
    select(&mut bot, pid).await;
    let seven = enter(&mut bot, pid).await;
    // The header: a player (1), difficulty 255, template vocation 7 × 10000 (world.md 5.4).
    assert_eq!(seven[8..14], [1, 255, 0x70, 0x11, 0x01, 0x00]);
    let blob = world::decode_blob(&seven[14..]).unwrap();
    let get = |i: u16| {
        blob.iter()
            .find(|(index, _)| *index == i)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(get(1), Some(BlobValue::Str(b"Lin".to_vec())));
    assert_eq!(get(5), Some(BlobValue::Int(7)));
    assert_eq!(get(6), Some(BlobValue::Int(3)));
    assert_eq!(get(9), Some(BlobValue::Int(37)));
    assert_eq!(
        get(16),
        Some(BlobValue::Vec([100.0, -200.0, 300.0, 90.0])),
        "[world]'s start"
    );
    // The speed, which nothing else sets: rate 100 ran about 5 m/s (runs m4-e2, m4-e3).
    assert_eq!(get(1325), Some(BlobValue::Int(100)));
    // Full health: with 1458 at 0 the client draws the player dead (world.md 5.6).
    assert_eq!(get(300), Some(BlobValue::Num(1.0)));
    assert_eq!(get(304), Some(BlobValue::Int(world::FULL_HEALTH)));
    assert_eq!(get(1458), Some(BlobValue::Int(world::FULL_HEALTH)));
    // The look the 304 gave (`common::request`): its portrait, voice, base face, face and
    // second outfit.
    let face = b"v:5;2:1,2,3;3:1,2,3,4,5;4:1,2,3,4".to_vec();
    assert_eq!(get(105), Some(BlobValue::Int(24 << 16)));
    assert_eq!(get(177), Some(BlobValue::Int(4)));
    assert_eq!(get(214), Some(BlobValue::Int(2)));
    assert_eq!(get(215), Some(BlobValue::Str(face)));
    let outfit = world::outfit_table(world::creation_outfit(7, 2).unwrap());
    assert_eq!(
        get(222),
        Some(BlobValue::Pickled(vec![Value::Table(outfit)]))
    );
    // A second report changes nothing: the world answers once.
    bot.send_native(Cts::ClientEnter.to_u8(), &CLIENT_ENTER)
        .await;
    bot.quiet(300).await;
    s.shutdown().await;
}

#[tokio::test]
async fn a_short_client_enter_closes() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    let (mut bot, pid) = in_lobby(&s, "tester", "Lin").await;
    select(&mut bot, pid).await;
    bot.send_native(Cts::ClientEnter.to_u8(), &CLIENT_ENTER[..20])
        .await;
    // The bot's reads give up after 5 s, so an end within 2 s is the server's close.
    let end = tokio::time::timeout(Duration::from_secs(2), bot.next()).await;
    assert!(matches!(end, Ok(None)), "{end:?}");
    saved_at(s.accounts().store(), "tester", pid, START_PLACE).await;
    s.shutdown().await;
}

#[tokio::test]
async fn exit_and_leave_end_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    // 392 as 0xFF (`send_gateway`, as `RequestQuitGame` sends it in the world), as 0, and type 4
    // (world.md 1.3, 1.5): each ends the session, and the world lets the player go.
    let exit = fenji_proto::packet::script_body(lobby::CLIENT_EXIT, &[]).unwrap();
    let ways = [
        ("aaa", "Lin", 0xFF, exit.clone()),
        ("bbb", "Mai", 0, exit),
        ("ccc", "Kai", Cts::ClientLeave.to_u8(), Vec::new()),
    ];
    for (name, character, ty, body) in ways {
        let (mut bot, pid) = in_lobby(&s, name, character).await;
        select(&mut bot, pid).await;
        enter(&mut bot, pid).await;
        // The store's copy changes while it stands at the start: only the world's save on
        // leaving puts the start back.
        let accounts = s.accounts();
        let store = accounts.store();
        let older = Place {
            x: 1.0,
            ..START_PLACE
        };
        assert!(store.save_place(pid, &older).await.unwrap());
        bot.send_native(ty, &body).await;
        // The idle timeout is 60 s and the bot's reads give up after 5 s, so an end within 2 s
        // is the server closing.
        let end = tokio::time::timeout(Duration::from_secs(2), bot.next()).await;
        assert!(matches!(end, Ok(None)), "{name}: {end:?}");
        saved_at(store, name, pid, START_PLACE).await;
    }
    s.shutdown().await;
}

#[tokio::test]
async fn later_world_messages_are_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    let (mut a, lin) = in_lobby(&s, "aaa", "Lin").await;
    select(&mut a, lin).await;
    enter(&mut a, lin).await;
    let (mut b, mai) = in_lobby(&s, "bbb", "Mai").await;
    select(&mut b, mai).await;
    enter(&mut b, mai).await;
    // Both stand at the start: the world announces each to the other (6) in the tick Mai entered.
    // Slice 1 encodes none of that, so neither client gets a byte for it.
    tokio::time::sleep(Duration::from_millis(300)).await;
    for bot in [&mut a, &mut b] {
        bot.send_native(Cts::TimeSync.to_u8(), &time_sync(7)).await;
        let next = bot.recv_message().await;
        assert_eq!(next.ty, Stc::TimeSync.to_u8(), "{next:?}");
        assert_eq!(next.body[..4], 7u32.to_le_bytes());
    }
    // The client's other messages get nothing in slice 1, and the connection stays.
    a.send_native(5, &[0; 18]).await;
    a.send_native(6, &[1, 2, 3]).await;
    a.send(12_345, &[]).await;
    a.quiet(300).await;
    a.send_native(Cts::TimeSync.to_u8(), &time_sync(8)).await;
    assert_eq!(a.recv_message().await.ty, Stc::TimeSync.to_u8());
    s.shutdown().await;
}

#[tokio::test]
async fn a_connection_closed_while_loading_leaves_and_its_place_is_saved() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    let (mut bot, pid) = in_lobby(&s, "tester", "Lin").await;
    select(&mut bot, pid).await;
    drop(bot);
    saved_at(s.accounts().store(), "tester", pid, START_PLACE).await;
    s.shutdown().await;
}

#[tokio::test]
async fn a_second_login_in_the_world_comes_back_where_it_stood() {
    let dir = tempfile::tempdir().unwrap();
    // The replaced connection lingers while its client keeps the socket open, as a real client
    // may, so the world still holds its player when the newer login enters.
    let limits = GateLimits {
        linger: Duration::from_secs(10),
        ..GateLimits::default()
    };
    let (s, _clock) = server(dir.path(), WORLD, limits).await;
    let (mut first, pid) = in_lobby(&s, "tester", "Lin").await;
    select(&mut first, pid).await;
    enter(&mut first, pid).await;
    // The store's copy is older than where it stands, as between two periodic saves.
    let older = Place {
        x: -9_000.0,
        ..START_PLACE
    };
    assert!(s.accounts().store().save_place(pid, &older).await.unwrap());
    let t = ticket(&s, "tester").await;
    let mut second = Bot::connect(s.login_addr, payload(&t, "tester", false)).await;
    second.expect_work().await;
    assert_eq!(first.expect_closed().await, DUPLICATE_LOGIN.as_bytes());
    assert_eq!(
        select(&mut second, pid).await,
        load_scene(pid, 7, START),
        "where it stood, not the store's older copy"
    );
    enter(&mut second, pid).await;
    s.shutdown().await;
}

#[tokio::test]
async fn a_client_that_stops_reading_in_the_world_is_closed() {
    let dir = tempfile::tempdir().unwrap();
    let limits = GateLimits {
        write_deadline: Duration::from_millis(300),
        ..GateLimits::default()
    };
    let (s, _clock) = server(dir.path(), WORLD, limits).await;
    let (mut bot, pid) = in_lobby(&s, "tester", "Lin").await;
    select(&mut bot, pid).await;
    enter(&mut bot, pid).await;
    // Time syncs, their replies never read: once the socket's buffers are full, a reply's write
    // takes past the deadline, and the server stops reading too.
    let mut sent = 0;
    while bot
        .send_many(
            Cts::TimeSync.to_u8(),
            &time_sync(1),
            4_096,
            Duration::from_millis(500),
        )
        .await
    {
        sent += 4_096;
        assert!(sent < 2_000_000, "the server never stopped reading");
    }
    assert!(
        bot.ends_within(Duration::from_secs(10)).await,
        "the server closed the connection"
    );
    saved_at(s.accounts().store(), "tester", pid, START_PLACE).await;
    s.shutdown().await;
}

#[tokio::test]
async fn a_deleted_account_is_closed_in_the_world() {
    // Ruling R5: the lobby closes a deleted account at its next message, but a player in the world
    // who only moves sends none, so the link checks the account every `account_check`.
    let dir = tempfile::tempdir().unwrap();
    let limits = GateLimits {
        account_check: Duration::from_millis(200),
        ..GateLimits::default()
    };
    let (s, _clock) = server(dir.path(), WORLD, limits).await;
    let (mut bot, pid) = in_lobby(&s, "tester", "Lin").await;
    select(&mut bot, pid).await;
    enter(&mut bot, pid).await;
    let accounts = s.accounts();
    let store = accounts.store();
    let id = store
        .account_by_username("tester")
        .await
        .unwrap()
        .unwrap()
        .id;
    assert!(store.delete_account("tester").await.unwrap());
    assert_eq!(bot.expect_closed().await, AUTH_FAILED.as_bytes());
    // The world lets the player go and saves its place: the save finds no row, and makes none.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(store.character(id, pid).await.unwrap(), None);
    assert!(store.list_accounts().await.unwrap().is_empty());
    s.shutdown().await;
}

#[tokio::test]
async fn a_silent_client_in_the_world_is_closed_after_the_idle_time() {
    // Ruling R6: only the client's reads move the idle deadline. The account check (Ruling R5),
    // shorter here, must not restart it, or a client gone silent without a FIN (a crash, a lost
    // network) would keep its online place and stand in the world for good.
    let dir = tempfile::tempdir().unwrap();
    let idle = Duration::from_millis(500);
    let limits = GateLimits {
        idle,
        account_check: Duration::from_millis(100),
        ..GateLimits::default()
    };
    let (s, _clock) = server(dir.path(), WORLD, limits).await;
    let (mut bot, pid) = in_lobby(&s, "tester", "Lin").await;
    select(&mut bot, pid).await;
    // Type 3 is the client's last message: the bot sends nothing after it.
    let last = std::time::Instant::now();
    enter(&mut bot, pid).await;
    let accounts = s.accounts();
    let store = accounts.store();
    let older = Place {
        x: 1.0,
        ..START_PLACE
    };
    assert!(store.save_place(pid, &older).await.unwrap());
    // The bot's reads give up after 5 s, so an end within 2 s is the server closing.
    let end = tokio::time::timeout(Duration::from_secs(2), bot.next()).await;
    assert!(matches!(end, Ok(None)), "still open: {end:?}");
    let after = last.elapsed();
    assert!(
        after >= idle,
        "closed {after:?} after type 3, before the idle time"
    );
    // The world let the player go, and saved where it stood.
    saved_at(store, "tester", pid, START_PLACE).await;
    s.shutdown().await;
}
