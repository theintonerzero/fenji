//! The world's entering, seeing and moving messages (docs/protocol/packets/world.md; M4 world plan
//! Amendments A, B and C): each encoder makes its golden vector, and each decoder reads it back.
//! Chat's are in `tests/chat.rs`.
use fenji_proto::Error;
use fenji_proto::lobby::Appearance;
use fenji_proto::pickle::{self, Table, Value};
use fenji_proto::world::{self, BlobValue, MoveDetail, MoveDir, PlayerActor, SyncFilter};

fn unhex(h: &str) -> Vec<u8> {
    let h: String = h.split_whitespace().collect();
    (0..h.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap())
        .collect()
}

fn s(x: &str) -> Value {
    Value::Str(x.as_bytes().to_vec())
}

/// `[world]`'s default start: scene 5's point (Global Constraints).
const START: [f64; 4] = [2366.0, -71735.0, 64854.0, 345.0];

/// V1: 772 for pid 258, scene 5 at the start.
const V1: &str = "
    05 04 04 03 04 02 01 0b 00 00 00 00 08 09 73 63 65 6e 65 5f 75 69 64 03 05
    08 09 73 63 65 6e 65 5f 74 69 64 03 05 08 08 73 63 65 6e 65 5f 6c 76 03 00
    08 0a 73 63 65 6e 65 5f 74 79 70 65 03 00 00 0b 04 00 00 00
    07 00 00 00 00 00 7c a2 40 07 00 00 00 00 70 83 f1 c0
    07 00 00 00 00 c0 aa ef 40 07 00 00 00 00 00 90 75 40 00
    06 73 7a 92 79 7d bc 4f 41";

/// V3: a type-3 body as the client builds it (Windows 10 build 19045, no parent flag).
const V3: &str = "01 00 00 00 00 00 00 00 00 65 4a 00 0a 00 00 00 00 00 00 00 00";

/// V7: world.md §11's type 7 for Lin, at full health.
const V7: &str = "
    02 01 00 00 00 00 00 00 01 ff 70 11 01 00
    01 90 03 4c 69 6e 00
    04 30 01   05 30 07   06 30 03   09 30 25
    10 80 00 e0 13 45 80 1b 8c c7 00 56 7d 47 00 80 ac 43
    69 50 00 00 a9 00
    6d 10   80 10
    b1 30 03   d6 30 01
    d7 90 1b 76 3a 35 3b 32 3a 30 2c 30 2c 30 3b 33 3a 30 2c 30 2c 30 2c 30 2c 30 3b 34 3a 30 00
    de c0 2a 01 0b 00 00 00 00 03 02 05 13 0a 00 06 03 03 05 15 0a 00 06
             03 05 05 19 0a 00 06 03 06 05 1b 0a 00 06 03 0b 05 d0 00 00 05 00
    2c 71 00 00 00 00 00 00 f0 3f   30 41 e8 03
    2d 35 64
    b2 45 e8 03";

#[test]
fn load_scene_is_byte_for_byte() {
    let got = world::load_scene(258, 5, START);
    assert_eq!(got, unhex(V1));
    assert_eq!(got.len(), 116);
}

#[test]
fn load_scene_unpickles_as_the_client_reads_it() {
    let v = pickle::decode(&world::load_scene(258, 5, START)).unwrap();
    let scene = Table {
        array: Vec::new(),
        hash: vec![
            (s("scene_uid"), Value::Int(5)),
            (s("scene_tid"), Value::Int(5)),
            (s("scene_lv"), Value::Int(0)),
            (s("scene_type"), Value::Int(0)),
        ],
    };
    let at = Table {
        array: START.iter().map(|c| Value::Float(*c)).collect(),
        hash: Vec::new(),
    };
    assert_eq!(
        v,
        vec![
            Value::Int(world::LOAD_SCENE),
            Value::Int(258),
            Value::Table(scene),
            Value::Table(at),
            Value::Int(world::PROPS_VERSION),
        ]
    );
    assert_eq!(world::PROPS_VERSION, 4_706_187_382_722_034_291);
}

#[test]
fn client_enter_wants_21_bytes() {
    let v3 = unhex(V3);
    assert_eq!(v3.len(), world::CLIENT_ENTER_LEN);
    assert_eq!(world::client_enter(&v3), Ok(()));
    assert_eq!(
        world::client_enter(&v3[..20]),
        Err(Error::BadBody { opcode: 3, len: 20 })
    );
    let mut long = v3.clone();
    long.push(0);
    assert_eq!(
        world::client_enter(&long),
        Err(Error::BadBody { opcode: 3, len: 22 })
    );
    assert_eq!(
        Error::BadBody { opcode: 3, len: 20 }.to_string(),
        "a type-3 body of 20 bytes doesn't fit its layout"
    );
}

