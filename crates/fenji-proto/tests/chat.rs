//! Chat's messages (docs/protocol/packets/chat.md; M4 world plan Task 16): each encoder makes its
//! golden vector, and the parser reads the lines the client's own scripts send.
use fenji_proto::chat::{self, Channel, Line, Player, Refusal, Target, Transmit, channel, code};
use fenji_proto::lobby;
use fenji_proto::pickle::{self, Table, Value};

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

const LIN: Player<'static> = Player {
    pid: 258,
    name: "Lin",
    vocation: 7,
};

const MAI: Player<'static> = Player {
    pid: 300,
    name: "Mai",
    vocation: 2,
};

/// Lin's player link, as `GetPlayerLink(258, "Lin", 7, 0)` writes it (chat.md 2.2): a space before
/// and after.
const LIN_LINK: &str = " [link type=\"player\" pid=\"258\" name=\"Lin\" vocation=\"7\" gid=\"0\"] ";

/// Lin's link in a pickle: its length (64), then its bytes.
const LIN_LINK_HEX: &str = "
    08 40 20 5b 6c 69 6e 6b 20 74 79 70 65 3d 22 70 6c 61 79 65 72 22 20 70 69 64 3d 22 32 35 38
    22 20 6e 61 6d 65 3d 22 4c 69 6e 22 20 76 6f 63 61 74 69 6f 6e 3d 22 37 22 20 67 69 64 3d 22
    30 22 5d 20";

/// Mai's link in a pickle.
const MAI_LINK_HEX: &str = "
    08 40 20 5b 6c 69 6e 6b 20 74 79 70 65 3d 22 70 6c 61 79 65 72 22 20 70 69 64 3d 22 33 30 30
    22 20 6e 61 6d 65 3d 22 4d 61 69 22 20 76 6f 63 61 74 69 6f 6e 3d 22 32 22 20 67 69 64 3d 22
    30 22 5d 20";

/// V16: 378 for Lin's "hi" nearby (channel 0), 80 bytes: the id, the channel, the sender's link,
/// the text, an empty receiver, and no options or template.
fn v16() -> Vec<u8> {
    unhex(&format!(
        "07 04 7a 01 03 00 {LIN_LINK_HEX} 08 02 68 69 08 00 00 00"
    ))
}

/// V17: 378 for Lin's whisper "psst" to Mai (channel 1), 146 bytes: both ends get it, with Mai's
/// link as the receiver.
fn v17() -> Vec<u8> {
    unhex(&format!(
        "07 04 7a 01 03 01 {LIN_LINK_HEX} 08 04 70 73 73 74 {MAI_LINK_HEX} 00 00"
    ))
}

/// V17b: 378 for Mai's "all &lt;3" on the world channel (5), the text as the client wrote it.
fn v17b() -> Vec<u8> {
    unhex(&format!(
        "07 04 7a 01 03 05 {MAI_LINK_HEX} 08 09 61 6c 6c 20 26 6c 74 3b 33 08 00 00 00"
    ))
}

/// V18: 378 for the announcement "Restart at 20:00": the marquee's channel 66, no sender.
const V18: &str = "
    07 04 7a 01 03 42 08 00
    08 10 52 65 73 74 61 72 74 20 61 74 20 32 30 3a 30 30
    08 00 00 00";

/// V19: 378 for the server's line "usage: tp <name> [<name>]" to one player: channel 64, no
/// sender, the text with `&`, `<` and `>` as entities and the brackets escaped.
const V19: &str = "
    07 04 7a 01 03 40 08 00
    08 27 75 73 61 67 65 3a 20 74 70 20 26 6c 74 3b 6e 61 6d 65 26 67 74 3b 20 5c 5b 26 6c 74 3b
          6e 61 6d 65 26 67 74 3b 5c 5d
    08 00 00 00";

/// V20: 270 with `ERR_CHAT_TOO_FAST`.
const V20: &str = "02 04 0e 01 06 bf 43 f1 a4 f5 bc 88 93";

/// V21: the client's 115 for "hi" nearby: the id, the channel, the text and the nil `extra`.
const V21: &str = "04 03 73 03 00 08 02 68 69 00";

