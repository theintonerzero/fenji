//! `extract` end to end, on an invented copy and exe: no game bytes.
use std::path::Path;

use fenji_data::PeImage;
use fenji_data::cache::{Cache, Table, TableId, Value, write};
use fenji_data::client::GameCopy;
use fenji_data::test_util::{exe_with_files, lz4_blob, stored_blob, zfs_part};
use fenji_translate::extract::extract;
use fenji_translate::lines::Use;
use sha1::{Digest, Sha1};

fn key(bytes: &[u8]) -> [u8; 20] {
    Sha1::digest(bytes).into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A copy at `root` holding `files`, each stored at its path, and an `_index` that names them.
fn copy(root: &Path, files: &[(&str, Vec<u8>)]) -> GameCopy {
    let data = root.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let records = files
        .iter()
        .map(|(_, b)| (1, key(b), stored_blob(b)))
        .collect();
    std::fs::write(data.join("data000"), zfs_part(&[records])).unwrap();
    let index: String = files
        .iter()
        .map(|(path, b)| format!("{}\t{path}\n", hex(&key(b))))
        .collect();
    std::fs::write(data.join("_index"), stored_blob(index.as_bytes())).unwrap();
    GameCopy::open(root).unwrap()
}

fn s(t: &str) -> Value<'_> {
    Value::Str(t.as_bytes())
}

/// A locale cache like the client's: a root naming one table per context, `""` the main one
/// mapping each source line to its English, and a context of one line, `noun`.
fn locale(pairs: &[(&str, &str)]) -> Vec<u8> {
    let main = Table {
        array: vec![],
        hash: pairs.iter().map(|(zh, en)| (s(zh), s(en))).collect(),
    };
    let noun = Table {
        array: vec![],
        hash: vec![(s("确定"), s("Confirmation"))],
    };
    let root = Table {
        array: vec![],
        hash: vec![
            (s(""), Value::Table(TableId(1))),
            (s("noun"), Value::Table(TableId(2))),
        ],
    };
    write(&Cache {
        tables: vec![root, main, noun],
        values: vec![Value::Table(TableId(0))],
    })
}

/// A design tables' cache with two sheets: `items`, each cell a row, a field and its `LString`'s
/// argument as the client writes it; and `tips`, whose rows are `LString`s themselves.
fn all_tables(cells: &[(i64, &str, &str)]) -> Vec<u8> {
    let tip = Value::Magic {
        name: b"LString",
        arg: Box::new(Value::Str("L(\"小心脚下\")".as_bytes())),
    };
    // Text held by the root itself, which no sheet's walk reaches.
    let motto = Value::Magic {
        name: b"LString",
        arg: Box::new(Value::Str("L(\"天下\")".as_bytes())),
    };
    let mut tables = vec![
        Table {
            array: vec![],
            hash: vec![
                (Value::Str(b"items"), Value::Table(TableId(1))),
                (Value::Str(b"tips"), Value::Table(TableId(2))),
                (Value::Str(b"motto"), motto),
            ],
        },
        Table::default(),
        Table {
            array: vec![tip],
            hash: vec![],
        },
    ];
    // A row whose text is in its array part: a list of lines.
    let listed = Value::Magic {
        name: b"LString",
        arg: Box::new(Value::Str("L(\"第一句\")".as_bytes())),
    };
    tables.push(Table {
        array: vec![listed],
        hash: vec![],
    });
    tables[1]
        .hash
        .push((Value::Int(900), Value::Table(TableId(3))));
    let mut rows: Vec<i64> = cells.iter().map(|c| c.0).collect();
    rows.dedup();
    for row in rows {
        let fields = cells
            .iter()
            .filter(|c| c.0 == row)
            .map(|(_, field, arg)| {
                (
                    Value::Str(field.as_bytes()),
                    Value::Magic {
                        name: b"LString",
                        arg: Box::new(Value::Str(arg.as_bytes())),
                    },
                )
            })
            .collect();
        let id = tables.len();
        tables.push(Table {
            array: vec![],
            hash: fields,
        });
        tables[1]
            .hash
            .push((Value::Int(row), Value::Table(TableId(id))));
    }
    write(&Cache {
        tables,
        values: vec![Value::Table(TableId(0))],
    })
}

/// An exe whose one embedded script is `source`, compiled by stock Lua 5.3.
fn exe(source: &str) -> PeImage {
    let chunk = mlua::Lua::new()
        .load(source)
        .into_function()
        .unwrap()
        .dump(true);
    let pe = exe_with_files(&[("interface/ui/Test.luac", lz4_blob(&chunk, 0))]);
    PeImage::parse(pe).unwrap()
}

