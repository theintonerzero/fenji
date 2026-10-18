//! The glossary (retranslation spec §6.5, step 2; RT8, RT9): the candidates a session drafts it
//! from, the glossary itself in the work folder, and its export for `fenji-content`, which holds
//! no Chinese (ground rule 3).
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use aho_corasick::AhoCorasick;
use anyhow::{Context, Result, bail};
use fenji_pack::{escape, fingerprint};
use serde::Serialize;

use crate::area::{Area, classify};
use crate::check::{Glossary, TermKind};
use crate::extract::is_cjk;
use crate::lines::{Line, Use};
use crate::markup::{SLOT, tokens};

/// The glossary in the work folder, as Task 11 drafts it.
pub const GLOSSARY: &str = "glossary.json";

/// The candidates in the work folder, most used first, as `translate glossary candidates` writes
/// them.
pub const CANDIDATES: &str = "candidates.tsv";

/// The design tables' fields whose lines are names, and what they name, from every line's uses
/// (Task 5). Any other sheet's names, items' among them, are the translators' to word.
pub const NAME_FIELDS: &[(&str, &str, Group)] = &[
    ("npcs", "name", Group::Character),
    ("new_npcs", "prop_base_name", Group::Character),
    ("npcname_temple", "name", Group::Character),
    ("cutscene_dialog", "RoleName", Group::Character),
    ("maps", "name", Group::Place),
    ("scene", "name", Group::Place),
    ("submaps", "name", Group::Place),
    ("areas", "name", Group::Place),
    ("world_map", "name", Group::Place),
    ("dungeons", "name", Group::Place),
    ("npcorg", "name", Group::Sect),
    ("reputation", "Name", Group::Sect),
    ("vocation_templ", "Name", Group::Class),
    ("vocation_templ", "WeaponName", Group::Class),
    ("skill", "name", Group::Skill),
    ("passive_skill", "name", Group::Skill),
    ("title", "name", Group::Title),
];

/// The tags' text attributes that hold names, and what they name: a speaker's name, and the title
/// a dialogue shows with it.
pub const NAME_ATTRIBUTES: &[(&str, &str, Group)] = &[
    ("q_npcname", "text", Group::Character),
    ("d_npc", "name", Group::Character),
    ("d_npc", "nickname", Group::Title),
];

/// The fewest items' names a word recurs in to be a candidate.
pub const WORD_MIN_ITEMS: usize = 20;

/// The longest word, in characters, looked for in items' names.
const WORD_MAX_CHARS: usize = 4;

/// What a candidate names, from where it was found: Task 11 drafts every class, sect, place and
/// title.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Group {
    Character,
    Place,
    Sect,
    Class,
    Skill,
    Title,
    /// A word that recurs in items' names.
    Item,
}

/// A term the glossary may hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Candidate {
    pub zh: String,
    /// Its English today, where a line of its own has some.
    pub en_old: Option<String>,
    /// How many lines hold it: how many a change to it touches.
    pub uses: usize,
    pub kind: TermKind,
    pub groups: BTreeSet<Group>,
}