/// V22: the client's 115 for a whisper typed with `/w Mai `: `extra` is `{name = "Mai@Unknown"}`,
/// so the header counts one table (payloads.md P.7).
const V22: &str = "
    84 00 00 01 03 73 03 01 08 04 70 73 73 74
    0b 00 00 00 00 08 04 6e 61 6d 65 08 0b 4d 61 69 40 55 6e 6b 6e 6f 77 6e 00";

/// V23: the client's 115 for a whisper set by clicking Mai's name: `extra` is
/// `{pid = 300, name = "Mai@Unknown"}`.
const V23: &str = "
    84 00 00 01 03 73 03 01 08 04 70 73 73 74
    0b 00 00 00 00 08 03 70 69 64 04 2c 01 08 04 6e 61 6d 65 08 0b 4d 61 69 40 55 6e 6b 6e 6f 77
    6e 00";

/// A pickle's values after its id, as the link gets 115's arguments.
fn args(bytes: &[u8]) -> Vec<Value> {
    let mut v = pickle::decode(bytes).unwrap();
    assert_eq!(v.remove(0), Value::Int(chat::CHAT_TRANSMIT));
    v
}

fn table(pairs: Vec<(Value, Value)>) -> Value {
    Value::Table(Table {
        array: Vec::new(),
        hash: pairs,
    })
}

#[test]
fn player_link_is_the_clients_own() {
    assert_eq!(chat::player_link(&LIN), LIN_LINK);
    // EscapeLinkParam: a space, `'`, `"` and `\` as `\` and their byte in three digits.
    let odd = Player {
        pid: 1,
        name: "O'Neil \"x\" \\ y",
        vocation: 0,
    };
    assert_eq!(
        chat::player_link(&odd),
        " [link type=\"player\" pid=\"1\" name=\"O\\039Neil\\032\\034x\\034\\032\\092\\032y\" \
         vocation=\"0\" gid=\"0\"] "
    );
    // A pid above 2^32 is written whole.
    let far = Player {
        pid: (1 << 32) + 1,
        ..LIN
    };
    assert!(chat::player_link(&far).contains(" pid=\"4294967297\" "));
}

#[test]
fn say_is_byte_for_byte() {
    let hi = Line::Said {
        channel: Channel::Nearby,
        from: LIN,
        to: None,
        text: "hi",
    };
    let got = chat::say(&hi);
    assert_eq!(got, v16());
    assert_eq!(got.len(), 80);
    let psst = Line::Said {
        channel: Channel::Whisper,
        from: LIN,
        to: Some(MAI),
        text: "psst",
    };
    let got = chat::say(&psst);
    assert_eq!(got, v17());
    assert_eq!(got.len(), 146);
    let all = Line::Said {
        channel: Channel::World,
        from: MAI,
        to: None,
        text: "all &lt;3",
    };
    assert_eq!(chat::say(&all), v17b());
    let note = Line::Announce {
        text: "Restart at 20:00",
    };
    assert_eq!(chat::say(&note), unhex(V18));
    let usage = Line::System {
        text: "usage: tp <name> [<name>]",
    };
    assert_eq!(chat::say(&usage), unhex(V19));
}

#[test]
fn say_unpickles_as_the_handler_reads_it() {
    // on_chat_message takes six arguments (chat_receive.luac 0.16): channel, sender, content,
    // receiver, options and template.
    let v = pickle::decode(&v17()).unwrap();
    assert_eq!(
        v,
        [
            Value::Int(chat::SYS_CHAT_MESSAGE),
            Value::Int(channel::WHISPER),
            s(LIN_LINK),
            s("psst"),
            s(&chat::player_link(&MAI)),
            Value::Nil,
            Value::Nil,
        ]
    );
    // A line from the server names no sender and no receiver.
    let v = pickle::decode(&unhex(V18)).unwrap();
    assert_eq!(
        v[1..5],
        [
            Value::Int(channel::MARQUEE),
            s(""),
            s("Restart at 20:00"),
            s("")
        ]
    );
    let v = pickle::decode(&unhex(V19)).unwrap();
    assert_eq!(v[1], Value::Int(channel::SYSTEM));
}

