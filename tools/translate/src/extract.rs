//! `translate extract` (retranslation spec §6.5, step 1): every source line of the pinned copy,
//! its English today, and where the game uses it: the locale map, the design tables' `LString`s,
//! the scripts' string constants and the `.tab` files' cells.
use std::collections::{BTreeMap, BTreeSet, HashSet};

use anyhow::{Context, Result, bail};
use fenji_data::cache::{self, Cache, TableId, Value};
use fenji_data::client::GameCopy;
use fenji_data::{ExeFs, PeImage};
use fenji_lua::{Constant, Function};
use fenji_luatool::client::{Decoded, decode};
use fenji_pack::{fingerprint, fingerprint_in};

use crate::area::classify;
use crate::lines::{Line, Use};

/// The English locale cache, whose largest table maps each source line to its English.
pub const LOCALE_CACHE: &str = "data/caches/locale_en.new_cache";
/// The design tables' cache.
pub const TABLE_CACHE: &str = "data/caches/all_tables.new_cache";

/// The counts behind `report.txt`, of the spec's §5 kind: numbers only.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    /// The locale map's main table's entries.
    pub map: usize,
    /// Lines in the map's other contexts, such as `noun`.
    pub contexts: usize,
    /// Languages' translation tables under `data/tables/locale/`: the maps' own sources, so
    /// never uses.
    pub locale_tabs: usize,
    /// `LString`s in the design tables.
    pub lstrings: usize,
    /// `LString`s with no `L("…")` in them.
    pub no_l: usize,
    /// Distinct `L("…")` sources in the design tables.
    pub table_sources: usize,
    /// `LString`s in every table of the cache, however reached, and the distinct sources among
    /// them that the walk from the sheets missed: those become lines too, with no use.
    pub arena_lstrings: usize,
    pub unwalked: usize,
    /// Of those, the ones the map has no entry for.
    pub not_in_map: usize,
    /// Script constants that are known lines, and those with Chinese that aren't.
    pub script_uses: usize,
    pub script_chinese_elsewhere: usize,
    /// `.tab` cells that are known lines, and those with Chinese that aren't.
    pub tab_uses: usize,
    pub tab_chinese_elsewhere: usize,
    /// Sources left out because they aren't UTF-8.
    pub not_utf8: usize,
    /// Entries whose English is the source itself, and English with Chinese in it.
    pub english_is_source: usize,
    pub english_with_chinese: usize,
}

/// What `extract` found.
#[derive(Debug)]
pub struct Extracted {
    pub lines: Vec<Line>,
    pub report: Report,
}

/// Whether `c` is a CJK ideograph.
pub fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

fn has_cjk(bytes: &[u8]) -> bool {
    String::from_utf8_lossy(bytes).chars().any(is_cjk)
}

/// A cache from the copy, unzipped when it's the compressed form (m2-tables.md 2.1).
fn read_cache(copy: &GameCopy, path: &str) -> Result<Vec<u8>> {
    let bytes = copy.read(path).with_context(|| path.to_string())?;
    Ok(cache::unzip(&bytes)?.unwrap_or(bytes))
}

/// One locale context's lines: each source line and its English.
type ContextLines = BTreeMap<Vec<u8>, Vec<u8>>;

/// The locale cache's map, by context: its root names a table for each, `""` the main one with
/// nearly every line, the others a few lines each (docs/re/retranslation-locale.md, Task 9).
fn locale_contexts(c: &Cache) -> Result<BTreeMap<String, ContextLines>> {
    let [Value::Table(root)] = c.values.as_slice() else {
        bail!("{LOCALE_CACHE}'s root isn't one table");
    };
    let mut contexts = BTreeMap::new();
    for (k, v) in &c.tables[root.0].hash {
        let (Value::Str(env), Value::Table(id)) = (k, v) else {
            continue;
        };
        let lines: ContextLines = c.tables[id.0]
            .hash
            .iter()
            .filter_map(|(k, v)| match (k, v) {
                (Value::Str(k), Value::Str(v)) => Some((k.to_vec(), v.to_vec())),
                _ => None,
            })
            .collect();
        contexts.insert(String::from_utf8_lossy(env).into_owned(), lines);
    }
    if !contexts.contains_key("") {
        bail!("{LOCALE_CACHE} has no main table (context \"\")");
    }
    Ok(contexts)
}

