//! The batches (retranslation spec §6.5, step 5): the lines grouped by context, each batch sized
//! for one run and carrying the glossary's terms its lines hold, as files in the work folder.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use aho_corasick::AhoCorasick;
use anyhow::{Context, Result};
use fenji_pack::Fingerprint;
use serde::{Deserialize, Serialize};

use crate::area::{Area, TIGHT_FIELDS, classify};
use crate::check::{Glossary, Rules, Term, budget};
use crate::lines::{Line, Use};

/// The batches' folder in the work folder.
pub const BATCHES: &str = "batches";

/// The most lines a batch holds.
pub const MAX_LINES: usize = 80;

/// The most characters of source a batch holds, its markup's among them. A longer single line is
/// a batch of its own.
pub const MAX_CHARS: usize = 4_000;

/// One run's work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Batch {
    /// `<area>-<00001>`, its file's name.
    pub id: String,
    pub lines: Vec<BatchLine>,
    /// The glossary's terms its lines hold.
    pub glossary: Vec<Term>,
}

/// One line of a batch, with what its translator needs to know.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchLine {
    pub fp: Fingerprint,
    pub zh: String,
    pub en_old: Option<String>,
    /// Where the game uses it.
    pub context: String,
    pub tight: bool,
    /// A tight line's most characters drawn ([`crate::check::budget`]).
    pub budget: Option<usize>,
}

/// Where a line sits: the group whose lines stay together, in a batch of their area, in order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Group {
    /// A table's row: an item's name and description, a conversation's line.
    Row { sheet: String, row: Vec<Chunk> },
    /// A script's strings.
    Script(String),
    /// A `.tab` file's cells.
    Tab(String),
    /// A line only the locale map holds, where the game shows it unknown.
    Map,
}

/// A piece of a natural sort key: `row 10` after `row 9`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Chunk {
    Number(u64),
    Text(String),
}

fn natural(s: &str) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut rest = s;
    while let Some(c) = rest.chars().next() {
        let digit = c.is_ascii_digit();
        let end = rest
            .find(|c: char| c.is_ascii_digit() != digit)
            .unwrap_or(rest.len());
        let (piece, tail) = rest.split_at(end);
        chunks.push(match piece.parse() {
            Ok(n) if digit => Chunk::Number(n),
            _ => Chunk::Text(piece.to_string()),
        });
        rest = tail;
    }
    chunks
}

/// A line in its group.
struct Member<'a> {
    /// Its order in the group: a row's name and title before its other fields.
    order: (bool, Vec<Chunk>),
    line: &'a Line,
    /// Its first use in its own area.
    first: Option<&'a Use>,
}

/// A line's place: its group, from its first use in its own area.
fn place(line: &Line) -> (Group, Member<'_>) {
    let first = line
        .uses
        .iter()
        .find(|u| classify(std::slice::from_ref(u)).0 == line.area);
    let (group, order) = match first {
        Some(Use::Table { sheet, row, field }) => (
            Group::Row {
                sheet: sheet.clone(),
                row: natural(row),
            },
            (!TIGHT_FIELDS.contains(&field.as_str()), natural(field)),
        ),
        Some(Use::Script { path }) => (Group::Script(path.clone()), (false, Vec::new())),
        Some(Use::Tab { path, column }) => (Group::Tab(path.clone()), (false, natural(column))),
        None => (Group::Map, (false, Vec::new())),
    };
    (group, Member { order, line, first })
}

fn describe(u: &Use) -> String {
    match u {
        Use::Table { sheet, row, field } => format!("{sheet} row {row}, {field}"),
        Use::Script { path } => format!("script {path}"),
        Use::Tab { path, column } => format!("{path}, column {column}"),
    }
}

/// Where the game uses `line`, for its translator: its first use, and up to three others.
fn context(line: &Line, first: Option<&Use>) -> String {
    let Some(first) = first else {
        return "only in the locale map; where the game shows it is unknown".into();
    };
    let others: Vec<String> = line
        .uses
        .iter()
        .filter(|u| *u != first)
        .map(describe)
        .collect();
    match others.len() {
        0 => describe(first),
        n if n <= 3 => format!("{}; also {}", describe(first), others.join("; ")),
        n => format!(
            "{}; also {} and {} more",
            describe(first),
            others[..3].join("; "),
            n - 3
        ),
    }
}

