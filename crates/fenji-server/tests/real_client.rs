//! The real client's create request from run m3-l3b (M3 plan Task 9), kept as a fixture. It is the
//! client's own bytes: its packet header (`82 00 00 01`, with the table count the client always
//! writes), its keys, their order, its integer and string forms, LZ4-compressed in its framing.
//! Only the face string's value digits are ours, a running counter with the same sections, item
//! counts and digit counts, since the real ones may hold the client's built-in presets (plan
//! Amendment A). Nothing in it is machine data, and the name is ours.
mod common;

use common::*;
use fenji_proto::lobby::{self, CreateRequest};
use fenji_proto::packet::Up;
use fenji_proto::pickle::Value;
use fenji_server::server::GateLimits;

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

/// The face string the fixture carries.
fn fixture_face() -> Value {
    let Up::Script { args, .. } = Up::decode(0xFF, &fixture()).unwrap() else {
        panic!("not a script message")
    };
    let face = CreateRequest::parse(&args)
        .unwrap()
        .appearance
        .face
        .unwrap();
    Value::Str(face.into_bytes())
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
    assert!(face.starts_with("v:5;") && face.contains('|'), "{face}");
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
    s.shutdown().await;
}