/// What one source line collects: its English, if the map has any, and its uses.
type Entries = BTreeMap<Vec<u8>, (Option<Vec<u8>>, BTreeSet<Use>)>;

fn key_text(v: &Value) -> String {
    match v {
        Value::Int(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Str(s) => String::from_utf8_lossy(s).into_owned(),
        Value::Bool(b) => b.to_string(),
        other => format!("{other:?}"),
    }
}

/// Every `LString` under `v`, through nested tables, each table once: an items row's `__index`
/// refers to the row itself (m2-tables.md 2.8).
fn lstrings<'a>(
    c: &Cache<'a>,
    v: &Value<'a>,
    seen: &mut HashSet<TableId>,
    out: &mut Vec<&'a [u8]>,
) {
    match v {
        Value::Magic {
            name: b"LString",
            arg,
        } => {
            if let Value::Str(s) = arg.as_ref() {
                out.push(s);
            }
        }
        Value::Table(id) if seen.insert(*id) => {
            let t = &c.tables[id.0];
            for v in t.array.iter().chain(t.hash.iter().map(|(_, v)| v)) {
                lstrings(c, v, seen, out);
            }
        }
        _ => {}
    }
}

/// Every design table's `LString`, by sheet, row and field, into `entries`.
fn table_uses(c: &Cache, entries: &mut Entries, report: &mut Report) {
    let [Value::Table(root)] = c.values.as_slice() else {
        return;
    };
    let mut sources = BTreeSet::new();
    for (sheet, v) in &c.tables[root.0].hash {
        let (Value::Str(sheet), Value::Table(sheet_id)) = (sheet, v) else {
            continue;
        };
        let sheet = String::from_utf8_lossy(sheet).into_owned();
        let t = &c.tables[sheet_id.0];
        let rows = (1..)
            .zip(&t.array)
            .map(|(i, v)| (i.to_string(), v))
            .chain(t.hash.iter().map(|(k, v)| (key_text(k), v)));
        for (row, v) in rows {
            // A row is a table of fields, its array part's numbered from 1, or a sheet's row is
            // an LString itself (field "").
            let fields: Vec<(String, &Value)> = match v {
                Value::Table(row_id) => {
                    let row = &c.tables[row_id.0];
                    (1..)
                        .zip(&row.array)
                        .map(|(i, v)| (i.to_string(), v))
                        .chain(row.hash.iter().map(|(field, v)| (key_text(field), v)))
                        .collect()
                }
                other => vec![(String::new(), other)],
            };
            let row_id = match v {
                Value::Table(id) => Some(*id),
                _ => None,
            };
            for (field, v) in fields {
                let mut found = Vec::new();
                let mut seen: HashSet<TableId> = row_id.into_iter().collect();
                lstrings(c, v, &mut seen, &mut found);
                for arg in found {
                    report.lstrings += 1;
                    let inner = l_sources(arg);
                    if inner.is_empty() {
                        report.no_l += 1;
                    }
                    for src in inner {
                        sources.insert(src.clone());
                        let entry = entries.entry(src).or_insert_with(|| {
                            report.not_in_map += 1;
                            (None, BTreeSet::new())
                        });
                        entry.1.insert(Use::Table {
                            sheet: sheet.clone(),
                            row: row.clone(),
                            field: field.clone(),
                        });
                    }
                }
            }
        }
    }
    report.table_sources = sources.len();
    // Then every table of the cache, so that no source is lost to a path the walk doesn't take.
    let mut unwalked = BTreeSet::new();
    for t in &c.tables {
        let values = t
            .array
            .iter()
            .chain(t.hash.iter().flat_map(|(k, v)| [k, v]));
        for v in values {
            let Value::Magic {
                name: b"LString",
                arg,
            } = v
            else {
                continue;
            };
            report.arena_lstrings += 1;
            if let Value::Str(s) = arg.as_ref() {
                unwalked.extend(
                    l_sources(s)
                        .into_iter()
                        .filter(|src| !sources.contains(src)),
                );
            }
        }
    }
    report.unwalked = unwalked.len();
    for src in unwalked {
        entries.entry(src).or_insert_with(|| {
            report.not_in_map += 1;
            (None, BTreeSet::new())
        });
    }
}

