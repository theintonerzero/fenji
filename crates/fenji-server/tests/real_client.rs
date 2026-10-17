//! The real client's create request from run m3-l3b (M3 plan Task 9), kept as a fixture. It is the
//! client's own bytes: its packet header (`82 00 00 01`, with the table count the client always
//! writes), its keys, their order, its integer and string forms, LZ4-compressed in its framing.
//! Only the face string's value digits are ours, a running counter with the same sections, item
//! counts and digit counts, since the real ones may hold the client's built-in presets (plan
//! Amendment A); two of them, section 4's mask and section 7's tail, were then set to fit the
//! shape the client's shared validator writes, as the real face did (#391). Nothing in it is
//! machine data, and the name is ours.
//!
//! Beside it, run m4-e1's first time sync (type 2) and its type 3 (M4 world plan Task 11), as the
//! server's trace log showed them. The log masks the bytes that describe the player's machine
//! (world.md 3.2, 7.2), so they are zero here: type 2's two counters, and type 3's Windows version
//! and parent-process flag.
//!
//! And run m4-s4's two type-5 bodies (Task 13), from client 1, whose player Lin saw Mai (pid 2):
//! its pull of Mai and its sync filter's 32 for her, as the trace log showed them, with byte 17,
//! which describes the player's machine (world.md 14.4), zero.
//!
//! And run t16-c1's five 115s (Task 16), from a build for that run alone that logged their bodies
//! (chat.md §7): Lin's nearby line, Mai's world line, Mai's whisper to Lin typed with `/w Lin `,
//! Lin's whisper to Mai set by clicking his name, and Mai's team line. Their texts are ours, and
//! nothing in a 115 describes the player's machine.
mod common;

use common::*;
use fenji_proto::chat::{self, Channel, Target, Transmit};
use fenji_proto::lobby::{self, CreateRequest};
use fenji_proto::opcode::{Cts, Stc};
use fenji_proto::packet::Up;
use fenji_proto::pickle::{self, Value};
use fenji_proto::world;
use fenji_server::clock::Clock;
use fenji_server::server::{GateLimits, RunningServer};

