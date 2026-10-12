//! The lobby's messages (docs/protocol/packets/lobby.md; M3 plan Task 4).
use fenji_proto::lobby::{self, AccountExtras, Actor, Appearance, CreateRequest, Refusal, code};
use fenji_proto::pickle::{self, Table, Value};

fn s(x: &str) -> Value {
    Value::Str(x.as_bytes().to_vec())
}

fn get<'a>(t: &'a Table, k: &str) -> Option<&'a Value> {
    t.hash.iter().find(|(kk, _)| *kk == s(k)).map(|(_, v)| v)
}

/// The create request as `CreatePanel.luac` builds it (lobby.md §5).
fn request(name: &str, vocation: i64, skeleton: i64) -> Vec<Value> {
    vec![Value::Table(Table {
        array: Vec::new(),
        hash: vec![
            (s("prop_base_name"), s(name)),
            (s("prop_base_vocation"), Value::Int(vocation)),
            (s("prop_base_skeleton"), Value::Int(skeleton)),
            (s("prop_sys_player_voice"), Value::Int(4)),
            (s("prop_sys_avatar_feature"), Value::Int(2)),
            (
                s("prop_sys_avatar_feature_face"),
                s("v:5;2:1,2,3;3:1,2,3,4,5;4:1,2,3,4"),
            ),
            (s("prop_sys_player_tattoo_pos"), Value::Int(0)),
            (s("prop_sys_player_birthday"), Value::Int(0)),
            (s("prop_base_equips_id"), Value::Int(2)),
            (s("prop_sys_portrait"), Value::Int(24 << 16)),
            (s("something_else"), Value::Int(9)),
        ],
    })]
}

fn with(mut r: Vec<Value>, index: usize, v: Value) -> Vec<Value> {
    if let Value::Table(t) = &mut r[0] {
        t.hash[index].1 = v;
    }
    r
}

#[test]
fn a_create_request_parses_into_typed_fields() {
    let r = CreateRequest::parse(&request("Lin", 0, 2)).unwrap();
    assert_eq!((r.name.as_str(), r.vocation, r.skeleton), ("Lin", 0, 2));
    assert_eq!(
        r.appearance.face.as_deref(),
        Some("v:5;2:1,2,3;3:1,2,3,4,5;4:1,2,3,4")
    );
    assert_eq!(
        (
            r.appearance.base_face,
            r.appearance.equips_id,
            r.appearance.portrait,
            r.appearance.voice
        ),
        (2, 2, 24 << 16, Some(4))
    );
}

#[test]
fn names_follow_the_western_rule() {
    for ok in ["Lin", "Zoë", "Ça_va-bien", "a.b", &"x".repeat(24)] {
        assert!(CreateRequest::parse(&request(ok, 0, 1)).is_ok(), "{ok}");
    }
    for (name, why) in [
        (String::new(), "empty"),
        ("L".to_string(), "one byte"),
        ("x".repeat(25), "25 bytes"),
        ("ü".repeat(13), "26 bytes"),
    ] {
        assert_eq!(
            CreateRequest::parse(&request(&name, 0, 1)).unwrap_err(),
            Refusal::NameLength,
            "{why}"
        );
    }
    for ok in ["Äöüé", "Àlà", "Ýves", "Íñ".replace('ñ', "i").as_str()] {
        assert!(CreateRequest::parse(&request(ok, 0, 1)).is_ok(), "{ok}");
    }
    // Only the diacritics the Western rule names: umlauts, grave and acute accents, cedillas.
    for bad in [
        "a b", "Lin2", ".Lin", "Lin.", "林林", "Li\u{0}n", "a×b", "Niño", "Æsir", "Groß", "Ølaf",
        "Âme",
    ] {
        assert_eq!(
            CreateRequest::parse(&request(bad, 0, 1)).unwrap_err(),
            Refusal::NameChars,
            "{bad:?}"
        );
    }
    assert_eq!(Refusal::NameChars.code(), code::ROLE_NAME_ALLCHINESE);
    assert_eq!(Refusal::NameLength.code(), code::ROLE_NAME_LENERR);
}

#[test]
fn vocation_and_skeleton_must_match_the_western_screen() {
    for (v, sk) in [(0, 1), (0, 2), (5, 2), (6, 1), (7, 3), (9, 3)] {
        assert!(
            CreateRequest::parse(&request("Lin", v, sk)).is_ok(),
            "{v} {sk}"
        );
    }
    for (v, sk) in [(8, 1), (6, 2), (7, 1), (9, 2), (10, 1), (-1, 1), (0, 3)] {
        assert_eq!(
            CreateRequest::parse(&request("Lin", v, sk)).unwrap_err(),
            Refusal::VocationSkeleton,
            "{v} {sk}"
        );
    }
    assert_eq!(
        Refusal::VocationSkeleton.code(),
        code::ROLE_VOC_SKELETON_NOT_MATCH
    );
}

#[test]
fn shapes_no_client_sends_are_refused() {
    assert_eq!(CreateRequest::parse(&[]).unwrap_err(), Refusal::Malformed);
    assert_eq!(
        CreateRequest::parse(&[Value::Int(1)]).unwrap_err(),
        Refusal::Malformed
    );
    let table_face = with(request("Lin", 0, 1), 5, Value::Table(Table::default()));
    assert_eq!(
        CreateRequest::parse(&table_face).unwrap_err(),
        Refusal::Malformed
    );
    let long_face = with(request("Lin", 0, 1), 5, s(&"9".repeat(5000)));
    assert_eq!(
        CreateRequest::parse(&long_face).unwrap_err(),
        Refusal::Malformed
    );
    let no_vocation = {
        let mut r = request("Lin", 0, 1);
        if let Value::Table(t) = &mut r[0] {
            t.hash.remove(1);
        }
        r
    };
    assert_eq!(
        CreateRequest::parse(&no_vocation).unwrap_err(),
        Refusal::Malformed
    );
    let ref_name = with(request("Lin", 0, 1), 0, Value::Ref(0));
    assert_eq!(
        CreateRequest::parse(&ref_name).unwrap_err(),
        Refusal::Malformed
    );
}

