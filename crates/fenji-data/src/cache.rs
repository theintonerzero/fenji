//! The client's table caches (docs/re/m2-tables.md §2): one value in the client's pickle
//! (docs/protocol/payloads.md P.1–P.2), with tables that later values refer back to and "magic"
//! objects. `data/caches/all_tables.new_cache` holds the design tables this way. Reading is for the
//! owner's local copy: the cache is large (300 MB) and read whole; nothing here comes from a peer.
use crate::{Error, Result};

pub mod json;

/// Tables nest at most this deep ("unpack table too depth!", FUN_1400D4AC0). Magic objects count
/// as a level too, which keeps the recursion bounded.
pub const MAX_DEPTH: usize = 32;
/// The largest raw size a compressed cache may declare ("pkt bad size, too large zip size",
/// FUN_1400D55C0).
pub const MAX_UNZIPPED: usize = 0x10_0000;
/// The most array slots reserved up front for one table; past this the array grows as it fills.
const PREALLOC: usize = 64;

/// A table's index in [`Cache::tables`]: the order its `0B` tag was read in. References name the
/// first tables by the same index (m2-tables.md 2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TableId(pub usize);

/// One value. Strings borrow from the cache's bytes.
#[derive(Debug, Clone, PartialEq)]
pub enum Value<'a> {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(&'a [u8]),
    Table(TableId),
    /// Tag 0C, a null light userdata.
    Null,
    /// Tag 0D: the client rebuilds it with the constructor registered under `name`
    /// (m2-tables.md 2.4–2.7).
    Magic {
        name: &'a [u8],
        arg: Box<Value<'a>>,
    },
}

/// A table: its array part (indices 1, 2, …) and its other pairs, in the cache's order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Table<'a> {
    pub array: Vec<Value<'a>>,
    pub hash: Vec<(Value<'a>, Value<'a>)>,
}

/// A whole cache: every table, and the top-level values (one, for the caches seen).
#[derive(Debug, Default)]
pub struct Cache<'a> {
    pub tables: Vec<Table<'a>>,
    pub values: Vec<Value<'a>>,
}

impl<'a> Cache<'a> {
    /// The value of a string key in a table's other pairs.
    pub fn field(&self, table: &TableId, key: &str) -> Option<&Value<'a>> {
        self.tables[table.0]
            .hash
            .iter()
            .find(|(k, _)| *k == Value::Str(key.as_bytes()))
            .map(|(_, v)| v)
    }

    /// A sheet of `all_tables`: a field of the one root table (m2-tables.md §3).
    pub fn sheet(&self, name: &str) -> Option<&TableId> {
        let [Value::Table(root)] = self.values.as_slice() else {
            return None;
        };
        match self.field(root, name)? {
            Value::Table(id) => Some(id),
            _ => None,
        }
    }
}

fn err<T>(what: impl Into<String>) -> Result<T> {
    Err(Error::BadCache(what.into()))
}

/// The raw bytes of a compressed cache (m2-tables.md 2.1): `FF`, a 24-bit big-endian raw size,
/// then an LZ4 block that decodes into at most that many bytes. `None` if `bytes` doesn't start
/// with `FF`.
pub fn unzip(bytes: &[u8]) -> Result<Option<Vec<u8>>> {
    if bytes.first() != Some(&0xff) {
        return Ok(None);
    }
    let Some(&[a, b, c]) = bytes.get(1..4) else {
        return err("a compressed cache without its raw size");
    };
    let raw = u32::from_be_bytes([0, a, b, c]) as usize;
    if raw > MAX_UNZIPPED {
        return err(format!(
            "a compressed cache of {raw} bytes, at most {MAX_UNZIPPED}"
        ));
    }
    let mut out = vec![0; raw];
    let n = lz4_flex::block::decompress_into(&bytes[4..], &mut out)
        .or_else(|e| err(format!("LZ4: {e}")))?;
    out.truncate(n);
    Ok(Some(out))
}

