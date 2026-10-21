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
    /// What each tight field holds today, in characters drawn: the length a quarter of its lines'
    /// English reaches, where enough of them show it. A tight line may always be that long (#491).
    pub floors: BTreeMap<String, usize>,
}

/// How many of a field's lines need English today for its floor to stand.
const FLOOR_LINES: usize = 30;

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

/// Characters today's English uses that the new English doesn't: curly quotes, since its quotes
/// are straight (the style guide), and the replacement character, which marks broken text.
pub const NEVER: &[char] = &['‘', '’', '“', '”', '\u{FFFD}'];

impl Rules {
    /// Printable ASCII, a line break and a tab, and every other character today's English uses
    /// that isn't East Asian or [`NEVER`]; tight lines within 110% of today's length, or as long
    /// as their fields hold today.
    pub fn from_lines(lines: &[Line]) -> Rules {
        let mut allowed: BTreeSet<char> = (' '..='~').chain(['\n', '\t']).collect();
        for line in lines {
            if let Some(en) = &line.en {
                allowed.extend(
                    en.chars()
                        .filter(|&c| !east_asian(c) && !c.is_control() && !NEVER.contains(&c)),
                );
            }
        }
        Rules {
            allowed,
            budget_percent: 110,
            floors: floors(lines),
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

/// What may follow the root of an -ise or -yse stem, the stem without its e: realis-ing,
/// organis-er, recognis-able.
const ROOT_SUFFIXES: &[&str] = &["ing", "er", "ers", "ation", "ations", "able"];
/// What may come before a British stem and leave it British: un-favourable, im-mobilise.
const PREFIXES: &[&str] = &[
    "", "un", "re", "dis", "im", "in", "mis", "de", "pre", "non", "over", "under", "out", "multi",
    "anti", "semi", "super", "counter",
];

fn british_word(word: &str) -> bool {
    BRITISH
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .any(|stem| {
            let root = stem
                .strip_suffix('e')
                .filter(|root| root.ends_with("is") || root.ends_with("ys"));
            PREFIXES
                .iter()
                .filter_map(|prefix| word.strip_prefix(prefix))
                .any(|rest| {
                    rest.strip_prefix(stem)
                        .is_some_and(|tail| SUFFIXES.contains(&tail))
                        || root
                            .and_then(|root| rest.strip_prefix(root))
                            .is_some_and(|tail| ROOT_SUFFIXES.contains(&tail))
                })
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

/// Each tight field's floor ([`Rules::floors`]): the length a quarter of its lines' English today
/// reaches, in characters drawn, where [`FLOOR_LINES`] or more of its lines have English.
fn floors(lines: &[Line]) -> BTreeMap<String, usize> {
    let mut lengths: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for line in lines {
        let Some(today) = line.en.as_deref().filter(|_| line.tight) else {
            continue;
        };
        let n = drawn(today).chars().count();
        for field in crate::area::tight_fields(&line.uses).into_iter().flatten() {
            lengths.entry(field).or_default().push(n);
        }
    }
    lengths
        .into_iter()
        .filter(|(_, l)| l.len() >= FLOOR_LINES)
        .map(|(field, mut l)| {
            l.sort_unstable();
            (field, l[l.len() * 3 / 4])
        })
        .collect()
}

/// The most characters a tight line's English may draw (RT11): its English today's, by the rules'
/// percentage, or what the strictest of its fields holds today, whichever is more. Where today's
/// English is short pinyin, its field still has room for the name's meaning (#491). A line that
/// isn't tight, or has no English today, has no budget.
pub fn budget(line: &Line, rules: &Rules) -> Option<usize> {
    let today = line.en.as_deref().filter(|_| line.tight)?;
    let own = (drawn(today).chars().count() * rules.budget_percent as usize).div_ceil(100);
    Some(own.max(floor(line, rules)))
}

/// What every tight field `line` is shown in holds today: the strictest one's floor, or nothing
/// where one has none, as a script's string has no field.
fn floor(line: &Line, rules: &Rules) -> usize {
    crate::area::tight_fields(&line.uses)
        .iter()
        .map(|f| f.as_ref().and_then(|f| rules.floors.get(f)).copied())
        .collect::<Option<Vec<usize>>>()
        .and_then(|floors| floors.into_iter().min())
        .unwrap_or(0)
}

/// Where `term` is in `source`, as byte ranges, overlapping ones too.
fn places<'a>(source: &'a str, term: &'a str) -> impl Iterator<Item = (usize, usize)> + 'a {
    source
        .char_indices()
        .filter(move |&(i, _)| source[i..].starts_with(term))
        .map(move |(i, _)| (i, i + term.len()))
}

/// Every problem `english` has as `line`'s translation, within the line's [`budget`].
pub fn check(line: &Line, english: &str, glossary: &Glossary, rules: &Rules) -> Vec<Problem> {
    check_within(line, english, glossary, rules, budget(line, rules))
}

/// [`check`] within `budget`: a batch line's, which planning set from the line's fields, and which
/// its prompt shows the translator.
pub fn check_within(
    line: &Line,
    english: &str,
    glossary: &Glossary,
    rules: &Rules,
    budget: Option<usize>,
) -> Vec<Problem> {
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
    // A name the English doesn't write is still met where the source holds it only inside a longer
    // name the English does: a place given with its region may drop the region when the longer
    // name's variants say so.
    let names: Vec<&Term> = glossary
        .terms
        .iter()
        .filter(|t| t.kind == TermKind::Name && line.zh.contains(&t.zh))
        .collect();
    let written = |t: &Term| {
        english.contains(&t.en) || t.variants.iter().any(|v| english.contains(v.as_str()))
    };
    let met: Vec<(usize, usize)> = names
        .iter()
        .filter(|t| written(t))
        .flat_map(|t| places(&line.zh, &t.zh))
        .collect();
    for term in names.iter().filter(|t| !written(t)) {
        let inside =
            |(a, b): (usize, usize)| met.iter().any(|&(c, d)| c <= a && b <= d && d - c > b - a);
        if !places(&line.zh, &term.zh).all(inside) {
            problems.push(Problem::Term(term.en.clone()));
        }
    }
    if let Some(budget) = budget {
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
    use crate::lines::Use;

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
            floors: BTreeMap::new(),
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
    fn a_name_inside_a_longer_name_the_english_writes_is_met() {
        let name = |zh: &str, en: &str, variants: &[&str]| Term {
            zh: zh.into(),
            en: en.into(),
            kind: TermKind::Name,
            variants: variants.iter().map(|v| (*v).into()).collect(),
            note: String::new(),
        };
        let glossary = Glossary {
            terms: vec![
                name("青石岭", "Bluestone Ridge", &[]),
                name("望月台", "Moonwatch Terrace", &[]),
                name(
                    "青石岭·望月台",
                    "Bluestone Ridge: Moonwatch Terrace",
                    &["Moonwatch Terrace"],
                ),
            ],
        };
        let terms = |zh: &str, english: &str| -> Vec<Problem> {
            check(&line(zh, None, false), english, &glossary, &rules())
                .into_iter()
                .filter(|p| matches!(p, Problem::Term(_)))
                .collect()
        };
        // The place's short form drops its region, as the longer name allows.
        assert_eq!(terms("青石岭·望月台", "Moonwatch Terrace"), []);
        // Where the region also stands alone, it's still asked for.
        assert_eq!(
            terms(
                "青石岭的风吹过青石岭·望月台",
                "The wind blows over Moonwatch Terrace"
            ),
            [Problem::Term("Bluestone Ridge".into())]
        );
        // A longer name the English doesn't write meets nothing inside it.
        assert_eq!(
            terms("青石岭·望月台", "The terrace"),
            [
                Problem::Term("Bluestone Ridge".into()),
                Problem::Term("Moonwatch Terrace".into()),
                Problem::Term("Bluestone Ridge: Moonwatch Terrace".into()),
            ]
        );
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
    fn a_tight_line_has_the_room_its_field_holds_today() {
        // An NPC named in three letters of pinyin today: 110% of that is four, but a quarter of
        // the field's names today reach thirteen (#491).
        let shown_in = |l: Line, fields: &[(&str, &str)]| Line {
            uses: fields
                .iter()
                .map(|&(sheet, field)| Use::Table {
                    sheet: sheet.into(),
                    row: "1".into(),
                    field: field.into(),
                })
                .collect(),
            ..l
        };
        let mut lines: Vec<Line> = (0..40)
            .map(|i| {
                let en = if i < 30 {
                    "Ten letter"
                } else {
                    "Thirteen char"
                };
                shown_in(line(&format!("名{i}"), Some(en), true), &[("npcs", "name")])
            })
            .collect();
        // A voting list's names, eight letters each; a field with too few lines for a floor.
        lines.extend((0..30).map(|i| {
            shown_in(
                line(&format!("票{i}"), Some("Eight ch"), true),
                &[("vote", "name")],
            )
        }));
        lines.extend((0..29).map(|i| {
            shown_in(
                line(&format!("少{i}"), Some("A long name"), true),
                &[("few", "name")],
            )
        }));
        let rules = Rules::from_lines(&lines);
        assert_eq!(rules.floors.get("npcs.name"), Some(&13));
        assert_eq!(rules.floors.get("vote.name"), Some(&8));
        assert_eq!(rules.floors.get("few.name"), None);

        let xie = |fields: &[(&str, &str)]| shown_in(line("蝎妖", Some("Xie"), true), fields);
        let npc = xie(&[("npcs", "name")]);
        assert_eq!(budget(&npc, &rules), Some(13));
        assert_eq!(
            check(&npc, "Scorpion Yao", &Glossary::default(), &rules),
            []
        );
        // The strictest field it's shown in, and no floor where one field has none.
        assert_eq!(
            budget(&xie(&[("npcs", "name"), ("vote", "name")]), &rules),
            Some(8)
        );
        assert_eq!(
            budget(&xie(&[("npcs", "name"), ("few", "name")]), &rules),
            Some(4)
        );
        // A script's string has no field whose room today's English shows.
        let mut scripted = npc.clone();
        scripted.uses.push(Use::Script {
            path: "data/interface/ui/a.luac".into(),
        });
        assert_eq!(budget(&scripted, &rules), Some(4));
        // Never less than today's own English allows.
        let long = shown_in(
            line("长名", Some("A very long name today"), true),
            &[("vote", "name")],
        );
        assert_eq!(budget(&long, &rules), Some(25));
        // The check takes the budget it's given: a batch's, set with the line's fields.
        let bare = line("蝎妖", Some("Xie"), true);
        let within = |b| check_within(&bare, "Scorpion Yao", &Glossary::default(), &rules, b);
        assert_eq!(within(Some(13)), []);
        assert_eq!(within(Some(4)), [Problem::TooLong { len: 12, budget: 4 }]);
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
    fn every_form_of_a_british_spelling_is_refused() {
        // An -ise verb's other forms, a stem behind a prefix, and stems today's English uses.
        for word in [
            "realising",
            "organiser",
            "recognisable",
            "paralysing",
            "practising",
            "immobilisation",
            "unfavourable",
            "dishonoured",
            "multicoloured",
            "stabilised",
            "fertiliser",
            "sceptres",
            "duelling",
            "channelled",
            "rancour",
            "aeons",
            "anaemic",
        ] {
            assert!(british_word(word), "{word}");
        }
        // Their American forms, and words American English spells the same way.
        for word in [
            "realizing",
            "organizer",
            "recognizable",
            "paralyzing",
            "practicing",
            "immobilization",
            "unfavorable",
            "dishonored",
            "multicolored",
            "stabilized",
            "fertilizer",
            "scepters",
            "dueling",
            "channeled",
            "rancor",
            "eons",
            "anemic",
            "rise",
            "promise",
            "exercise",
            "precise",
            "otherwise",
            "analysis",
            "aesthetic",
            "glamour",
            "dialogue",
            "your",
            "hour",
        ] {
            assert!(!british_word(word), "{word}");
        }
    }

    #[test]
    fn the_rules_allow_todays_characters_but_chinese_curly_quotes_and_breakage() {
        let lines = [
            line("剑", Some("Ü – sword。"), false),
            line("盾", None, false),
            // Today's English has curly quotes and broken text here and there.
            line("刀", Some("“It’s ‘mine’”\u{FFFD}"), false),
        ];
        let r = Rules::from_lines(&lines);
        for c in ['a', '~', ' ', '\n', '\t', 'Ü', '–', 'I'] {
            assert!(r.allowed.contains(&c), "{c:?}");
        }
        for c in ['。', '剑', '“', '”', '‘', '’', '\u{FFFD}'] {
            assert!(!r.allowed.contains(&c), "{c:?}");
        }
        assert_eq!(r.budget_percent, 110);
    }
}
