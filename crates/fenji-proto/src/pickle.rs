//! The client's own serialisation format, its "pickle" (docs/protocol/payloads.md): script
//! messages and the auth payload. FUN_1400D5850 (header FUN_1400D56D0, values FUN_1400D4AC0)
//! unpacks it; `kslua_pickle_pack` (FUN_1400D54F0) packs it. A packet is a count of values, then
//! the values, each a tag and its data, little-endian. A packet of 128 bytes or more usually
//! arrives LZ4-compressed (payloads.md P.1, P.5). Fenji reads tags 00–0C, compressed or not, and
//! writes them uncompressed, which the client reads too. It handles no table references and no
//! "magic" objects, and refuses an empty packet, which the client reads as no values. The
//! client's table caches use the same format with those two forms: `fenji_data::cache` reads them
//! (docs/re/m2-tables.md §2). The two readers share the format but not code, as fenji-proto
//! depends on nothing internal; a server that must read references from peers would join them.
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
}

/// A Lua table: the array part (indices 1, 2, …) and the other pairs, in wire order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Table {
    pub array: Vec<Value>,
    pub hash: Vec<(Value, Value)>,
}

/// Tables nest at most this deep ("unpack table too depth!", FUN_1400D4AC0).
pub const MAX_DEPTH: usize = 32;
/// The largest raw size a compressed packet may declare ("pkt bad size, too large zip size",
/// FUN_1400D55C0).
pub const MAX_UNZIPPED: usize = 0x10_0000;
/// The most values a packet without table references holds (its count byte stays below 0x80).
pub const MAX_VALUES: usize = 0x7f;
/// The most array slots reserved up front for a table, whatever count it declares. Nested tables
/// may each declare a count as large as the rest of the input; reserving that much at every level
/// would let a 1 MiB message claim gigabytes. Past this, the array grows as values are read, and
/// every value takes at least one input byte.
const PREALLOC: usize = 64;

fn err<T>(what: impl Into<String>) -> Result<T> {
    Err(Error::Pickle(what.into()))
}

/// Encodes `values` with the shortest integer and string forms.
pub fn encode(values: &[Value]) -> Result<Vec<u8>> {
    if values.len() > MAX_VALUES {
        return err(format!("{} values, at most {MAX_VALUES}", values.len()));
    }
    let mut out = vec![values.len() as u8];
    for v in values {
        put(v, 0, &mut out)?;
    }
    Ok(out)
}

fn put(v: &Value, depth: usize, out: &mut Vec<u8>) -> Result<()> {
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
        Value::Str(s) => {
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
        }
        Value::Table(t) => {
            if depth >= MAX_DEPTH {
                return err(format!("tables nested over {MAX_DEPTH} deep"));
            }
            let n = u32::try_from(t.array.len()).or_else(|_| err("an array over 2^32 values"))?;
            out.push(0x0b);
            out.extend_from_slice(&n.to_le_bytes());
            for a in &t.array {
                put(a, depth + 1, out)?;
            }
            for (k, v) in &t.hash {
                if *k == Value::Nil {
                    return err("a nil key would end the table");
                }
                put(k, depth + 1, out)?;
                put(v, depth + 1, out)?;
            }
            out.push(0x00);
        }
        Value::Null => out.push(0x0c),
    }
    Ok(())
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
    let mut r = Reader { buf: bytes, at: 0 };
    let count = r.byte()?;
    if count >= 0x80 {
        return err("table references aren't supported");
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
            0x0d => return err("\"magic\" objects (0D) aren't supported"),
            0x0e..=0x10 => return err("table references aren't supported"),
            other => return err(format!("unknown tag {other:#04x}")),
        })
    }
}

#[cfg(test)]
mod tests {
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
            "01 0D 00",          // magic
            "01 0E 00",          // table reference, u8
            "01 0F 00 00",       // table reference, u16
            "01 10 00 00 00 00", // table reference, u32
            "01 11",             // unknown tag
            "81 00 00 00 00",    // count with table references
            "01 00 00",          // trailing byte
        ] {
            assert!(decode(&unhex(bad)).is_err(), "{bad}");
        }
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
            "FF 00 00 05 50 81 00 00 00 00", // inside, a count with table references
            "FF 00 00 01 10 FF",             // inside, FF again: a count byte, not a second FF
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
    fn nesting_is_bounded_at_32_tables() {
        let mut v = Value::Nil;
        for _ in 0..MAX_DEPTH {
            v = Value::Table(Table {
                array: vec![v],
                hash: vec![],
            });
        }
        let ok = encode(std::slice::from_ref(&v)).unwrap();
        assert_eq!(decode(&ok).unwrap(), [v.clone()]);
        let deeper = Value::Table(Table {
            array: vec![v],
            hash: vec![],
        });
        assert!(encode(std::slice::from_ref(&deeper)).is_err());
        // The same 33 levels written by hand are refused by the decoder too.
        let mut b = vec![0x01];
        for _ in 0..=MAX_DEPTH {
            b.extend([0x0B, 1, 0, 0, 0]);
        }
        b.push(0x00);
        b.extend(std::iter::repeat_n(0x00, MAX_DEPTH + 1));
        assert!(decode(&b).is_err());
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
        // 33 nested tables, each declaring an array count as large as the rest of a 1 MiB
        // input: refused at depth 33, with at most PREALLOC slots reserved per level.
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