/// Reads an uncompressed cache. A compressed one (a leading `FF`) goes through [`unzip`] first.
pub fn read(bytes: &[u8]) -> Result<Cache<'_>> {
    let mut r = Reader {
        buf: bytes,
        at: 0,
        declared: 0,
        depth: 0,
        tables: Vec::new(),
    };
    let first = r.byte()?;
    if first == 0xff {
        return err("a compressed cache: unzip it first");
    }
    let count = if first >= 0x80 {
        let [a, b, c] = r.array::<3>()?;
        r.declared = u32::from_be_bytes([0, a, b, c]) as usize;
        first & 0x7f
    } else {
        first
    };
    let mut values = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        values.push(r.value()?);
    }
    if r.at != bytes.len() {
        return err(format!("{} bytes after the last value", bytes.len() - r.at));
    }
    Ok(Cache {
        tables: r.tables,
        values,
    })
}

struct Reader<'a> {
    buf: &'a [u8],
    at: usize,
    /// How many tables references may name: the first `declared` read.
    declared: usize,
    depth: usize,
    tables: Vec<Table<'a>>,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
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

    fn enter(&mut self) -> Result<()> {
        if self.depth >= MAX_DEPTH {
            return err(format!("values nested over {MAX_DEPTH} deep"));
        }
        self.depth += 1;
        Ok(())
    }

    fn value(&mut self) -> Result<Value<'a>> {
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
                Value::Str(self.take(n)?)
            }
            0x0b => self.table()?,
            0x0c => Value::Null,
            0x0d => {
                self.enter()?;
                let v = match self.value()? {
                    // A nil name: the next value stands for itself.
                    Value::Nil => self.value()?,
                    Value::Str(name) => Value::Magic {
                        name,
                        arg: Box::new(self.value()?),
                    },
                    other => return err(format!("a magic object named by {other:?}")),
                };
                self.depth -= 1;
                v
            }
            0x0e..=0x10 => {
                let i = match tag {
                    0x0e => usize::from(self.byte()?),
                    0x0f => usize::from(u16::from_le_bytes(self.array()?)),
                    _ => u32::from_le_bytes(self.array()?) as usize,
                };
                // The first `declared` tables are referable, once their tag has been read.
                if i >= self.declared.min(self.tables.len()) {
                    return err(format!("a reference to table {i}, not yet registered"));
                }
                Value::Table(TableId(i))
            }
            other => return err(format!("unknown tag {other:#04x}")),
        })
    }

    /// A `0B` table: registered (given its id) before its contents are read (FUN_1400D4AC0).
    fn table(&mut self) -> Result<Value<'a>> {
        self.enter()?;
        let n = u32::from_le_bytes(self.array()?) as usize;
        // Every value takes at least one byte, so a larger count can't be honest.
        let left = self.buf.len() - self.at;
        if n > left {
            return err(format!("an array of {n} values in {left} bytes"));
        }
        let id = self.tables.len();
        self.tables.push(Table::default());
        let mut t = Table {
            array: Vec::with_capacity(n.min(PREALLOC)),
            hash: Vec::new(),
        };
        for _ in 0..n {
            t.array.push(self.value()?);
        }
        loop {
            let k = self.value()?;
            if k == Value::Nil {
                break;
            }
            let v = self.value()?;
            t.hash.push((k, v));
        }
        self.tables[id] = t;
        self.depth -= 1;
        Ok(Value::Table(TableId(id)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(h: &str) -> Vec<u8> {
        let h: String = h.split_whitespace().collect();
        (0..h.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap())
            .collect()
    }

    /// One root table naming itself (`a`), an empty table (`b`) and that table again (`c`).
    const SELF_REF: &str = "81 00 00 02  0B 00 00 00 00  08 01 61 0E 00  08 01 62 0B 00 00 00 00 00  08 01 63 0E 01  00";
    /// `LString` around `L("x")`.
    const MAGIC: &str = "01 0D 08 07 4C 53 74 72 69 6E 67 08 06 4C 28 22 78 22 29";

    #[test]
    fn references_name_tables_in_creation_order() {
        let bytes = unhex(SELF_REF);
        let c = read(&bytes).unwrap();
        assert_eq!(c.values, [Value::Table(TableId(0))]);
        assert_eq!(c.tables.len(), 2);
        assert_eq!(
            c.tables[0].hash,
            [
                (Value::Str(b"a"), Value::Table(TableId(0))),
                (Value::Str(b"b"), Value::Table(TableId(1))),
                (Value::Str(b"c"), Value::Table(TableId(1))),
            ]
        );
        assert_eq!(c.field(&TableId(0), "b"), Some(&Value::Table(TableId(1))));
    }

    #[test]
    fn a_reference_must_name_a_registered_table() {
        // Ahead of what's registered so far.
        assert!(read(&unhex("81 00 00 02  0B 00 00 00 00  08 01 61 0E 01  00")).is_err());
        // Past the declared count: only the first table is registered.
        let past = "81 00 00 01  0B 00 00 00 00  08 01 61 0B 00 00 00 00 00  08 01 62 0E 01  00";
        assert!(read(&unhex(past)).is_err());
        // No references declared at all.
        assert!(read(&unhex("01 0B 00 00 00 00 08 01 61 0E 00 00")).is_err());
    }

    #[test]
    fn magic_objects() {
        let bytes = unhex(MAGIC);
        assert_eq!(
            read(&bytes).unwrap().values,
            [Value::Magic {
                name: b"LString",
                arg: Box::new(Value::Str(b"L(\"x\")")),
            }]
        );
        // A nil name means the next value stands for itself.
        assert_eq!(
            read(&unhex("01 0D 00 03 07")).unwrap().values,
            [Value::Int(7)]
        );
        // Only string names.
        assert!(read(&unhex("01 0D 03 01 03 02")).is_err());
    }

    #[test]
    fn nesting_is_bounded() {
        // Each table's one array element is the next table; the innermost's is nil; then each
        // table's pairs end with a nil key.
        let nested = |levels: usize| {
            let mut b = vec![0x01];
            for _ in 0..levels {
                b.extend([0x0B, 1, 0, 0, 0]);
            }
            b.push(0x00);
            b.extend(std::iter::repeat_n(0x00, levels));
            b
        };
        assert!(read(&nested(MAX_DEPTH)).is_ok());
        assert!(read(&nested(MAX_DEPTH + 1)).is_err());
        // A chain of nil-named magic objects counts too.
        let mut chain = vec![0x01];
        for _ in 0..=MAX_DEPTH {
            chain.extend([0x0D, 0x00]);
        }
        chain.push(0x00);
        assert!(read(&chain).is_err());
    }

    #[test]
    fn refused_forms() {
        for (what, bytes) in [
            ("trailing byte", format!("{SELF_REF} 00")),
            ("unknown tag", "01 11".into()),
            ("compressed", "FF 00 00 01 10 01".into()),
            ("empty", String::new()),
            ("array count past the input", "01 0B FF FF FF 7F".into()),
        ] {
            assert!(read(&unhex(&bytes)).is_err(), "{what}");
        }
        for v in [SELF_REF, MAGIC] {
            let full = unhex(v);
            for cut in 0..full.len() {
                assert!(read(&full[..cut]).is_err(), "cut {cut} of {v}");
            }
        }
    }

    #[test]
    fn unzip_reads_the_compressed_form() {
        // m2-tables.md 2.1, payloads.md P.1: FF, a 24-bit big-endian raw size, an LZ4 block.
        let packed = unhex("FF 00 00 18  78 01 08 15 61 62 63 64 04 00  50 57 58 59 5A 21");
        let raw = unzip(&packed).unwrap().unwrap();
        assert_eq!(raw.len(), 0x18);
        assert_eq!(
            read(&raw).unwrap().values,
            [Value::Str(b"abcdabcdabcdabcdWXYZ!")]
        );
        assert!(unzip(&unhex("FF 10 00 01 00")).is_err(), "over 1 MiB");
        assert!(unzip(&unhex(SELF_REF)).unwrap().is_none());
    }
}
