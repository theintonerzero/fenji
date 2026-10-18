//! The three jobs' prompts (retranslation spec §6.5, steps 6 and 8), and the shapes of their
//! answers, which the schemas in `prompts/` hold the runs to.
use std::collections::HashMap;

use fenji_pack::Fingerprint;
use serde::{Deserialize, Serialize};

use crate::batch::{Batch, BatchLine};
use crate::check::Term;

/// The translators' instructions.
pub const TRANSLATE: &str = include_str!("../prompts/translate.md");
/// The reviewers' instructions.
pub const REVIEW: &str = include_str!("../prompts/review.md");
/// The instructions for settling what a review disputes.
pub const SETTLE: &str = include_str!("../prompts/settle.md");
/// The style guide (§6.6), the one copy: Task 8 publishes it in `fenji-content` as `STYLE.md`.
pub const STYLE: &str = include_str!("../prompts/style.md");
/// The shapes of the three answers.
pub const TRANSLATE_SCHEMA: &str = include_str!("../prompts/translate.schema.json");
pub const REVIEW_SCHEMA: &str = include_str!("../prompts/review.schema.json");
pub const SETTLE_SCHEMA: &str = include_str!("../prompts/settle.schema.json");

/// A translator's answer: every line of its batch, once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub lines: Vec<AnswerLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnswerLine {
    pub fp: Fingerprint,
    pub en: String,
}

/// A reviewer's answer: every line of its batch, kept, or replaced with a reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub lines: Vec<ReviewLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewLine {
    pub fp: Fingerprint,
    pub keep: bool,
    pub en: Option<String>,
    pub reason: Option<String>,
}

/// A line the review replaced: the two readings, and the reviewer's reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dispute {
    pub fp: Fingerprint,
    pub first: String,
    pub second: String,
    pub reason: String,
}

/// The third reading's answer: every disputed line, once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settled {
    pub lines: Vec<SettledLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettledLine {
    pub fp: Fingerprint,
    pub en: String,
    pub reason: String,
}

/// A prompt: the job's instructions, the style guide, the glossary's terms the lines hold, and the
/// lines, as JSON. The lines are the batch's alone.
fn compose(instructions: &str, style: &str, glossary: &[Term], lines: &impl Serialize) -> String {
    format!(
        "{instructions}\n## The style guide\n\n{style}\n\n## The glossary's terms in these lines\n\n\
         ```json\n{}\n```\n\n## The lines\n\n```json\n{}\n```\n",
        serde_json::to_string_pretty(glossary).expect("terms as JSON"),
        serde_json::to_string_pretty(lines).expect("lines as JSON"),
    )
}

/// The prompt that translates `b`.
pub fn translate_prompt(b: &Batch, style: &str) -> String {
    compose(TRANSLATE, style, &b.glossary, &b.lines)
}

/// A line to review: the batch's, with its new English.
#[derive(Serialize)]
struct ToReview<'a> {
    #[serde(flatten)]
    line: &'a BatchLine,
    en_new: Option<&'a str>,
}

/// The prompt that reviews `answer`, `b`'s translation. Lines the answer holds that `b` doesn't
/// stay out.
pub fn review_prompt(b: &Batch, answer: &Answer, style: &str) -> String {
    let new: HashMap<Fingerprint, &str> =
        answer.lines.iter().map(|l| (l.fp, l.en.as_str())).collect();
    let lines: Vec<ToReview> = b
        .lines
        .iter()
        .map(|line| ToReview {
            line,
            en_new: new.get(&line.fp).copied(),
        })
        .collect();
    compose(REVIEW, style, &b.glossary, &lines)
}

/// A line to settle: the batch's, with its two readings and the reviewer's reason.
#[derive(Serialize)]
struct ToSettle<'a> {
    #[serde(flatten)]
    line: &'a BatchLine,
    first: &'a str,
    second: &'a str,
    reason: &'a str,
}

/// The prompt that settles `disputes`, `b`'s lines the review replaced: those lines alone, with
/// the terms they hold.
pub fn settle_prompt(b: &Batch, disputes: &[Dispute], style: &str) -> String {
    let lines: Vec<ToSettle> = disputes
        .iter()
        .filter_map(|d| {
            let line = b.lines.iter().find(|l| l.fp == d.fp)?;
            Some(ToSettle {
                line,
                first: &d.first,
                second: &d.second,
                reason: &d.reason,
            })
        })
        .collect();
    let terms: Vec<Term> = b
        .glossary
        .iter()
        .filter(|t| lines.iter().any(|l| l.line.zh.contains(&t.zh)))
        .cloned()
        .collect();
    compose(SETTLE, style, &terms, &lines)
}