#[test]
fn time_sync_request_reads_the_clock() {
    let v2 = unhex("78 56 34 12 01 00 00 00 00 00 00 00 00");
    assert_eq!(world::time_sync_request(&v2), Ok(0x1234_5678));
    assert_eq!(
        world::time_sync_request(&v2[..12]),
        Err(Error::BadBody { opcode: 2, len: 12 })
    );
    let mut long = v2.clone();
    long.push(0);
    assert_eq!(
        world::time_sync_request(&long),
        Err(Error::BadBody { opcode: 2, len: 14 })
    );
    assert!(world::time_sync_request(&[]).is_err());
}

#[test]
fn time_sync_reply_is_byte_for_byte() {
    let want = unhex("78 56 34 12 00 70 b5 37 a1 01 00 00");
    assert_eq!(
        world::time_sync_reply(0x1234_5678, 1_791_936_000_000),
        want[..]
    );
}

/// One entry on its own, as `put_entry` writes it.
fn entry(index: u16, value: &BlobValue) -> Vec<u8> {
    let mut out = Vec::new();
    world::put_entry(&mut out, index, value).unwrap();
    out
}

#[test]
fn blob_entries_follow_the_client() {
    // Amendment A's entries under V7, and the integer boundaries: the tag goes by the absolute
    // value, as the client's pickle packer chooses it (payloads.md P.3), so -128 takes tag 4.
    let mut long = vec![0xd7, 0xa0, 0x2c, 0x01];
    long.extend([b'f'; 300]);
    long.push(0);
    let cases: Vec<(u16, BlobValue, Vec<u8>)> = vec![
        (4, BlobValue::Int(1), unhex("04 30 01")),
        (4, BlobValue::Int(-129), unhex("04 40 7f ff")),
        (4, BlobValue::Int(-128), unhex("04 40 80 ff")),
        (4, BlobValue::Int(-127), unhex("04 30 81")),
        (4, BlobValue::Int(127), unhex("04 30 7f")),
        (4, BlobValue::Int(128), unhex("04 40 80 00")),
        (4, BlobValue::Int(-32_768), unhex("04 50 00 80 ff ff")),
        (4, BlobValue::Int(32_767), unhex("04 40 ff 7f")),
        (
            4,
            BlobValue::Int(-(1 << 31)),
            unhex("04 60 00 00 00 80 ff ff ff ff"),
        ),
        (
            28,
            BlobValue::Int(1 << 31),
            unhex("1c 60 00 00 00 80 00 00 00 00"),
        ),
        (
            1,
            BlobValue::Str(b"Lin".to_vec()),
            unhex("01 90 03 4c 69 6e 00"),
        ),
        (215, BlobValue::Str(vec![b'f'; 300]), long),
        (109, BlobValue::Bool(false), unhex("6d 10")),
        (109, BlobValue::Bool(true), unhex("6d 20")),
        (
            16,
            BlobValue::Vec([2366.0, -71735.0, 64854.0, 345.0]),
            unhex("10 80 00 e0 13 45 80 1b 8c c7 00 56 7d 47 00 80 ac 43"),
        ),
        (1325, BlobValue::Int(100), unhex("2d 35 64")),
        // A number takes tag 7 and an f64; the health entries of V7 (run m4-e1).
        (
            300,
            BlobValue::Num(1.0),
            unhex("2c 71 00 00 00 00 00 00 f0 3f"),
        ),
        (1458, BlobValue::Int(1000), unhex("b2 45 e8 03")),
    ];
    for (index, value, want) in cases {
        assert_eq!(entry(index, &value), want, "{index} {value:?}");
        assert_eq!(
            world::decode_blob(&want).unwrap(),
            vec![(index, value.clone())],
            "{index} {value:?}"
        );
    }
}

#[test]
fn put_entry_refuses_what_the_client_cannot_set() {
    let mut out = Vec::new();
    // Index 0 ends the blob, and an index past the class's 2,439 raises (world.md 4.4, 4.5).
    for index in [0, 2440, 4095] {
        assert!(
            world::put_entry(&mut out, index, &BlobValue::Int(1)).is_err(),
            "{index}"
        );
    }
    // A pickled value must unpack to exactly one value (4.5).
    let two = BlobValue::Pickled(vec![Value::Int(1), Value::Int(2)]);
    assert!(world::put_entry(&mut out, 222, &two).is_err());
    assert!(out.is_empty(), "nothing written for a refused entry");
}

