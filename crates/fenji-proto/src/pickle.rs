//! The client's own serialisation format, its "pickle" (docs/protocol/payloads.md): script
//! messages and the auth payload. FUN_1400D5850 (header FUN_1400D56D0, values FUN_1400D4AC0)
//! unpacks it; `kslua_pickle_pack` (FUN_1400D54F0) packs it. A packet is a count of values, then
//! the values, each a tag and its data, little-endian. A packet of 128 bytes or more usually
//! arrives LZ4-compressed (payloads.md P.1, P.5). Fenji reads every form the client writes,
//! compressed or not: the header's table count, table references and "magic" objects included
//! (payloads.md P.3), within [`MAX_DECODED`] values a packet. Like the client's unpacker, it also
//! takes forms the client's packers never write, such as an integer in a wider tag than it needs.
//! It writes every form uncompressed, which the client reads too, and refuses an empty packet,
//! which the client reads as no values. The client's table caches use the same format:
//! `fenji_data::cache` reads those into an arena, as they are far larger; fenji-proto depends on
//! nothing internal, so the two share the format, not code.
use crate::{Error, Result};

/// One value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Vec<u8>),
    Table(Table),
    /// Tag 0C, a null light userdata.
    Null,
    /// Tags 0E, 0F and 10: the table with this index in the order the client numbers them, which
    /// [`tables`] lists. Only the first tables the header counts can be named (FUN_1400D4AC0). A
    /// table is numbered before its contents, so a reference may name the table it sits in or one
    /// around it: code that follows references must guard against cycles. Nothing in fenji-proto
    /// follows one.
    Ref(u32),
    /// Tag 0D: a "magic" object, which the client rebuilds with the constructor registered under
    /// `name` (payloads.md P.3), e.g. a `Float4` from four numbers.
    Magic {
        name: Vec<u8>,
        arg: Box<Value>,
    },
}

impl Value {
    /// What kind of value this is, for a message about it: a peer's value is never printed whole.
    pub fn kind(&self) -> &'static str {
        match self {
            Value::Nil => "nil",
            Value::Bool(_) => "a boolean",
            Value::Int(_) => "an integer",
            Value::Float(_) => "a float",
            Value::Str(_) => "a string",
            Value::Table(_) => "a table",
            Value::Null => "a null userdata",
            Value::Ref(_) => "a table reference",
            Value::Magic { .. } => "a magic object",
        }
    }
}

/// A Lua table: the array part (indices 1, 2, …) and the other pairs, in wire order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Table {
    pub array: Vec<Value>,
    pub hash: Vec<(Value, Value)>,
}

/// Tables nest at most this deep. The client counts from 1 and refuses a table that takes the
/// count past 32, both unpacking ("unpack table too depth!", FUN_1400D5850, FUN_1400D4AC0) and
/// packing (FUN_1400D33F0, FUN_1400D3950): payloads.md P.2. Fenji's decoder also counts each magic
/// object as a level, which the client doesn't (P.8), so a chain of them stays bounded.
pub const MAX_DEPTH: usize = 31;
/// The largest raw size a compressed packet may declare ("pkt bad size, too large zip size",
/// FUN_1400D55C0).
pub const MAX_UNZIPPED: usize = 0x10_0000;
/// The most top-level values a packet holds (the count byte's low 7 bits). With the table count
/// (a packet with a [`Value::Ref`]), one fewer: its header byte, `0x80 | 127`, would be `FF`, the
/// compressed marker (payloads.md P.1).
pub const MAX_VALUES: usize = 0x7f;
/// The most values one packet decodes into, every value read counting, a table's end marker
/// included (#84). A compressed packet of a few KiB can unzip to [`MAX_UNZIPPED`] bytes, about a
/// million values; this bounds what a peer can make the server allocate per message.
pub const MAX_DECODED: usize = 1 << 16;
/// The most array slots reserved up front for a table, whatever count it declares. Nested tables
/// may each declare a count as large as the rest of the input; reserving that much at every level
/// would let a 1 MiB message claim gigabytes. Past this, the array grows as values are read, and
/// every value takes at least one input byte.
const PREALLOC: usize = 64;

fn err<T>(what: impl Into<String>) -> Result<T> {
    Err(Error::Pickle(what.into()))
}

