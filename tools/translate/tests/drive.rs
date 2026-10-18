//! The driver of headless runs (retranslation plan, Task 7), against `fake-claude.sh`, with two
//! invented batches in a checkout of their own.
#![cfg(unix)]

use std::collections::BTreeSet;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use fenji_pack::fingerprint;
use fenji_translate::area::classify;
use fenji_translate::batch::{plan_batches, read_batches, write_batches};
use fenji_translate::check::{Glossary, Rules};
use fenji_translate::drive::{
    ANSWERS, Driver, PAUSE, QUEUE, REVIEWS, RUNS, SETTLED, Summary, Usage, status, usage,
};
use fenji_translate::lines::{Line, Use, write_lines};
use fenji_translate::prompt::{Answer, Settled, TRANSLATE};

/// A test's folders: a checkout with the work folder in its `re/`, and, outside it, the folder
/// runs start in, the fake's log and the wrapper that sets the fake's mode.
struct Setup {
    _checkout: tempfile::TempDir,
    _outside: tempfile::TempDir,
    checkout: PathBuf,
    work: PathBuf,
    cwd: PathBuf,
    log: PathBuf,
    claude: PathBuf,
}

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

/// Two batches: an item's name and description, and a conversation with a placeholder and a line
/// the fake's review replaces.
fn lines() -> Vec<Line> {
    vec![
        line("铁剑", "items", "name"),
        line("一把铁剑", "items", "desc"),
        line("林月REVISE来了", "quest_dialog", "text"),
        line("{0}把{1}交给了你。", "quest_dialog", "text2"),
    ]
}

