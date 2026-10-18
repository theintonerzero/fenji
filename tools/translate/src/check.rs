//! The checks every translated line passes (retranslation spec §6.6, §6.7): its markup kept, no
//! Chinese left, only the characters the game's font draws, the glossary's names, the length the
//! interface has room for, and American spelling.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::lines::Line;
use crate::markup::{Family, drawn, tokens};

/// The glossary: names and recurring words, each with one English form (RT8, RT9).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Glossary {
    pub terms: Vec<Term>,
}

/// One term.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Term {
    /// The term in the source, as the game's lines hold it.
    pub zh: String,
    /// Its one English form.
    pub en: String,
    pub kind: TermKind,
    /// Other forms a line may use instead: a short form, a plural.
    #[serde(default)]
    pub variants: Vec<String>,
    #[serde(default)]
    pub note: String,
}

/// A name must appear as written; a word is the translators' guide, not a check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TermKind {
    Name,
    Word,
}

/// The rules a translation follows that the early check in the game sets (RT10, RT11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rules {
    /// The characters the game's English font draws.
    pub allowed: BTreeSet<char>,
    /// How long a tight line may be, against today's English.
    pub budget_percent: u32,
}

/// `rules.json`, which the early check writes (Task 11): each field overrides.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RulesFile {
    budget_percent: Option<u32>,
    /// Every allowed character, as one string.
    allowed: Option<String>,
}

/// Whether `c` is East Asian: an ideograph, kana, hangul, or their punctuation and full-width
/// forms. Today's English holds some only where it was never translated.
pub fn east_asian(c: char) -> bool {
    crate::extract::is_cjk(c)
        || matches!(c as u32,
            0x3000..=0x30FF | 0xFF00..=0xFFEF | 0xAC00..=0xD7AF | 0x1100..=0x11FF | 0x3130..=0x318F)
}

impl Rules {
    /// Printable ASCII, a line break and a tab, and every other character today's English uses
    /// that isn't East Asian; tight lines within 110% of today's length.
    pub fn from_lines(lines: &[Line]) -> Rules {
        let mut allowed: BTreeSet<char> = (' '..='~').chain(['\n', '\t']).collect();
        for line in lines {
            if let Some(en) = &line.en {
                allowed.extend(en.chars().filter(|&c| !east_asian(c) && !c.is_control()));
            }
        }
        Rules {
            allowed,
            budget_percent: 110,
        }
    }

    /// [`Rules::from_lines`], overridden by `work`'s `rules.json` where there is one.
    pub fn load(work: &Path, lines: &[Line]) -> Result<Rules> {
        let mut rules = Rules::from_lines(lines);
        let path = work.join("rules.json");
        if path.exists() {
            let text =
                std::fs::read_to_string(&path).with_context(|| path.display().to_string())?;
            let file: RulesFile =
                serde_json::from_str(&text).with_context(|| path.display().to_string())?;
            if let Some(p) = file.budget_percent {
                rules.budget_percent = p;
            }
            if let Some(chars) = file.allowed {
                rules.allowed = chars.chars().collect();
            }
        }
        Ok(rules)
    }
}

/// What's wrong with a translated line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Problem {
    /// A token of the source that the translation lacks, by its shape.
    MissingToken(String),
    /// A token the translation has more of than the source.
    ExtraToken(String),
    /// A closing tag with no opening before it, where the source had one.
    OutOfOrder(String),
    /// Chinese left in it.
    Cjk,
    /// A character the game's font doesn't draw.
    Char(char),
    /// A glossary name it should hold, as written.
    Term(String),
    /// Longer than a tight line's budget, in characters drawn.
    TooLong { len: usize, budget: usize },
    /// A British spelling.
    British(String),
    /// Nothing drawn, where the source draws something.
    Empty,
}

/// Our own list: each British spelling, then its American form (D3).
const BRITISH: &str = include_str!("british.txt");
/// What may follow a British stem and still be the same word.
const SUFFIXES: &[&str] = &[
    "", "s", "d", "ed", "ing", "er", "ers", "ful", "fully", "less", "able", "ably", "ation",
    "ations", "ite", "ites", "ism",
];

fn british_word(word: &str) -> bool {
    BRITISH
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .any(|stem| {
            word.strip_prefix(stem)
                .is_some_and(|rest| SUFFIXES.contains(&rest))
        })
}

