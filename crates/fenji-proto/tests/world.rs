//! The world's entering messages (docs/protocol/packets/world.md; M4 world plan Amendment A): each
//! encoder makes its golden vector, and each decoder reads it back.
use fenji_proto::Error;
use fenji_proto::lobby::Appearance;
use fenji_proto::pickle::{self, Table, Value};
use fenji_proto::world::{self, BlobValue, SelfActor};

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
    d7 90 04 76 3a 35 3b 00
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
        face: Some("v:5;".into()),
        equips_id: 2,
        portrait: 169 << 16,
    }
}

fn lin(appearance: &Appearance) -> SelfActor<'_> {
    SelfActor {
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
fn sync_enter_self_is_byte_for_byte() {
    let look = lin_look();
    let got = world::sync_enter_self(&lin(&look));
    assert_eq!(got, unhex(V7));
    assert_eq!(got.len(), 141);
}

fn indexes(body: &[u8]) -> Vec<u16> {
    world::decode_blob(&body[14..])
        .unwrap()
        .iter()
        .map(|(i, _)| *i)
        .collect()
}

#[test]
fn sync_enter_self_leaves_out_what_is_unset() {
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
    let got = indexes(&world::sync_enter_self(&lin(&bare)));
    assert!(!got.contains(&177), "no voice, no 177: {got:?}");
    assert!(!got.contains(&215), "no face, no 215: {got:?}");
    let third = Appearance {
        equips_id: 3,
        ..lin_look()
    };
    assert!(!indexes(&world::sync_enter_self(&lin(&third))).contains(&222));
    let look = lin_look();
    let ten = SelfActor {
        vocation: 10,
        ..lin(&look)
    };
    assert!(!indexes(&world::sync_enter_self(&ten)).contains(&222));
}

#[test]
fn the_header_carries_the_template_when_it_fits() {
    // world.md 5.4: type 1, difficulty 255, template vocation × 10000 as a u32, or 0.
    let look = lin_look();
    for (vocation, template) in [(0, 0u32), (7, 70_000), (9, 90_000), (-1, 0), (500_000, 0)] {
        let body = world::sync_enter_self(&SelfActor {
            vocation,
            ..lin(&look)
        });
        assert_eq!(&body[..8], &258u64.to_le_bytes(), "{vocation}");
        assert_eq!(body[8..10], [1, 255], "{vocation}");
        assert_eq!(body[10..14], template.to_le_bytes(), "{vocation}");
    }
    let far = world::sync_enter_self(&SelfActor {
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
        (215, BlobValue::Str(b"v:5;".to_vec())),
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