fn lin_look() -> Appearance {
    Appearance {
        voice: Some(3),
        base_face: 1,
        face: Some("v:5;2:0,0,0;3:0,0,0,0,0;4:0".into()),
        equips_id: 2,
        portrait: 169 << 16,
    }
}

fn lin(appearance: &Appearance) -> PlayerActor<'_> {
    PlayerActor {
        pid: 258,
        name: "Lin",
        vocation: 7,
        skeleton: 3,
        level: 37,
        appearance,
        pos: START,
        speed: 100,
    }
}

#[test]
fn sync_enter_player_is_byte_for_byte() {
    let look = lin_look();
    let got = world::sync_enter_player(&lin(&look));
    assert_eq!(got, unhex(V7));
    assert_eq!(got.len(), 164);
}

fn indexes(body: &[u8]) -> Vec<u16> {
    world::decode_blob(&body[14..])
        .unwrap()
        .iter()
        .map(|(i, _)| *i)
        .collect()
}

#[test]
fn sync_enter_player_leaves_out_what_is_unset() {
    let full = indexes(&unhex(V7));
    assert_eq!(
        full,
        [
            1, 4, 5, 6, 9, 16, 105, 109, 128, 177, 214, 215, 222, 300, 304, 1325, 1458
        ]
    );
    let bare = Appearance {
        voice: None,
        face: None,
        ..lin_look()
    };
    let got = indexes(&world::sync_enter_player(&lin(&bare)));
    assert!(!got.contains(&177), "no voice, no 177: {got:?}");
    assert!(!got.contains(&215), "no face, no 215: {got:?}");
    let third = Appearance {
        equips_id: 3,
        ..lin_look()
    };
    assert!(!indexes(&world::sync_enter_player(&lin(&third))).contains(&222));
    let look = lin_look();
    let ten = PlayerActor {
        vocation: 10,
        ..lin(&look)
    };
    assert!(!indexes(&world::sync_enter_player(&ten)).contains(&222));
}

#[test]
fn the_header_carries_the_template_when_it_fits() {
    // world.md 5.4: type 1, difficulty 255, template vocation × 10000 as a u32, or 0.
    let look = lin_look();
    for (vocation, template) in [(0, 0u32), (7, 70_000), (9, 90_000), (-1, 0), (500_000, 0)] {
        let body = world::sync_enter_player(&PlayerActor {
            vocation,
            ..lin(&look)
        });
        assert_eq!(&body[..8], &258u64.to_le_bytes(), "{vocation}");
        assert_eq!(body[8..10], [1, 255], "{vocation}");
        assert_eq!(body[10..14], template.to_le_bytes(), "{vocation}");
    }
    let far = world::sync_enter_player(&PlayerActor {
        pid: (1 << 40) + 1,
        ..lin(&look)
    });
    assert_eq!(far[..8], unhex("01 00 00 00 00 01 00 00")[..]);
}

fn outfit(pairs: &[(i64, i64)]) -> Value {
    Value::Table(Table {
        array: Vec::new(),
        hash: pairs
            .iter()
            .map(|(slot, id)| (Value::Int(*slot), Value::Int(*id)))
            .collect(),
    })
}

#[test]
fn decode_blob_reads_back_v7() {
    let v7 = unhex(V7);
    let want = vec![
        (1, BlobValue::Str(b"Lin".to_vec())),
        (4, BlobValue::Int(1)),
        (5, BlobValue::Int(7)),
        (6, BlobValue::Int(3)),
        (9, BlobValue::Int(37)),
        (16, BlobValue::Vec([2366.0, -71735.0, 64854.0, 345.0])),
        (105, BlobValue::Int(169 << 16)),
        (109, BlobValue::Bool(false)),
        (128, BlobValue::Bool(false)),
        (177, BlobValue::Int(3)),
        (214, BlobValue::Int(1)),
        (215, BlobValue::Str(b"v:5;2:0,0,0;3:0,0,0,0,0;4:0".to_vec())),
        (
            222,
            BlobValue::Pickled(vec![outfit(&[
                (2, 100_665_875),
                (3, 100_665_877),
                (5, 100_665_881),
                (6, 100_665_883),
                (11, 83_886_288),
            ])]),
        ),
        // Full health: the ratio, the most and the current (run m4-e1).
        (300, BlobValue::Num(1.0)),
        (304, BlobValue::Int(world::FULL_HEALTH)),
        (1325, BlobValue::Int(100)),
        (1458, BlobValue::Int(world::FULL_HEALTH)),
    ];
    assert_eq!(world::FULL_HEALTH, 1000);
    assert_eq!(world::decode_blob(&v7[14..]).unwrap(), want);
}

