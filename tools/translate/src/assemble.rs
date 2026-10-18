//! The assembly (retranslation spec §6.5, step 9): the new English, by area, into a `fenji-content`
//! checkout, with the glossary and the style guide, and what it covers in the work folder.
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use fenji_pack::{Entry, Fingerprint, write_tsv};

use crate::area::Area;
use crate::batch::read_batches;
use crate::drive::{ANSWERS, REVIEWS, SETTLED};
use crate::extract::is_cjk;
use crate::glossary::{export, load};
use crate::lines::Line;
use crate::prompt::{Answer, Review, STYLE, Settled};

/// The coverage report in the work folder.
pub const COVERAGE: &str = "coverage.txt";

/// What the new English covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coverage {
    pub lines: usize,
    pub translated: usize,
    /// The lines with no English yet, sorted.
    pub missing: Vec<Fingerprint>,
}

/// Writes `lines`' new English from `work` into the `fenji-content` checkout `to`:
/// - `text/en/<area>.tsv`, sorted, for each area with English, and no other area's;
/// - `glossary.tsv`, the glossary's export;
/// - `STYLE.md`, the style guide;
///
/// and `work`'s [`COVERAGE`]. A line's English is its settled form if there is one, else its
/// review's, else its answer's. Nothing is written if any English holds Chinese, or if `to` is
/// inside the checkout that holds `work` (Fenji's own, whose `re/` holds the copy's text).
pub fn assemble(work: &Path, lines: &[Line], to: &Path) -> Result<Coverage> {
    let work = work
        .canonicalize()
        .with_context(|| work.display().to_string())?;
    let to = resolved(to)?;
    if let Some(checkout) = work.ancestors().find(|d| d.join(".git").exists())
        && to.starts_with(checkout)
    {
        bail!(
            "{} is inside {}, the checkout that holds the work folder: fenji-content is a checkout of its own",
            to.display(),
            checkout.display()
        );
    }
    if to.starts_with(&work) {
        bail!("{} is inside the work folder", to.display());
    }
    let english = english(&work)?;
    let mut by_area: BTreeMap<Area, Vec<Entry>> = BTreeMap::new();
    let mut counts: BTreeMap<Area, (usize, usize)> = BTreeMap::new();
    let mut missing = Vec::new();
    for line in lines {
        let count = counts.entry(line.area).or_default();
        count.0 += 1;
        let Some(en) = english.get(&line.fp) else {
            missing.push(line.fp);
            continue;
        };
        if en.chars().any(is_cjk) {
            bail!(
                "{}'s English holds Chinese, which fenji-content never does",
                line.fp
            );
        }
        count.1 += 1;
        by_area.entry(line.area).or_default().push(Entry {
            fingerprint: line.fp,
            english: en.clone(),
        });
    }
    missing.sort();
    let glossary = export(&load(&work)?)?;

    let text = to.join("text").join("en");
    std::fs::create_dir_all(&text).with_context(|| text.display().to_string())?;
    let files: BTreeSet<String> = by_area
        .keys()
        .map(|a| format!("{}.tsv", a.name()))
        .collect();
    for entry in std::fs::read_dir(&text)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if name.ends_with(".tsv") && !files.contains(&name) {
            std::fs::remove_file(text.join(&name))?;
        }
    }
    for (area, mut entries) in by_area {
        let path = text.join(format!("{}.tsv", area.name()));
        std::fs::write(&path, write_tsv(&mut entries))
            .with_context(|| path.display().to_string())?;
    }
    std::fs::write(to.join("glossary.tsv"), glossary)?;
    std::fs::write(to.join("STYLE.md"), STYLE)?;
    let coverage = Coverage {
        lines: lines.len(),
        translated: lines.len() - missing.len(),
        missing,
    };
    std::fs::write(work.join(COVERAGE), report(&coverage, &counts))?;
    Ok(coverage)
}

/// Each line's English, from every batch's files: settled, else the review's, else the answer's.
fn english(work: &Path) -> Result<HashMap<Fingerprint, String>> {
    let mut all = HashMap::new();
    for b in read_batches(work)? {
        let file = |dir: &str| work.join(dir).join(format!("{}.json", b.id));
        let Some(answer) = read_if::<Answer>(&file(ANSWERS))? else {
            continue;
        };
        let mut en: HashMap<Fingerprint, String> =
            answer.lines.into_iter().map(|l| (l.fp, l.en)).collect();
        if let Some(review) = read_if::<Review>(&file(REVIEWS))? {
            for l in review.lines.into_iter().filter(|l| !l.keep) {
                if let Some(e) = l.en {
                    en.insert(l.fp, e);
                }
            }
        }
        if let Some(settled) = read_if::<Settled>(&file(SETTLED))? {
            en.extend(settled.lines.into_iter().map(|l| (l.fp, l.en)));
        }
        for (fp, e) in en {
            if all.insert(fp, e).is_some() {
                bail!("{fp} is in two batches' answers");
            }
        }
    }
    Ok(all)
}

