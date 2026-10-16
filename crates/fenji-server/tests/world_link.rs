//! The world through the gate (M4 world plan, the local slices): entering (Task 11; Amendment A),
//! seeing each other (Task 13; Amendment B), moving (Task 15; Amendment C), and chat and the
//! admins' commands (Task 16; docs/protocol/packets/chat.md). The lobby's 10 hands the connection
//! to the link, which speaks for it to the world, with the bot as the client. Amendment D gives
//! the protocol bot's script these tests begin.
mod common;

use std::time::Duration;

use common::*;
use fenji_proto::chat;
use fenji_proto::lobby::{self, code};
use fenji_proto::opcode::{Cts, Stc};
use fenji_proto::packet::script_body;
use fenji_proto::pickle::{self, Table, Value};
use fenji_proto::world::{self, BlobValue, MoveDetail};
use fenji_server::clock::Clock;
use fenji_server::gate::{
    AUTH_FAILED, BANNED, CLIENT_EXIT, DUPLICATE_LOGIN, KICKED, SERVER_STOPPING,
};
use fenji_server::server::{GateLimits, RunningServer};
use fenji_server::world::WorldIn;
use fenji_store::{Place, Store};
use tokio::net::TcpStream;

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

/// The next message is a 6 announcing `pid` at `[world]`'s start (world.md 14.1).
async fn announced(bot: &mut Bot, pid: i64) {
    let six = bot.recv_message().await;
    assert_eq!(
        (six.ty, six.body),
        (
            Stc::SyncIdEnter.to_u8(),
            world::sync_id_enter(pid, [START[0], START[1], START[2]]).to_vec()
        )
    );
}