#[test]
fn extract_joins_the_map_the_tables_and_the_scripts() {
    let root = tempfile::tempdir().unwrap();
    // A .tab file: its names, their descriptions (in Chinese, never displayed), its defaults,
    // then one row.
    let tab = "\u{feff}id\ttext\r\n编号\t文本\r\n\t\r\n1\t确定\r\n";
    let copy = copy(
        root.path(),
        &[
            (
                "data/caches/locale_en.new_cache",
                locale(&[("铁剑", "Iron Sword"), ("确定", "OK"), ("天色已晚", "It's late")]),
            ),
            (
                "data/caches/all_tables.new_cache",
                all_tables(&[
                    (101, "name", "L(\"铁剑\")"),
                    (101, "desc", "L(\"一把普通的铁剑。\")"),
                ]),
            ),
            ("data/tables/present/ui.tab", tab.as_bytes().to_vec()),
            // A language's translation table: the map's own source, never a use.
            (
                "data/tables/locale/en.tab",
                "\u{feff}ID\tOrigin\tTranslation\tLocaleEnv\r\n\t\t\t\r\n\t\t\t\r\n1\t铁剑\tIron Sword\t\r\n"
                    .as_bytes()
                    .to_vec(),
            ),
        ],
    );
    let found = extract(&copy, &exe("local ok = \"确定\"")).unwrap();
    let lines = found.lines;
    // Three in the main table, one in a context, one only in a table, one a sheet's row, one in
    // a row's list, and one only the sweep of every table finds.
    assert_eq!(lines.len(), 8);
    assert_eq!(found.report.unwalked, 1);
    assert!(lines.iter().any(|l| l.zh == "天下" && l.uses.is_empty()));
    for line in &lines {
        assert_eq!(
            line.fp,
            fenji_pack::fingerprint_in(&line.env, line.zh.as_bytes())
        );
    }
    let noun = lines.iter().find(|l| l.env == "noun").unwrap();
    assert_eq!(noun.zh, "确定");
    assert_eq!(noun.en.as_deref(), Some("Confirmation"));
    assert_eq!(found.report.locale_tabs, 1);
    let line = |zh: &str| {
        lines
            .iter()
            .find(|l| l.zh == zh && l.env.is_empty())
            .unwrap()
    };
    assert_eq!(
        line("第一句").uses,
        [Use::Table {
            sheet: "items".into(),
            row: "900".into(),
            field: "1".into()
        }]
    );
    assert_eq!(
        line("小心脚下").uses,
        [Use::Table {
            sheet: "tips".into(),
            row: "1".into(),
            field: String::new()
        }]
    );
    let cell = |field: &str| Use::Table {
        sheet: "items".into(),
        row: "101".into(),
        field: field.into(),
    };
    assert_eq!(line("铁剑").en.as_deref(), Some("Iron Sword"));
    assert_eq!(line("铁剑").uses, [cell("name")]);
    assert_eq!(line("一把普通的铁剑。").en, None);
    assert_eq!(line("一把普通的铁剑。").uses, [cell("desc")]);
    let ok = &line("确定").uses;
    assert!(
        ok.iter()
            .any(|u| matches!(u, Use::Script { path } if path.ends_with("interface/ui/Test.luac"))),
        "{ok:?}"
    );
    assert!(ok.contains(&Use::Tab {
        path: "data/tables/present/ui.tab".into(),
        column: "text".into()
    }));
    assert!(line("天色已晚").uses.is_empty());
    // en.tab named 铁剑, and isn't one of its uses.
    assert!(
        line("铁剑")
            .uses
            .iter()
            .all(|u| !matches!(u, Use::Tab { .. }))
    );
}

#[test]
fn sources_differing_by_a_trailing_space_stay_two_lines() {
    let root = tempfile::tempdir().unwrap();
    let copy = copy(
        root.path(),
        &[
            (
                "data/caches/locale_en.new_cache",
                locale(&[("好", "Good"), ("好 ", "Fine")]),
            ),
            ("data/caches/all_tables.new_cache", all_tables(&[])),
        ],
    );
    let lines = extract(&copy, &exe("local x = 1")).unwrap().lines;
    let main: Vec<_> = lines.iter().filter(|l| l.zh.starts_with('好')).collect();
    assert_eq!(main.len(), 2);
    assert_ne!(main[0].fp, main[1].fp);
}