fn read_if<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(path).with_context(|| path.display().to_string())?;
    Ok(Some(
        serde_json::from_slice(&bytes).with_context(|| path.display().to_string())?,
    ))
}

/// `path` absolute, through its nearest existing ancestor's real path, so that no symlink hides
/// where it is. One with `..` is refused.
fn resolved(path: &Path) -> Result<PathBuf> {
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        bail!("{} contains '..'", path.display());
    }
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let (mut existing, mut rest) = (abs.as_path(), Vec::new());
    while !existing.exists() {
        rest.push(
            existing
                .file_name()
                .context("no existing ancestor")?
                .to_owned(),
        );
        existing = existing.parent().context("no existing ancestor")?;
    }
    let mut out = existing.canonicalize()?;
    out.extend(rest.iter().rev());
    Ok(out)
}

/// The coverage report: the totals, each area's, and every line with no English yet.
fn report(c: &Coverage, counts: &BTreeMap<Area, (usize, usize)>) -> String {
    let mut out = format!(
        "{} lines, {} translated, {} missing\n",
        c.lines,
        c.translated,
        c.missing.len()
    );
    for (area, (lines, translated)) in counts {
        out.push_str(&format!("{}: {translated} of {lines}\n", area.name()));
    }
    if !c.missing.is_empty() {
        out.push_str("\nMissing:\n");
        for fp in &c.missing {
            out.push_str(&format!("{fp}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fenji_pack::{fingerprint, read_tsv};

    use super::*;
    use crate::area::classify;
    use crate::batch::{Batch, plan_batches, read_batches, write_batches};
    use crate::check::Rules;
    use crate::drive::{ANSWERS, REVIEWS, SETTLED};
    use crate::glossary::{GLOSSARY, export, load};
    use crate::lines::{Use, write_lines};
    use crate::prompt::{Answer, AnswerLine, Review, ReviewLine, STYLE, Settled, SettledLine};

    fn line(zh: &str, sheet: &str, field: &str) -> Line {
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
            en: None,
            uses,
            area,
            tight,
        }
    }

    /// A checkout with the work folder in its `re/`, five invented lines in three batches, and
    /// a glossary; and, apart, the `fenji-content` checkout.
    struct Setup {
        _dirs: (tempfile::TempDir, tempfile::TempDir),
        checkout: PathBuf,
        work: PathBuf,
        to: PathBuf,
        lines: Vec<Line>,
        batches: Vec<Batch>,
    }

    fn setup() -> Setup {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let checkout = a.path().canonicalize().unwrap();
        std::fs::create_dir_all(checkout.join(".git")).unwrap();
        std::fs::write(checkout.join("CLAUDE.md"), "# Fenji\n").unwrap();
        let work = checkout.join("re").join("retranslation");
        std::fs::create_dir_all(&work).unwrap();
        let lines = vec![
            line("铁剑", "items", "name"),
            line("一把铁剑", "items", "desc"),
            line("林月来了", "quest_dialog", "text"),
            line("{0}把{1}交给了你。", "quest_dialog", "text2"),
            line("古书一卷", "book", "text"),
        ];
        write_lines(&work, &lines).unwrap();
        std::fs::write(
            work.join(GLOSSARY),
            r#"{"terms":[{"zh":"林月","en":"Lin Yue","kind":"name","note":"a sect's head"}]}"#,
        )
        .unwrap();
        let glossary = load(&work).unwrap();
        write_batches(
            &work,
            &plan_batches(&lines, &glossary, &Rules::from_lines(&lines)),
        )
        .unwrap();
        let batches = read_batches(&work).unwrap();
        Setup {
            to: b.path().join("fenji-content"),
            _dirs: (a, b),
            checkout,
            work,
            lines,
            batches,
        }
    }

    fn write<T: serde::Serialize>(work: &Path, dir: &str, id: &str, value: &T) {
        std::fs::create_dir_all(work.join(dir)).unwrap();
        std::fs::write(
            work.join(dir).join(format!("{id}.json")),
            serde_json::to_vec(value).unwrap(),
        )
        .unwrap();
    }

    /// Answers every line of the items and dialogue batches, as "EN <n>".
    fn answer(s: &Setup) {
        for b in s.batches.iter().filter(|b| !b.id.starts_with("lore-")) {
            let lines = b
                .lines
                .iter()
                .enumerate()
                .map(|(i, l)| AnswerLine {
                    fp: l.fp,
                    en: format!("EN {} {i}", b.id),
                })
                .collect();
            write(&s.work, ANSWERS, &b.id, &Answer { lines });
        }
    }

    fn fp(zh: &str) -> Fingerprint {
        fingerprint(zh.as_bytes())
    }

    #[test]
    fn assemble_writes_one_sorted_file_per_area_and_counts_coverage() {
        let s = setup();
        answer(&s);
        let coverage = assemble(&s.work, &s.lines, &s.to).unwrap();
        assert_eq!(
            coverage,
            Coverage {
                lines: 5,
                translated: 4,
                missing: vec![fp("古书一卷")],
            }
        );
        let text = s.to.join("text").join("en");
        let mut areas: Vec<String> = std::fs::read_dir(&text)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        areas.sort();
        assert_eq!(
            areas,
            ["dialogue.tsv", "items.tsv"],
            "an area with no English has no file"
        );
        let items = read_tsv(&std::fs::read_to_string(text.join("items.tsv")).unwrap()).unwrap();
        assert_eq!(items.len(), 2, "and read_tsv refuses lines out of order");
        assert!(items.iter().any(|e| e.fingerprint == fp("铁剑")));
        let glossary = load(&s.work).unwrap();
        assert_eq!(
            std::fs::read_to_string(s.to.join("glossary.tsv")).unwrap(),
            export(&glossary).unwrap()
        );
        assert_eq!(
            std::fs::read_to_string(s.to.join("STYLE.md")).unwrap(),
            STYLE
        );
        let report = std::fs::read_to_string(s.work.join(COVERAGE)).unwrap();
        for held in [
            "5 lines",
            "4 translated",
            "1 missing",
            &fp("古书一卷").to_string(),
        ] {
            assert!(report.contains(held), "{held}: {report}");
        }
    }

    #[test]
    fn assemble_refuses_chinese_and_refuses_the_fenji_repository_or_re() {
        let s = setup();
        answer(&s);
        for inside in [
            s.checkout.join("content"),
            s.work.join("out"),
            s.checkout.join("re"),
        ] {
            assert!(
                assemble(&s.work, &s.lines, &inside).is_err(),
                "{}",
                inside.display()
            );
            assert!(!inside.join("text").exists());
        }
        let items = s
            .batches
            .iter()
            .find(|b| b.id.starts_with("items-"))
            .unwrap();
        let chinese = Answer {
            lines: items
                .lines
                .iter()
                .map(|l| AnswerLine {
                    fp: l.fp,
                    en: "Iron 剑".into(),
                })
                .collect(),
        };
        write(&s.work, ANSWERS, &items.id, &chinese);
        assert!(assemble(&s.work, &s.lines, &s.to).is_err());
        assert!(!s.to.join("text").exists(), "nothing written");
    }

    #[test]
    fn the_settled_form_wins() {
        let s = setup();
        answer(&s);
        let dialogue = s
            .batches
            .iter()
            .find(|b| b.id.starts_with("dialogue-"))
            .unwrap();
        let [first, second] = [dialogue.lines[0].fp, dialogue.lines[1].fp];
        let english = |s: &Setup| {
            assemble(&s.work, &s.lines, &s.to).unwrap();
            let text = std::fs::read_to_string(s.to.join("text/en/dialogue.tsv")).unwrap();
            read_tsv(&text)
                .unwrap()
                .into_iter()
                .map(|e| (e.fingerprint, e.english))
                .collect::<std::collections::HashMap<_, _>>()
        };
        assert_eq!(
            english(&s)[&first],
            format!("EN {} 0", dialogue.id),
            "the answer's"
        );
        let review = Review {
            lines: vec![
                ReviewLine {
                    fp: first,
                    keep: false,
                    en: Some("Reviewed".into()),
                    reason: Some("Livelier.".into()),
                },
                ReviewLine {
                    fp: second,
                    keep: true,
                    en: None,
                    reason: None,
                },
            ],
        };
        write(&s.work, REVIEWS, &dialogue.id, &review);
        assert_eq!(english(&s)[&first], "Reviewed", "the review's");
        assert_eq!(
            english(&s)[&second],
            format!("EN {} 1", dialogue.id),
            "kept"
        );
        let settled = Settled {
            lines: vec![SettledLine {
                fp: first,
                en: "Settled".into(),
                reason: "Closer.".into(),
            }],
        };
        write(&s.work, SETTLED, &dialogue.id, &settled);
        assert_eq!(english(&s)[&first], "Settled", "the settled form");
    }

    #[test]
    fn assembling_twice_gives_the_same_bytes() {
        let s = setup();
        answer(&s);
        let read_all = |s: &Setup| {
            let mut files = Vec::new();
            for path in [
                "text/en/items.tsv",
                "text/en/dialogue.tsv",
                "glossary.tsv",
                "STYLE.md",
            ] {
                files.push(std::fs::read(s.to.join(path)).unwrap());
            }
            files.push(std::fs::read(s.work.join(COVERAGE)).unwrap());
            files
        };
        assemble(&s.work, &s.lines, &s.to).unwrap();
        let once = read_all(&s);
        assemble(&s.work, &s.lines, &s.to).unwrap();
        assert_eq!(read_all(&s), once);
    }
}