#[test]
fn a_servers_line_shows_as_written() {
    // chat.md 2.5: the entities the client's own input writes, and a backslash before each
    // bracket, which its tag parser reads as the bracket itself.
    let content = |text: &str| {
        let v = pickle::decode(&chat::say(&Line::System { text })).unwrap();
        v[3].clone()
    };
    assert_eq!(
        content("a & b <c> [d] e\\f"),
        s("a &amp; b &lt;c&gt; \\[d\\] e\\f")
    );
    assert_eq!(
        content("invite link: http://127.0.0.1:7801/#invite=ABCD-EFGH"),
        s("invite link: http://127.0.0.1:7801/#invite=ABCD-EFGH")
    );
    // Players' lines and announcements are already in the client's form, checked by the world,
    // and go as they are.
    let v = pickle::decode(&chat::say(&Line::Announce {
        text: "a &lt;b&gt; [1]",
    }))
    .unwrap();
    assert_eq!(v[3], s("a &lt;b&gt; [1]"));
}

#[test]
fn refusal_is_byte_for_byte() {
    assert_eq!(chat::refusal(Refusal::TooFast), unhex(V20));
    // Each refusal's code is the `errors` sheet's ID for its literal (chat.md §5).
    for (r, c, id) in [
        (
            Refusal::Invalid,
            code::CHAT_INVALID_CONTENT,
            8_639_093_037_559_110_139,
        ),
        (
            Refusal::TooLong,
            code::CHAT_MAX_CHAR,
            -6_664_778_746_412_662_761,
        ),
        (
            Refusal::TooFast,
            code::CHAT_TOO_FAST,
            -7_815_789_390_081_211_457,
        ),
        (
            Refusal::NotOnline,
            code::CHAT_RECEIVER_OFFLINE,
            2_418_279_538_352_872_837,
        ),
        (
            Refusal::ToSelf,
            code::CHAT_TALK_TOSELF,
            437_753_046_416_001_599,
        ),
        (
            Refusal::Channel,
            code::CHAT_INVALID_CHANNEL,
            1_246_893_794_701_349_639,
        ),
    ] {
        assert_eq!(c, id, "{r:?}");
        assert_eq!(r.code(), id, "{r:?}");
        assert_eq!(chat::refusal(r), lobby::error_code(id), "{r:?}");
    }
}

#[test]
fn the_clients_lines_parse() {
    assert_eq!(
        chat::parse_transmit(&args(&unhex(V21))),
        Ok(Transmit {
            channel: Channel::Nearby,
            text: "hi".into(),
            to: None
        })
    );
    // Without the trailing nil, as a cached send would have it (chat.md 1.2), the same.
    let mut three = args(&unhex(V21));
    three.pop();
    assert_eq!(
        chat::parse_transmit(&three),
        chat::parse_transmit(&args(&unhex(V21)))
    );
    let world = [Value::Int(5), s("all &lt;3"), Value::Nil];
    assert_eq!(
        chat::parse_transmit(&world),
        Ok(Transmit {
            channel: Channel::World,
            text: "all &lt;3".into(),
            to: None
        })
    );
    // A whisper typed with /w names its target; one set by a click carries the pid too, which
    // wins (chat.md 1.4).
    assert_eq!(
        chat::parse_transmit(&args(&unhex(V22))),
        Ok(Transmit {
            channel: Channel::Whisper,
            text: "psst".into(),
            to: Some(Target::Name("Mai@Unknown".into()))
        })
    );
    let by_pid = Ok(Transmit {
        channel: Channel::Whisper,
        text: "psst".into(),
        to: Some(Target::Pid(300)),
    });
    assert_eq!(chat::parse_transmit(&args(&unhex(V23))), by_pid);
    // A Lua table's pairs come in any order.
    let swapped = [
        Value::Int(1),
        s("psst"),
        table(vec![
            (s("name"), s("Mai@Unknown")),
            (s("pid"), Value::Int(300)),
        ]),
    ];
    assert_eq!(chat::parse_transmit(&swapped), by_pid);
    // Values after `extra` are ignored.
    let mut more = args(&unhex(V23));
    more.push(Value::Int(9));
    assert_eq!(chat::parse_transmit(&more), by_pid);
}