fn fixture() -> Vec<u8> {
    let hex = include_str!("fixtures/l3-create.hex").trim();
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

/// The fixture's packet before compression (payloads.md P.1: FF, a 24-bit raw size, an LZ4 block).
fn unzipped(body: &[u8]) -> Vec<u8> {
    let raw = u32::from_be_bytes([0, body[1], body[2], body[3]]) as usize;
    lz4_flex::block::decompress(&body[4..], raw).unwrap()
}

/// The face string as the fixture's bytes hold it, read without the parser under test: the 490
/// bytes after its key and its `09 EA 01` tag.
fn fixture_face() -> Value {
    let raw = unzipped(&fixture());
    let key = b"prop_sys_avatar_feature_face";
    let at = raw.windows(key.len()).position(|w| w == key).unwrap() + key.len() + 3;
    Value::Str(raw[at..at + 490].to_vec())
}

#[test]
fn the_real_create_request_keeps_the_clients_encoding() {
    let body = fixture();
    assert_eq!(
        body[1..4],
        [0, 2, 0xF6],
        "758 bytes unzipped, as in run m3-l3b"
    );
    let raw = unzipped(&body);
    assert_eq!(raw.len(), 758);
    assert_eq!(
        raw[..4],
        [0x82, 0, 0, 1],
        "two values, then the table count the client writes for every table"
    );
    // The face is a string with a 16-bit length: tag 09, then 490, little-endian.
    let key = b"prop_sys_avatar_feature_face";
    let at = raw.windows(key.len()).position(|w| w == key).unwrap() + key.len();
    assert_eq!(raw[at..at + 3], [0x09, 0xEA, 0x01]);
}

#[test]
fn the_real_create_request_parses_as_the_run_recorded() {
    let body = fixture();
    assert_eq!(
        body[0], 0xFF,
        "compressed, as the client sends 128 bytes and more"
    );
    let Up::Script { id, args, gateway } = Up::decode(0xFF, &body).unwrap() else {
        panic!("not a script message")
    };
    assert_eq!((id, gateway), (Value::Int(lobby::CREATE_ACTOR), true));
    let r = CreateRequest::parse(&args).unwrap();
    assert_eq!((r.name.as_str(), r.vocation, r.skeleton), ("Lin", 7, 3));
    let a = &r.appearance;
    assert_eq!(
        (a.voice, a.base_face, a.equips_id, a.portrait),
        (Some(3), 1, 2, 169 << 16)
    );
    let face = a.face.as_deref().unwrap();
    assert_eq!(face.len(), 490);
    let sections: Vec<&str> = face
        .split(';')
        .map(|s| &s[..s.find(':').unwrap_or(0)])
        .collect();
    assert_eq!(
        sections,
        ["v", "2", "3", "4", "5", "7", ""],
        "six sections and a trailing ';'"
    );
    assert!(face.starts_with("v:5;"), "{face}");
    // In the shape the client's shared validator writes, so the server keeps it (#391).
    assert!(fenji_proto::face::valid(face, 7), "{face}");
    assert_eq!(Value::Str(face.as_bytes().to_vec()), fixture_face());
    // Section 7 holds two pairs joined by `|` (lobby.md 5.9).
    let seven = face.split(';').find(|s| s.starts_with("7:")).unwrap();
    let pairs: Vec<usize> = seven[2..]
        .split('|')
        .map(|p| p.split(',').count())
        .collect();
    assert_eq!(pairs, [2, 2], "{seven}");
}

#[tokio::test]
async fn the_real_create_request_makes_a_character_the_list_shows() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    account(&s, "tester").await;
    let t = ticket(&s, "tester").await;
    let mut bot = Bot::connect(s.login_addr, payload(&t, "tester", true)).await;
    bot.expect_work().await;
    bot.send_raw(&fixture()).await;
    let (id, args) = bot.recv().await;
    assert_eq!(id, lobby::ROLE_CREATE);
    let Value::Table(actor) = &args[0] else {
        panic!("{args:?}")
    };
    assert_eq!(
        field(actor, "prop_base_name"),
        Some(&Value::Str(b"Lin".to_vec()))
    );
    assert_eq!(field(actor, "prop_base_vocation"), Some(&Value::Int(7)));
    assert_eq!(field(actor, "prop_base_skeleton"), Some(&Value::Int(3)));
    assert_eq!(
        field(actor, "prop_sys_portrait"),
        Some(&Value::Int(169 << 16))
    );
    assert_eq!(
        field(actor, "prop_sys_avatar_feature_face"),
        Some(&fixture_face())
    );
    let pid = field(actor, "pid").cloned().unwrap();
    bot.send(lobby::QUERY_ACTORS, &[]).await;
    let (id, args) = bot.recv().await;
    assert_eq!(id, lobby::ROLE_LIST);
    let Value::Table(list) = &args[1] else {
        panic!("{args:?}")
    };
    assert_eq!(list.array.len(), 1);
    let Value::Table(listed) = &list.array[0] else {
        panic!("{list:?}")
    };
    assert_eq!(field(listed, "pid"), Some(&pid));
    assert_eq!(
        field(listed, "prop_base_name"),
        Some(&Value::Str(b"Lin".to_vec()))
    );
    assert_eq!(
        field(listed, "prop_sys_avatar_feature_face"),
        Some(&fixture_face())
    );
    s.shutdown().await.unwrap();
}