/// Every string constant of `f` and the functions inside it.
fn strings<'f>(f: &'f Function, out: &mut Vec<&'f [u8]>) {
    for k in &f.constants {
        if let Constant::ShortString(s) | Constant::LongString(s) = k {
            out.push(s);
        }
    }
    for p in &f.protos {
        strings(p, out);
    }
}

/// Every script's string constants that are known lines, as `Script` uses.
fn script_uses(exe: &PeImage, entries: &mut Entries, report: &mut Report) -> Result<()> {
    let fs = ExeFs::scan(exe)?;
    for e in fs.entries() {
        let Decoded::Lua(bytes, dialect) = decode(&fs, e) else {
            continue;
        };
        let path = e.engine_path();
        let chunk = fenji_lua::parse(&bytes, dialect).with_context(|| path.clone())?;
        let mut found = Vec::new();
        strings(&chunk.main, &mut found);
        for s in found {
            match entries.get_mut(s) {
                Some(entry) => {
                    if entry.1.insert(Use::Script { path: path.clone() }) {
                        report.script_uses += 1;
                    }
                }
                None if has_cjk(s) => report.script_chinese_elsewhere += 1,
                None => {}
            }
        }
    }
    Ok(())
}

/// Every `.tab` file under `data/tables`: UTF-8 with a byte-order mark, tab-separated, its first
/// line the columns' names, the next two their descriptions and defaults, then its rows
/// (m2-tables.md 2.9).
fn tab_uses(copy: &GameCopy, entries: &mut Entries, report: &mut Report) -> Result<()> {
    let tabs: Vec<String> = copy
        .paths()
        .filter(|p| p.starts_with("data/tables/") && p.ends_with(".tab"))
        .map(String::from)
        .collect();
    for path in tabs {
        // Each language's translation table: its rows are the map's, not places it's used.
        if path.starts_with("data/tables/locale/") {
            report.locale_tabs += 1;
            continue;
        }
        let bytes = copy.read(&path).with_context(|| path.clone())?;
        let text = String::from_utf8_lossy(&bytes);
        let mut lines = text.trim_start_matches('\u{feff}').lines();
        let names: Vec<&str> = lines.next().unwrap_or("").split('\t').collect();
        for line in lines.skip(2) {
            for (column, cell) in names.iter().zip(line.split('\t')) {
                match entries.get_mut(cell.as_bytes()) {
                    Some(entry) => {
                        if entry.1.insert(Use::Tab {
                            path: path.clone(),
                            column: column.to_string(),
                        }) {
                            report.tab_uses += 1;
                        }
                    }
                    None if cell.chars().any(is_cjk) => report.tab_chinese_elsewhere += 1,
                    None => {}
                }
            }
        }
    }
    Ok(())
}

/// Every source line of `copy`, with its English and its uses in `copy` and `exe`.
pub fn extract(copy: &GameCopy, exe: &PeImage) -> Result<Extracted> {
    let mut report = Report::default();
    let locale_bytes = read_cache(copy, LOCALE_CACHE)?;
    let locale = cache::read(&locale_bytes).context(LOCALE_CACHE)?;
    let mut contexts = locale_contexts(&locale)?;
    let main = contexts.remove("").unwrap_or_default();
    report.map = main.len();
    report.contexts = contexts.values().map(BTreeMap::len).sum();
    let mut entries: Entries = main
        .into_iter()
        .map(|(zh, en)| (zh, (Some(en), BTreeSet::new())))
        .collect();
    let table_bytes = read_cache(copy, TABLE_CACHE)?;
    let tables = cache::read(&table_bytes).context(TABLE_CACHE)?;
    table_uses(&tables, &mut entries, &mut report);
    script_uses(exe, &mut entries, &mut report)?;
    tab_uses(copy, &mut entries, &mut report)?;
    let mut lines = Vec::with_capacity(entries.len());
    for (src, (en, uses)) in entries {
        let fp = fingerprint(&src);
        let Ok(zh) = String::from_utf8(src) else {
            report.not_utf8 += 1;
            continue;
        };
        let en = en.map(|en| String::from_utf8_lossy(&en).into_owned());
        if let Some(en) = &en {
            if *en == zh {
                report.english_is_source += 1;
            }
            if en.chars().any(is_cjk) {
                report.english_with_chinese += 1;
            }
        }
        let uses: Vec<Use> = uses.into_iter().collect();
        let (area, tight) = classify(&uses);
        lines.push(Line {
            fp,
            env: String::new(),
            zh,
            en,
            uses,
            area,
            tight,
        });
    }
    // A context's few lines are interface words (roles, ranks, time and size abbreviations):
    // tight, and keyed in their context.
    for (env, context) in contexts {
        for (src, en) in context {
            let Ok(zh) = String::from_utf8(src) else {
                report.not_utf8 += 1;
                continue;
            };
            lines.push(Line {
                fp: fingerprint_in(&env, zh.as_bytes()),
                env: env.clone(),
                zh,
                en: Some(String::from_utf8_lossy(&en).into_owned()),
                uses: Vec::new(),
                area: crate::area::Area::Interface,
                tight: true,
            });
        }
    }
    Ok(Extracted { lines, report })
}