/// The glossary's candidates in `lines`, most used first:
/// - names: the lines of [`NAME_FIELDS`], and the values of [`NAME_ATTRIBUTES`];
/// - words: what recurs in [`WORD_MIN_ITEMS`] or more items' names, but no piece of a longer word
///   that nearly always holds it.
///
/// A candidate has two characters or more, holds Chinese, and has no markup or white space.
pub fn candidates(lines: &[Line]) -> Vec<Candidate> {
    let mut names: BTreeMap<&str, BTreeSet<Group>> = BTreeMap::new();
    let mut items: BTreeSet<&str> = BTreeSet::new();
    let mut markup: Vec<(String, Group)> = Vec::new();
    for line in lines {
        for t in tokens(&line.zh) {
            let Some((tag, false)) = t.tag() else {
                continue;
            };
            for (attribute, value) in slot_attributes(&t.shape).iter().zip(t.visible) {
                if let Some(&(_, _, group)) = NAME_ATTRIBUTES
                    .iter()
                    .find(|(tg, a, _)| *tg == tag && a == attribute)
                {
                    markup.push((value, group));
                }
            }
        }
        for u in &line.uses {
            let Use::Table { sheet, field, .. } = u else {
                continue;
            };
            if let Some(&(_, _, group)) = NAME_FIELDS
                .iter()
                .find(|(s, f, _)| s == sheet && f == field)
            {
                names.entry(&line.zh).or_default().insert(group);
            }
            if field.eq_ignore_ascii_case("name")
                && classify(std::slice::from_ref(u)).0 == Area::Items
            {
                items.insert(&line.zh);
            }
        }
    }
    for (value, group) in &markup {
        names.entry(value).or_default().insert(*group);
    }
    names.retain(|zh, _| is_term(zh));
    let words: Vec<String> = recurring_words(&items)
        .into_iter()
        .filter(|w| !names.contains_key(w.as_str()))
        .collect();

    let today: HashMap<&str, &str> = lines
        .iter()
        .filter(|l| l.env.is_empty())
        .filter_map(|l| Some((l.zh.as_str(), l.en.as_deref()?)))
        .collect();
    let mut found: Vec<Candidate> = names
        .into_iter()
        .map(|(zh, groups)| (zh.to_string(), TermKind::Name, groups))
        .chain(
            words
                .into_iter()
                .map(|w| (w, TermKind::Word, BTreeSet::from([Group::Item]))),
        )
        .map(|(zh, kind, groups)| Candidate {
            en_old: today.get(zh.as_str()).map(|en| en.to_string()),
            zh,
            uses: 0,
            kind,
            groups,
        })
        .collect();
    count_uses(&mut found, lines);
    found.sort_by(|a, b| b.uses.cmp(&a.uses).then_with(|| a.zh.cmp(&b.zh)));
    found
}

/// Whether `zh` can be a term: two characters or more, Chinese, and no markup or white space.
fn is_term(zh: &str) -> bool {
    zh.chars().count() >= 2
        && zh.chars().any(is_cjk)
        && !zh.chars().any(char::is_whitespace)
        && tokens(zh).is_empty()
}

/// The attribute each of a tag's slots is the value of, in order: `name` for `name="\u{1}"`.
fn slot_attributes(shape: &str) -> Vec<String> {
    shape
        .split(SLOT)
        .take(shape.matches(SLOT).count())
        .map(|before| {
            before
                .trim_end_matches(['"', '\''])
                .trim_end()
                .trim_end_matches('=')
                .trim_end()
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect()
        })
        .collect()
}

/// The words that recur in [`WORD_MIN_ITEMS`] or more of `names`: runs of two to
/// [`WORD_MAX_CHARS`] Chinese characters, less the pieces of a word one character longer that
/// holds them nine times in ten or more.
fn recurring_words(names: &BTreeSet<&str>) -> BTreeSet<String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for name in names {
        let mut seen = BTreeSet::new();
        let chars: Vec<char> = name.chars().collect();
        for run in chars.split(|c| !is_cjk(*c)) {
            for n in 2..=WORD_MAX_CHARS {
                seen.extend(run.windows(n).map(|w| w.iter().collect::<String>()));
            }
        }
        for word in seen {
            *counts.entry(word).or_default() += 1;
        }
    }
    counts.retain(|_, n| *n >= WORD_MIN_ITEMS);
    let mut pieces = BTreeSet::new();
    for (longer, &n) in &counts {
        let chars: Vec<char> = longer.chars().collect();
        if chars.len() < 3 {
            continue;
        }
        for piece in [&chars[1..], &chars[..chars.len() - 1]] {
            let piece: String = piece.iter().collect();
            if counts.get(&piece).is_some_and(|&m| n * 10 >= m * 9) {
                pieces.insert(piece);
            }
        }
    }
    counts.into_keys().filter(|w| !pieces.contains(w)).collect()
}

/// Sets each candidate's uses: the lines whose source holds it.
fn count_uses(found: &mut [Candidate], lines: &[Line]) {
    // The patterns are at most the lines' own texts, far below the automaton's limits.
    let automaton = AhoCorasick::new(found.iter().map(|c| c.zh.as_str()))
        .expect("an automaton of the candidates");
    let mut last_line = vec![usize::MAX; found.len()];
    for (i, line) in lines.iter().enumerate() {
        for m in automaton.find_overlapping_iter(&line.zh) {
            let p = m.pattern().as_usize();
            if last_line[p] != i {
                last_line[p] = i;
                found[p].uses += 1;
            }
        }
    }
}