#[test]
fn a_face_out_of_the_validators_shape_goes_to_no_one() {
    // #391: in the world, a client hands another player's face straight to native code, without
    // the shared validator it runs on the select screen (lobby.md 5.11).
    for bad in [
        "v:5;",
        "v:5;2:1,2,3;3:1,2,3,4,5;4:1,2,3,4",
        "v:5;2:1,2,3;3:1,2,3,4,5;4:7,2,3,4;7:1,1|512,1",
    ] {
        let look = Appearance {
            face: Some(bad.into()),
            ..lin_look()
        };
        let v7 = world::sync_enter_player(&lin(&look));
        let entries = world::decode_blob(&v7[14..]).unwrap();
        assert!(entries.iter().all(|(index, _)| *index != 215), "{bad}");
    }
    // A fox's ears and tail go out for a Fox Mage alone: the check takes the actor's vocation.
    let fox = "v:5;2:1,2,3;3:1,2,3,4,5;4:7,2,3,4;7:1,1|300,502";
    let look = Appearance {
        face: Some(fox.into()),
        ..lin_look()
    };
    let sent = |vocation| {
        let v7 = world::sync_enter_player(&PlayerActor {
            vocation,
            ..lin(&look)
        });
        let entries = world::decode_blob(&v7[14..]).unwrap();
        entries.iter().any(|(index, _)| *index == 215)
    };
    assert!(sent(7), "a Fox Mage's");
    assert!(!sent(0), "a Spearmaster's");
}

#[test]
fn decode_blob_refuses_what_fenji_never_writes() {
    // Unlike the client, which stops quietly: a tag-15 entry, and an entry cut short.
    assert!(
        world::decode_blob(&unhex("04 30 01 05 f0")).is_err(),
        "tag 15"
    );
    assert!(
        world::decode_blob(&unhex("04 40 7f")).is_err(),
        "an i16 cut short"
    );
    assert!(
        world::decode_blob(&unhex("04")).is_err(),
        "a header cut short"
    );
    assert!(
        world::decode_blob(&unhex("01 90 05 4c 69")).is_err(),
        "a string cut short"
    );
    assert!(
        world::decode_blob(&unhex("de c0 05 01 03 01")).is_err(),
        "a pickle cut short"
    );
    // A pickled value that isn't exactly one value, which the client refuses (4.5).
    assert!(world::decode_blob(&unhex("de c0 05 02 03 01 03 02")).is_err());
    // An index past the class's 2,439 properties raises in the client (4.5): 2440 is 0x988.
    assert!(world::decode_blob(&unhex("88 39 01")).is_err());
}

#[test]
fn decode_blob_reads_what_the_client_accepts() {
    // Index 0 is the client's end marker: what follows it is never set (world.md 4.4).
    assert_eq!(
        world::decode_blob(&unhex("04 30 01 00 30 05 05 30 07")).unwrap(),
        vec![(4, BlobValue::Int(1))]
    );
    // A last string without its NUL: the client writes the 0 past the end and stops (4.6).
    assert_eq!(
        world::decode_blob(&unhex("05 30 07 01 90 03 4c 69 6e")).unwrap(),
        vec![(5, BlobValue::Int(7)), (1, BlobValue::Str(b"Lin".to_vec()))]
    );
    // Integers in a wider tag than they need, as the client's reader takes any width.
    assert_eq!(
        world::decode_blob(&unhex("04 60 01 00 00 00 00 00 00 00")).unwrap(),
        vec![(4, BlobValue::Int(1))]
    );
    assert_eq!(world::decode_blob(&[]).unwrap(), vec![]);
}

/// World.md §9's outfit table: (vocation, outfit, [chest, wrist, trousers, boots, weapon]), with
/// `None` for a missing piece.
fn world_md_outfits() -> Vec<(i64, i64, [Option<i64>; 5])> {
    let doc = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/protocol/packets/world.md"
    ));
    let section = doc
        .split("## 9. The creation outfit")
        .nth(1)
        .unwrap()
        .split("\n## ")
        .next()
        .unwrap();
    let mut rows = Vec::new();
    for line in section.lines() {
        let cells: Vec<&str> = line
            .trim()
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if cells.len() != 7 {
            continue;
        }
        let (Ok(vocation), Ok(outfit)) = (cells[0].parse(), cells[1].parse()) else {
            continue;
        };
        let pieces = [2, 3, 4, 5, 6].map(|i| cells[i].parse().ok());
        rows.push((vocation, outfit, pieces));
    }
    rows
}