#[test]
fn a_whisper_takes_what_it_can_of_its_target() {
    let whisper = |extra: Value| chat::parse_transmit(&[Value::Int(1), s("psst"), extra]);
    let to = |t: Option<Target>| {
        Ok(Transmit {
            channel: Channel::Whisper,
            text: "psst".into(),
            to: t,
        })
    };
    // A pid alone, or one written as a float with no fraction (the client's tonumber).
    assert_eq!(
        whisper(table(vec![(s("pid"), Value::Int(300))])),
        to(Some(Target::Pid(300)))
    );
    assert_eq!(
        whisper(table(vec![(s("pid"), Value::Float(300.0))])),
        to(Some(Target::Pid(300)))
    );
    // A pid that isn't a whole number gives way to the name.
    for pid in [
        Value::Float(300.5),
        s("300"),
        Value::Bool(true),
        Value::Float(f64::NAN),
    ] {
        assert_eq!(
            whisper(table(vec![(s("pid"), pid.clone()), (s("name"), s("Mai"))])),
            to(Some(Target::Name("Mai".into()))),
            "{pid:?}"
        );
    }
    // No usable target: the world refuses it as not online.
    for extra in [
        Value::Nil,
        Value::Int(300),
        s("Mai"),
        table(vec![]),
        table(vec![(s("name"), Value::Str(vec![0xff, 0xfe]))]),
        table(vec![(s("name"), Value::Int(1))]),
    ] {
        assert_eq!(whisper(extra.clone()), to(None), "{extra:?}");
    }
    assert_eq!(
        chat::parse_transmit(&[Value::Int(1), s("psst")]),
        to(None),
        "no extra at all"
    );
    // Another channel's `extra` is ignored.
    assert_eq!(
        chat::parse_transmit(&[
            Value::Int(0),
            s("hi"),
            table(vec![(s("pid"), Value::Int(300))])
        ]),
        Ok(Transmit {
            channel: Channel::Nearby,
            text: "hi".into(),
            to: None
        })
    );
}

#[test]
fn other_channels_are_refused() {
    for ch in [
        2,
        3,
        4,
        6,
        7,
        8,
        9,
        10,
        14,
        17,
        18,
        50,
        52,
        64,
        66,
        -1,
        i64::MAX,
        i64::MIN,
    ] {
        assert_eq!(
            chat::parse_transmit(&[Value::Int(ch), s("hi"), Value::Nil]),
            Err(Refusal::Channel),
            "{ch}"
        );
    }
    for ch in [
        Value::Float(0.0),
        s("0"),
        Value::Nil,
        Value::Bool(false),
        table(vec![]),
    ] {
        assert_eq!(
            chat::parse_transmit(&[ch.clone(), s("hi"), Value::Nil]),
            Err(Refusal::Channel),
            "{ch:?}"
        );
    }
    assert_eq!(chat::parse_transmit(&[]), Err(Refusal::Channel));
}

#[test]
fn a_line_that_isnt_text_is_invalid() {
    for text in [
        Value::Nil,
        Value::Int(1),
        Value::Float(1.5),
        Value::Bool(true),
        table(vec![]),
        Value::Str(vec![0x68, 0xff, 0x69]),
    ] {
        assert_eq!(
            chat::parse_transmit(&[Value::Int(0), text.clone(), Value::Nil]),
            Err(Refusal::Invalid),
            "{text:?}"
        );
    }
    assert_eq!(
        chat::parse_transmit(&[Value::Int(5)]),
        Err(Refusal::Invalid),
        "no text"
    );
}

#[test]
fn channels_have_the_clients_numbers() {
    assert_eq!(Channel::Nearby.number(), channel::NEARBY);
    assert_eq!(Channel::Whisper.number(), channel::WHISPER);
    assert_eq!(Channel::World.number(), channel::WORLD);
    assert_eq!(
        [
            channel::NEARBY,
            channel::WHISPER,
            channel::WORLD,
            channel::SYSTEM,
            channel::MARQUEE
        ],
        [0, 1, 5, 64, 66]
    );
    assert_eq!((chat::CHAT_TRANSMIT, chat::SYS_CHAT_MESSAGE), (115, 378));
}

#[test]
fn a_lines_channel_is_the_one_say_writes() {
    let said = |channel| Line::Said {
        channel,
        from: LIN,
        to: None,
        text: "hi",
    };
    for line in [
        said(Channel::Nearby),
        said(Channel::Whisper),
        said(Channel::World),
        Line::Announce { text: "a" },
        Line::System { text: "b" },
    ] {
        let v = pickle::decode(&chat::say(&line)).unwrap();
        assert_eq!(v[1], Value::Int(line.channel()), "{line:?}");
    }
    assert_eq!(Line::Announce { text: "a" }.channel(), channel::MARQUEE);
    assert_eq!(Line::System { text: "b" }.channel(), channel::SYSTEM);
}