/// Pulls `pid` (5 with 0): its 7, then its 8. The body of 7.
async fn pulled(bot: &mut Bot, pid: i64) -> Vec<u8> {
    bot.send_native(Cts::SyncFilterSet.to_u8(), &pull(pid))
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

/// The next message is a 9 taking `pids` away (world.md 14.11).
async fn left(bot: &mut Bot, pids: &[i64]) {
    let nine = bot.recv_message().await;
    assert_eq!(
        (nine.ty, nine.body),
        (Stc::SyncLeave.to_u8(), world::sync_leave(pids))
    );
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
    s.shutdown().await.unwrap();
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
    s.shutdown().await.unwrap();
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
    s.shutdown().await.unwrap();
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
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn client_enter_brings_7_and_8_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    let (mut bot, pid) = in_lobby(&s, "tester", "Lin").await;
    select(&mut bot, pid).await;
    let seven = enter(&mut bot, pid).await;
    // The header: a player (1), difficulty 255, template vocation 7 × 10000 (world.md 5.4).
    assert_eq!(seven[8..SEVEN_HEADER], [1, 255, 0x70, 0x11, 0x01, 0x00]);
    let blob = world::decode_blob(&seven[SEVEN_HEADER..]).unwrap();
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
    s.shutdown().await.unwrap();
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
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn exit_and_leave_end_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    // 392 as 0xFF (`send_gateway`, as `RequestQuitGame` sends it in the world), as 0, and type 4
    // (world.md 1.3, 1.5): each ends the session, and the world lets the player go. 392 closes with
    // `client-exit`, which makes the client quit (world.md 10.1): closed without a reason, it took
    // the player to its login screen and an error (#382). Type 4 waits for 330 (#282).
    let exit = fenji_proto::packet::script_body(lobby::CLIENT_EXIT, &[]).unwrap();
    let ways = [
        ("aaa", "Lin", 0xFF, exit.clone(), Some(CLIENT_EXIT)),
        ("bbb", "Mai", 0, exit, Some(CLIENT_EXIT)),
        ("ccc", "Kai", Cts::ClientLeave.to_u8(), Vec::new(), None),
    ];
    for (name, character, ty, body, reason) in ways {
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
        match reason {
            Some(reason) => assert!(
                matches!(&end, Ok(Some(fenji_proto::client::ClientEvent::Closed(r))) if r == reason.as_bytes()),
                "{name}: {end:?}"
            ),
            None => assert!(matches!(end, Ok(None)), "{name}: {end:?}"),
        }
        saved_at(store, name, pid, START_PLACE).await;
    }
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_told_line_is_a_378_and_other_messages_get_no_answer() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    // Signed in first and second: online ids 1 and 2.
    let (mut a, lin) = in_lobby(&s, "aaa", "Lin").await;
    select(&mut a, lin).await;
    enter(&mut a, lin).await;
    let (mut b, mai) = in_lobby(&s, "bbb", "Mai").await;
    select(&mut b, mai).await;
    enter(&mut b, mai).await;
    // Both stand at the start: the world announces each to the other (6) in the tick Mai entered,
    // and Lin pulls Mai.
    announced(&mut a, mai).await;
    announced(&mut b, lin).await;
    pulled(&mut a, mai).await;
    // A line for Lin alone, as `!invite`'s answer comes: a 378 on the system channel (chat.md §3).
    let world = s.world();
    assert!(world.send(WorldIn::Tell {
        conn: 1,
        text: "a line".into()
    }));
    assert_eq!(heard(&mut a).await, server_line("a line"));
    b.quiet(300).await;
    // The client's other messages get nothing, and the connection stays.
    a.send_native(6, &[1, 2, 3]).await;
    a.send(12_345, &[]).await;
    a.quiet(300).await;
    a.send_native(Cts::TimeSync.to_u8(), &time_sync(8)).await;
    assert_eq!(a.recv_message().await.ty, Stc::TimeSync.to_u8());
    // And online id 2 was Mai's: letting it go ends her connection, and Lin hears she left.
    assert!(world.send(WorldIn::Leave { conn: 2 }));
    assert!(b.next_is_end().await);
    left(&mut a, &[mai]).await;
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn two_players_see_each_other() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    let (mut a, lin) = in_lobby(&s, "aaa", "Lin").await;
    select(&mut a, lin).await;
    let lins_own = enter(&mut a, lin).await;
    let (mut b, mai) = in_lobby(&s, "bbb", "Mai").await;
    select(&mut b, mai).await;
    let mais_own = enter(&mut b, mai).await;
    // Each is announced to the other where it stands, pulls it, and gets the other's own 7 for
    // that character, the same bytes as that player's client got (world.md 14.9), and its 8.
    announced(&mut a, mai).await;
    announced(&mut b, lin).await;
    let mai_seen = pulled(&mut a, mai).await;
    let lin_seen = pulled(&mut b, lin).await;
    assert_eq!(mai_seen, mais_own, "Lin sees Mai as Mai's client has her");
    assert_eq!(lin_seen, lins_own, "Mai sees Lin as Lin's client has her");
    let blob = world::decode_blob(&mai_seen[SEVEN_HEADER..]).unwrap();
    assert_eq!(blob[0], (1, BlobValue::Str(b"Mai".to_vec())), "the name");
    // A second pull of a player already sent gets nothing, and nothing else comes.
    a.send_native(Cts::SyncFilterSet.to_u8(), &pull(mai)).await;
    a.quiet(300).await;
    b.quiet(300).await;
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_player_leaving_is_a_9_for_whoever_saw_it() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    let (mut a, lin) = in_lobby(&s, "aaa", "Lin").await;
    select(&mut a, lin).await;
    enter(&mut a, lin).await;
    let (mut b, mai) = in_lobby(&s, "bbb", "Mai").await;
    select(&mut b, mai).await;
    enter(&mut b, mai).await;
    announced(&mut a, mai).await;
    announced(&mut b, lin).await;
    // Lin pulls Mai; Kai, who enters next, is only told of her.
    pulled(&mut a, mai).await;
    let (mut c, kai) = in_lobby(&s, "ccc", "Kai").await;
    select(&mut c, kai).await;
    enter(&mut c, kai).await;
    announced(&mut a, kai).await;
    announced(&mut b, kai).await;
    // Kai is told of the others in pid order.
    announced(&mut c, lin).await;
    announced(&mut c, mai).await;
    // Mai's client goes: both who knew of her hear she left, whether they had pulled her or not.
    drop(b);
    left(&mut a, &[mai]).await;
    left(&mut c, &[mai]).await;
    a.quiet(300).await;
    c.quiet(300).await;
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_connection_closed_while_loading_leaves_and_its_place_is_saved() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    let (mut bot, pid) = in_lobby(&s, "tester", "Lin").await;
    select(&mut bot, pid).await;
    drop(bot);
    saved_at(s.accounts().store(), "tester", pid, START_PLACE).await;
    s.shutdown().await.unwrap();
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
    // Both clients close, so neither keeps the shutdown waiting out its 10 s linger.
    drop((first, second));
    s.shutdown().await.unwrap();
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
    s.shutdown().await.unwrap();
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
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_stopping_server_closes_players_in_the_world_with_server_not_running() {
    // #219: the world lets everyone go with the shutdown reason before the gate ends the
    // connections, so a player reads why (world.md 10.3), and the shutdown saves where it stood.
    // A client that stopped reading, one in the lobby and one that never sent its auth don't
    // hold the shutdown: the grace is a write deadline and a linger, and only a player writing its
    // close needs it.
    let dir = tempfile::tempdir().unwrap();
    let limits = GateLimits {
        write_deadline: Duration::from_secs(3),
        linger: Duration::from_millis(300),
        ..GateLimits::default()
    };
    let (s, _clock) = server(dir.path(), WORLD, limits).await;
    let (mut lin_bot, lin) = in_lobby(&s, "aaa", "Lin").await;
    select(&mut lin_bot, lin).await;
    enter(&mut lin_bot, lin).await;
    // Mai's client reads nothing more once it is in the world, and keeps its socket open.
    let (mut mai_bot, mai) = in_lobby(&s, "bbb", "Mai").await;
    select(&mut mai_bot, mai).await;
    enter(&mut mai_bot, mai).await;
    // Mai's arrival is announced to Lin (6, slice 2), before anything else reaches her.
    announced(&mut lin_bot, mai).await;
    let (_kai_bot, _) = in_lobby(&s, "ccc", "Kai").await;
    let _silent = TcpStream::connect(s.login_addr).await.unwrap();
    // The store's copies change while they stand at the start: only the world's last saves put
    // the start back.
    let accounts = s.accounts();
    let older = Place {
        x: 1.0,
        ..START_PLACE
    };
    for pid in [lin, mai] {
        assert!(accounts.store().save_place(pid, &older).await.unwrap());
    }
    let began = std::time::Instant::now();
    let stopping = tokio::spawn(s.shutdown());
    assert_eq!(lin_bot.expect_closed().await, SERVER_STOPPING.as_bytes());
    drop(lin_bot);
    // Mai's link lingers 300 ms for a close that never comes. Waiting out the 4.3 s grace instead
    // would mean a connection held the shutdown that should have ended at once.
    tokio::time::timeout(Duration::from_secs(10), stopping)
        .await
        .expect("the shutdown ends")
        .unwrap()
        .expect("everyone was saved");
    let took = began.elapsed();
    assert!(took < Duration::from_millis(2_500), "{took:?}");
    let store = Store::open(&dir.path().join("fenji.db")).await.unwrap();
    for (name, pid) in [("aaa", lin), ("bbb", mai)] {
        saved_at(&store, name, pid, START_PLACE).await;
    }
    drop(mai_bot);
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
    s.shutdown().await.unwrap();
}

// --- Slice 3 (Amendment C): moving. `common`'s `move_body` lays out the client's type 8.

/// Lin and Mai, each in the world at `[world]`'s start and each holding the other (6, 5, 7, 8):
/// their bots and pids, after the world's budget for moving has had time to fill.
async fn two_who_see_each_other(s: &RunningServer) -> (Bot, i64, Bot, i64) {
    let (mut a, lin) = in_lobby(s, "aaa", "Lin").await;
    select(&mut a, lin).await;
    enter(&mut a, lin).await;
    let (mut b, mai) = in_lobby(s, "bbb", "Mai").await;
    select(&mut b, mai).await;
    enter(&mut b, mai).await;
    announced(&mut a, mai).await;
    announced(&mut b, lin).await;
    pulled(&mut a, mai).await;
    pulled(&mut b, lin).await;
    // The speed check's budget fills from the join: 300 ms hold 382 units at rate 100.
    tokio::time::sleep(Duration::from_millis(300)).await;
    (a, lin, b, mai)
}

#[tokio::test]
async fn a_move_reaches_the_player_who_sees_the_mover() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    let (mut a, lin, mut b, _mai) = two_who_see_each_other(&s).await;
    // Lin runs 50 units east from the start, facing −45°, with a time 4 s ahead of the server's.
    let body = move_body(NOW_MS + 4_000, [150, -200, 300], -45, 1, 100);
    a.send_native(Cts::MoveDir.to_u8(), &body).await;
    let relay = b.recv_message().await;
    assert_eq!(relay.ty, Stc::MoveDir.to_u8());
    // Her position as the world checked it; the time held to a second ahead of the server's, then
    // floored to 16 ms.
    let detail = MoveDetail {
        time: NOW_MS + 992,
        dir: 1,
        yaw_offset: 0,
        rate: 100,
        pitch: 0,
        pitch2: 0,
    };
    assert_eq!(
        relay.body,
        world::move_dir_relay(lin, [150.0, -200.0, 300.0, -45.0], &detail)
    );
    // Lin hears nothing of her own move.
    a.quiet(300).await;
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_jump_too_far_is_corrected_and_not_relayed() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    let (mut a, lin, mut b, _mai) = two_who_see_each_other(&s).await;
    // 9,000 units off: past the budget's second at rate 100 and the tolerance.
    let body = move_body(NOW_MS, [9_100, -200, 300], 90, 1, 100);
    a.send_native(Cts::MoveDir.to_u8(), &body).await;
    let sync = a.recv_message().await;
    assert_eq!(
        (sync.ty, sync.body),
        (
            Stc::MoveSyncSvr.to_u8(),
            world::move_sync_svr(lin, world::sync_time(NOW_MS), START).to_vec()
        ),
        "where she stood"
    );
    b.quiet(300).await;
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_hostile_move_body_is_corrected_and_not_relayed() {
    // The plan's V15 (Review Focus 3): x at i32::MAX and the yaw −32768, which the world refuses.
    // Type 8's coordinates are integers, so no body carries a NaN.
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    let (mut a, lin, mut b, _mai) = two_who_see_each_other(&s).await;
    let body = move_body(NOW_MS, [i32::MAX, -200, 300], i16::MIN, 1, 100);
    a.send_native(Cts::MoveDir.to_u8(), &body).await;
    let sync = a.recv_message().await;
    assert_eq!(
        (sync.ty, sync.body),
        (
            Stc::MoveSyncSvr.to_u8(),
            world::move_sync_svr(lin, world::sync_time(NOW_MS), START).to_vec()
        )
    );
    b.quiet(300).await;
    // A move that fits after it is relayed as usual: the refusal changed nothing.
    let body = move_body(NOW_MS, [130, -200, 300], 90, 1, 100);
    a.send_native(Cts::MoveDir.to_u8(), &body).await;
    assert_eq!(b.recv_message().await.ty, Stc::MoveDir.to_u8());
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_place_after_moving_is_where_the_next_session_starts() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    let (mut bot, pid) = in_lobby(&s, "tester", "Lin").await;
    select(&mut bot, pid).await;
    enter(&mut bot, pid).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    // Two moves, then a stop where the second left her, facing −45°.
    for (x, y, dir) in [(130, -200, 1), (150, -180, 1), (150, -180, 0)] {
        let body = move_body(NOW_MS, [x, y, 300], -45, dir, 100);
        bot.send_native(Cts::MoveDir.to_u8(), &body).await;
        tokio::time::sleep(Duration::from_millis(60)).await;
    }
    bot.send_native(Cts::ClientLeave.to_u8(), &[]).await;
    assert!(bot.next_is_end().await);
    let moved = Place {
        scene: 7,
        x: 150.0,
        y: -180.0,
        z: 300.0,
        yaw: -45.0,
    };
    saved_at(s.accounts().store(), "tester", pid, moved).await;
    // The next session enters there.
    let t = ticket(&s, "tester").await;
    let mut again = Bot::connect(s.login_addr, payload(&t, "tester", false)).await;
    again.expect_work().await;
    assert_eq!(
        select(&mut again, pid).await,
        load_scene(pid, 7, [150.0, -180.0, 300.0, -45.0])
    );
    s.shutdown().await.unwrap();
}

// --- Chat and the admins' commands (Task 16; chat.md).

fn string(x: &str) -> Value {
    Value::Str(x.as_bytes().to_vec())
}

/// A whisper's `extra`, as the client's input box builds it (chat.md 1.4).
fn extra(pairs: Vec<(&str, Value)>) -> Value {
    Value::Table(Table {
        array: Vec::new(),
        hash: pairs.into_iter().map(|(k, v)| (string(k), v)).collect(),
    })
}

/// The client's 115 (chat.md 1.1): a type-0 script message, as `net_manager_send` sends it.
async fn say(bot: &mut Bot, args: &[Value]) {
    let body = script_body(chat::CHAT_TRANSMIT, args).unwrap();
    bot.send_native(Cts::Script.to_u8(), &body).await;
}

/// A 378's channel, sender, content and receiver.
type Heard = (i64, String, String, String);

/// The next message is a 378 with no options or template: what it says (chat.md §2).
async fn heard(bot: &mut Bot) -> Heard {
    let m = bot.recv_message().await;
    assert_eq!(m.ty, Stc::Script.to_u8());
    let v = pickle::decode(&m.body).unwrap();
    let text = |i: usize| match &v[i] {
        Value::Str(b) => String::from_utf8(b.clone()).unwrap(),
        other => panic!("{other:?} in {v:?}"),
    };
    assert_eq!(v[0], Value::Int(chat::SYS_CHAT_MESSAGE), "{v:?}");
    assert_eq!(v[5..], [Value::Nil, Value::Nil], "{v:?}");
    let Value::Int(channel) = v[1] else {
        panic!("{v:?}")
    };
    (channel, text(2), text(3), text(4))
}

/// The next message is a 270 with `code`.
async fn refused(bot: &mut Bot, code: i64) {
    assert_eq!(
        bot.recv().await,
        (lobby::ERROR_CODE, vec![Value::Int(code)])
    );
}

/// The link of a character the bots make (vocation 7).
fn link(pid: i64, name: &str) -> String {
    chat::player_link(&chat::Player {
        pid,
        name,
        vocation: 7,
    })
}

/// A player's line on `channel`, from `from` to `to`.
fn line(channel: i64, from: &str, text: &str, to: &str) -> Heard {
    (channel, from.into(), text.into(), to.into())
}

/// A line from the server to one player (64).
fn server_line(text: &str) -> Heard {
    (
        chat::channel::SYSTEM,
        String::new(),
        text.into(),
        String::new(),
    )
}

/// Kai's saved place: the start's scene, 1 km east of the start, out of the others' view.
const FAR: Place = Place {
    scene: 7,
    x: 100_100.0,
    y: -200.0,
    z: 300.0,
    yaw: 90.0,
};

/// The next message is a 6 announcing `pid` at `at`.
async fn announced_at(bot: &mut Bot, pid: i64, at: [f64; 3]) {
    let six = bot.recv_message().await;
    assert_eq!(
        (six.ty, six.body),
        (
            Stc::SyncIdEnter.to_u8(),
            world::sync_id_enter(pid, at).to_vec()
        )
    );
}

/// Lin (aaa) and Mai (bbb) at `[world]`'s start, each told of the other, and Kai (ccc) at
/// [`FAR`], told of no one: the bots and pids.
async fn three_players(s: &RunningServer) -> (Bot, i64, Bot, i64, Bot, i64) {
    let (mut a, lin) = in_lobby(s, "aaa", "Lin").await;
    select(&mut a, lin).await;
    enter(&mut a, lin).await;
    let (mut b, mai) = in_lobby(s, "bbb", "Mai").await;
    select(&mut b, mai).await;
    enter(&mut b, mai).await;
    announced(&mut a, mai).await;
    announced(&mut b, lin).await;
    let (mut c, kai) = in_lobby(s, "ccc", "Kai").await;
    assert!(s.accounts().store().save_place(kai, &FAR).await.unwrap());
    select(&mut c, kai).await;
    enter(&mut c, kai).await;
    (a, lin, b, mai, c, kai)
}

#[tokio::test]
async fn nearby_world_and_whispers_reach_the_right_players() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    let (mut a, lin, mut b, _mai, mut c, kai) = three_players(&s).await;
    // Nearby reaches whoever was told of the speaker, and the speaker, whose client shows nothing
    // of its own (chat.md 1.1).
    say(&mut a, &[Value::Int(0), string("hi"), Value::Nil]).await;
    let hi = line(0, &link(lin, "Lin"), "hi", "");
    assert_eq!(heard(&mut a).await, hi, "echoed");
    assert_eq!(heard(&mut b).await, hi);
    c.quiet(300).await;
    // The world channel reaches everyone, Kai's line from 1 km off included.
    say(&mut c, &[Value::Int(5), string("all &lt;3"), Value::Nil]).await;
    let all = line(5, &link(kai, "Kai"), "all &lt;3", "");
    for bot in [&mut a, &mut b, &mut c] {
        assert_eq!(heard(bot).await, all);
    }
    // A whisper typed with `/w kai` names its target, in any case and with the client's server
    // name; both ends get the same line, with the receiver's link.
    let typed = extra(vec![("name", string("kai@Unknown"))]);
    say(&mut a, &[Value::Int(1), string("psst"), typed]).await;
    let psst = line(1, &link(lin, "Lin"), "psst", &link(kai, "Kai"));
    assert_eq!(heard(&mut c).await, psst);
    assert_eq!(heard(&mut a).await, psst);
    // One set by clicking a name carries the pid too.
    let clicked = extra(vec![
        ("pid", Value::Int(lin)),
        ("name", string("Lin@Unknown")),
    ]);
    say(&mut c, &[Value::Int(1), string("back"), clicked]).await;
    let back = line(1, &link(kai, "Kai"), "back", &link(lin, "Lin"));
    assert_eq!(heard(&mut a).await, back);
    assert_eq!(heard(&mut c).await, back);
    b.quiet(300).await;
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_refused_line_is_a_270() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    let (mut a, _lin, mut b, _mai, mut c, _kai) = three_players(&s).await;
    // Team chat: the link refuses a channel M4 doesn't carry.
    say(&mut a, &[Value::Int(TEAM), string("hi"), Value::Nil]).await;
    refused(&mut a, chat::code::CHAT_INVALID_CHANNEL).await;
    // A forged player link, then 141 characters: the world refuses them.
    let forged = "[link type=\"player\" pid=\"1\" name=\"Admin\" gm=\"1\"]";
    say(&mut a, &[Value::Int(0), string(forged), Value::Nil]).await;
    refused(&mut a, chat::code::CHAT_INVALID_CONTENT).await;
    say(
        &mut a,
        &[Value::Int(0), string(&"a".repeat(141)), Value::Nil],
    )
    .await;
    refused(&mut a, chat::code::CHAT_MAX_CHAR).await;
    // A whisper to oneself, to no one, and to a name with an invisible character in it.
    for (name, code) in [
        ("Lin@Unknown", chat::code::CHAT_TALK_TOSELF),
        ("Nobody@Unknown", chat::code::CHAT_RECEIVER_OFFLINE),
        ("Mai\u{200B}@Unknown", chat::code::CHAT_RECEIVER_OFFLINE),
    ] {
        let to = extra(vec![("name", string(name))]);
        say(&mut a, &[Value::Int(1), string("you"), to]).await;
        refused(&mut a, code).await;
    }
    // Those five lines spent the burst: the next is too fast, and goes nowhere.
    say(&mut a, &[Value::Int(0), string("ok"), Value::Nil]).await;
    refused(&mut a, chat::code::CHAT_TOO_FAST).await;
    b.quiet(300).await;
    c.quiet(300).await;
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_admin_runs_each_command() {
    let dir = tempfile::tempdir().unwrap();
    // `[access].admins` names Lin's account, in another case.
    let config = format!("{WORLD}[access]\nadmins = [\"AAA\"]\n");
    let (s, _clock) = server(dir.path(), &config, GateLimits::default()).await;
    let (mut a, lin, mut b, mai, mut c, kai) = three_players(&s).await;
    // A player's `!` line is chat like any other.
    say(&mut b, &[Value::Int(0), string("!kick Lin"), Value::Nil]).await;
    let bang = line(0, &link(mai, "Mai"), "!kick Lin", "");
    assert_eq!(heard(&mut b).await, bang);
    assert_eq!(heard(&mut a).await, bang);
    // The marquee (66), for everyone, and nothing else.
    say(
        &mut a,
        &[
            Value::Int(0),
            string("!announce Restart at 20:00"),
            Value::Nil,
        ],
    )
    .await;
    let note = line(chat::channel::MARQUEE, "", "Restart at 20:00", "");
    for bot in [&mut a, &mut b, &mut c] {
        assert_eq!(heard(bot).await, note);
    }
    // An invite, whose link only the admin sees; it works, and carries its label.
    say(
        &mut a,
        &[Value::Int(0), string("!invite for Bob"), Value::Nil],
    )
    .await;
    let (channel, sender, content, receiver) = heard(&mut a).await;
    assert_eq!((channel, sender.as_str(), receiver.as_str()), (64, "", ""));
    let code = content
        .strip_prefix("invite link: http://127.0.0.1:0/#invite=")
        .unwrap_or_else(|| panic!("{content}"));
    s.accounts()
        .register(code, "dan", "a long password", LOCAL)
        .await
        .unwrap();
    let invites = s.accounts().store().list_invites().await.unwrap();
    assert!(
        invites
            .iter()
            .any(|i| i.label.as_deref() == Some("for Bob")
                && i.redeemed_by.as_deref() == Some("dan")),
        "{invites:?}"
    );
    // To Kai, 1 km off: Lin's client gets a 20 at Kai's place and the answer; Mai's, who saw Lin,
    // a 9; then Lin's forgets Mai, and Lin's and Kai's are told of each other.
    say(&mut a, &[Value::Int(0), string("!tp Kai"), Value::Nil]).await;
    let twenty = a.recv_message().await;
    let far = [FAR.x, FAR.y, FAR.z, FAR.yaw];
    assert_eq!(
        (twenty.ty, twenty.body),
        (
            Stc::MoveSyncSvr.to_u8(),
            world::move_sync_svr(lin, world::sync_time(NOW_MS), far).to_vec()
        )
    );
    assert_eq!(heard(&mut a).await, server_line("Lin is with Kai"));
    left(&mut b, &[lin]).await;
    left(&mut a, &[mai]).await;
    announced_at(&mut a, kai, [FAR.x, FAR.y, FAR.z]).await;
    announced_at(&mut c, lin, [FAR.x, FAR.y, FAR.z]).await;
    // A kick closes the player's connection with the reason its client shows.
    say(&mut a, &[Value::Int(0), string("!kick Mai"), Value::Nil]).await;
    assert_eq!(b.expect_closed().await, KICKED.as_bytes());
    assert_eq!(heard(&mut a).await, server_line("Mai was kicked"));
    // A command without its name answers with its usage, written for the client's markup.
    say(&mut a, &[Value::Int(0), string("!tp"), Value::Nil]).await;
    assert_eq!(
        heard(&mut a).await,
        server_line("usage: tp &lt;name&gt; \\[&lt;name&gt;\\]")
    );
    c.quiet(300).await;
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_op_in_the_database_enters_the_world_as_an_admin() {
    // M4 hosting spec §8, HA5: an admin is in [access].admins or in ops, and this config names
    // none.
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    let (mut a, lin) = in_lobby(&s, "aaa", "Lin").await;
    let store = s.accounts().store().clone();
    let id = store.account_by_username("aaa").await.unwrap().unwrap().id;
    assert!(store.op(id).await.unwrap());
    select(&mut a, lin).await;
    enter(&mut a, lin).await;
    say(
        &mut a,
        &[Value::Int(0), string("!announce Doors open"), Value::Nil],
    )
    .await;
    assert_eq!(
        heard(&mut a).await,
        line(chat::channel::MARQUEE, "", "Doors open", "")
    );
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_command_reads_what_the_admin_typed_and_its_answer_shows_it_so() {
    // The client's input writes &, < and > as entities (chat.md 1.5): the world reads a command's
    // arguments back to what the admin typed, and its answer, the server's own text, is written
    // for the client's markup once.
    let dir = tempfile::tempdir().unwrap();
    let config = format!("{WORLD}[access]\nadmins = [\"aaa\"]\n");
    let (s, _clock) = server(dir.path(), &config, GateLimits::default()).await;
    let (mut a, lin) = in_lobby(&s, "aaa", "Lin").await;
    select(&mut a, lin).await;
    enter(&mut a, lin).await;
    say(
        &mut a,
        &[Value::Int(0), string("!tp Nobody&lt;3"), Value::Nil],
    )
    .await;
    assert_eq!(
        heard(&mut a).await,
        server_line("no one called Nobody&lt;3 is in the game")
    );
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_motd_greets_a_player_once_its_own_actor_is_in() {
    // M4 hosting spec §8, HA1: a system line after its own 7 and 8, written for the client's
    // markup as the server's own text is.
    let dir = tempfile::tempdir().unwrap();
    let config = format!("motd = \"Fish & chips [today]\"\n{WORLD}");
    let (s, _clock) = server(dir.path(), &config, GateLimits::default()).await;
    let (mut bot, pid) = in_lobby(&s, "tester", "Lin").await;
    select(&mut bot, pid).await;
    enter(&mut bot, pid).await;
    assert_eq!(
        heard(&mut bot).await,
        server_line("Fish &amp; chips \\[today\\]")
    );
    bot.quiet(300).await;
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_banned_account_in_the_lobby_stays_out_of_the_world() {
    // M4 hosting spec §8, HA5: a ban that came after the login, while the player picks a
    // character. Its 10 closes the connection as a ban closes one in the world.
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    let (mut bot, pid) = in_lobby(&s, "tester", "Lin").await;
    let store = s.accounts().store().clone();
    let id = store
        .account_by_username("tester")
        .await
        .unwrap()
        .unwrap()
        .id;
    assert!(store.ban(id, "Griefing", NOW).await.unwrap());
    bot.send(lobby::SELECT_ACTOR, &[Value::Int(pid)]).await;
    assert_eq!(bot.expect_closed().await, BANNED.as_bytes());
    assert!(s.status().names().is_empty(), "it never entered");
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_store_that_cant_say_whether_the_account_is_banned_keeps_it_in_the_lobby() {
    // The world's door holds on a store error, as the read of the character does: a 270 the
    // lobby shows, and the player, still in the lobby, can try again. Here the character still
    // reads, and only the ban doesn't.
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    let (mut bot, pid) = in_lobby(&s, "tester", "Lin").await;
    s.accounts().store().break_bans().await.unwrap();
    bot.send(lobby::SELECT_ACTOR, &[Value::Int(pid)]).await;
    refused(&mut bot, code::SERVER_NOT_RESP).await;
    bot.send(lobby::QUERY_ACTORS, &[]).await;
    assert_eq!(bot.recv().await.0, lobby::ROLE_LIST, "still in the lobby");
    assert!(s.status().names().is_empty(), "it never entered");
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_ban_closes_the_accounts_connection_in_the_lobby_and_in_the_world() {
    // What the console's `ban` calls besides `WorldIn::Ban` (HA5): the online registry tells the
    // account's connection to close, as it tells one a newer login replaced, wherever it is.
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    let (mut in_the_lobby, _) = in_lobby(&s, "aaa", "Lin").await;
    let (mut in_the_world, mai) = in_lobby(&s, "bbb", "Mai").await;
    select(&mut in_the_world, mai).await;
    enter(&mut in_the_world, mai).await;
    let online = s.api_state().online;
    assert_eq!(online.count(), 2);
    let store = s.accounts().store().clone();
    for name in ["aaa", "bbb"] {
        let id = store.account_by_username(name).await.unwrap().unwrap().id;
        assert!(online.ban(id), "{name}");
    }
    assert_eq!(in_the_lobby.expect_closed().await, BANNED.as_bytes());
    assert_eq!(in_the_world.expect_closed().await, BANNED.as_bytes());
    assert_eq!(online.count(), 0);
    // The world heard Mai leave.
    for _ in 0..100 {
        if s.status().names().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(s.status().names().is_empty(), "{:?}", s.status().names());
    s.shutdown().await.unwrap();
}

// --- The host console through the gate (M4 hosting spec §3.3, §3.9; §8, HA5).

#[tokio::test]
async fn the_consoles_online_names_who_is_in_the_lobby_and_who_is_in_the_world() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    let (_in_the_lobby, _) = in_lobby(&s, "aaa", "Lin").await;
    let (mut in_the_world, mai) = in_lobby(&s, "bbb", "Mai").await;
    select(&mut in_the_world, mai).await;
    enter(&mut in_the_world, mai).await;
    let online = s.console().run_line("online").await;
    assert!(online.ok, "{online:?}");
    assert_eq!(
        online.lines,
        [
            "account  character  scene  since",
            "aaa      lobby      -      2027-01-15 08:00 UTC",
            "bbb      Mai        7      2027-01-15 08:00 UTC",
        ]
    );
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_consoles_ban_closes_the_accounts_connection_wherever_it_is() {
    // Carry-forwards 2 and 12: the registry closes the lobby's connection, and the world's, with
    // the ban's reason; the world hears its player leave.
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    let (mut in_the_lobby, _) = in_lobby(&s, "aaa", "Lin").await;
    let (mut in_the_world, mai) = in_lobby(&s, "bbb", "Mai").await;
    select(&mut in_the_world, mai).await;
    enter(&mut in_the_world, mai).await;
    let c = s.console();
    let banned = c.run_line("ban aaa").await;
    assert!(banned.ok, "{banned:?}");
    assert_eq!(in_the_lobby.expect_closed().await, BANNED.as_bytes());
    let banned = c.run_line("ban BBB Griefing").await;
    assert_eq!(
        banned.lines,
        ["banned bbb", "bbb was online, and was let go"]
    );
    assert_eq!(in_the_world.expect_closed().await, BANNED.as_bytes());
    for _ in 0..100 {
        if s.status().names().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(s.status().names().is_empty(), "{:?}", s.status().names());
    assert_eq!(c.run_line("online").await.lines, ["nobody is online"]);
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn maintenance_from_the_console_is_announced_to_players_in_the_world() {
    // M4 hosting spec §3.9: on the marquee, when it starts and when it ends.
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), WORLD, GateLimits::default()).await;
    let (mut bot, pid) = in_lobby(&s, "tester", "Lin").await;
    select(&mut bot, pid).await;
    enter(&mut bot, pid).await;
    let c = s.console();
    let on = c.run_line("maintenance on 30m Updating").await;
    assert_eq!(
        on.lines.last().map(String::as_str),
        Some("announced to the one player in the game"),
        "{on:?}"
    );
    assert_eq!(
        heard(&mut bot).await,
        line(chat::channel::MARQUEE, "", "Maintenance: Updating", "")
    );
    let off = c.run_line("maintenance off").await;
    assert!(off.ok, "{off:?}");
    assert_eq!(
        heard(&mut bot).await,
        line(chat::channel::MARQUEE, "", "Maintenance is over.", "")
    );
    s.shutdown().await.unwrap();
}
