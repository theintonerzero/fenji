//! Rows of a cache's sheet as JSON lines (M2 plan Amendment B). Integers and finite floats are
//! numbers, other floats `null`; strings are JSON strings (bytes that aren't UTF-8 replaced); `nil`
//! and the null userdata are `null`; a table with only an array part is an array, any other an
//! object (array entries under "1", "2", …; a key that isn't a string is written as its own JSON
//! text); a magic object is `{"<name>": <argument>}`; a table already being written further up,
//! or past `MAX_DEPTH` tables down, is `{"$ref": <id>}`.
use std::fmt::Write;

use crate::cache::{Cache, TableId, Value};

/// How many tables deep a value is written out before the rest become references. A table shared
/// within one value (m2-tables.md 2.2) is written out again wherever it appears, so a crafted
/// cache whose tables hold each other many times over gives output that grows with every level;
/// the client's caches have no such sharing in the dumped sheets (m2-tables.md §3).
pub const MAX_DEPTH: usize = 64;

/// JSON for one value.
pub fn value(cache: &Cache, v: &Value) -> String {
    let mut out = String::new();
    write_value(cache, v, &mut Vec::new(), &mut out);
    out
}

/// One JSON object per row of `sheet`: `"$key"`, then the row's own fields in the cache's order.
/// An `__index` that names the row itself (every `items` row has one, m2-tables.md 2.8) carries
/// nothing and is left out; nothing is merged from anywhere else. Rows in the array part are keyed
/// 1, 2, …, and `nil` ones are skipped. A row that isn't a table is `{"$key": …, "$value": …}`.
pub fn rows(cache: &Cache, sheet: &TableId) -> Vec<String> {
    let t = &cache.tables[sheet.0];
    let array = t
        .array
        .iter()
        .enumerate()
        .filter(|(_, v)| **v != Value::Nil)
        .map(|(i, v)| row(cache, &Value::Int(i as i64 + 1), v));
    let hash = t.hash.iter().map(|(k, v)| row(cache, k, v));
    array.chain(hash).collect()
}

fn row(cache: &Cache, key: &Value, v: &Value) -> String {
    let mut out = String::from("{\"$key\":");
    write_value(cache, key, &mut Vec::new(), &mut out);
    match v {
        Value::Table(id) => {
            let mut path = vec![*id];
            let t = &cache.tables[id.0];
            let array = t
                .array
                .iter()
                .enumerate()
                .map(|(i, a)| ((i + 1).to_string(), a));
            let mut hash = Vec::new();
            for (k, hv) in &t.hash {
                if *k == Value::Str(b"__index") && *hv == Value::Table(*id) {
                    continue;
                }
                hash.push((key_string(cache, k, &mut path), hv));
            }
            for (name, fv) in array.chain(hash) {
                out.push(',');
                write_str(name.as_bytes(), &mut out);
                out.push(':');
                write_value(cache, fv, &mut path, &mut out);
            }
        }
        other => {
            out.push_str(",\"$value\":");
            write_value(cache, other, &mut Vec::new(), &mut out);
        }
    }
    out.push('}');
    out
}

/// A table key as an object member name: a string as itself, anything else as its JSON text,
/// written from where the key sits (`path`) so it gets the same cycle check and depth cap.
fn key_string(cache: &Cache, k: &Value, path: &mut Vec<TableId>) -> String {
    match k {
        Value::Str(s) => String::from_utf8_lossy(s).into_owned(),
        other => {
            let mut out = String::new();
            write_value(cache, other, path, &mut out);
            out
        }
    }
}

fn write_value(cache: &Cache, v: &Value, path: &mut Vec<TableId>, out: &mut String) {
    match v {
        Value::Nil | Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(n) => write!(out, "{n}").expect("String"),
        Value::Float(f) if f.is_finite() => write!(out, "{f}").expect("String"),
        Value::Float(_) => out.push_str("null"),
        Value::Str(s) => write_str(s, out),
        Value::Magic { name, arg } => {
            out.push('{');
            write_str(name, out);
            out.push(':');
            write_value(cache, arg, path, out);
            out.push('}');
        }
        Value::Table(id) => {
            if path.contains(id) || path.len() >= MAX_DEPTH {
                write!(out, "{{\"$ref\":{}}}", id.0).expect("String");
                return;
            }
            path.push(*id);
            let t = &cache.tables[id.0];
            if t.hash.is_empty() {
                out.push('[');
                for (i, a) in t.array.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_value(cache, a, path, out);
                }
                out.push(']');
            } else {
                out.push('{');
                let mut first = true;
                let mut member =
                    |name: String, v: &Value, path: &mut Vec<TableId>, out: &mut String| {
                        if !first {
                            out.push(',');
                        }
                        first = false;
                        write_str(name.as_bytes(), out);
                        out.push(':');
                        write_value(cache, v, path, out);
                    };
                for (i, a) in t.array.iter().enumerate() {
                    member((i + 1).to_string(), a, path, out);
                }
                for (k, hv) in &t.hash {
                    let name = key_string(cache, k, path);
                    member(name, hv, path, out);
                }
                out.push('}');
            }
            path.pop();
        }
    }
}

