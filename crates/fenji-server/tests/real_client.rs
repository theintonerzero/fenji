//! The real client's create request from run m3-l3b (M3 plan Task 9), kept as a fixture: its own
//! shape and values, LZ4-compressed as the client sent it, with the face string replaced by a
//! synthetic one of the same form (plan Amendment A), since the real one may hold the client's
//! built-in presets. Nothing in it is machine data, and the name is ours.
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
    assert!(
        face.starts_with("v:5;") && face.split(';').count() == 7,
        "{face}"
    );
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
    bot.send(lobby::QUERY_ACTORS, &[]).await;
    let (id, args) = bot.recv().await;
    assert_eq!(id, lobby::ROLE_LIST);
    let Value::Table(list) = &args[1] else {
        panic!("{args:?}")
    };
    assert_eq!(list.array.len(), 1);
    s.shutdown().await;
}