/// `lines` in batches, by area, each line in exactly one. A group's lines (a row's, a script's)
/// share a batch unless they're more than [`MAX_LINES`] or [`MAX_CHARS`]; groups follow each
/// other in their sheet's and row's order, and lines only the locale map holds come last, sorted
/// so that lines alike sit together.
pub fn plan_batches(lines: &[Line], glossary: &Glossary, rules: &Rules) -> Vec<Batch> {
    let mut groups: BTreeMap<(Area, Group), Vec<Member>> = BTreeMap::new();
    for line in lines {
        let (group, member) = place(line);
        groups.entry((line.area, group)).or_default().push(member);
    }
    // The patterns are at most the glossary's terms, far below the automaton's limits.
    let terms = AhoCorasick::new(glossary.terms.iter().map(|t| t.zh.as_str()))
        .expect("an automaton of the glossary's terms");
    let mut planner = Planner {
        glossary,
        terms: &terms,
        batches: Vec::new(),
        per_area: BTreeMap::new(),
        area: Area::Other,
        current: Vec::new(),
        chars: 0,
    };
    for ((area, _), mut members) in groups {
        members.sort_by(|a, b| {
            a.order
                .cmp(&b.order)
                .then_with(|| a.line.zh.cmp(&b.line.zh))
        });
        let size: usize = members.iter().map(|m| m.line.zh.chars().count()).sum();
        if area != planner.area
            || planner.current.len() + members.len() > MAX_LINES
            || planner.chars + size > MAX_CHARS
        {
            planner.flush();
        }
        planner.area = area;
        for Member { line, first, .. } in members {
            let n = line.zh.chars().count();
            if !planner.current.is_empty()
                && (planner.current.len() == MAX_LINES || planner.chars + n > MAX_CHARS)
            {
                planner.flush();
            }
            planner.current.push(BatchLine {
                fp: line.fp,
                zh: line.zh.clone(),
                en_old: line.en.clone(),
                context: context(line, first),
                tight: line.tight,
                budget: budget(line, rules),
            });
            planner.chars += n;
        }
    }
    planner.flush();
    planner.batches
}

/// The batch being filled, and those done.
struct Planner<'a> {
    glossary: &'a Glossary,
    terms: &'a AhoCorasick,
    batches: Vec<Batch>,
    per_area: BTreeMap<Area, usize>,
    area: Area,
    current: Vec<BatchLine>,
    chars: usize,
}

impl Planner<'_> {
    fn flush(&mut self) {
        if self.current.is_empty() {
            return;
        }
        let lines = std::mem::take(&mut self.current);
        self.chars = 0;
        let held: BTreeSet<usize> = lines
            .iter()
            .flat_map(|l| {
                self.terms
                    .find_overlapping_iter(&l.zh)
                    .map(|m| m.pattern().as_usize())
            })
            .collect();
        let n = self.per_area.entry(self.area).or_default();
        *n += 1;
        self.batches.push(Batch {
            id: format!("{}-{n:05}", self.area.name()),
            lines,
            glossary: held
                .into_iter()
                .map(|i| self.glossary.terms[i].clone())
                .collect(),
        });
    }
}

/// Writes `batches` to `work`'s batches folder, one file each, and removes any other batch there,
/// so the folder holds this plan only.
pub fn write_batches(work: &Path, batches: &[Batch]) -> Result<()> {
    let dir = work.join(BATCHES);
    std::fs::create_dir_all(&dir).with_context(|| dir.display().to_string())?;
    let ids: BTreeSet<String> = batches.iter().map(|b| format!("{}.json", b.id)).collect();
    for entry in std::fs::read_dir(&dir)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if name.ends_with(".json") && !ids.contains(&name) {
            std::fs::remove_file(dir.join(&name))?;
        }
    }
    for b in batches {
        let path = dir.join(format!("{}.json", b.id));
        let tmp = dir.join(format!("{}.json.tmp", b.id));
        std::fs::write(&tmp, serde_json::to_vec_pretty(b)?)?;
        std::fs::rename(&tmp, &path).with_context(|| path.display().to_string())?;
    }
    Ok(())
}