/// What the client's packers do with a finished packet (FUN_1400D53D0, payloads.md P.5): from
/// 0x80 bytes up, LZ4-compress it and keep the result when it and its 4-byte header are shorter:
/// `FF`, the raw size as a 24-bit big-endian number, then the block (P.1). Fenji's server writes
/// plain packets; this is for the client simulator, so tests send what the real client sends.
pub fn encode_compressed(values: &[Value]) -> Result<Vec<u8>> {
    let plain = encode(values)?;
    if plain.len() < 0x80 || plain.len() > MAX_UNZIPPED {
        return Ok(plain);
    }
    let block = lz4_flex::block::compress(&plain);
    if block.len() + 4 >= plain.len() {
        return Ok(plain);
    }
    let raw = u32::try_from(plain.len()).expect("at most MAX_UNZIPPED");
    let mut out = Vec::with_capacity(block.len() + 4);
    out.push(0xFF);
    out.extend_from_slice(&raw.to_be_bytes()[1..]);
    out.extend_from_slice(&block);
    Ok(out)
}

/// Encodes `values` with the shortest integer, string and reference forms. A packet with a
/// [`Value::Ref`] gets the header's table count, as the client's packer writes it (every table
/// counted); one without keeps the one-byte count.
pub fn encode(values: &[Value]) -> Result<Vec<u8>> {
    if values.len() > MAX_VALUES {
        return err(format!("{} values, at most {MAX_VALUES}", values.len()));
    }
    let mut w = Writer {
        out: Vec::new(),
        registered: 0,
    };
    if values.iter().any(has_ref) {
        if values.len() == MAX_VALUES {
            return err(format!(
                "{MAX_VALUES} values with table references: the header byte would be FF, which reads as compressed"
            ));
        }
        let n = tables(values).len();
        let Some(n) = u32::try_from(n).ok().filter(|&n| n < 1 << 24) else {
            return err(format!("{n} tables, at most 2^24 - 1"));
        };
        w.out.push(0x80 | values.len() as u8);
        w.out.extend_from_slice(&n.to_be_bytes()[1..]);
    } else {
        w.out.push(values.len() as u8);
    }
    for v in values {
        w.put(v, 0)?;
    }
    Ok(w.out)
}