/// A fixture's bytes, from its hex.
fn hex_fixture(text: &str) -> Vec<u8> {
    let hex: String = text.split_whitespace().collect();
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

fn time_sync_fixture() -> Vec<u8> {
    hex_fixture(include_str!("fixtures/m4-e1-time-sync.hex"))
}

fn client_enter_fixture() -> Vec<u8> {
    hex_fixture(include_str!("fixtures/m4-e1-client-enter.hex"))
}

/// The client's clock in m4-e1's first time sync: the low 32 bits of its Unix time in ms at
/// 01:13:21.781 UTC, which the server's receipt matched to the millisecond (world.md 7.2).
const M4E1_CLOCK: u32 = 0x37f8_9a75;

#[test]
fn the_real_world_bodies_fit_their_layouts() {
    let sync = time_sync_fixture();
    assert_eq!(sync.len(), world::TIME_SYNC_LEN);
    assert_eq!(world::time_sync_request(&sync), Ok(M4E1_CLOCK));
    assert_eq!(sync[4], 1, "the byte 1 after the clock");
    let enter = client_enter_fixture();
    assert_eq!(world::client_enter(&enter), Ok(()));
    assert_eq!(enter[0], 1, "type 3's constant first byte (world.md §3)");
    assert!(
        enter[1..].iter().all(|b| *b == 0),
        "zeros, and the masked bytes"
    );
}

#[tokio::test]
async fn the_real_bodies_carry_a_character_into_the_world() {
    let dir = tempfile::tempdir().unwrap();
    let (s, clock) = server(dir.path(), "", GateLimits::default()).await;
    account(&s, "tester").await;
    let t = ticket(&s, "tester").await;
    let mut bot = Bot::connect(s.login_addr, payload(&t, "tester", true)).await;
    bot.expect_work().await;
    bot.send_raw(&fixture()).await;
    let (id, args) = bot.recv().await;
    assert_eq!(id, lobby::ROLE_CREATE);
    let Value::Table(actor) = &args[0] else {
        panic!("{args:?}")
    };
    let pid = pid_of(actor);
    bot.send(lobby::SELECT_ACTOR, &[Value::Int(pid)]).await;
    let m = bot.recv_message().await;
    assert_eq!(
        pickle::decode(&m.body).unwrap()[..2],
        [Value::Int(world::LOAD_SCENE), Value::Int(pid)]
    );
    bot.send_native(Cts::TimeSync.to_u8(), &time_sync_fixture())
        .await;
    let reply = bot.recv_message().await;
    let now = u64::try_from(clock.now_ms()).unwrap();
    assert_eq!(
        (reply.ty, reply.body),
        (
            Stc::TimeSync.to_u8(),
            world::time_sync_reply(M4E1_CLOCK, now).to_vec()
        )
    );
    bot.send_native(Cts::ClientEnter.to_u8(), &client_enter_fixture())
        .await;
    for want in [Stc::SyncEnter, Stc::SyncEnterFinish] {
        let m = bot.recv_message().await;
        assert_eq!(m.ty, want.to_u8());
        assert_eq!(m.body[..8], pid.to_le_bytes());
    }
    s.shutdown().await.unwrap();
}

fn pull_fixture() -> Vec<u8> {
    hex_fixture(include_str!("fixtures/m4-s4-pull.hex"))
}

fn filter_fixture() -> Vec<u8> {
    hex_fixture(include_str!("fixtures/m4-s4-filter.hex"))
}

/// The pid in m4-s4's type-5 bodies: Mai's, the second character its database made.
const M4S4_MAI: i64 = 2;

#[test]
fn the_real_type_5_bodies_fit_their_layout() {
    let pull = pull_fixture();
    let filter = filter_fixture();
    assert_eq!(
        world::sync_filter_set(&pull),
        Ok(world::SyncFilter {
            pid: M4S4_MAI,
            value: world::PULL
        })
    );
    assert_eq!(
        world::sync_filter_set(&filter),
        Ok(world::SyncFilter {
            pid: M4S4_MAI,
            value: 32
        })
    );
    for body in [&pull, &filter] {
        assert_eq!(body[12], 1, "the constant byte (world.md 14.4)");
        assert_eq!(body[13..17], [0; 4], "the zero u32 (world.md 14.4)");
    }
}

/// A new account `name`, its character made and brought into the world at `[world]`'s start: the
/// bot, past the character's own 7 and 8, and the character's pid.
async fn in_the_world(
    s: &RunningServer,
    name: &str,
    character: &str,
    vocation: i64,
    skeleton: i64,
) -> (Bot, i64) {
    account(s, name).await;
    let t = ticket(s, name).await;
    let mut bot = Bot::connect(s.login_addr, payload(&t, name, false)).await;
    bot.expect_work().await;
    bot.send(lobby::CREATE_ACTOR, &request(character, vocation, skeleton))
        .await;
    let (id, args) = bot.recv().await;
    assert_eq!(id, lobby::ROLE_CREATE);
    let Value::Table(actor) = &args[0] else {
        panic!("{args:?}")
    };
    let pid = pid_of(actor);
    bot.send(lobby::SELECT_ACTOR, &[Value::Int(pid)]).await;
    let m = bot.recv_message().await;
    assert_eq!(
        pickle::decode(&m.body).unwrap()[..2],
        [Value::Int(world::LOAD_SCENE), Value::Int(pid)]
    );
    bot.send_native(Cts::ClientEnter.to_u8(), &client_enter_fixture())
        .await;
    for want in [Stc::SyncEnter, Stc::SyncEnterFinish] {
        let m = bot.recv_message().await;
        assert_eq!(m.ty, want.to_u8());
        assert_eq!(m.body[..8], pid.to_le_bytes());
    }
    (bot, pid)
}

#[tokio::test]
async fn the_real_pull_brings_the_other_player_and_the_real_32_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), "", GateLimits::default()).await;
    // As in m4-s4: Lin enters first, then Mai.
    let (mut lin, lin_pid) = in_the_world(&s, "tester", "Lin", 7, 3).await;
    let (mut mai, mai_pid) = in_the_world(&s, "second", "Mai", 2, 1).await;
    assert_eq!(
        (lin_pid, mai_pid),
        (1, M4S4_MAI),
        "a new database numbers its characters from 1, as m4-s4's did"
    );
    for (bot, other) in [(&mut lin, mai_pid), (&mut mai, lin_pid)] {
        let six = bot.recv_message().await;
        assert_eq!(six.ty, Stc::SyncIdEnter.to_u8());
        assert_eq!(six.body[..8], other.to_le_bytes());
    }
    // Client 1's 32 for Mai is no pull: sent before the pull, it brings nothing.
    lin.send_native(Cts::SyncFilterSet.to_u8(), &filter_fixture())
        .await;
    lin.quiet(300).await;
    // Its own pull of her brings her 7, then her 8.
    lin.send_native(Cts::SyncFilterSet.to_u8(), &pull_fixture())
        .await;
    for want in [Stc::SyncEnter, Stc::SyncEnterFinish] {
        let m = lin.recv_message().await;
        assert_eq!(m.ty, want.to_u8());
        assert_eq!(m.body[..8], mai_pid.to_le_bytes());
    }
    // The 32 it sent a few seconds later in the run brings nothing either, and the connection
    // stays.
    lin.send_native(Cts::SyncFilterSet.to_u8(), &filter_fixture())
        .await;
    lin.quiet(300).await;
    mai.quiet(300).await;
    lin.send_native(Cts::TimeSync.to_u8(), &time_sync_fixture())
        .await;
    assert_eq!(lin.recv_message().await.ty, Stc::TimeSync.to_u8());
    s.shutdown().await.unwrap();
}