#[test]
fn creation_outfit_matches_world_md() {
    assert_eq!(
        world::creation_outfit(7, 2),
        Some(
            &[
                (2, 100_665_875),
                (3, 100_665_877),
                (5, 100_665_881),
                (6, 100_665_883),
                (11, 83_886_288)
            ][..]
        )
    );
    assert!(
        world::creation_outfit(8, 1)
            .unwrap()
            .iter()
            .all(|(slot, _)| *slot != 5),
        "vocation 8 has no trousers"
    );
    assert_eq!(world::creation_outfit(0, 3), None);
    assert_eq!(world::creation_outfit(10, 1), None);
    assert_eq!(world::creation_outfit(-1, 1), None);
    let rows = world_md_outfits();
    assert_eq!(rows.len(), 20, "vocations 0–9, outfits 1 and 2");
    for (vocation, outfit, pieces) in rows {
        let want: Vec<(i64, i64)> = [2, 3, 5, 6, 11]
            .into_iter()
            .zip(pieces)
            .filter_map(|(slot, id)| Some((slot, id?)))
            .collect();
        let got = world::creation_outfit(vocation, outfit).unwrap();
        assert_eq!(got, &want[..], "vocation {vocation}, outfit {outfit}");
        assert!(
            got.iter().all(|(slot, _)| *slot != 1 && *slot != 4),
            "no head or belt"
        );
        assert!(got.windows(2).all(|w| w[0].0 < w[1].0), "in slot order");
        for (slot, id) in got {
            // type << 24 | id: 5 for a weapon, 6 for armour (world.md 9.3).
            let kind = if *slot == 11 { 5 } else { 6 };
            assert_eq!(id >> 24, kind, "vocation {vocation}, slot {slot}");
        }
    }
}

#[test]
fn sync_prop_force_speed_is_byte_for_byte() {
    assert_eq!(world::MOVE_SPEED, 1325);
    let want = unhex("02 01 00 00 00 00 00 00 2d 05 64 00");
    assert_eq!(world::sync_prop_force_speed(258, 100), want[..]);
}

#[test]
fn sync_enter_finish_is_byte_for_byte() {
    assert_eq!(
        world::sync_enter_finish(258),
        unhex("02 01 00 00 00 00 00 00")[..]
    );
    assert_eq!(
        world::sync_enter_finish((1 << 40) + 1),
        unhex("01 00 00 00 00 01 00 00")[..]
    );
}

/// V8: type 6 for pid 258 at the default start (world.md 14.1).
const V8: &str = "02 01 00 00 00 00 00 00 3e 09 00 00 c9 e7 fe ff 56 fd 00 00 00 00";
/// V8b: pid 2^32 + 1 at (2.5, -2.5, 3 x 10^9), rounded half away from zero and clamped to i32.
const V8B: &str = "01 00 00 00 01 00 00 00 03 00 00 00 fd ff ff ff ff ff ff 7f 00 00";
/// V9a: the client's pull of pid 258 (world.md 14.3, 14.4).
const V9A: &str = "02 01 00 00 00 00 00 00 00 00 00 00 01 00 00 00 00 00";
/// V9b: the sync filter's value 32 for pid 2^32 + 1, with byte 17 set (world.md 14.12).
const V9B: &str = "01 00 00 00 01 00 00 00 20 00 00 00 01 00 00 00 00 01";
/// V10: type 9 for pids 258 and 2^32 + 1 (world.md 14.11).
const V10: &str = "02 01 00 00 00 00 00 00 01 00 00 00 01 00 00 00";

#[test]
fn sync_id_enter_is_byte_for_byte() {
    assert_eq!(world::SYNC_ID_ENTER_LEN, 22);
    assert_eq!(
        world::sync_id_enter(258, [2366.0, -71735.0, 64854.0]),
        unhex(V8)[..]
    );
    assert_eq!(
        world::sync_id_enter((1 << 32) + 1, [2.5, -2.5, 3.0e9]),
        unhex(V8B)[..]
    );
    // NaN gives 0, and what rounds to nothing is 0 (Rust's `as` casts saturate).
    let zeros = world::sync_id_enter(258, [f64::NAN, -0.4, 0.4]);
    assert_eq!(zeros[8..20], [0; 12]);
    let low = world::sync_id_enter(258, [-3.0e9, f64::NEG_INFINITY, f64::INFINITY]);
    assert_eq!(low[8..12], i32::MIN.to_le_bytes());
    assert_eq!(low[12..16], i32::MIN.to_le_bytes());
    assert_eq!(low[16..20], i32::MAX.to_le_bytes());
    // The fast flag and the priority are 0 (world.md 14.2, 14.3).
    assert_eq!(low[20..], [0, 0]);
}