/// A JSON string: `"` and `\` escaped, control characters as escapes, bytes that aren't UTF-8
/// replaced.
fn write_str(bytes: &[u8], out: &mut String) {
    out.push('"');
    for c in String::from_utf8_lossy(bytes).chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => write!(out, "\\u{:04x}", c as u32).expect("String"),
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::{Cache, Table, TableId, Value};

    fn s(x: &str) -> Value<'_> {
        Value::Str(x.as_bytes())
    }

    #[test]
    fn values_as_json() {
        let empty = Cache::default();
        for (v, want) in [
            (Value::Nil, "null"),
            (Value::Null, "null"),
            (Value::Bool(true), "true"),
            (Value::Int(-7), "-7"),
            (Value::Float(1.5), "1.5"),
            (Value::Float(f64::NAN), "null"),
            (Value::Float(f64::INFINITY), "null"),
            (Value::Str(b"a\"b\\c\n\x01"), r#""a\"b\\c\n\u0001""#),
            (Value::Str(b"\xff"), "\"\u{fffd}\""),
            (
                Value::Magic {
                    name: b"LString",
                    arg: Box::new(s("L(\"x\")")),
                },
                r#"{"LString":"L(\"x\")"}"#,
            ),
        ] {
            assert_eq!(value(&empty, &v), want, "{v:?}");
        }
    }

    #[test]
    fn tables_as_json() {
        let c = Cache {
            tables: vec![
                // 0: a pure array
                Table {
                    array: vec![Value::Int(1), s("x")],
                    hash: vec![],
                },
                // 1: mixed, with integer and string keys, and itself
                Table {
                    array: vec![Value::Bool(true)],
                    hash: vec![
                        (Value::Int(9), s("nine")),
                        (s("self"), Value::Table(TableId(1))),
                    ],
                },
                // 2: four floats, as Float4's argument
                Table {
                    array: vec![
                        Value::Float(1.0),
                        Value::Float(0.5),
                        Value::Float(-2.0),
                        Value::Float(0.0),
                    ],
                    hash: vec![],
                },
            ],
            values: vec![],
        };
        assert_eq!(value(&c, &Value::Table(TableId(0))), r#"[1,"x"]"#);
        assert_eq!(
            value(&c, &Value::Table(TableId(1))),
            r#"{"1":true,"9":"nine","self":{"$ref":1}}"#
        );
        let float4 = Value::Magic {
            name: b"Float4",
            arg: Box::new(Value::Table(TableId(2))),
        };
        assert_eq!(value(&c, &float4), r#"{"Float4":[1,0.5,-2,0]}"#);
    }

    #[test]
    fn rows_write_their_own_fields() {
        // m2-tables.md 2.8: an `items` row's `__index` names the row itself, and is left out. A
        // row whose `__index` names another table keeps it as an ordinary field: nothing is merged.
        let c = Cache {
            tables: vec![
                // 0: the sheet: an array row, a nil array entry, a keyed row
                Table {
                    array: vec![Value::Table(TableId(1)), Value::Nil],
                    hash: vec![(Value::Int(7), Value::Table(TableId(2)))],
                },
                // 1: a row naming itself
                Table {
                    array: vec![],
                    hash: vec![
                        (s("a"), Value::Int(1)),
                        (s("__index"), Value::Table(TableId(1))),
                        (s("b"), Value::Int(2)),
                    ],
                },
                // 2: a row naming another table
                Table {
                    array: vec![],
                    hash: vec![
                        (s("__index"), Value::Table(TableId(3))),
                        (s("c"), Value::Int(3)),
                    ],
                },
                // 3
                Table {
                    array: vec![],
                    hash: vec![(s("d"), Value::Int(4))],
                },
            ],
            values: vec![],
        };
        assert_eq!(
            rows(&c, &TableId(0)),
            [
                r#"{"$key":1,"a":1,"b":2}"#,
                r#"{"$key":7,"__index":{"d":4},"c":3}"#,
            ]
        );
    }

    #[test]
    fn a_row_that_isnt_a_table_is_a_value() {
        let c = Cache {
            tables: vec![Table {
                array: vec![],
                hash: vec![(s("k"), Value::Int(5))],
            }],
            values: vec![],
        };
        assert_eq!(rows(&c, &TableId(0)), [r#"{"$key":"k","$value":5}"#]);
    }

    #[test]
    fn a_table_keyed_by_itself_is_a_reference() {
        // A table key goes through the same cycle check as a value.
        let c = Cache {
            tables: vec![Table {
                array: vec![],
                hash: vec![(Value::Table(TableId(0)), Value::Int(1))],
            }],
            values: vec![],
        };
        assert_eq!(
            value(&c, &Value::Table(TableId(0))),
            r#"{"{\"$ref\":0}":1}"#
        );
    }

    #[test]
    fn tables_past_the_depth_cap_are_references() {
        // Table i holds table i + 1 under "n"; the last one is empty.
        let n = MAX_DEPTH + 2;
        let tables = (0..n)
            .map(|i| Table {
                array: vec![],
                hash: if i + 1 < n {
                    vec![(s("n"), Value::Table(TableId(i + 1)))]
                } else {
                    vec![]
                },
            })
            .collect();
        let c = Cache {
            tables,
            values: vec![],
        };
        let json = value(&c, &Value::Table(TableId(0)));
        assert_eq!(json.matches("{\"n\":").count(), MAX_DEPTH);
        assert!(
            json.contains(&format!("{{\"$ref\":{MAX_DEPTH}}}")),
            "{json}"
        );
    }
}