/// `candidates` as the work folder's [`CANDIDATES`]: a header, then one per line, with uses, kind,
/// groups, the source and its English today, escaped as the pack's files are.
pub fn candidates_tsv(candidates: &[Candidate]) -> String {
    let mut out = String::from("uses\tkind\tgroups\tzh\ten_old\n");
    for c in candidates {
        let kind = match c.kind {
            TermKind::Name => "name",
            TermKind::Word => "word",
        };
        let groups: Vec<&str> = c.groups.iter().map(|g| g.name()).collect();
        out.push_str(&format!(
            "{}\t{kind}\t{}\t{}\t{}\n",
            c.uses,
            groups.join(","),
            escape(&c.zh),
            escape(c.en_old.as_deref().unwrap_or(""))
        ));
    }
    out
}

impl Group {
    pub fn name(self) -> &'static str {
        match self {
            Group::Character => "character",
            Group::Place => "place",
            Group::Sect => "sect",
            Group::Class => "class",
            Group::Skill => "skill",
            Group::Title => "title",
            Group::Item => "item",
        }
    }
}

/// The glossary in `work`, as Task 11 drafts it.
pub fn load(work: &Path) -> Result<Glossary> {
    let path = work.join(GLOSSARY);
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("{}: draft the glossary first", path.display()))?;
    serde_json::from_str(&text).with_context(|| path.display().to_string())
}

/// `glossary` as `fenji-content`'s `glossary.tsv`: per term, sorted by its fingerprint, the
/// fingerprint of its source, its English, its kind and its note, escaped as the pack's files
/// are. A term whose English or note holds Chinese is refused, since the file leaves `re/`, and so
/// is a term there twice.
pub fn export(glossary: &Glossary) -> Result<String> {
    let mut rows = BTreeMap::new();
    for term in &glossary.terms {
        let fp = fingerprint(term.zh.as_bytes());
        for (what, text) in [("English", &term.en), ("note", &term.note)] {
            if text.chars().any(is_cjk) {
                bail!("the term {fp}'s {what} holds Chinese, which fenji-content never does");
            }
        }
        if term.en.trim().is_empty() {
            bail!("the term {fp} has no English");
        }
        let kind = match term.kind {
            TermKind::Name => "name",
            TermKind::Word => "word",
        };
        let row = format!(
            "{fp}\t{}\t{kind}\t{}\n",
            escape(&term.en),
            escape(&term.note)
        );
        if rows.insert(fp, row).is_some() {
            bail!("the term {fp} is in the glossary twice");
        }
    }
    Ok(rows.into_values().collect())
}

#[cfg(test)]
mod tests {
    use fenji_pack::fingerprint;

    use super::*;
    use crate::area::Area;
    use crate::check::Term;
    use crate::extract::is_cjk;
    use crate::lines::Use;

    /// An invented line.
    fn line(zh: &str, en: Option<&str>, uses: Vec<Use>) -> Line {
        Line {
            fp: fingerprint(zh.as_bytes()),
            env: String::new(),
            zh: zh.into(),
            en: en.map(String::from),
            uses,
            area: Area::Other,
            tight: false,
        }
    }

    fn field(sheet: &str, field: &str) -> Use {
        Use::Table {
            sheet: sheet.into(),
            row: "1".into(),
            field: field.into(),
        }
    }