#[test]
fn sync_filter_set_reads_the_pid_and_the_value() {
    assert_eq!(world::SYNC_FILTER_LEN, 18);
    assert_eq!(world::PULL, 0);
    let v9a = unhex(V9A);
    let pull = SyncFilter {
        pid: 258,
        value: world::PULL,
    };
    assert_eq!(world::sync_filter_set(&v9a), Ok(pull));
    assert_eq!(
        world::sync_filter_set(&unhex(V9B)),
        Ok(SyncFilter {
            pid: 4_294_967_297,
            value: 32
        })
    );
    // Byte 17 describes the player's machine and is never read (world.md 14.4).
    let mut machine = v9a.clone();
    machine[17] = 1;
    assert_eq!(world::sync_filter_set(&machine), Ok(pull));
    let mut long = v9a.clone();
    long.push(0);
    for body in [&v9a[..17], &long[..]] {
        assert_eq!(
            world::sync_filter_set(body),
            Err(Error::BadBody {
                opcode: 5,
                len: body.len()
            })
        );
    }
    assert!(world::sync_filter_set(&[]).is_err());
}

#[test]
fn sync_leave_is_byte_for_byte() {
    assert_eq!(world::sync_leave(&[258, (1 << 32) + 1]), unhex(V10));
    assert_eq!(world::sync_leave(&[]), Vec::<u8>::new());
}

// --- Slice 3 (Amendment C): moving.

/// T: 1,791,936,000,000 ms, `00 70 b5 37 a1 01 00 00`.
const T: u64 = 1_791_936_000_000;

/// V11: a type 8 at the default start with yaw −15, direction 1, yaw offset 0, rate 100, pitches −10
/// and 10, and the facts about the machine 0x5A (world.md §16), 200 bytes.
fn v11() -> Vec<u8> {
    let mut b = unhex("00 70 b5 37 a1 01 00 00 3e 09 00 00 c9 e7 fe ff 56 fd 00 00");
    b.extend(unhex("00 00 00 00 00 f1 ff 00 00 00 00"));
    b.extend([0u8; 134]);
    b.extend(unhex("01 f1 ff 00 00 64 00 f6 ff 0a 00"));
    b.extend(unhex(
        "00 00 00 80 3e 5a 00 00 00 3f 14 00 00 00 00 00 00 00 5a 5a 5a 5a 5a 00",
    ));
    assert_eq!(b.len(), 200);
    b
}

/// V11b: V11's bytes 0–30 with `01 00 00 00` at 27, 119 bytes of 0, then V11's bytes 165–199.
fn v11b() -> Vec<u8> {
    let v = v11();
    let mut b = v[..31].to_vec();
    b[27..31].copy_from_slice(&[1, 0, 0, 0]);
    b.extend([0u8; 119]);
    b.extend(&v[165..]);
    assert_eq!(b.len(), 185);
    b
}

/// V14: V11 with time 0, its state blocks all `ff`, and the direction part `c8 e7 03 e8 03 ff ff
/// 00 80 ff 7f`: direction 200, yaw 999, yaw offset 1000, rate 65535, pitches −32768 and 32767.
fn v14() -> Vec<u8> {
    let mut b = v11();
    b[..8].fill(0);
    b[31..165].fill(0xff);
    b[165..176].copy_from_slice(&unhex("c8 e7 03 e8 03 ff ff 00 80 ff 7f"));
    b
}

/// V15: V11 with `ff ff ff 7f` at 8 and `00 80` at 25: x at `i32::MAX`, the yaw −32768.
fn v15() -> Vec<u8> {
    let mut b = v11();
    b[8..12].copy_from_slice(&unhex("ff ff ff 7f"));
    b[25..27].copy_from_slice(&unhex("00 80"));
    b
}

/// V11's position: the default start with yaw −15.
const V11_POS: [f64; 4] = [2366.0, -71735.0, 64854.0, -15.0];

/// V11's detail with `now_ms` = T + 100.
const V11_DETAIL: MoveDetail = MoveDetail {
    time: T,
    dir: 1,
    yaw_offset: 0,
    rate: 100,
    pitch: -10,
    pitch2: 10,
};

/// V14's detail with `now_ms` = T + 100: each field held to its bounds, the time to 5 s behind.
const V14_DETAIL: MoveDetail = MoveDetail {
    time: 1_791_935_995_088,
    dir: 8,
    yaw_offset: -80,
    rate: 340,
    pitch: -90,
    pitch2: 90,
};

