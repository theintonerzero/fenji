//! A cache written back: the reader's inverse (docs/protocol/payloads.md P.1–P.2;
//! docs/re/m2-tables.md §2), for the retranslation's rebuilt locale cache and the tools' tests.
//! Integers and strings take the smallest form that holds them. A table is written where it
//! first appears, and referred to after, every table referable.
use std::collections::HashMap;

use super::{Cache, TableId, Value};

/// `cache` as the client's pickle. Its table ids need not be in the order they appear: they're
/// numbered again as they're written, as the reader numbers them.
///
/// # Panics
///
/// On a cache no reader made: 128 or more top-level values, or a table id past its tables.
pub fn write(cache: &Cache) -> Vec<u8> {
    let count = u8::try_from(cache.values.len())
        .ok()
        .filter(|&n| n < 0x80)
        .expect("fewer than 128 top-level values");
    let mut w = Writer {
        cache,
        out: Vec::new(),
        ids: HashMap::new(),
    };
    if cache.tables.is_empty() {
        w.out.push(count);
    } else {
        let declared = u32::try_from(cache.tables.len()).expect("fewer than 2^24 tables");
        w.out.push(0x80 | count);
        w.out.extend_from_slice(&declared.to_be_bytes()[1..]);
    }
    for v in &cache.values {
        w.value(v);
    }
    w.out
}

struct Writer<'c, 'a> {
    cache: &'c Cache<'a>,
    out: Vec<u8>,
    /// Each table's number as written, by its id in `cache`.
    ids: HashMap<TableId, usize>,
}

impl Writer<'_, '_> {
    fn value(&mut self, v: &Value) {
        match v {
            Value::Nil => self.out.push(0x00),
            Value::Bool(true) => self.out.push(0x01),
            Value::Bool(false) => self.out.push(0x02),
            Value::Int(i) => self.int(*i),
            Value::Float(f) => {
                self.out.push(0x07);
                self.out.extend_from_slice(&f.to_le_bytes());
            }
            Value::Str(s) => self.str(s),
            Value::Table(id) => self.table(*id),
            Value::Null => self.out.push(0x0c),
            Value::Magic { name, arg } => {
                self.out.push(0x0d);
                self.str(name);
                self.value(arg);
            }
        }
    }

    fn int(&mut self, i: i64) {
        if let Ok(n) = i8::try_from(i) {
            self.out.push(0x03);
            self.out.extend_from_slice(&n.to_le_bytes());
        } else if let Ok(n) = i16::try_from(i) {
            self.out.push(0x04);
            self.out.extend_from_slice(&n.to_le_bytes());
        } else if let Ok(n) = i32::try_from(i) {
            self.out.push(0x05);
            self.out.extend_from_slice(&n.to_le_bytes());
        } else {
            self.out.push(0x06);
            self.out.extend_from_slice(&i.to_le_bytes());
        }
    }

    fn str(&mut self, s: &[u8]) {
        if let Ok(n) = u8::try_from(s.len()) {
            self.out.push(0x08);
            self.out.push(n);
        } else if let Ok(n) = u16::try_from(s.len()) {
            self.out.push(0x09);
            self.out.extend_from_slice(&n.to_le_bytes());
        } else {
            self.out.push(0x0a);
            let n = u32::try_from(s.len()).expect("a string under 4 GiB");
            self.out.extend_from_slice(&n.to_le_bytes());
        }
        self.out.extend_from_slice(s);
    }

    /// A table where it first appears, a reference to it after: `0B`, its array's length and
    /// values, its pairs, and the `nil` key that ends them.
    fn table(&mut self, id: TableId) {
        if let Some(&n) = self.ids.get(&id) {
            if let Ok(n) = u8::try_from(n) {
                self.out.push(0x0e);
                self.out.push(n);
            } else if let Ok(n) = u16::try_from(n) {
                self.out.push(0x0f);
                self.out.extend_from_slice(&n.to_le_bytes());
            } else {
                self.out.push(0x10);
                let n = u32::try_from(n).expect("fewer than 2^32 tables");
                self.out.extend_from_slice(&n.to_le_bytes());
            }
            return;
        }
        self.ids.insert(id, self.ids.len());
        let t = &self.cache.tables[id.0];
        self.out.push(0x0b);
        let n = u32::try_from(t.array.len()).expect("an array under 2^32 values");
        self.out.extend_from_slice(&n.to_le_bytes());
        for v in &t.array {
            self.value(v);
        }
        for (k, v) in &t.hash {
            self.value(k);
            self.value(v);
        }
        self.out.push(0x00);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::{Table, TableId, Value, read};

    fn unhex(h: &str) -> Vec<u8> {
        let h: String = h.split_whitespace().collect();
        (0..h.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap())
            .collect()
    }

    /// The reader's own vectors (cache.rs's tests): a root table naming itself and an empty table
    /// twice, and an `LString`. Both take the smallest forms, as a packer would write them.
    const SELF_REF: &str = "81 00 00 02  0B 00 00 00 00  08 01 61 0E 00  08 01 62 0B 00 00 00 00 00  08 01 63 0E 01  00";
    const MAGIC: &str = "01 0D 08 07 4C 53 74 72 69 6E 67 08 06 4C 28 22 78 22 29";

    #[test]
    fn a_cache_written_reads_back_the_same() {
        for vector in [SELF_REF, MAGIC] {
            let bytes = unhex(vector);
            assert_eq!(write(&read(&bytes).unwrap()), bytes, "{vector}");
        }
        // Every scalar form, a long string, a shared table and a magic object, built by hand.
        let long = vec![b'x'; 70_000];
        let c = Cache {
            tables: vec![
                Table {
                    array: vec![
                        Value::Int(-2),
                        Value::Int(300),
                        Value::Int(70_000),
                        Value::Int(1 << 40),
                        Value::Float(1.5),
                        Value::Bool(true),
                        Value::Bool(false),
                        Value::Null,
                        Value::Table(TableId(1)),
                    ],
                    hash: vec![
                        (Value::Str(b"again"), Value::Table(TableId(1))),
                        (Value::Str(b"long"), Value::Str(&long)),
                        (
                            Value::Str(b"name"),
                            Value::Magic {
                                name: b"LString",
                                arg: Box::new(Value::Str("L(\"剑\")".as_bytes())),
                            },
                        ),
                    ],
                },
                Table {
                    array: vec![Value::Str(b"shared")],
                    hash: vec![],
                },
            ],
            values: vec![Value::Table(TableId(0))],
        };
        let bytes = write(&c);
        let back = read(&bytes).unwrap();
        assert_eq!(back.tables, c.tables);
        assert_eq!(back.values, c.values);
        // The second appearance of table 1 is a reference, not a copy.
        assert_eq!(back.tables.len(), 2);
    }
}