/// `report.txt`: what `extract` found, in numbers only, of the spec's §5 kind.
pub fn report_text(found: &Extracted) -> String {
    use std::fmt::Write;
    let lines = &found.lines;
    let english = lines.iter().filter(|l| l.en.is_some()).count();
    let tight = lines.iter().filter(|l| l.tight).count();
    let mut by_area: BTreeMap<crate::area::Area, usize> = BTreeMap::new();
    for l in lines {
        *by_area.entry(l.area).or_default() += 1;
    }
    let r = &found.report;
    let mut t = String::new();
    let _ = writeln!(t, "lines: {}", lines.len());
    let _ = writeln!(t, "  with English today: {english}");
    let _ = writeln!(t, "  with none: {}", lines.len() - english);
    let _ = writeln!(t, "  tight: {tight}");
    let _ = writeln!(t, "by area:");
    for (area, n) in &by_area {
        let _ = writeln!(t, "  {}: {n}", area.name());
    }
    let _ = writeln!(t, "the locale map's entries: {}", r.map);
    let _ = writeln!(t, "  lines in its other contexts: {}", r.contexts);
    let _ = writeln!(
        t,
        "languages' translation tables, not uses: {}",
        r.locale_tabs
    );
    let _ = writeln!(t, "LStrings in the design tables: {}", r.lstrings);
    let _ = writeln!(t, "  with no L(\"…\"): {}", r.no_l);
    let _ = writeln!(t, "  distinct table sources: {}", r.table_sources);
    let _ = writeln!(
        t,
        "LStrings in every table, however reached: {}",
        r.arena_lstrings
    );
    let _ = writeln!(
        t,
        "  distinct sources the sheets' walk missed: {}",
        r.unwalked
    );
    let _ = writeln!(t, "table sources not in the map: {}", r.not_in_map);
    let _ = writeln!(t, "script uses: {}", r.script_uses);
    let _ = writeln!(
        t,
        "  script strings with Chinese, not lines: {}",
        r.script_chinese_elsewhere
    );
    let _ = writeln!(t, ".tab uses: {}", r.tab_uses);
    let _ = writeln!(
        t,
        "  .tab cells with Chinese, not lines: {}",
        r.tab_chinese_elsewhere
    );
    let _ = writeln!(t, "sources left out, not UTF-8: {}", r.not_utf8);
    let _ = writeln!(
        t,
        "English that is the source itself: {}",
        r.english_is_source
    );
    let _ = writeln!(t, "English holding Chinese: {}", r.english_with_chinese);
    t
}