/// The batches in `work`, by id.
pub fn read_batches(work: &Path) -> Result<Vec<Batch>> {
    let dir = work.join(BATCHES);
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .with_context(|| format!("{}: plan the batches first", dir.display()))?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<_>>()?;
    paths.retain(|p| p.extension().is_some_and(|e| e == "json"));
    paths.sort();
    paths
        .iter()
        .map(|p| {
            let text = std::fs::read(p).with_context(|| p.display().to_string())?;
            serde_json::from_slice(&text).with_context(|| p.display().to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use fenji_pack::fingerprint;

    use super::*;
    use crate::area::{Area, classify};
    use crate::check::TermKind;
    use crate::lines::Use;

    /// An invented line, with its area and tightness from its uses as the extractor gives them.
    fn line(zh: &str, en: Option<&str>, uses: Vec<Use>) -> Line {
        let (area, tight) = classify(&uses);
        Line {
            fp: fingerprint(zh.as_bytes()),
            env: String::new(),
            zh: zh.into(),
            en: en.map(String::from),
            uses,
            area,
            tight,
        }
    }

    fn table(sheet: &str, row: &str, field: &str) -> Use {
        Use::Table {
            sheet: sheet.into(),
            row: row.into(),
            field: field.into(),
        }
    }

    fn script(path: &str) -> Use {
        Use::Script { path: path.into() }
    }

    fn rules() -> Rules {
        Rules::from_lines(&[])
    }

    /// Which batch holds each fingerprint.
    fn batch_of(batches: &[Batch]) -> std::collections::HashMap<Fingerprint, &str> {
        batches
            .iter()
            .flat_map(|b| b.lines.iter().map(move |l| (l.fp, b.id.as_str())))
            .collect()
    }

    fn term(zh: &str, en: &str) -> Term {
        Term {
            zh: zh.into(),
            en: en.into(),
            kind: TermKind::Name,
            variants: vec![],
            note: String::new(),
        }
    }

    #[test]
    fn a_rows_lines_stay_together_and_batches_keep_their_limits() {
        let mut lines = Vec::new();
        for row in 1..=150 {
            lines.push(line(
                &format!("铁剑{row}"),
                Some("Iron Sword"),
                vec![table("items", &row.to_string(), "name")],
            ));
            lines.push(line(
                &format!("一把铁剑{row}"),
                None,
                vec![table("items", &row.to_string(), "desc")],
            ));
        }
        let long = "长".repeat(MAX_CHARS + 10);
        lines.push(line(&long, None, vec![table("book", "1", "text")]));
        for (path, n) in [("ui/a.luac", 3), ("ui/b.luac", 4)] {
            for i in 0..n {
                lines.push(line(&format!("{path}按钮{i}"), None, vec![script(path)]));
            }
        }
        let batches = plan_batches(&lines, &Glossary::default(), &rules());
        let of = batch_of(&batches);
        assert_eq!(of.len(), lines.len(), "every line in one batch");
        let held: usize = batches.iter().map(|b| b.lines.len()).sum();
        assert_eq!(held, lines.len(), "and only once");
        let area_of: std::collections::HashMap<Fingerprint, Area> =
            lines.iter().map(|l| (l.fp, l.area)).collect();
        for row in 1..=150 {
            let name = fingerprint(format!("铁剑{row}").as_bytes());
            let desc = fingerprint(format!("一把铁剑{row}").as_bytes());
            assert_eq!(of[&name], of[&desc], "row {row}'s lines together");
        }
        for b in &batches {
            let chars: usize = b.lines.iter().map(|l| l.zh.chars().count()).sum();
            assert!(b.lines.len() <= MAX_LINES, "{}", b.id);
            assert!(chars <= MAX_CHARS || b.lines.len() == 1, "{}", b.id);
            let areas: BTreeSet<Area> = b.lines.iter().map(|l| area_of[&l.fp]).collect();
            assert_eq!(areas.len(), 1, "{}: one area", b.id);
            let area = areas.first().unwrap().name();
            assert!(b.id.starts_with(&format!("{area}-")), "{}", b.id);
        }
        let long_batch = of[&fingerprint(long.as_bytes())];
        assert_eq!(
            batches
                .iter()
                .find(|b| b.id == long_batch)
                .unwrap()
                .lines
                .len(),
            1
        );
        assert!(long_batch.starts_with("lore-"), "{long_batch}");
        for path in ["ui/a.luac", "ui/b.luac"] {
            let held: BTreeSet<&str> = lines
                .iter()
                .filter(|l| l.uses == [script(path)])
                .map(|l| of[&l.fp])
                .collect();
            assert_eq!(held.len(), 1, "{path}'s lines together");
        }
        let first = &batches
            .iter()
            .find(|b| b.id == of[&fingerprint("铁剑1".as_bytes())])
            .unwrap();
        assert!(first.id.starts_with("items-"));
        assert_eq!(
            first.lines[0].zh, "铁剑1",
            "a row's lines in their row's order"
        );
        assert_eq!(first.lines[0].context, "items row 1, name");
        assert_eq!(first.lines[0].en_old.as_deref(), Some("Iron Sword"));
        assert!(first.lines[0].tight);
        assert_eq!(first.lines[0].budget, Some(11));
        assert_eq!(first.lines[1].budget, None);
    }

    #[test]
    fn a_batch_carries_only_the_terms_its_lines_hold() {
        let lines = vec![
            line("林月来了", None, vec![table("quest_dialog", "1", "text")]),
            line("青石", None, vec![table("items", "1", "name")]),
        ];
        let glossary = Glossary {
            terms: vec![
                term("林月", "Lin Yue"),
                term("青石", "Bluestone"),
                term("白云", "White Cloud"),
            ],
        };
        let batches = plan_batches(&lines, &glossary, &rules());
        assert_eq!(batches.len(), 2, "two areas");
        for b in &batches {
            let terms: Vec<&str> = b.glossary.iter().map(|t| t.zh.as_str()).collect();
            let want: Vec<&str> = if b.id.starts_with("dialogue-") {
                vec!["林月"]
            } else {
                vec!["青石"]
            };
            assert_eq!(terms, want, "{}", b.id);
        }
    }

    #[test]
    fn batches_go_to_files_and_back() {
        let work = tempfile::tempdir().unwrap();
        let lines = vec![line("青石", None, vec![table("items", "1", "name")])];
        let batches = plan_batches(&lines, &Glossary::default(), &rules());
        assert_eq!(batches[0].id, "items-00001");
        std::fs::create_dir_all(work.path().join(BATCHES)).unwrap();
        std::fs::write(work.path().join(BATCHES).join("old-00001.json"), "{}").unwrap();
        write_batches(work.path(), &batches).unwrap();
        assert_eq!(read_batches(work.path()).unwrap(), batches, "and no other");
    }
}