/// The five 115s of run t16-c1, in the order they came: each as `net_manager_send` sent it.
fn t16_lines() -> [Vec<u8>; 5] {
    [
        hex_fixture(include_str!("fixtures/t16-c1-nearby.hex")),
        hex_fixture(include_str!("fixtures/t16-c1-world.hex")),
        hex_fixture(include_str!("fixtures/t16-c1-whisper-typed.hex")),
        hex_fixture(include_str!("fixtures/t16-c1-whisper-clicked.hex")),
        hex_fixture(include_str!("fixtures/t16-c1-team.hex")),
    ]
}

/// A 115's arguments, after its id, as the link reads them from a type-0 script message.
fn transmit_args(body: &[u8]) -> Vec<Value> {
    match Up::decode(Cts::Script.to_u8(), body).unwrap() {
        Up::Script {
            id: Value::Int(chat::CHAT_TRANSMIT),
            args,
            ..
        } => args,
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_real_lines_parse_as_the_run_recorded() {
    let [nearby, world_line, typed, clicked, team] = t16_lines();
    let line = |channel, text: &str, to| {
        Ok(Transmit {
            channel,
            text: text.into(),
            to,
        })
    };
    // Four values, the nil `extra` included (chat.md 1.2), on every channel but a whisper's.
    for body in [&nearby, &world_line, &team] {
        let args = transmit_args(body);
        assert_eq!(args.len(), 3, "{args:?}");
        assert_eq!(args[2], Value::Nil);
    }
    assert_eq!(
        chat::parse_transmit(&transmit_args(&nearby)),
        line(Channel::Nearby, "hello from Lin", None)
    );
    assert_eq!(
        chat::parse_transmit(&transmit_args(&world_line)),
        line(Channel::World, "hi world from Mai", None)
    );
    // A whisper's `extra` is a table, so its header counts one (payloads.md P.7): typed, a name
    // with the client's server name; clicked, the pid too (chat.md 1.4).
    for body in [&typed, &clicked] {
        assert_eq!(body[..4], [0x84, 0, 0, 1]);
    }
    assert_eq!(
        chat::parse_transmit(&transmit_args(&typed)),
        line(
            Channel::Whisper,
            "psst Lin from Mai",
            Some(Target::Name("Lin@Unknown".into()))
        )
    );
    assert_eq!(
        chat::parse_transmit(&transmit_args(&clicked)),
        line(Channel::Whisper, "psst Mai from Lin", Some(Target::Pid(2)))
    );
    assert_eq!(
        chat::parse_transmit(&transmit_args(&team)),
        Err(chat::Refusal::Channel)
    );
}

/// The next message is a 378: its channel, sender, content and receiver.
async fn heard(bot: &mut Bot) -> (i64, Value, Value, Value) {
    let m = bot.recv_message().await;
    assert_eq!(m.ty, Stc::Script.to_u8());
    let v = pickle::decode(&m.body).unwrap();
    assert_eq!(v[0], Value::Int(chat::SYS_CHAT_MESSAGE), "{v:?}");
    let Value::Int(channel) = v[1] else {
        panic!("{v:?}")
    };
    (channel, v[2].clone(), v[3].clone(), v[4].clone())
}

fn link(pid: i64, name: &str, vocation: i64) -> Value {
    let link = chat::player_link(&chat::Player {
        pid,
        name,
        vocation,
    });
    Value::Str(link.into_bytes())
}

fn text(x: &str) -> Value {
    Value::Str(x.as_bytes().to_vec())
}

#[tokio::test]
async fn the_real_lines_reach_whom_they_reached_in_the_run() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), "", GateLimits::default()).await;
    // As in t16-c1: Lin (pid 1, a Fox Mage) and Mai (pid 2, a Spellsword), side by side.
    let (mut lin, lin_pid) = in_the_world(&s, "tester", "Lin", 7, 3).await;
    let (mut mai, mai_pid) = in_the_world(&s, "second", "Mai", 2, 1).await;
    assert_eq!((lin_pid, mai_pid), (1, 2));
    for (bot, other) in [(&mut lin, mai_pid), (&mut mai, lin_pid)] {
        let six = bot.recv_message().await;
        assert_eq!(six.ty, Stc::SyncIdEnter.to_u8());
        assert_eq!(six.body[..8], other.to_le_bytes());
    }
    let [nearby, world_line, typed, clicked, team] = t16_lines();
    let (lin_link, mai_link) = (link(1, "Lin", 7), link(2, "Mai", 2));
    let say = Cts::Script.to_u8();
    lin.send_native(say, &nearby).await;
    let hi = (0, lin_link.clone(), text("hello from Lin"), text(""));
    assert_eq!(heard(&mut lin).await, hi);
    assert_eq!(heard(&mut mai).await, hi);
    mai.send_native(say, &world_line).await;
    let all = (5, mai_link.clone(), text("hi world from Mai"), text(""));
    assert_eq!(heard(&mut lin).await, all);
    assert_eq!(heard(&mut mai).await, all);
    mai.send_native(say, &typed).await;
    let psst = (
        1,
        mai_link.clone(),
        text("psst Lin from Mai"),
        lin_link.clone(),
    );
    assert_eq!(heard(&mut lin).await, psst);
    assert_eq!(heard(&mut mai).await, psst);
    lin.send_native(say, &clicked).await;
    let back = (1, lin_link, text("psst Mai from Lin"), mai_link);
    assert_eq!(heard(&mut mai).await, back);
    assert_eq!(heard(&mut lin).await, back);
    mai.send_native(say, &team).await;
    assert_eq!(
        mai.recv().await,
        (
            lobby::ERROR_CODE,
            vec![Value::Int(chat::code::CHAT_INVALID_CHANNEL)]
        )
    );
    lin.quiet(300).await;
    s.shutdown().await.unwrap();
}
