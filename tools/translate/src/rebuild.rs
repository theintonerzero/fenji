//! The locale cache rebuilt with new English (retranslation plan, Task 10): what the early check
//! loads, and what the patcher's content hook (#336) may load in turn.
use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use fenji_data::cache::{self, Value};
use fenji_pack::{Fingerprint, fingerprint_in};

/// `original`, a locale cache (compressed or not), with each line whose fingerprint `english`
/// holds given that English, and each of `added`'s lines, a source and its English, put in the
/// main table (context `""`). Written uncompressed, which the client's unpacker reads as it reads
/// the compressed form (m2-tables.md 2.1).
pub fn rebuild_locale(
    original: &[u8],
    english: &HashMap<Fingerprint, String>,
    added: &[(String, String)],
) -> Result<Vec<u8>> {
    Ok(rebuild(original, english, added)?.bytes)
}

/// A rebuilt locale cache, and how many of its lines took new English.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rebuilt {
    pub bytes: Vec<u8>,
    pub replaced: usize,
    pub added: usize,
}

/// [`rebuild_locale`], with its counts.
pub fn rebuild(
    original: &[u8],
    english: &HashMap<Fingerprint, String>,
    added: &[(String, String)],
) -> Result<Rebuilt> {
    let raw = cache::unzip(original)?.unwrap_or_else(|| original.to_vec());
    let mut c = cache::read(&raw).context("the locale cache")?;
    let root = match c.values.as_slice() {
        [Value::Table(root)] => root.0,
        _ => bail!("the locale cache's root isn't one table"),
    };
    let contexts: Vec<(String, usize)> = c.tables[root]
        .hash
        .iter()
        .filter_map(|(env, t)| match (env, t) {
            (Value::Str(env), Value::Table(t)) => {
                Some((String::from_utf8_lossy(env).into_owned(), t.0))
            }
            _ => None,
        })
        .collect();
    let (mut main, mut replaced, mut new) = (None, 0, 0);
    for (env, t) in &contexts {
        if env.is_empty() {
            main = Some(*t);
        }
        for (key, value) in &mut c.tables[*t].hash {
            if let Value::Str(source) = key
                && let Some(en) = english.get(&fingerprint_in(env, source))
            {
                *value = Value::Str(en.as_bytes());
                replaced += 1;
            }
        }
    }
    let main = main.context("the locale cache has no main table (context \"\")")?;
    for (source, en) in added {
        let table = &mut c.tables[main].hash;
        match table
            .iter_mut()
            .find(|(k, _)| matches!(k, Value::Str(k) if *k == source.as_bytes()))
        {
            Some((_, value)) => {
                *value = Value::Str(en.as_bytes());
                replaced += 1;
            }
            None => {
                table.push((Value::Str(source.as_bytes()), Value::Str(en.as_bytes())));
                new += 1;
            }
        }
    }
    Ok(Rebuilt {
        bytes: cache::write(&c),
        replaced,
        added: new,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use fenji_data::cache::{self, Cache, Table, TableId, Value};
    use fenji_pack::fingerprint_in;

    use super::*;

    fn s(text: &str) -> Value<'_> {
        Value::Str(text.as_bytes())
    }

    /// An invented locale cache: its root maps a context to a table, the main one `""` with two
    /// lines, and `noun` with one.
    fn locale() -> Vec<u8> {
        let c = Cache {
            tables: vec![
                Table {
                    array: vec![],
                    hash: vec![
                        (s(""), Value::Table(TableId(1))),
                        (s("noun"), Value::Table(TableId(2))),
                    ],
                },
                Table {
                    array: vec![],
                    hash: vec![(s("你好"), s("Hello")), (s("再见"), s("Bye"))],
                },
                Table {
                    array: vec![],
                    hash: vec![(s("你好"), s("A greeting"))],
                },
            ],
            values: vec![Value::Table(TableId(0))],
        };
        cache::write(&c)
    }

    /// Each context's lines, source to English.
    fn contexts(bytes: &[u8]) -> BTreeMap<String, BTreeMap<String, String>> {
        let c = cache::read(bytes).unwrap();
        let text = |v: &Value| match v {
            Value::Str(b) => String::from_utf8(b.to_vec()).unwrap(),
            other => panic!("{other:?}"),
        };
        let Value::Table(root) = &c.values[0] else {
            panic!("no root table")
        };
        c.tables[root.0]
            .hash
            .iter()
            .map(|(env, t)| {
                let Value::Table(t) = t else { panic!("{t:?}") };
                let lines = c.tables[t.0]
                    .hash
                    .iter()
                    .map(|(k, v)| (text(k), text(v)))
                    .collect();
                (text(env), lines)
            })
            .collect()
    }

    #[test]
    fn rebuild_replaces_and_adds_by_fingerprint() {
        let english = HashMap::from([
            (
                fingerprint_in("", "你好".as_bytes()),
                "Hi there".to_string(),
            ),
            (
                fingerprint_in("noun", "你好".as_bytes()),
                "Greeting".to_string(),
            ),
        ]);
        let added = [("新的".to_string(), "New".to_string())];
        let rebuilt = rebuild_locale(&locale(), &english, &added).unwrap();
        let map = contexts(&rebuilt);
        assert_eq!(map[""]["你好"], "Hi there");
        assert_eq!(
            map[""]["再见"], "Bye",
            "a line with no new English keeps its old"
        );
        assert_eq!(map[""]["新的"], "New", "an added line is in the main table");
        assert_eq!(
            map["noun"]["你好"], "Greeting",
            "a context's line, by its own fingerprint"
        );
        assert_eq!(map.len(), 2);
    }
}