/// `src`, the inside of a Lua string literal, as Lua 5.3 reads it: `\a \b \f \n \r \t \v \\ \" \'`,
/// a backslash before a line break, `\z` and the white space after it, `\ddd` (decimal, at most 3
/// digits), `\xXX` and `\u{X…}`. An escape Lua refuses is kept as written.
pub fn lua_unescape(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len());
    let mut i = 0;
    while i < src.len() {
        if src[i] != b'\\' || i + 1 == src.len() {
            out.push(src[i]);
            i += 1;
            continue;
        }
        let c = src[i + 1];
        i += 2;
        match c {
            b'a' => out.push(7),
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'n' | b'\n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(11),
            b'\\' | b'"' | b'\'' => out.push(c),
            b'z' => {
                while i < src.len() && src[i].is_ascii_whitespace() {
                    i += 1;
                }
            }
            b'0'..=b'9' => {
                let mut n = u32::from(c - b'0');
                for _ in 0..2 {
                    match src.get(i) {
                        Some(d @ b'0'..=b'9') => {
                            n = n * 10 + u32::from(d - b'0');
                            i += 1;
                        }
                        _ => break,
                    }
                }
                match u8::try_from(n) {
                    Ok(b) => out.push(b),
                    Err(_) => out.extend_from_slice(format!("\\{n}").as_bytes()),
                }
            }
            b'x' => {
                let hex = src.get(i..i + 2).and_then(|h| std::str::from_utf8(h).ok());
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(b) => {
                        out.push(b);
                        i += 2;
                    }
                    None => out.extend_from_slice(b"\\x"),
                }
            }
            b'u' if src.get(i) == Some(&b'{') => {
                let end = src[i..].iter().position(|&b| b == b'}').map(|p| i + p);
                let ch = end
                    .and_then(|e| std::str::from_utf8(&src[i + 1..e]).ok())
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .and_then(char::from_u32);
                match (ch, end) {
                    (Some(ch), Some(e)) => {
                        let mut buf = [0; 4];
                        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        i = e + 1;
                    }
                    _ => out.extend_from_slice(b"\\u"),
                }
            }
            other => {
                out.push(b'\\');
                out.push(other);
            }
        }
    }
    out
}

/// The source of every `L("…")` or `L('…')` in an `LString`'s argument, unescaped
/// (m2-tables.md 2.6): one for the plain form, and those inside the other forms' calls.
pub fn l_sources(arg: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(at) = arg[i..].windows(2).position(|w| w == b"L(").map(|p| i + p) {
        i = at + 2;
        // An `L(` that's the end of a longer name isn't a call of `L`.
        if at > 0 && (arg[at - 1].is_ascii_alphanumeric() || arg[at - 1] == b'_') {
            continue;
        }
        let Some(&quote @ (b'"' | b'\'')) = arg.get(i) else {
            continue;
        };
        let start = i + 1;
        let mut j = start;
        while j < arg.len() && arg[j] != quote {
            j += if arg[j] == b'\\' { 2 } else { 1 };
        }
        if j >= arg.len() {
            break;
        }
        out.push(lua_unescape(&arg[start..j]));
        i = j + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::area::Area;

    #[test]
    fn the_report_counts_lines_by_area_english_and_tightness() {
        let line = |zh: &str, en: Option<&str>, area: Area, tight: bool| Line {
            fp: fingerprint(zh.as_bytes()),
            env: String::new(),
            zh: zh.into(),
            en: en.map(String::from),
            uses: vec![],
            area,
            tight,
        };
        let found = Extracted {
            lines: vec![
                line("剑", Some("Sword"), Area::Items, true),
                line("盾", None, Area::Items, false),
                line("好", Some("Good"), Area::Interface, true),
            ],
            report: Report {
                map: 2,
                not_in_map: 1,
                ..Report::default()
            },
        };
        let text = report_text(&found);
        for want in [
            "lines: 3",
            "with English today: 2",
            "with none: 1",
            "tight: 2",
            "items: 2",
            "interface: 1",
            "the locale map's entries: 2",
            "table sources not in the map: 1",
        ] {
            assert!(text.contains(want), "{want:?} in:\n{text}");
        }
    }

    #[test]
    fn escapes_in_an_lstring_unescape_as_lua_does() {
        assert_eq!(lua_unescape(br"a\nb"), b"a\nb");
        assert_eq!(lua_unescape(br#"say \"hi\""#), b"say \"hi\"");
        assert_eq!(lua_unescape(br"back\\slash"), b"back\\slash");
        assert_eq!(lua_unescape(br"\100\65"), b"dA");
        assert_eq!(lua_unescape(br"\x41\x4a"), b"AJ");
        assert_eq!(lua_unescape(br"\u{4E2D}"), "中".as_bytes());
        assert_eq!(lua_unescape(b"a\\z  \n  b"), b"ab");
        // The inner L("…") of the other forms, each one.
        assert_eq!(l_sources(br#"L("a\"b")"#), [b"a\"b".to_vec()]);
        assert_eq!(
            l_sources("A(L(\"剑\"), L('盾'))".as_bytes()),
            ["剑".as_bytes().to_vec(), "盾".as_bytes().to_vec()]
        );
        assert!(l_sources(b"E(3)").is_empty());
    }
}