#[test]
fn move_dir_reads_the_position_and_bounds_the_rest() {
    assert_eq!((world::MOVE_DIR_LEN, world::MOVE_DIR_SHORT_LEN), (200, 185));
    // Task 15's runs: real clients' commands came up to 2.01 s late (world.md 16.15).
    assert_eq!(
        (world::MOVE_TIME_BEHIND, world::MOVE_TIME_AHEAD),
        (5_000, 1_000)
    );
    let want = MoveDir {
        pos: V11_POS,
        detail: V11_DETAIL,
        dir_yaw: -15,
    };
    assert_eq!(world::move_dir(&v11(), T + 100), Ok(want));
    assert_eq!(
        V11_DETAIL.to_bytes()[..],
        unhex("00 70 b5 37 a1 01 00 00 01 00 00 64 00 f6 ff 0a 00")[..]
    );
    // The 185-byte form reads the same: its direction part is the last 35 bytes too.
    assert_eq!(world::move_dir(&v11b(), T + 100), Ok(want));
    // The time is held to the server's clock less 5 s to plus 1 s, then floored to 16 ms.
    let early = world::move_dir(&v11(), T - 5_000).unwrap().detail;
    assert_eq!(early.time, T - 4_000);
    assert_eq!(early.to_bytes()[..8], unhex("60 60 b5 37 a1 01 00 00")[..]);
    let edge = world::move_dir(&v11(), T + 5_000).unwrap().detail;
    assert_eq!(edge.time, T, "5 s late is kept");
    let late = world::move_dir(&v11(), T + 10_000).unwrap().detail;
    assert_eq!(late.time, T + 4_992);
    assert_eq!(late.to_bytes()[..8], unhex("80 83 b5 37 a1 01 00 00")[..]);
}

#[test]
fn move_dir_refuses_other_lengths() {
    let short = v11()[..199].to_vec();
    let mut long = v11();
    long.push(0);
    let mut worded = v11();
    worded[27] = 1;
    let mut unworded = v11b();
    unworded[27..31].fill(0);
    for body in [short, long, worded, unworded, Vec::new()] {
        assert_eq!(
            world::move_dir(&body, T),
            Err(Error::BadBody {
                opcode: 8,
                len: body.len()
            })
        );
    }
}

#[test]
fn a_hostile_move_is_bounded() {
    let m = world::move_dir(&v14(), T + 100).unwrap();
    assert_eq!(
        m.pos, V11_POS,
        "the position, as it is: the world checks it"
    );
    assert_eq!(m.detail, V14_DETAIL);
    assert_eq!(
        m.detail.to_bytes()[..],
        unhex("d0 5c b5 37 a1 01 00 00 08 b0 ff 54 01 a6 ff 5a 00")[..]
    );
    assert_eq!(m.dir_yaw, 999, "read for the trace, as it is");
    // A far point and a yaw past 360 are passed on as they are, for the world to refuse.
    let far = world::move_dir(&v15(), T + 100).unwrap();
    assert_eq!(far.pos, [2_147_483_647.0, -71735.0, 64854.0, -32768.0]);
}

#[test]
fn move_detail_round_trips_and_is_bounded_again() {
    assert_eq!(MoveDetail::LEN, 17);
    let now = T + 100;
    assert_eq!(
        MoveDetail::from_bytes(&V11_DETAIL.to_bytes(), now),
        Some(V11_DETAIL)
    );
    let bytes = V11_DETAIL.to_bytes();
    assert_eq!(MoveDetail::from_bytes(&bytes[..16], now), None);
    let mut long = bytes.to_vec();
    long.push(0);
    assert_eq!(MoveDetail::from_bytes(&long, now), None);
    assert_eq!(MoveDetail::from_bytes(&[], now), None);
    // V14's raw fields, as 17 bytes: bounded on the way in, as the decoder bounds them.
    let raw = unhex("00 00 00 00 00 00 00 00 c8 e8 03 ff ff 00 80 ff 7f");
    assert_eq!(MoveDetail::from_bytes(&raw, now), Some(V14_DETAIL));
    // Bounding is idempotent, and a yaw offset of 180 stays, as −180 becomes 180.
    assert_eq!(V14_DETAIL.bounded(now), V14_DETAIL);
    let half = |yaw_offset| {
        MoveDetail {
            yaw_offset,
            ..V11_DETAIL
        }
        .bounded(now)
        .yaw_offset
    };
    assert_eq!(
        (half(180), half(-180), half(-181), half(i16::MIN)),
        (180, 180, 179, -8)
    );
}