/// Shapes and how many of each.
fn counts(s: &str) -> BTreeMap<String, usize> {
    let mut c = BTreeMap::new();
    for t in tokens(s) {
        *c.entry(t.shape).or_default() += 1;
    }
    c
}

/// Closing tags with no opening of their name before them, by shape.
fn orphan_closes(s: &str) -> BTreeMap<String, usize> {
    let mut open: BTreeMap<String, usize> = BTreeMap::new();
    let mut orphans = BTreeMap::new();
    for t in tokens(s) {
        if !matches!(t.family, Family::Bracket | Family::Html) {
            continue;
        }
        let Some((name, closes)) = t.tag() else {
            continue;
        };
        let n = open.entry(name).or_default();
        if !closes {
            *n += 1;
        } else if *n > 0 {
            *n -= 1;
        } else {
            *orphans.entry(t.shape).or_default() += 1;
        }
    }
    orphans
}

/// Every problem `english` has as `line`'s translation.
pub fn check(line: &Line, english: &str, glossary: &Glossary, rules: &Rules) -> Vec<Problem> {
    let mut problems = Vec::new();
    let (source, translation) = (counts(&line.zh), counts(english));
    for (shape, &n) in &source {
        let m = translation.get(shape).copied().unwrap_or(0);
        if m < n {
            problems.push(Problem::MissingToken(shape.clone()));
        }
    }
    for (shape, &m) in &translation {
        if m > source.get(shape).copied().unwrap_or(0) {
            problems.push(Problem::ExtraToken(shape.clone()));
        }
    }
    let source_orphans = orphan_closes(&line.zh);
    for (shape, &n) in &orphan_closes(english) {
        if n > source_orphans.get(shape).copied().unwrap_or(0) {
            problems.push(Problem::OutOfOrder(shape.clone()));
        }
    }
    // What's drawn: a token's kept part, such as a guide link's key, is the source's own, and
    // never drawn.
    let shown = drawn(english);
    if shown.chars().any(crate::extract::is_cjk) {
        problems.push(Problem::Cjk);
    }
    let mut odd: Vec<char> = shown
        .chars()
        .filter(|c| !rules.allowed.contains(c) && !crate::extract::is_cjk(*c))
        .collect();
    odd.sort_unstable();
    odd.dedup();
    problems.extend(odd.into_iter().map(Problem::Char));
    for term in &glossary.terms {
        if term.kind == TermKind::Name
            && line.zh.contains(&term.zh)
            && !english.contains(&term.en)
            && !term.variants.iter().any(|v| english.contains(v.as_str()))
        {
            problems.push(Problem::Term(term.en.clone()));
        }
    }
    if let (true, Some(today)) = (line.tight, &line.en) {
        let today = drawn(today).chars().count();
        let budget = (today * rules.budget_percent as usize).div_ceil(100);
        let len = shown.chars().count();
        if len > budget {
            problems.push(Problem::TooLong { len, budget });
        }
    }
    for word in shown
        .split(|c: char| !c.is_ascii_alphabetic())
        .filter(|w| !w.is_empty())
    {
        let word = word.to_ascii_lowercase();
        if british_word(&word) {
            problems.push(Problem::British(word));
        }
    }
    if shown.trim().is_empty() && !drawn(&line.zh).trim().is_empty() {
        problems.push(Problem::Empty);
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::area::Area;

    /// An invented line.
    fn line(zh: &str, en: Option<&str>, tight: bool) -> Line {
        Line {
            fp: fenji_pack::fingerprint(zh.as_bytes()),
            env: String::new(),
            zh: zh.into(),
            en: en.map(String::from),
            uses: vec![],
            area: Area::Other,
            tight,
        }
    }

    fn rules() -> Rules {
        Rules {
            allowed: (' '..='~').chain(['\n', '\t', '–']).collect(),
            budget_percent: 110,
        }
    }

    fn glossary() -> Glossary {
        Glossary {
            terms: vec![Term {
                zh: "林月".into(),
                en: "Lin Yue".into(),
                kind: TermKind::Name,
                variants: vec!["Yue".into()],
                note: String::new(),
            }],
        }
    }

    fn problems(l: &Line, english: &str) -> Vec<Problem> {
        check(l, english, &glossary(), &rules())
    }

    #[test]
    fn a_good_translation_has_no_problems() {
        let l = line(
            "[c 99ccff]林月[/c]说：{0}",
            Some("[c 99ccff]Linyue[/c] says: {0}"),
            false,
        );
        assert_eq!(problems(&l, "[c 99ccff]Lin Yue[/c] says: {0}"), []);
    }

    #[test]
    fn every_token_must_survive() {
        let l = line("[c 99ccff]剑[/c]{0}|cp,1|", None, false);
        assert!(
            problems(&l, "[c 99ccff]Sword {0}|cp,1|")
                .contains(&Problem::MissingToken("[/c]".into()))
        );
        assert!(
            problems(&l, "[c 99ccff]Sword[/c][/c]{0}|cp,1|")
                .contains(&Problem::ExtraToken("[/c]".into()))
        );
        assert!(
            problems(&l, "[/c]Sword[c 99ccff]{0}|cp,1|")
                .contains(&Problem::OutOfOrder("[/c]".into()))
        );
        // Placeholders and codes may move, as English word order needs.
        assert_eq!(problems(&l, "|cp,1|{0}[c 99ccff]Sword[/c]"), []);
    }

    #[test]
    fn visible_text_inside_markup_is_translated() {
        let l = line("[q_npcname text=\"某人\"]来了", None, false);
        assert!(problems(&l, "[q_npcname text=\"某人\"] has come").contains(&Problem::Cjk));
        assert_eq!(problems(&l, "[q_npcname text=\"Someone\"] has come"), []);
    }

    #[test]
    fn chinese_a_token_keeps_isnt_drawn_so_passes() {
        // A guide link's target is the guide's own key: kept as it is, never drawn.
        let l = line("<A HREF=\"guide;任务\">任务指南</A>", None, false);
        assert_eq!(problems(&l, "<A HREF=\"guide;任务\">Quest guide</A>"), []);
    }

    #[test]
    fn an_unbalanced_source_is_matched_not_mended() {
        let l = line("[c 99ccff]剑", None, false);
        assert_eq!(problems(&l, "[c 99ccff]Sword"), []);
        assert!(problems(&l, "[c 99ccff]Sword[/c]").contains(&Problem::ExtraToken("[/c]".into())));
    }

    #[test]
    fn glossary_names_are_required_as_written() {
        let l = line("林月来了", None, false);
        assert!(problems(&l, "Linyue has come").contains(&Problem::Term("Lin Yue".into())));
        assert_eq!(problems(&l, "Lin Yue has come"), []);
        assert_eq!(problems(&l, "Yue has come"), []);
    }

    #[test]
    fn the_strictest_budget_applies_to_a_tight_line() {
        // Six characters today; 110% of that is 6.6, so seven fit.
        let l = line("确定", Some("Accept"), true);
        assert_eq!(problems(&l, "Confirm"), []);
        assert!(problems(&l, "Confirms").contains(&Problem::TooLong { len: 8, budget: 7 }));
        // Markup doesn't count, only what's drawn.
        let colored = line("[c 99ccff]确定[/c]", Some("[c 99ccff]Accept[/c]"), true);
        assert_eq!(problems(&colored, "[c 99ccff]Confirm[/c]"), []);
        let loose = line("确定", Some("Accept"), false);
        assert_eq!(problems(&loose, "Confirm it now"), []);
        // A tight line with no English today has no budget.
        assert_eq!(problems(&line("确定", None, true), "Confirm it now"), []);
    }

    #[test]
    fn british_spellings_and_unknown_characters_are_refused() {
        let l = line("颜色", None, false);
        assert!(problems(&l, "The colour").contains(&Problem::British("colour".into())));
        assert!(problems(&l, "Colourful").contains(&Problem::British("colourful".into())));
        assert_eq!(problems(&l, "The colors – all of them"), []);
        assert!(problems(&l, "“Hi”").contains(&Problem::Char('”')));
        assert!(problems(&l, "").contains(&Problem::Empty));
        assert!(problems(&l, "   ").contains(&Problem::Empty));
    }

    #[test]
    fn the_rules_allow_what_todays_english_uses_but_chinese() {
        let lines = [
            line("剑", Some("Ü – sword。"), false),
            line("盾", None, false),
        ];
        let r = Rules::from_lines(&lines);
        for c in ['a', '~', ' ', '\n', '\t', 'Ü', '–'] {
            assert!(r.allowed.contains(&c), "{c:?}");
        }
        for c in ['。', '剑', '“'] {
            assert!(!r.allowed.contains(&c), "{c:?}");
        }
        assert_eq!(r.budget_percent, 110);
    }
}