/// Every table in `values`, in the order the client numbers them: a table before its contents,
/// its array part, then each key and value, in wire order. [`Value::Ref`] indexes this list.
pub fn tables(values: &[Value]) -> Vec<&Table> {
    fn walk<'a>(v: &'a Value, out: &mut Vec<&'a Table>) {
        match v {
            Value::Table(t) => {
                out.push(t);
                for a in &t.array {
                    walk(a, out);
                }
                for (k, v) in &t.hash {
                    walk(k, out);
                    walk(v, out);
                }
            }
            Value::Magic { arg, .. } => walk(arg, out),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for v in values {
        walk(v, &mut out);
    }
    out
}

fn has_ref(v: &Value) -> bool {
    match v {
        Value::Ref(_) => true,
        Value::Table(t) => {
            t.array.iter().any(has_ref) || t.hash.iter().any(|(k, v)| has_ref(k) || has_ref(v))
        }
        Value::Magic { arg, .. } => has_ref(arg),
        _ => false,
    }
}

struct Writer {
    out: Vec<u8>,
    /// Tables written so far: a reference may name only these.
    registered: u32,
}

impl Writer {
    fn put(&mut self, v: &Value, depth: usize) -> Result<()> {
        let out = &mut self.out;
        match v {
            Value::Nil => out.push(0x00),
            Value::Bool(true) => out.push(0x01),
            Value::Bool(false) => out.push(0x02),
            Value::Int(n) => {
                if let Ok(x) = i8::try_from(*n) {
                    out.push(0x03);
                    out.extend_from_slice(&x.to_le_bytes());
                } else if let Ok(x) = i16::try_from(*n) {
                    out.push(0x04);
                    out.extend_from_slice(&x.to_le_bytes());
                } else if let Ok(x) = i32::try_from(*n) {
                    out.push(0x05);
                    out.extend_from_slice(&x.to_le_bytes());
                } else {
                    out.push(0x06);
                    out.extend_from_slice(&n.to_le_bytes());
                }
            }
            Value::Float(f) => {
                out.push(0x07);
                out.extend_from_slice(&f.to_le_bytes());
            }
            Value::Str(s) => self.put_str(s)?,
            Value::Table(t) => {
                if depth >= MAX_DEPTH {
                    return err(format!("tables nested over {MAX_DEPTH} deep"));
                }
                let n =
                    u32::try_from(t.array.len()).or_else(|_| err("an array over 2^32 values"))?;
                out.push(0x0b);
                out.extend_from_slice(&n.to_le_bytes());
                self.registered += 1;
                for a in &t.array {
                    self.put(a, depth + 1)?;
                }
                for (k, v) in &t.hash {
                    match k {
                        Value::Nil => return err("a nil key would end the table"),
                        Value::Float(f) if f.is_nan() => {
                            return err("a NaN key, which Lua refuses as a table index");
                        }
                        _ => {}
                    }
                    self.put(k, depth + 1)?;
                    self.put(v, depth + 1)?;
                }
                self.out.push(0x00);
            }
            Value::Null => out.push(0x0c),
            Value::Ref(i) => {
                if *i >= self.registered {
                    return err(format!("a reference to table {i}, not yet written"));
                }
                if let Ok(x) = u8::try_from(*i) {
                    out.extend([0x0e, x]);
                } else if let Ok(x) = u16::try_from(*i) {
                    out.push(0x0f);
                    out.extend_from_slice(&x.to_le_bytes());
                } else {
                    out.push(0x10);
                    out.extend_from_slice(&i.to_le_bytes());
                }
            }
            Value::Magic { name, arg } => {
                if depth >= MAX_DEPTH {
                    return err(format!("values nested over {MAX_DEPTH} deep"));
                }
                out.push(0x0d);
                self.put_str(name)?;
                self.put(arg, depth + 1)?;
            }
        }
        Ok(())
    }

    fn put_str(&mut self, s: &[u8]) -> Result<()> {
        let out = &mut self.out;
        if let Ok(n) = u8::try_from(s.len()) {
            out.push(0x08);
            out.push(n);
        } else if let Ok(n) = u16::try_from(s.len()) {
            out.push(0x09);
            out.extend_from_slice(&n.to_le_bytes());
        } else if let Ok(n) = u32::try_from(s.len()) {
            out.push(0x0a);
            out.extend_from_slice(&n.to_le_bytes());
        } else {
            return err("a string over 4 GiB");
        }
        out.extend_from_slice(s);
        Ok(())
    }
}

/// Decodes a whole packet, compressed or not.
pub fn decode(bytes: &[u8]) -> Result<Vec<Value>> {
    if bytes.first() == Some(&0xff) {
        return values(&unzip(bytes)?);
    }
    values(bytes)
}

/// A compressed packet (payloads.md P.1): FF, a 24-bit big-endian raw size, then an LZ4 block
/// that decodes into at most that many bytes (FUN_1400D55C0, FUN_1400CF400).
fn unzip(bytes: &[u8]) -> Result<Vec<u8>> {
    let Some(&[a, b, c]) = bytes.get(1..4) else {
        return err("a compressed packet without its raw size");
    };
    let raw = u32::from_be_bytes([0, a, b, c]) as usize;
    if raw > MAX_UNZIPPED {
        return err(format!(
            "a compressed packet of {raw} bytes, at most {MAX_UNZIPPED}"
        ));
    }
    let mut out = vec![0; raw];
    let n = lz4_flex::block::decompress_into(&bytes[4..], &mut out)
        .or_else(|e| err(format!("LZ4: {e}")))?;
    out.truncate(n);
    Ok(out)
}

/// The count byte and the values, uncompressed.
fn values(bytes: &[u8]) -> Result<Vec<Value>> {
    let mut r = Reader {
        buf: bytes,
        at: 0,
        declared: 0,
        registered: 0,
        decoded: 0,
    };
    let mut count = r.byte()?;
    // The 0x80 bit: a 24-bit big-endian count of tables references may name (FUN_1400D56D0).
    if count >= 0x80 {
        let [a, b, c] = r.array::<3>()?;
        r.declared = u32::from_be_bytes([0, a, b, c]) as usize;
        count &= 0x7f;
    }
    let mut out = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        out.push(r.value(0)?);
    }
    if r.at != bytes.len() {
        return err(format!("{} bytes after the last value", bytes.len() - r.at));
    }
    Ok(out)
}