/// V12: V11's move relayed for pid 258.
const V12: &str = "02 01 00 00 00 00 00 00 01 00 70 b5 37 a1 01 00 00 3e 09 00 00 c9 e7 fe ff
    56 fd 00 00 f1 ff 01 f1 ff 00 00 64 00 f6 ff 0a 00";
/// V12b: pid 2^32 + 1 at (2.5, −2.5, 3 × 10^9) with yaw 540, an empty detail at T.
const V12B: &str = "01 00 00 00 01 00 00 00 01 00 70 b5 37 a1 01 00 00 03 00 00 00 fd ff ff ff
    ff ff ff 7f b4 00 00 b4 00 00 00 00 00 00 00 00 00";
/// V12c: pid 258 relayed from V14's detail and V11's position, as the world checked it.
const V12C: &str = "02 01 00 00 00 00 00 00 01 d0 5c b5 37 a1 01 00 00 3e 09 00 00 c9 e7 fe ff
    56 fd 00 00 f1 ff 08 f1 ff b0 ff 54 01 a6 ff 5a 00";

#[test]
fn move_dir_relay_is_byte_for_byte() {
    assert_eq!(world::MOVE_DIR_RELAY_LEN, 42);
    assert_eq!(
        world::move_dir_relay(258, V11_POS, &V11_DETAIL),
        unhex(V12)[..]
    );
    let still = MoveDetail {
        time: T,
        dir: 0,
        yaw_offset: 0,
        rate: 0,
        pitch: 0,
        pitch2: 0,
    };
    assert_eq!(
        world::move_dir_relay((1 << 32) + 1, [2.5, -2.5, 3.0e9, 540.0], &still),
        unhex(V12B)[..]
    );
    // NaN gives 0, the yaw folds into (−180, 180], and the clamp holds at both ends.
    let nan = world::move_dir_relay(258, [f64::NAN; 4], &still);
    assert_eq!(nan[17..31], [0; 14]);
    assert_eq!(nan[32..34], [0, 0]);
    let edges = world::move_dir_relay(258, [-3.0e9, f64::NEG_INFINITY, 0.4, -180.0], &still);
    assert_eq!(edges[17..21], i32::MIN.to_le_bytes());
    assert_eq!(edges[21..25], i32::MIN.to_le_bytes());
    assert_eq!(edges[25..29], [0; 4]);
    assert_eq!(edges[29..31], 180i16.to_le_bytes());
    assert_eq!(
        edges[32..34],
        180i16.to_le_bytes(),
        "the part repeats the header's yaw"
    );
}

#[test]
fn a_relay_from_a_hostile_body_carries_only_the_checked_position() {
    // Task 15 Step 1 (Review Focus 3): what the world relays is built from the position it checked
    // and the detail's bounded bytes, which the viewer's link bounds again; nothing else of the
    // client's body reaches another client.
    let now = T + 100;
    let hostile = world::move_dir(&v14(), now).unwrap();
    let detail = MoveDetail::from_bytes(&hostile.detail.to_bytes(), now).unwrap();
    let checked = V11_POS;
    assert_eq!(
        world::move_dir_relay(258, checked, &detail),
        unhex(V12C)[..]
    );
}

/// V13: pid 258 put at the default start with yaw −15, with the server's clock at T.
const V13: &str = "02 01 00 00 00 00 00 00 60 6f b5 37 a1 01 00 00 c0 00 3e 09 00 00 c9 e7 fe ff
    56 fd 00 00 f1 ff";

#[test]
fn move_sync_svr_is_byte_for_byte() {
    assert_eq!((world::MOVE_SYNC_SVR_LEN, world::SYNC_LAG), (32, 160));
    assert_eq!(world::sync_time(T), T - 160);
    assert_eq!(world::sync_time(T + 15), T - 160, "floored to 16 ms first");
    assert_eq!(world::sync_time(T + 16), T - 144);
    assert_eq!(world::sync_time(100), 0);
    assert_eq!(
        world::move_sync_svr(258, world::sync_time(T), V11_POS),
        unhex(V13)[..]
    );
    let far = world::move_sync_svr((1 << 32) + 1, 0, [2.5, -2.5, 3.0e9, 540.0]);
    assert_eq!(far[..8], unhex("01 00 00 00 01 00 00 00")[..]);
    assert_eq!(far[16..18], [0xc0, 0x00], "the position and the yaw");
    assert_eq!(
        far[18..32],
        unhex("03 00 00 00 fd ff ff ff ff ff ff 7f b4 00")[..]
    );
}