#[test]
fn out_of_range_appearance_values_are_clamped_as_the_client_does() {
    // check_base_face (face_verification.luac 0.36) clamps: below 1 gives 1, above 2 gives 2.
    let r = with(request("Lin", 0, 1), 4, Value::Int(7)); // base face
    let r = with(r, 8, Value::Int(9)); // outfit
    let a = CreateRequest::parse(&r).unwrap().appearance;
    assert_eq!((a.base_face, a.equips_id), (2, 2));
    let r = with(request("Lin", 0, 1), 4, Value::Int(-3));
    let r = with(r, 8, Value::Int(0));
    let a = CreateRequest::parse(&r).unwrap().appearance;
    assert_eq!((a.base_face, a.equips_id), (1, 1));
}

#[test]
fn appearance_round_trips_through_its_table() {
    let a = CreateRequest::parse(&request("Lin", 0, 2))
        .unwrap()
        .appearance;
    assert_eq!(Appearance::from_table(&a.to_table()), a);
    let bare = Appearance::from_table(&Table::default());
    assert_eq!(
        (bare.base_face, bare.equips_id, bare.face, bare.voice),
        (1, 1, None, None)
    );
}

fn lin() -> Actor {
    Actor {
        pid: 7,
        name: "Lin".into(),
        level: 1,
        vocation: 0,
        skeleton: 2,
        appearance: CreateRequest::parse(&request("Lin", 0, 2))
            .unwrap()
            .appearance,
    }
}

#[test]
fn an_actor_table_has_what_the_select_screen_reads() {
    let t = lin().to_table(3);
    assert_eq!(get(&t, "pid"), Some(&Value::Int(7)));
    assert_eq!(get(&t, "slot"), Some(&Value::Int(3)));
    assert_eq!(get(&t, "prop_base_name"), Some(&s("Lin")));
    assert_eq!(get(&t, "prop_base_lv"), Some(&Value::Int(1)));
    assert_eq!(get(&t, "prop_base_vocation"), Some(&Value::Int(0)));
    assert_eq!(get(&t, "prop_base_skeleton"), Some(&Value::Int(2)));
    assert_eq!(get(&t, "prop_base_gender"), Some(&Value::Int(1)));
    assert_eq!(get(&t, "prop_sys_portrait"), Some(&Value::Int(24 << 16)));
    assert_eq!(get(&t, "prop_sys_avatar_feature"), Some(&Value::Int(2)));
    assert!(matches!(
        get(&t, "prop_sys_avatar_feature_face"),
        Some(Value::Str(_))
    ));
    assert_eq!(
        get(&t, "prop_sys_equips"),
        Some(&Value::Table(Table::default()))
    );
    for flag in lobby::SHOW_FLAGS {
        assert_eq!(get(&t, flag), Some(&Value::Bool(true)), "{flag}");
    }
    for never in ["enable", "block_cd", "delete_cd"] {
        assert!(get(&t, never).is_none(), "{never}");
    }
    let male = Actor {
        skeleton: 1,
        ..lin()
    };
    assert_eq!(
        get(&male.to_table(1), "prop_base_gender"),
        Some(&Value::Int(0))
    );
}

#[test]
fn role_list_is_last_pid_list_and_extras() {
    let extras = AccountExtras { role_num: 0 };
    let v = pickle::decode(&lobby::role_list(0, &[], &extras).unwrap()).unwrap();
    assert_eq!(v[0], Value::Int(228));
    assert_eq!(v[1], Value::Int(0));
    assert_eq!(
        v[2],
        Value::Table(Table::default()),
        "an empty list is a table"
    );
    let Value::Table(extra) = &v[3] else {
        panic!("extra must be a table")
    };
    for k in ["rename_num", "reborn_num", "change_gender_num", "role_num"] {
        assert_eq!(get(extra, k), Some(&Value::Int(0)), "{k}");
    }
    let v = pickle::decode(&lobby::role_list(7, &[lin(), lin()], &extras).unwrap()).unwrap();
    let Value::Table(list) = &v[2] else { panic!() };
    let Value::Table(second) = &list.array[1] else {
        panic!()
    };
    assert_eq!(
        get(second, "slot"),
        Some(&Value::Int(2)),
        "slots follow list order"
    );
}

#[test]
fn create_delete_and_error_messages() {
    let v = pickle::decode(&lobby::create_result(&lin(), 1).unwrap()).unwrap();
    assert_eq!(v[0], Value::Int(628));
    assert!(matches!(&v[1], Value::Table(t) if get(t, "pid") == Some(&Value::Int(7))));
    assert_eq!(
        pickle::decode(&lobby::delete_result(7)).unwrap(),
        vec![
            Value::Int(84),
            Value::Int(7),
            Value::Int(0),
            Value::Int(2_147_483_647)
        ]
    );
    assert_eq!(
        pickle::decode(&lobby::error_code(code::SERVER_NOT_RUNNING)).unwrap(),
        vec![Value::Int(270), Value::Int(7_248_304_305_738_093_764)]
    );
}

#[test]
fn nothing_the_lobby_builds_is_on_the_never_send_list() {
    for id in [
        lobby::ROLE_LIST,
        lobby::ROLE_CREATE,
        lobby::ROLE_DELETE,
        lobby::ERROR_CODE,
    ] {
        assert!(!lobby::NEVER_SEND.contains(&id));
    }
}