fn setup(mode: &str, sleep: &str) -> Setup {
    let checkout_dir = tempfile::tempdir().unwrap();
    let outside_dir = tempfile::tempdir().unwrap();
    let checkout = checkout_dir.path().canonicalize().unwrap();
    let outside = outside_dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(checkout.join(".git")).unwrap();
    let work = checkout.join("re").join("work");
    std::fs::create_dir_all(&work).unwrap();
    let all = lines();
    write_lines(&work, &all).unwrap();
    let batches = plan_batches(&all, &Glossary::default(), &Rules::from_lines(&all));
    assert_eq!(batches.len(), 2);
    write_batches(&work, &batches).unwrap();
    let fake = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fake-claude.sh");
    let log = outside.join("log");
    let claude = outside.join("claude");
    std::fs::write(
        &claude,
        format!(
            "#!/bin/sh\nexport FAKE_CLAUDE_MODE={mode} FAKE_CLAUDE_LOG='{}' FAKE_CLAUDE_SLEEP={sleep}\nexec '{}' \"$@\"\n",
            log.display(),
            fake.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
    Setup {
        _checkout: checkout_dir,
        _outside: outside_dir,
        cwd: outside.join("runs"),
        checkout,
        work,
        log,
        claude,
    }
}

fn driver(s: &Setup) -> Driver {
    Driver {
        claude: s.claude.clone(),
        model: "claude-opus-5-5".into(),
        effort: "high".into(),
        runs: 2,
        work: s.work.clone(),
        cwd: s.cwd.clone(),
        retries: 2,
        wait: Duration::from_millis(300),
        only: None,
    }
}

/// The fake's runs, a line each.
fn runs(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

/// The names of a folder's files, none if it isn't there.
fn files(dir: &Path) -> BTreeSet<String> {
    std::fs::read_dir(dir)
        .map(|d| {
            d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn named<const N: usize>(ids: [&str; N]) -> BTreeSet<String> {
    ids.iter().map(|id| format!("{id}.json")).collect()
}

fn read<T: serde::de::DeserializeOwned>(path: &Path) -> T {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

const ALL_DONE: Summary = Summary {
    translated: 2,
    reviewed: 2,
    settled: 1,
    queued: 0,
    waiting: false,
};

#[test]
fn the_driver_translates_reviews_and_settles_every_batch() {
    let s = setup("ok", "0.4");
    let done = driver(&s).run().unwrap();
    assert_eq!(done, ALL_DONE);
    assert_eq!(status(&s.work).unwrap(), ALL_DONE);
    let both = named(["dialogue-00001", "items-00001"]);
    assert_eq!(files(&s.work.join(ANSWERS)), both);
    assert_eq!(files(&s.work.join(REVIEWS)), both);
    assert_eq!(
        files(&s.work.join(SETTLED)),
        named(["dialogue-00001"]),
        "only where the review replaced a line"
    );
    let answer: Answer = read(&s.work.join(ANSWERS).join("items-00001.json"));
    assert_eq!(answer.lines.len(), 2);
    let settled: Settled = read(&s.work.join(SETTLED).join("dialogue-00001.json"));
    assert_eq!(settled.lines.len(), 1);
    assert!(settled.lines[0].en.starts_with("Settled"));

    let log = runs(&s.log);
    assert_eq!(
        log.len(),
        5,
        "two translations, two reviews and a settling: {log:#?}"
    );
    for run in &log {
        for arg in [
            "[-p]",
            "[--output-format] [json]",
            "[--tools] []",
            "[--strict-mcp-config]",
            "[--no-session-persistence]",
            "[--safe-mode]",
            "[--setting-sources] [project]",
            "[--model] [claude-opus-5-5]",
            "[--effort] [high]",
        ] {
            assert!(run.contains(arg), "{arg}: {run}");
        }
        let cwd = run.split("cwd=").nth(1).unwrap().split(' ').next().unwrap();
        assert_eq!(Path::new(cwd), s.cwd);
        assert!(
            !Path::new(cwd).starts_with(&s.checkout),
            "outside the checkout"
        );
    }
    let going: Vec<usize> = log
        .iter()
        .map(|r| {
            r.split("going=")
                .nth(1)
                .unwrap()
                .split(' ')
                .next()
                .unwrap()
                .parse()
                .unwrap()
        })
        .collect();
    assert!(going.iter().all(|&n| n <= 2), "{going:?}");
    assert!(going.contains(&2), "two at once: {going:?}");
}

#[test]
fn every_run_is_recorded_with_what_it_used() {
    let s = setup("ok", "0");
    assert_eq!(driver(&s).run().unwrap(), ALL_DONE);
    let text = std::fs::read_to_string(s.work.join(RUNS)).unwrap();
    let records: Vec<serde_json::Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(records.len(), 5, "a line per run: {text}");
    let jobs: BTreeSet<(&str, &str)> = records
        .iter()
        .map(|r| (r["batch"].as_str().unwrap(), r["job"].as_str().unwrap()))
        .collect();
    assert!(jobs.contains(&("dialogue-00001", "settle")));
    for r in &records {
        assert_eq!(r["model"], "claude-opus-5-5");
        assert_eq!(r["effort"], "high");
        assert_eq!(r["outcome"], "kept");
        assert_eq!(r["output_tokens"], 20);
        assert_eq!(r["input_tokens"], 80, "every input token, cached or not");
        assert!(r["at"].as_u64().unwrap() > 1_700_000_000);
    }
    assert_eq!(
        usage(&s.work).unwrap(),
        Usage {
            runs: 5,
            input_tokens: 400,
            output_tokens: 100,
            cost_usd: 2.5,
        }
    );

    // A run that fails is recorded too, with nothing used.
    let j = setup("junk", "0");
    driver(&j).run().unwrap();
    let text = std::fs::read_to_string(j.work.join(RUNS)).unwrap();
    assert_eq!(text.lines().count(), 6, "three tries a batch");
    assert!(
        text.lines().all(|l| l.contains("\"outcome\":\"failed\"")),
        "{text}"
    );
    assert_eq!(usage(&j.work).unwrap().output_tokens, 0);
}

#[test]
fn a_cwd_inside_a_checkout_is_refused() {
    let s = setup("ok", "0");
    let inside = Driver {
        cwd: s.checkout.join("runs"),
        ..driver(&s)
    };
    assert!(inside.run().is_err());
    let too_many = Driver {
        runs: 3,
        ..driver(&s)
    };
    assert!(too_many.run().is_err(), "two at most, by the owner's rule");
    assert!(runs(&s.log).is_empty());
}

#[test]
fn a_partial_or_junk_answer_is_never_kept() {
    for mode in ["short", "wrong-fp", "junk"] {
        let s = setup(mode, "0");
        let done = driver(&s).run().unwrap();
        let queued = Summary {
            queued: 2,
            ..Summary::default()
        };
        assert_eq!(done, queued, "{mode}");
        assert!(files(&s.work.join(ANSWERS)).is_empty(), "{mode}");
        for b in read_batches(&s.work).unwrap() {
            let q: serde_json::Value = read(&s.work.join(QUEUE).join(format!("{}.json", b.id)));
            assert_eq!(q["job"], "translate", "{mode}");
            let named: BTreeSet<&str> = q["lines"]
                .as_array()
                .unwrap()
                .iter()
                .map(|l| l["fp"].as_str().unwrap())
                .collect();
            let fps: Vec<String> = b.lines.iter().map(|l| l.fp.to_string()).collect();
            let held: BTreeSet<&str> = fps.iter().map(String::as_str).collect();
            assert_eq!(named, held, "{mode}");
        }
        assert_eq!(runs(&s.log).len(), 6, "{mode}: three tries a batch");
    }
}

#[test]
fn a_usage_limit_waits_and_resumes() {
    let s = setup("limit", "0");
    let d = driver(&s);
    let work = s.work.clone();
    let run = std::thread::spawn(move || d.run());
    let start = Instant::now();
    let mut seen = false;
    while !run.is_finished() && start.elapsed() < Duration::from_secs(60) {
        seen |= status(&work).unwrap().waiting;
        std::thread::sleep(Duration::from_millis(5));
    }
    let done = run.join().unwrap().unwrap();
    assert!(seen, "waiting, once");
    assert_eq!(done, ALL_DONE, "and no job queued for it");
    let log = runs(&s.log);
    assert_eq!(log.iter().filter(|r| r.contains(" limit-now ")).count(), 1);
    assert_eq!(
        log.len(),
        6,
        "the limited run again, and the others: {log:#?}"
    );
}

#[test]
fn a_server_error_waits_without_using_a_try() {
    let s = setup("overloaded", "0");
    // With no retries, a try spent on it would queue its batch.
    let d = Driver {
        retries: 0,
        ..driver(&s)
    };
    assert_eq!(d.run().unwrap(), ALL_DONE);
    let log = runs(&s.log);
    assert_eq!(
        log.iter()
            .filter(|r| r.contains(" overloaded-now "))
            .count(),
        1
    );
    assert_eq!(
        log.len(),
        6,
        "the overloaded run again, and the others: {log:#?}"
    );
}

#[test]
fn a_restart_resumes_and_a_pause_stops_new_jobs() {
    let s = setup("ok", "0.5");
    let mut child = Command::new(env!("CARGO_BIN_EXE_translate"))
        .current_dir(&s.checkout)
        .args(["drive", "--out", "re/work", "--claude"])
        .arg(&s.claude)
        .arg("--cwd")
        .arg(&s.cwd)
        .spawn()
        .unwrap();
    let start = Instant::now();
    while files(&s.work.join(ANSWERS)).is_empty() {
        assert!(start.elapsed() < Duration::from_secs(60), "no answer yet");
        std::thread::sleep(Duration::from_millis(10));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    let finished = files(&s.work.join(ANSWERS));
    let mut log = std::fs::OpenOptions::new()
        .append(true)
        .open(&s.log)
        .unwrap();
    writeln!(log, "restart").unwrap();

    assert_eq!(driver(&s).run().unwrap(), ALL_DONE);
    let after: Vec<String> = runs(&s.log)
        .into_iter()
        .skip_while(|r| r != "restart")
        .skip(1)
        .collect();
    for b in read_batches(&s.work).unwrap() {
        if !finished.contains(&format!("{}.json", b.id)) {
            continue;
        }
        let fps: String = b.lines.iter().map(|l| format!("{},", l.fp)).collect();
        assert!(
            !after
                .iter()
                .any(|r| r.starts_with("translate ") && r.contains(&fps)),
            "{} translated again",
            b.id
        );
    }
    for dir in [ANSWERS, REVIEWS, SETTLED, QUEUE] {
        for name in files(&s.work.join(dir)) {
            assert!(name.ends_with(".json"), "{dir}/{name}: half-written");
            let _: serde_json::Value = read(&s.work.join(dir).join(&name));
        }
    }

    let p = setup("ok", "0");
    std::fs::write(p.work.join(PAUSE), "").unwrap();
    assert_eq!(driver(&p).run().unwrap(), Summary::default());
    assert!(runs(&p.log).is_empty(), "no job started");
}

#[test]
fn a_line_failing_its_checks_is_retried_with_its_problems() {
    let s = setup("bad-token", "0");
    assert_eq!(driver(&s).run().unwrap(), ALL_DONE);
    let dir = PathBuf::from(format!("{}.prompts", s.log.display()));
    let prompts: Vec<String> = files(&dir)
        .iter()
        .map(|name| std::fs::read_to_string(dir.join(name)).unwrap())
        .collect();
    assert_eq!(prompts.len(), 6);
    let retried: Vec<&String> = prompts
        .iter()
        .filter(|p| p.contains("MissingToken(\"{0}\")"))
        .collect();
    assert_eq!(retried.len(), 1, "one retry");
    assert!(retried[0].starts_with(TRANSLATE));
    assert_eq!(runs(&s.log).len(), 6, "one more try: {:#?}", runs(&s.log));
}
