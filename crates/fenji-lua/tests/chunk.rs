mod common;

use fenji_lua::{Chunk, Dialect, Error, Function, header, parse, write};

#[test]
fn stock_round_trip_is_byte_exact() {
    for bytes in common::fixtures() {
        let chunk = parse(&bytes, Dialect::Stock53).expect("a stock chunk parses");
        assert_eq!(write(&chunk, Dialect::Stock53), bytes);
    }
}

#[test]
fn client_dialect_is_stock_without_the_size_t_byte() {
    for bytes in common::fixtures() {
        let chunk = parse(&bytes, Dialect::Stock53).unwrap();
        let client = write(&chunk, Dialect::Client);
        let mut expected = bytes.clone();
        expected.remove(13); // sizes: int, size_t, Instruction, lua_Integer, lua_Number
        assert_eq!(client, expected);
        assert_eq!(parse(&client, Dialect::Client).unwrap(), chunk);
        assert_eq!(Dialect::detect(&client), Some(Dialect::Client));
        assert_eq!(Dialect::detect(&bytes), Some(Dialect::Stock53));
    }
}

#[test]
fn the_wrong_dialect_is_refused() {
    let bytes = common::luac("print('hello')", true);
    assert_eq!(
        parse(&bytes, Dialect::Client),
        Err(Error::Header(Dialect::Client))
    );
}

#[test]
fn stripped_fixtures_look_like_the_clients() {
    let chunk = parse(&common::luac(common::MODULE, true), Dialect::Stock53).unwrap();
    assert!(chunk.main.source.is_none());
    assert!(chunk.main.line_info.is_empty() && chunk.main.loc_vars.is_empty());
}

#[test]
fn every_truncation_is_an_error() {
    let chunk = parse(&common::luac(common::MODULE, true), Dialect::Stock53).unwrap();
    let bytes = write(&chunk, Dialect::Client);
    for n in 0..bytes.len() {
        assert!(
            parse(&bytes[..n], Dialect::Client).is_err(),
            "prefix of {n} bytes"
        );
    }
}

#[test]
fn trailing_bytes_are_refused() {
    let mut bytes = common::luac("print('hello')", true);
    bytes.push(0);
    assert!(matches!(
        parse(&bytes, Dialect::Stock53),
        Err(Error::Trailing { extra: 1, .. })
    ));
}

/// Byte offset of the main function's instruction count in a stripped stock chunk: the header, the
/// upvalue count, an empty source (1 byte), two line numbers, then three bytes.
fn code_count_at() -> usize {
    header(Dialect::Stock53).len() + 1 + 1 + 8 + 3
}

#[test]
fn absurd_count_is_refused() {
    let mut bytes = common::luac("print('hello')", true);
    let at = code_count_at();
    bytes[at..at + 4].copy_from_slice(&0x7FFF_FFFFi32.to_le_bytes());
    assert!(matches!(
        parse(&bytes, Dialect::Stock53),
        Err(Error::BadCount { .. })
    ));
}

#[test]
fn unknown_constant_tag_is_refused() {
    let bytes = common::luac("return 'x'", true);
    let chunk = parse(&bytes, Dialect::Stock53).unwrap();
    let tag_at = code_count_at() + 4 + 4 * chunk.main.code.len() + 4; // count, code, constant count
    assert_eq!(
        bytes[tag_at], 4,
        "the first constant is the short string 'x'"
    );
    let mut bad = bytes.clone();
    bad[tag_at] = 7;
    assert!(matches!(
        parse(&bad, Dialect::Stock53),
        Err(Error::ConstantType { tag: 7, .. })
    ));
}

#[test]
fn deep_nesting_is_refused() {
    let mut f = Function::default();
    for _ in 0..300 {
        f = Function {
            protos: vec![f],
            ..Function::default()
        };
    }
    let bytes = write(
        &Chunk {
            upvalue_count: 0,
            main: f,
        },
        Dialect::Stock53,
    );
    assert!(matches!(
        parse(&bytes, Dialect::Stock53),
        Err(Error::TooDeep { .. })
    ));
}