struct Reader<'a> {
    buf: &'a [u8],
    at: usize,
    /// How many tables references may name: the header's count.
    declared: usize,
    /// Tables numbered so far: the first `declared` read.
    registered: usize,
    /// Values read so far, against [`MAX_DECODED`].
    decoded: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.buf.len());
        let Some(end) = end else {
            return err(format!("truncated at {}", self.at));
        };
        let got = &self.buf[self.at..end];
        self.at = end;
        Ok(got)
    }

    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().expect("N bytes"))
    }

    fn remaining(&self) -> usize {
        self.buf.len() - self.at
    }

    fn value(&mut self, depth: usize) -> Result<Value> {
        self.decoded += 1;
        if self.decoded > MAX_DECODED {
            return err(format!("over {MAX_DECODED} values in one packet"));
        }
        let tag = self.byte()?;
        Ok(match tag {
            0x00 => Value::Nil,
            0x01 => Value::Bool(true),
            0x02 => Value::Bool(false),
            0x03 => Value::Int(i64::from(i8::from_le_bytes(self.array()?))),
            0x04 => Value::Int(i64::from(i16::from_le_bytes(self.array()?))),
            0x05 => Value::Int(i64::from(i32::from_le_bytes(self.array()?))),
            0x06 => Value::Int(i64::from_le_bytes(self.array()?)),
            0x07 => Value::Float(f64::from_le_bytes(self.array()?)),
            0x08..=0x0a => {
                let n = match tag {
                    0x08 => usize::from(self.byte()?),
                    0x09 => usize::from(u16::from_le_bytes(self.array()?)),
                    _ => u32::from_le_bytes(self.array()?) as usize,
                };
                Value::Str(self.take(n)?.to_vec())
            }
            0x0b => {
                if depth >= MAX_DEPTH {
                    return err(format!("tables nested over {MAX_DEPTH} deep"));
                }
                let n = u32::from_le_bytes(self.array()?) as usize;
                // Every value takes at least one byte, so a larger count can't be honest.
                if n > self.remaining() {
                    return err(format!(
                        "an array of {n} values in {} bytes",
                        self.remaining()
                    ));
                }
                // Numbered before its contents, while the header's count allows (FUN_1400D4AC0).
                if self.registered < self.declared {
                    self.registered += 1;
                }
                let mut t = Table {
                    array: Vec::with_capacity(n.min(PREALLOC)),
                    hash: Vec::new(),
                };
                for _ in 0..n {
                    t.array.push(self.value(depth + 1)?);
                }
                loop {
                    let k = self.value(depth + 1)?;
                    if k == Value::Nil {
                        break;
                    }
                    let v = self.value(depth + 1)?;
                    t.hash.push((k, v));
                }
                Value::Table(t)
            }
            0x0c => Value::Null,
            0x0d => {
                // A magic object counts as a level, which keeps a chain of them bounded.
                if depth >= MAX_DEPTH {
                    return err(format!("values nested over {MAX_DEPTH} deep"));
                }
                match self.value(depth + 1)? {
                    // A nil name: the next value stands for itself (FUN_1400D4AC0, case 0D).
                    Value::Nil => self.value(depth + 1)?,
                    Value::Str(name) => Value::Magic {
                        name,
                        arg: Box::new(self.value(depth + 1)?),
                    },
                    other => return err(format!("a magic object named by {}", other.kind())),
                }
            }
            0x0e..=0x10 => {
                let i = match tag {
                    0x0e => usize::from(self.byte()?),
                    0x0f => usize::from(u16::from_le_bytes(self.array()?)),
                    _ => u32::from_le_bytes(self.array()?) as usize,
                };
                if i >= self.registered {
                    return err(format!("a reference to table {i}, not yet registered"));
                }
                Value::Ref(i as u32)
            }
            other => return err(format!("unknown tag {other:#04x}")),
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn encode_compressed_matches_the_clients_rule() {
        let small = vec![Value::Str(b"short".to_vec())];
        assert_eq!(
            encode_compressed(&small).unwrap(),
            encode(&small).unwrap(),
            "under 0x80: plain"
        );
        let big = vec![Value::Str(vec![b'a'; 300])];
        let z = encode_compressed(&big).unwrap();
        assert_eq!(z[0], 0xFF);
        let raw = encode(&big).unwrap();
        assert_eq!(
            u32::from_be_bytes([0, z[1], z[2], z[3]]) as usize,
            raw.len()
        );
        assert!(z.len() < raw.len());
        assert_eq!(decode(&z).unwrap(), big);
    }

    #[test]
    fn encode_compressed_starts_at_exactly_0x80_bytes() {
        // A string of n bytes packs to n + 3 bytes (count, tag, length) when n < 256.
        let below = vec![Value::Str(vec![b'a'; 124])];
        assert_eq!(encode(&below).unwrap().len(), 127);
        assert_eq!(encode_compressed(&below).unwrap(), encode(&below).unwrap());
        let at = vec![Value::Str(vec![b'a'; 125])];
        assert_eq!(encode(&at).unwrap().len(), 128);
        assert_eq!(encode_compressed(&at).unwrap()[0], 0xFF);
    }

    #[test]
    fn encode_compressed_keeps_the_plain_form_when_lz4_doesnt_help() {
        let noise: Vec<u8> = (0..200u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
            .collect();
        let v = vec![Value::Str(noise)];
        assert_eq!(encode_compressed(&v).unwrap(), encode(&v).unwrap());
    }

    use super::*;

    fn s(b: &str) -> Value {
        Value::Str(b.as_bytes().to_vec())
    }

    fn unhex(h: &str) -> Vec<u8> {
        let h: String = h.split_whitespace().collect();
        (0..h.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn set_net_connected_body() {
        // payloads.md: 414 as i16, nil, an empty table, nil.
        let v = [
            Value::Int(414),
            Value::Nil,
            Value::Table(Table::default()),
            Value::Nil,
        ];
        assert_eq!(
            encode(&v).unwrap(),
            unhex("04 04 9E 01 00 0B 00 00 00 00 00 00")
        );
        assert_eq!(
            decode(&unhex("04 04 9E 01 00 0B 00 00 00 00 00 00")).unwrap(),
            v
        );
    }

    #[test]
    fn gateway_402_body() {
        assert_eq!(encode(&[Value::Int(402)]).unwrap(), unhex("01 04 92 01"));
    }

    #[test]
    fn integer_widths_at_their_boundaries() {
        let cases: [(i64, &str); 8] = [
            (-128, "01 03 80"),
            (127, "01 03 7F"),
            (-129, "01 04 7F FF"),
            (128, "01 04 80 00"),
            (32767, "01 04 FF 7F"),
            (32768, "01 05 00 80 00 00"),
            (2_147_483_648, "01 06 00 00 00 80 00 00 00 00"),
            (-1, "01 03 FF"),
        ];
        for (n, want) in cases {
            assert_eq!(encode(&[Value::Int(n)]).unwrap(), unhex(want), "{n}");
            assert_eq!(decode(&unhex(want)).unwrap(), [Value::Int(n)], "{n}");
        }
    }

    #[test]
    fn floats_booleans_and_null() {
        let v = [
            Value::Float(1.5),
            Value::Bool(true),
            Value::Bool(false),
            Value::Null,
        ];
        let want = unhex("04 07 00 00 00 00 00 00 F8 3F 01 02 0C");
        assert_eq!(encode(&v).unwrap(), want);
        assert_eq!(decode(&want).unwrap(), v);
    }

    #[test]
    fn string_length_forms() {
        for (len, tag, prefix) in [
            (255usize, 0x08u8, 1usize),
            (256, 0x09, 2),
            (65_536, 0x0A, 4),
        ] {
            let v = [Value::Str(vec![b'x'; len])];
            let e = encode(&v).unwrap();
            assert_eq!(e[1], tag, "len {len}");
            assert_eq!(e.len(), 2 + prefix + len, "len {len}");
            assert_eq!(decode(&e).unwrap(), v, "len {len}");
        }
    }

    #[test]
    fn a_table_with_array_and_hash_parts() {
        let t = Table {
            array: vec![Value::Int(1), s("a")],
            hash: vec![(s("k"), Value::Bool(true))],
        };
        let want = unhex("01 0B 02 00 00 00 03 01 08 01 61 08 01 6B 01 00");
        assert_eq!(encode(&[Value::Table(t.clone())]).unwrap(), want);
        assert_eq!(decode(&want).unwrap(), [Value::Table(t)]);
    }

    #[test]
    fn every_truncation_is_an_error() {
        // Every vector in these tests, cut short anywhere, is refused.
        let mut vectors = vec![
            unhex("82 00 00 01  04 92 01  0B 00 00 00 00 08 01 61 03 01 00"),
            unhex(
                "81 00 00 02  0B 00 00 00 00  08 01 61 0E 00  08 01 62 0B 00 00 00 00 00  08 01 63 0E 01  00",
            ),
            unhex("01 0D 00 03 07"),
            unhex("04 04 9E 01 00 0B 02 00 00 00 03 01 08 01 61 08 01 6B 01 00 08 02 68 69"),
            unhex("04 04 9E 01 00 0B 00 00 00 00 00 00"),
            unhex("01 04 92 01"),
            unhex("01 0B 02 00 00 00 03 01 08 01 61 08 01 6B 01 00"),
            unhex("FF 00 00 18  78 01 08 15 61 62 63 64 04 00  50 57 58 59 5A 21"),
        ];
        let values = [
            Value::Int(-129),
            Value::Int(32_768),
            Value::Int(1 << 31),
            Value::Float(1.5),
            Value::Bool(false),
            Value::Bool(true),
            Value::Null,
            Value::Nil,
        ];
        vectors.extend(
            values
                .iter()
                .map(|v| encode(std::slice::from_ref(v)).unwrap()),
        );
        for len in [255, 256, 65_536] {
            vectors.push(encode(&[Value::Str(vec![b'x'; len])]).unwrap());
        }
        for full in &vectors {
            assert!(decode(full).is_ok(), "{:02x?}", &full[..full.len().min(8)]);
            for cut in 0..full.len() {
                assert!(
                    decode(&full[..cut]).is_err(),
                    "cut {cut} of {:02x?}",
                    &full[..full.len().min(8)]
                );
            }
        }
    }

    #[test]
    fn refused_forms() {
        for bad in [
            "01 0D 00",          // magic without its argument
            "01 0E 00",          // a reference with no tables declared
            "01 0F 00 00",       // the same, u16
            "01 10 00 00 00 00", // the same, u32
            "01 11",             // unknown tag
            "01 00 00",          // trailing byte
        ] {
            assert!(decode(&unhex(bad)).is_err(), "{bad}");
        }
    }

    fn table(hash: Vec<(Value, Value)>) -> Value {
        Value::Table(Table {
            array: vec![],
            hash,
        })
    }

    #[test]
    fn a_client_message_with_a_table_uses_the_reference_header() {
        // FUN_1400D5280 and FUN_1400D3950 (payloads.md P.3): the client's packer registers every
        // table it packs, so a message with a table has the count byte's 0x80 bit set and a
        // 24-bit big-endian count of tables. (402, {a = 1}):
        let bytes = unhex("82 00 00 01  04 92 01  0B 00 00 00 00 08 01 61 03 01 00");
        assert_eq!(
            decode(&bytes).unwrap(),
            [Value::Int(402), table(vec![(s("a"), Value::Int(1))])]
        );
    }

    #[test]
    fn references_name_tables_in_creation_order() {
        // FUN_1400D4AC0: a 0B table is registered before its contents, while fewer than the
        // header's count are registered, and 0E, 0F or 10 names one by index. The root is
        // table 0 and refers to itself; `b` is table 1, which `c` names again.
        let bytes = unhex(
            "81 00 00 02  0B 00 00 00 00  08 01 61 0E 00  08 01 62 0B 00 00 00 00 00  08 01 63 0E 01  00",
        );
        let want = table(vec![
            (s("a"), Value::Ref(0)),
            (s("b"), Value::Table(Table::default())),
            (s("c"), Value::Ref(1)),
        ]);
        let got = decode(&bytes).unwrap();
        assert_eq!(got, std::slice::from_ref(&want));
        let order = tables(&got);
        assert_eq!(order.len(), 2);
        assert_eq!(*order[1], Table::default());
        // Written back the same way: the header counts every table, the references are short.
        assert_eq!(encode(&[want]).unwrap(), bytes);
        for wider in [
            "81 00 00 01 0B 00 00 00 00 08 01 61 0F 00 00 00",
            "81 00 00 01 0B 00 00 00 00 08 01 61 10 00 00 00 00 00",
        ] {
            assert_eq!(
                decode(&unhex(wider)).unwrap(),
                [table(vec![(s("a"), Value::Ref(0))])],
                "{wider}"
            );
        }
    }

    #[test]
    fn a_reference_must_name_a_registered_table() {
        for bad in [
            "81 00 00 01 0E 00",                            // declared, none registered yet
            "81 00 00 02 0B 00 00 00 00 08 01 61 0E 01 00", // ahead of what's registered
            // past the declared count: only the first table is registered
            "81 00 00 01 0B 00 00 00 00 08 01 61 0B 00 00 00 00 00 08 01 62 0E 01 00",
        ] {
            assert!(decode(&unhex(bad)).is_err(), "{bad}");
        }
        // The encoder refuses one too.
        assert!(encode(&[Value::Ref(0)]).is_err());
        assert!(encode(&[table(vec![(s("a"), Value::Ref(1))])]).is_err());
    }

    #[test]
    fn magic_objects() {
        // FUN_1400D41A0 packs userdata with a `__pickle__` metamethod as 0D, a name and an
        // argument; FUN_1400D4AC0 calls the constructor registered under the name.
        let float4 = Value::Magic {
            name: b"Float4".to_vec(),
            arg: Box::new(Value::Table(Table {
                array: vec![Value::Float(1.0); 4],
                hash: vec![],
            })),
        };
        let bytes = encode(std::slice::from_ref(&float4)).unwrap();
        assert_eq!(bytes[..10], unhex("01 0D 08 06 46 6C 6F 61 74 34"));
        assert_eq!(decode(&bytes).unwrap(), [float4]);
        // A nil name: the next value stands for itself.
        assert_eq!(decode(&unhex("01 0D 00 03 07")).unwrap(), [Value::Int(7)]);
        // Only string names.
        assert!(decode(&unhex("01 0D 03 01 03 02")).is_err());
    }

    #[test]
    fn a_refused_value_is_named_by_its_kind() {
        // Not printed: one error line can't hold a whole table a peer sent. A magic object named
        // by a table of 1000 nils:
        let mut b = vec![0x01, 0x0D, 0x0B, 0xE8, 0x03, 0x00, 0x00];
        b.extend([0x00; 1001]);
        b.push(0x00);
        assert_eq!(
            decode(&b).unwrap_err().to_string(),
            "pickle: a magic object named by a table"
        );
    }

    #[test]
    fn magic_objects_count_toward_the_depth() {
        let chain = |n: usize| {
            let mut b = vec![0x01];
            for _ in 0..n {
                b.extend([0x0D, 0x00]);
            }
            b.push(0x00);
            b
        };
        assert!(decode(&chain(MAX_DEPTH)).is_ok());
        assert!(decode(&chain(MAX_DEPTH + 1)).is_err());
    }

    #[test]
    fn a_message_holds_at_most_max_decoded_values() {
        // #84: a 4 KiB compressed message can unzip to 1 MiB, about a million values. Every
        // value read counts, a table's end marker included: one table of n nils is n + 2.
        let nils = |n: usize| {
            let mut b = vec![0x01, 0x0B];
            b.extend((n as u32).to_le_bytes());
            b.extend(std::iter::repeat_n(0x00, n + 1));
            b
        };
        assert!(decode(&nils(MAX_DECODED - 2)).is_ok());
        assert!(decode(&nils(MAX_DECODED - 1)).is_err());
        // Compression doesn't get around it.
        let raw = nils(MAX_DECODED);
        let mut packet = vec![0xFF];
        packet.extend_from_slice(&(raw.len() as u32).to_be_bytes()[1..]);
        packet.extend(lz4_flex::block::compress(&raw));
        assert!(packet.len() < 2048, "{} bytes", packet.len());
        assert!(decode(&packet).is_err());
    }

    #[test]
    fn a_compressed_packet_decodes() {
        // payloads.md P.1: FF, a 24-bit big-endian raw size (0x18), then an LZ4 block. By the LZ4
        // block format: token 0x78 (7 literals, then a match of 8 + 4 bytes); the literals
        // `01 08 15 61 62 63 64`; offset 4, an overlapping copy that repeats "abcd" three times;
        // then token 0x50 and the last 5 literals, "WXYZ!".
        let packet = unhex("FF 00 00 18  78 01 08 15 61 62 63 64 04 00  50 57 58 59 5A 21");
        assert_eq!(decode(&packet).unwrap(), [s("abcdabcdabcdabcdWXYZ!")]);
    }

    #[test]
    fn a_compressed_packet_may_carry_the_table_count() {
        // P.1: the header inside the LZ4 block is read like any other, here `0x80 | 2` and a
        // 24-bit table count of 1, so the second value may name the first table.
        let values = [table(vec![(s("a"), s(&"x".repeat(200)))]), Value::Ref(0)];
        let packet = encode_compressed(&values).unwrap();
        assert_eq!(packet[0], 0xFF);
        assert_eq!(unzip(&packet).unwrap()[..4], [0x82, 0, 0, 1]);
        assert_eq!(decode(&packet).unwrap(), values);
    }

    #[test]
    fn the_raw_size_bounds_the_output() {
        // FUN_1400D55C0 keeps the length the LZ4 decoder returns, which may be below the
        // declared size, and the decoder refuses to write past it.
        let block = "78 01 08 15 61 62 63 64 04 00  50 57 58 59 5A 21";
        let roomy = unhex(&format!("FF 00 00 20 {block}"));
        assert_eq!(decode(&roomy).unwrap(), [s("abcdabcdabcdabcdWXYZ!")]);
        let tight = unhex(&format!("FF 00 00 17 {block}"));
        assert!(decode(&tight).is_err());
    }

    #[test]
    fn a_compressed_packet_may_hold_1_mib() {
        // One string filling exactly MAX_UNZIPPED bytes with its header: accepted. One byte
        // more is refused before decompressing, as the client does ("too large zip size").
        for (raw, ok) in [(MAX_UNZIPPED, true), (MAX_UNZIPPED + 1, false)] {
            let mut inner = vec![0x01, 0x0A];
            inner.extend_from_slice(&((raw - 6) as u32).to_le_bytes());
            inner.resize(raw, b'x');
            let mut packet = vec![0xFF];
            packet.extend_from_slice(&(raw as u32).to_be_bytes()[1..]);
            packet.extend(lz4_flex::block::compress(&inner));
            assert_eq!(decode(&packet).is_ok(), ok, "raw {raw:#x}");
        }
    }

    #[test]
    fn refused_compressed_forms() {
        for bad in [
            "FF",                                                          // no raw size
            "FF 00 00",                                                    // a short raw size
            "FF 00 00 18 78 01 08 15",                                     // literals cut short
            "FF 00 00 18 78 01 08 15 61 62 63 64 00 00 50 57 58 59 5A 21", // offset 0
            "FF 00 00 18 78 01 08 15 61 62 63 64 08 00 50 57 58 59 5A 21", // offset before the output
            "FF 00 00 01 10 FF", // inside, FF again: a count byte, not a second FF
        ] {
            assert!(decode(&unhex(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn array_count_beyond_the_input_is_refused() {
        // A table announcing 0x7fffffff array values in a 6-byte packet.
        assert!(decode(&unhex("01 0B FF FF FF 7F")).is_err());
    }

    #[test]
    fn tables_nest_31_deep_as_in_the_client() {
        // FUN_1400D5850 starts the unpacker's depth at 1 (its constant at 0x142F04970), and
        // FUN_1400D4AC0 refuses a table that takes it past 32 ("unpack table too depth!"). The
        // packer counts the same way (FUN_1400D33F0, FUN_1400D3950). So 31 tables, not 32.
        let nested = |n: usize| {
            let mut v = Value::Nil;
            for _ in 0..n {
                v = Value::Table(Table {
                    array: vec![v],
                    hash: vec![],
                });
            }
            v
        };
        let bytes = encode(&[nested(31)]).unwrap();
        assert_eq!(decode(&bytes).unwrap(), [nested(31)]);
        assert!(encode(&[nested(32)]).is_err());
        // Each table's one array value is the next table, the innermost's is nil, then each
        // table's pairs end with a nil key.
        let by_hand = |n: usize| {
            let mut b = vec![0x01];
            for _ in 0..n {
                b.extend([0x0B, 1, 0, 0, 0]);
            }
            b.push(0x00);
            b.extend(std::iter::repeat_n(0x00, n));
            b
        };
        assert_eq!(by_hand(31), bytes);
        assert!(decode(&by_hand(32)).is_err());
    }

    #[test]
    fn with_references_a_packet_holds_at_most_126_values() {
        // The header byte is 0x80 | count, so 127 would make it FF: the compressed marker, which
        // both our decoder and the client's (FUN_1400D56D0) would try to unzip. The client's own
        // packer packs at most 32 values (payloads.md P.3), so only ours could get there.
        let mut v = vec![table(vec![(s("a"), Value::Ref(0))])];
        v.extend(vec![Value::Nil; 125]);
        let bytes = encode(&v).unwrap();
        assert_eq!(bytes[0], 0x80 | 126);
        assert_eq!(decode(&bytes).unwrap(), v);
        v.push(Value::Nil);
        assert!(encode(&v).is_err());
        // Without a reference, 127 values keep the one-byte count.
        assert_eq!(encode(&vec![Value::Nil; 127]).unwrap()[0], 0x7F);
    }

    #[test]
    fn a_nan_key_is_refused() {
        // Lua refuses NaN as a table index ("table index is NaN"), so the client couldn't unpack it.
        let nan_key = Table {
            array: vec![],
            hash: vec![(Value::Float(f64::NAN), Value::Int(1))],
        };
        assert!(encode(&[Value::Table(nan_key)]).is_err());
    }

    #[test]
    fn encode_limits() {
        assert!(encode(&vec![Value::Nil; 128]).is_err());
        let nil_key = Table {
            array: vec![],
            hash: vec![(Value::Nil, Value::Int(1))],
        };
        assert!(encode(&[Value::Table(nil_key)]).is_err());
    }

    #[test]
    fn nested_huge_array_counts_fail_cleanly() {
        // One table more than MAX_DEPTH, each declaring an array count as large as the rest of
        // a 1 MiB input: refused at the last, with at most PREALLOC slots reserved per level.
        let total = 1 << 20;
        let mut b = vec![0x01];
        for level in 0..=MAX_DEPTH {
            let rest = total - (1 + 5 * (level + 1));
            b.push(0x0B);
            b.extend_from_slice(&(rest as u32).to_le_bytes());
        }
        b.resize(total, 0x00);
        assert!(decode(&b).is_err());
    }

    #[test]
    fn junk_never_panics() {
        let mut x = 0x2545_f491_4f6c_dd1du64;
        for _ in 0..500 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let len = (x % 64) as usize;
            let junk: Vec<u8> = (0..len)
                .map(|i| (x >> (i % 8 * 8)) as u8 ^ i as u8)
                .collect();
            let _ = decode(&junk);
            // The same junk as a compressed packet's block.
            let mut packet = vec![0xFF, 0x00, 0x01, 0x00];
            packet.extend(&junk);
            let _ = decode(&packet);
        }
    }
}
