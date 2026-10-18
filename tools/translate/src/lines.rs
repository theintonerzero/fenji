//! The lines the tools work on, as `re/retranslation/lines.jsonl`, one JSON object a line: each
//! source line of the pinned copy, its fingerprint, its English today, and where the game uses it.
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result};
use fenji_pack::Fingerprint;
use serde::{Deserialize, Serialize};

use crate::area::Area;

/// The lines' file in the work folder.
pub const LINES: &str = "lines.jsonl";

/// One source line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    /// Its fingerprint (retranslation spec §6.2), in its context ([`fenji_pack::fingerprint_in`]).
    pub fp: Fingerprint,
    /// Its locale context: `""` for the map's main table, else one of its few small ones, such as
    /// `noun`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub env: String,
    /// The line in the source language, as the client holds it.
    pub zh: String,
    /// Its English today, if the client has any.
    pub en: Option<String>,
    /// Where the game uses it, sorted.
    pub uses: Vec<Use>,
    /// Which of `fenji-content`'s files it goes in ([`crate::area::classify`]).
    pub area: Area,
    /// Whether the interface holds it tight (RT11).
    pub tight: bool,
}

/// One place the game uses a line.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Use {
    /// A field of a design table's row, as an `LString` (m2-tables.md 2.6).
    Table {
        sheet: String,
        row: String,
        field: String,
    },
    /// A string constant of one of the scripts in the exe.
    Script { path: String },
    /// A cell of a `.tab` file the client reads directly.
    Tab { path: String, column: String },
}

/// Writes `lines` to `work`'s lines file, replacing it whole.
pub fn write_lines(work: &Path, lines: &[Line]) -> Result<()> {
    std::fs::create_dir_all(work)?;
    let path = work.join(LINES);
    let new = work.join(format!("{LINES}.new"));
    let file = std::fs::File::create(&new).with_context(|| new.display().to_string())?;
    let mut w = BufWriter::new(file);
    for line in lines {
        serde_json::to_writer(&mut w, line)?;
        w.write_all(b"\n")?;
    }
    w.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    std::fs::rename(&new, &path).with_context(|| path.display().to_string())?;
    Ok(())
}

/// The lines `work` holds.
pub fn read_lines(work: &Path) -> Result<Vec<Line>> {
    let path = work.join(LINES);
    let file = std::fs::File::open(&path).with_context(|| path.display().to_string())?;
    BufReader::new(file)
        .lines()
        .enumerate()
        .map(|(i, text)| {
            serde_json::from_str(&text?).with_context(|| format!("{}:{}", path.display(), i + 1))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::area::Area;

    #[test]
    fn lines_round_trip_through_their_file() {
        let dir = tempfile::tempdir().unwrap();
        let lines = vec![Line {
            fp: fenji_pack::fingerprint("剑".as_bytes()),
            env: String::new(),
            zh: "剑".into(),
            en: Some("Sword".into()),
            uses: vec![Use::Script {
                path: "data/a.luac".into(),
            }],
            area: Area::Interface,
            tight: true,
        }];
        write_lines(dir.path(), &lines).unwrap();
        assert_eq!(read_lines(dir.path()).unwrap(), lines);
    }
}