#[cfg(test)]
mod tests {
    use fenji_pack::fingerprint;
    use serde_json::Value;

    use super::*;
    use crate::area::classify;
    use crate::batch::plan_batches;
    use crate::check::{Glossary, Rules, Term, TermKind};
    use crate::lines::{Line, Use};

    fn line(zh: &str, en: Option<&str>, sheet: &str, field: &str) -> Line {
        let uses = vec![Use::Table {
            sheet: sheet.into(),
            row: "1".into(),
            field: field.into(),
        }];
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

    #[test]
    fn the_prompts_hold_the_batch_the_style_and_no_other_line() {
        let lines = vec![
            line("林月来了", Some("Lin Yue's here"), "quest_dialog", "text"),
            line("青石", Some("Blue Stone"), "items", "name"),
        ];
        let glossary = Glossary {
            terms: vec![Term {
                zh: "林月".into(),
                en: "Lin Yue".into(),
                kind: TermKind::Name,
                variants: vec![],
                note: "a sect's head".into(),
            }],
        };
        let batches = plan_batches(&lines, &glossary, &Rules::from_lines(&[]));
        let dialogue = batches
            .iter()
            .find(|b| b.id.starts_with("dialogue-"))
            .unwrap();
        let items = batches.iter().find(|b| b.id.starts_with("items-")).unwrap();
        let (ours, theirs) = (dialogue.lines[0].fp, items.lines[0].fp);
        let style = "THE STYLE GUIDE";

        let p = translate_prompt(dialogue, style);
        assert!(p.starts_with(TRANSLATE));
        for held in [
            style,
            "林月来了",
            "Lin Yue's here",
            "a sect's head",
            &ours.to_string(),
        ] {
            assert!(p.contains(held), "{held}");
        }
        for other in ["青石", "Blue Stone", &theirs.to_string()] {
            assert!(!p.contains(other), "{other}");
        }

        let answer = Answer {
            lines: vec![
                AnswerLine {
                    fp: ours,
                    en: "Lin Yue has come.".into(),
                },
                // A stray line in an answer stays out of the review.
                AnswerLine {
                    fp: theirs,
                    en: "Bluestone".into(),
                },
            ],
        };
        let r = review_prompt(dialogue, &answer, style);
        assert!(r.starts_with(REVIEW));
        for held in [style, "林月来了", "Lin Yue's here", "Lin Yue has come."] {
            assert!(r.contains(held), "{held}");
        }
        for other in ["青石", "Bluestone", &theirs.to_string()] {
            assert!(!r.contains(other), "{other}");
        }

        let disputes = [Dispute {
            fp: ours,
            first: "Lin Yue has come.".into(),
            second: "Here comes Lin Yue.".into(),
            reason: "Livelier, as she is.".into(),
        }];
        let s = settle_prompt(dialogue, &disputes, style);
        assert!(s.starts_with(SETTLE));
        for held in [
            style,
            "林月来了",
            "Lin Yue has come.",
            "Here comes Lin Yue.",
            "Livelier, as she is.",
        ] {
            assert!(s.contains(held), "{held}");
        }
        assert!(!s.contains("青石"));
    }

    /// A guide's examples: each `` `source` → `English` `` pair, with `\n` as a line break.
    fn examples(guide: &str) -> Vec<(String, String)> {
        guide
            .lines()
            .filter_map(|l| {
                let (zh, en) = l.split_once("` → `")?;
                let zh = zh.rsplit_once('`')?.1;
                let en = en.strip_suffix('`')?;
                Some((zh.replace("\\n", "\n"), en.replace("\\n", "\n")))
            })
            .collect()
    }

    #[test]
    fn the_style_guide_holds_names_to_their_capitals_and_lets_words_take_ordinary_ones() {
        // The early check found words capitalized mid-sentence ("bring Healing to others") under a
        // rule that held every term to the glossary's capitals (#475).
        let rule = |start: &str| {
            STYLE
                .lines()
                .find(|l| l.starts_with(start))
                .unwrap_or_else(|| panic!("no rule starting {start}"))
        };
        assert!(rule("- **The glossary's names.**").contains("capitals included"));
        assert!(rule("- **The glossary's words.**").contains("ordinary English capitals"));
        assert!(!STYLE.contains("Every term in a batch's glossary is written exactly"));
    }

    #[test]
    fn the_budget_shortens_the_wording_never_the_meaning() {
        // The early check found a settings label that dropped its verb to fit its budget, and a
        // review that let it through (#477).
        assert!(STYLE.contains("A shorter wording, never a shorter meaning"));
        assert!(REVIEW.contains("within its budget without losing"));
    }

    #[test]
    fn the_style_guides_examples_pass_the_checks() {
        let rules = Rules::from_lines(&[]);
        let problems = |(zh, en): &(String, String)| {
            crate::check::check(
                &line(zh, None, "quest_dialog", "text"),
                en,
                &Glossary::default(),
                &rules,
            )
        };
        let found = examples(STYLE);
        assert_eq!(found.len(), 8);
        for example in &found {
            assert_eq!(problems(example), [], "{}", example.1);
        }
        let broken = examples("- `{0}把{1}交给了你。` → `He handed you the colour.`");
        assert_eq!(broken.len(), 1);
        assert_ne!(problems(&broken[0]), []);
    }

    /// An object schema's fields: those it requires, and whether it refuses others.
    fn fields(object: &Value) -> (Vec<&str>, bool) {
        let mut required: Vec<&str> = object["required"]
            .as_array()
            .map(|r| r.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        required.sort_unstable();
        (
            required,
            object["additionalProperties"] == Value::Bool(false),
        )
    }

    /// The object schema of one of an answer's lines.
    fn line_schema(schema: &str) -> Value {
        let schema: Value = serde_json::from_str(schema).unwrap();
        // The CLI refuses a schema naming the 2020-12 meta-schema, which its validator lacks
        // (Task 7's real run).
        assert_eq!(schema.get("$schema"), None);
        assert_eq!(fields(&schema), (vec!["lines"], true));
        schema["properties"]["lines"]["items"].clone()
    }

    #[test]
    fn the_schemas_accept_a_good_answer_and_refuse_a_bad_one() {
        let fp = "00112233445566778899aabbccddeeff";
        let answer: Answer =
            serde_json::from_str(&format!(r#"{{"lines":[{{"fp":"{fp}","en":"Hello."}}]}}"#))
                .unwrap();
        assert_eq!(answer.lines[0].en, "Hello.");
        for bad in [
            r#"{"lines":[{"fp":"0011","en":"Hello."}]}"#.to_string(),
            format!(r#"{{"lines":[{{"fp":"{fp}"}}]}}"#),
            format!(r#"{{"lines":[{{"fp":"{fp}","en":"Hello.","note":"mine"}}]}}"#),
        ] {
            assert!(serde_json::from_str::<Answer>(&bad).is_err(), "{bad}");
        }
        let review: Review = serde_json::from_str(&format!(
            r#"{{"lines":[{{"fp":"{fp}","keep":true,"en":null,"reason":null}},
                {{"fp":"{fp}","keep":false,"en":"Hi.","reason":"Too stiff."}}]}}"#
        ))
        .unwrap();
        assert_eq!(review.lines[1].en.as_deref(), Some("Hi."));
        for bad in [
            format!(r#"{{"lines":[{{"fp":"{fp}","keep":"yes","en":null,"reason":null}}]}}"#),
            format!(r#"{{"lines":[{{"fp":"{fp}","en":"Hi.","reason":"Too stiff."}}]}}"#),
        ] {
            assert!(serde_json::from_str::<Review>(&bad).is_err(), "{bad}");
        }
        // The schemas say what the types do.
        let translate = line_schema(TRANSLATE_SCHEMA);
        assert_eq!(fields(&translate), (vec!["en", "fp"], true));
        let review = line_schema(REVIEW_SCHEMA);
        assert_eq!(fields(&review), (vec!["en", "fp", "keep", "reason"], true));
        let settle = line_schema(SETTLE_SCHEMA);
        assert_eq!(fields(&settle), (vec!["en", "fp", "reason"], true));
        for schema in [&translate, &review, &settle] {
            assert_eq!(schema["properties"]["fp"]["pattern"], "^[0-9a-f]{32}$");
        }
    }
}