    fn find<'a>(c: &'a [Candidate], zh: &str) -> Option<&'a Candidate> {
        c.iter().find(|c| c.zh == zh)
    }

    #[test]
    fn candidates_are_names_in_markup_and_name_fields() {
        let lines = vec![
            line("[q_npcname text=\"某人\"]来了", None, vec![]),
            line(
                "[d_npc name=\"某人\" nickname=\"掌门\" action=\"\"]你好",
                None,
                vec![],
            ),
            line("某人", Some("Someone"), vec![field("npcs", "name")]),
            line("青山", Some("Green Hill"), vec![field("maps", "name")]),
            // An item's name is no name, nor a description.
            line("铁剑", Some("Iron Sword"), vec![field("items", "name")]),
            line("一段描述", None, vec![field("npcs", "desc")]),
            // A name needs two characters, and Chinese.
            line("猫", None, vec![field("npcs", "name")]),
            line("Tom", None, vec![field("npcs", "name")]),
        ];
        let c = candidates(&lines);
        assert_eq!(
            c.first().map(|c| c.zh.as_str()),
            Some("某人"),
            "most used first"
        );
        let someone = find(&c, "某人").unwrap();
        assert_eq!(someone.kind, TermKind::Name);
        assert_eq!(someone.uses, 3);
        assert_eq!(someone.en_old.as_deref(), Some("Someone"));
        assert_eq!(someone.groups, BTreeSet::from([Group::Character]));
        let hill = find(&c, "青山").unwrap();
        assert_eq!((hill.kind, hill.uses), (TermKind::Name, 1));
        assert_eq!(hill.groups, BTreeSet::from([Group::Place]));
        assert_eq!(
            find(&c, "掌门").unwrap().groups,
            BTreeSet::from([Group::Title])
        );
        for not in ["铁剑", "一段描述", "猫", "Tom"] {
            assert!(find(&c, not).is_none(), "{not}");
        }
    }

    #[test]
    fn recurring_words_in_items_names_are_words() {
        let stems = "甲乙丙丁戊己庚";
        let prefixes: Vec<String> = stems
            .chars()
            .flat_map(|a| stems.chars().map(move |b| format!("{a}{b}")))
            .collect();
        let mut lines = Vec::new();
        for p in &prefixes[..20] {
            lines.push(line(
                &format!("{p}宝箱碎片"),
                None,
                vec![field("items", "name")],
            ));
        }
        for p in &prefixes[20..40] {
            lines.push(line(
                &format!("{p}·碎片"),
                None,
                vec![field("equip", "name")],
            ));
        }
        // In too few items' names.
        for p in &prefixes[40..] {
            lines.push(line(
                &format!("{p}铁剑"),
                None,
                vec![field("items", "name")],
            ));
        }
        lines.push(line("打开宝箱碎片", None, vec![field("quest", "desc")]));
        let c = candidates(&lines);
        let whole = find(&c, "宝箱碎片").unwrap();
        assert_eq!((whole.kind, whole.uses), (TermKind::Word, 21));
        assert_eq!(whole.groups, BTreeSet::from([Group::Item]));
        assert_eq!(find(&c, "碎片").unwrap().uses, 41);
        // Its pieces never come without it, and a word doesn't cross a `·`.
        for not in ["宝箱碎", "箱碎片", "宝箱", "箱碎", "铁剑", "·碎"] {
            assert!(find(&c, not).is_none(), "{not}");
        }
    }

    #[test]
    fn the_export_holds_no_chinese() {
        let term = Term {
            zh: "林月".into(),
            en: "Lin Yue".into(),
            kind: TermKind::Name,
            variants: vec![],
            note: "the sect's head".into(),
        };
        let g = Glossary {
            terms: vec![term.clone()],
        };
        let out = export(&g).unwrap();
        assert_eq!(
            out,
            format!(
                "{}\tLin Yue\tname\tthe sect's head\n",
                fingerprint("林月".as_bytes())
            )
        );
        assert!(!out.chars().any(is_cjk));
        // A note that quotes the Chinese can't leave the work folder, and a term is there once.
        let quoting = Glossary {
            terms: vec![Term {
                note: "from 林月".into(),
                ..term.clone()
            }],
        };
        assert!(export(&quoting).is_err());
        let twice = Glossary {
            terms: vec![term.clone(), term],
        };
        assert!(export(&twice).is_err());
    }

    #[test]
    fn the_glossary_loads_from_the_work_folder() {
        let work = tempfile::tempdir().unwrap();
        assert!(load(work.path()).is_err(), "none yet");
        std::fs::write(
            work.path().join(GLOSSARY),
            r#"{"terms":[{"zh":"林月","en":"Lin Yue","kind":"name"}]}"#,
        )
        .unwrap();
        let g = load(work.path()).unwrap();
        assert_eq!(g.terms.len(), 1);
        assert_eq!(g.terms[0].kind, TermKind::Name);
    }
}
