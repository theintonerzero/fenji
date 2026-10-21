//! The driver of headless runs (retranslation spec §6.5): every batch through its three jobs
//! (translating, reviewing, and settling what the review disputes), in headless Claude Code runs
//! with no tools, at most two at a time.
//!
//! The work folder's files are its state:
//! - it resumes where it stopped;
//! - it keeps an answer only once the answer passes its checks;
//! - it queues for an interactive session what still fails them after its retries;
//! - it waits out a usage limit;
//! - it starts no job while `PAUSE` is there.
use std::collections::{BTreeSet, HashMap};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use fenji_pack::Fingerprint;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::area::Area;
use crate::batch::{Batch, BatchLine, read_batches};
use crate::check::{Glossary, Rules, check_within};
use crate::lines::{Line, read_lines};
use crate::prompt::{
    Answer, Dispute, REVIEW_SCHEMA, Review, SETTLE_SCHEMA, STYLE, Settled, TRANSLATE_SCHEMA,
    review_prompt, settle_prompt, translate_prompt,
};

/// The translators' answers, by batch.
pub const ANSWERS: &str = "answers";
/// The reviews, by batch.
pub const REVIEWS: &str = "reviews";
/// The settled disputes, by batch.
pub const SETTLED: &str = "settled";
/// What failed its checks after its retries, by batch, for an interactive session.
pub const QUEUE: &str = "queue";
/// While this file is in the work folder, no new job starts.
pub const PAUSE: &str = "PAUSE";
/// While the driver waits out a usage limit, this file holds when the wait ends.
pub const WAITING: &str = "WAITING";
/// A line per run, appended as each ends: its batch and job, the model and effort, how it ended,
/// how long it took, and what it used by the CLI's own count.
pub const RUNS: &str = "runs.jsonl";

/// The most runs at once, by the owner's rule.
pub const MAX_RUNS: usize = 2;
pub const DEFAULT_MODEL: &str = "claude-opus-5-5";
pub const DEFAULT_EFFORT: &str = "high";
/// How long one run may take before it's stopped and counted as a failed try.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// The driver, and how it runs `claude`.
#[derive(Debug, Clone)]
pub struct Driver {
    /// The Claude Code CLI.
    pub claude: PathBuf,
    pub model: String,
    pub effort: String,
    /// Runs at once: [`MAX_RUNS`] at most.
    pub runs: usize,
    /// The work folder.
    pub work: PathBuf,
    /// The folder the runs start in, outside any checkout, so that no project's instructions
    /// reach them.
    pub cwd: PathBuf,
    /// Tries after the first, before what fails is queued.
    pub retries: u32,
    /// How long to wait out a usage limit that names no reset time.
    pub wait: Duration,
    /// Only this batch.
    pub only: Option<String>,
}

impl Driver {
    /// A driver with the defaults: `claude` on the path, [`DEFAULT_MODEL`] at [`DEFAULT_EFFORT`],
    /// two runs at once, two retries, and a 30 minutes' wait.
    pub fn new(work: PathBuf, cwd: PathBuf) -> Driver {
        Driver {
            claude: PathBuf::from("claude"),
            model: DEFAULT_MODEL.into(),
            effort: DEFAULT_EFFORT.into(),
            runs: MAX_RUNS,
            work,
            cwd,
            retries: 2,
            wait: Duration::from_secs(30 * 60),
            only: None,
        }
    }
}

/// Where the work stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Summary {
    /// Batches with an answer.
    pub translated: usize,
    /// Batches with a review.
    pub reviewed: usize,
    /// Batches whose disputes are settled.
    pub settled: usize,
    /// Batches waiting for an interactive session.
    pub queued: usize,
    /// Whether the driver is waiting out a usage limit.
    pub waiting: bool,
}

/// What the runs so far have used, from [`RUNS`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Usage {
    pub runs: usize,
    /// Every input token, cached or not.
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// The CLI's own reckoning, in dollars. On a subscription it's a measure of use, not a bill.
    pub cost_usd: f64,
}

/// What the runs in `work` have used, from [`RUNS`]: nothing if there is none yet.
pub fn usage(work: &Path) -> Result<Usage> {
    let path = work.join(RUNS);
    if !path.exists() {
        return Ok(Usage::default());
    }
    let text = std::fs::read_to_string(&path).with_context(|| path.display().to_string())?;
    let mut u = Usage::default();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let v: Value = serde_json::from_str(line).with_context(|| path.display().to_string())?;
        u.runs += 1;
        u.input_tokens += v["input_tokens"].as_u64().unwrap_or(0);
        u.output_tokens += v["output_tokens"].as_u64().unwrap_or(0);
        u.cost_usd += v["cost_usd"].as_f64().unwrap_or(0.0);
    }
    Ok(u)
}

/// What one run used, by the CLI's result, and how long it took.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Ran {
    input_tokens: u64,
    output_tokens: u64,
    cost_usd: f64,
    seconds: f64,
}

/// The usage a CLI result reports: every input token, cached or not, the output tokens, and its
/// cost. Nothing for output that isn't its JSON.
fn usage_of(stdout: &[u8]) -> Ran {
    let Ok(v) = serde_json::from_str::<Value>(String::from_utf8_lossy(stdout).trim()) else {
        return Ran::default();
    };
    let u = &v["usage"];
    let n = |field: &str| u[field].as_u64().unwrap_or(0);
    Ran {
        input_tokens: n("input_tokens")
            + n("cache_creation_input_tokens")
            + n("cache_read_input_tokens"),
        output_tokens: n("output_tokens"),
        cost_usd: v["total_cost_usd"].as_f64().unwrap_or(0.0),
        seconds: 0.0,
    }
}

/// One line of [`RUNS`].
#[derive(Serialize)]
struct RunRecord<'a> {
    /// When the run ended, in seconds since the epoch.
    at: u64,
    batch: &'a str,
    job: &'a str,
    model: &'a str,
    effort: &'a str,
    /// `kept`, `failed`, `limit` or `busy`.
    outcome: &'a str,
    seconds: f64,
    input_tokens: u64,
    output_tokens: u64,
    cost_usd: f64,
}

/// Where the work in `work` stands, from its files.
pub fn status(work: &Path) -> Result<Summary> {
    Ok(Summary {
        translated: count(&work.join(ANSWERS))?,
        reviewed: count(&work.join(REVIEWS))?,
        settled: count(&work.join(SETTLED))?,
        queued: count(&work.join(QUEUE))?,
        waiting: work.join(WAITING).exists(),
    })
}

fn count(dir: &Path) -> Result<usize> {
    if !dir.exists() {
        return Ok(0);
    }
    let mut n = 0;
    for entry in std::fs::read_dir(dir).with_context(|| dir.display().to_string())? {
        if entry?.file_name().to_string_lossy().ends_with(".json") {
            n += 1;
        }
    }
    Ok(n)
}

/// A batch's job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Job {
    Translate,
    Review,
    Settle,
}

impl Job {
    fn name(self) -> &'static str {
        match self {
            Job::Translate => "translate",
            Job::Review => "review",
            Job::Settle => "settle",
        }
    }

    fn dir(self) -> &'static str {
        match self {
            Job::Translate => ANSWERS,
            Job::Review => REVIEWS,
            Job::Settle => SETTLED,
        }
    }

    /// The job's schema, as one line of JSON for the command line.
    fn schema(self) -> String {
        let schema = match self {
            Job::Translate => TRANSLATE_SCHEMA,
            Job::Review => REVIEW_SCHEMA,
            Job::Settle => SETTLE_SCHEMA,
        };
        let value: Value = serde_json::from_str(schema).expect("the schemas are JSON");
        value.to_string()
    }
}

/// A line that failed, for an interactive session: its last English, if any, and its problems.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedLine {
    pub fp: Fingerprint,
    pub en: Option<String>,
    pub problems: Vec<String>,
}

/// A batch's job that still failed after its retries: `queue/<id>.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Queued {
    /// `translate`, `review` or `settle`.
    pub job: String,
    pub tries: u32,
    /// The lines that failed, every one of the job's if the answer failed as a whole.
    pub lines: Vec<QueuedLine>,
    /// The last answer, as the run gave it, if it was JSON.
    pub answer: Option<Value>,
}

/// What a run gave.
#[derive(Debug, PartialEq)]
enum Outcome {
    /// The structured answer.
    Answer(Value),
    /// A usage limit, and when it resets, where the CLI says.
    Limit(Option<Reset>),
    /// The API's own trouble, an overload or a server error: why.
    Busy(String),
    /// Anything else: why.
    Failed(String),
}

/// When a usage limit resets.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Reset {
    /// An instant, as older CLIs gave it: `usage limit reached|<seconds since the epoch>`.
    At(SystemTime),
    /// A time of day in seconds, in a zone if the CLI names one, else the machine's: the docs'
    /// `You've hit your session limit · resets 3:45pm`.
    Clock { secs: u32, zone: Option<String> },
}

/// The CLI's output, `--output-format json`: one result object (Task 7's real run). Its
/// `structured_output` holds the answer in the schema's shape, whenever its `subtype` is
/// `success`.
fn parse_result(stdout: &[u8], stderr: &[u8]) -> Outcome {
    let out = String::from_utf8_lossy(stdout);
    let err = String::from_utf8_lossy(stderr);
    let value = serde_json::from_str::<Value>(out.trim())
        .ok()
        .or_else(|| {
            out.lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .and_then(|l| serde_json::from_str(l).ok())
        })
        .filter(Value::is_object);
    let Some(v) = value else {
        for text in [&out, &err] {
            if is_limit(text) {
                return Outcome::Limit(reset(text));
            }
        }
        return Outcome::Failed(format!(
            "the run's output isn't the CLI's JSON: {}",
            excerpt(&format!("{out} {err}"))
        ));
    };
    let subtype = v["subtype"].as_str();
    // Before 2.1.290, a schema run's success could carry is_error too.
    if subtype == Some("success")
        && let Some(answer) = v.get("structured_output").filter(|a| a.is_object())
    {
        return Outcome::Answer(answer.clone());
    }
    let result = v["result"].as_str().unwrap_or_default();
    let status = v["api_error_status"].as_u64();
    if status == Some(429) || is_limit(result) {
        return Outcome::Limit(reset(result));
    }
    if status.is_some_and(|s| s >= 500)
        || result.starts_with("API Error: 5")
        || result.contains("Overloaded")
    {
        return Outcome::Busy(excerpt(result));
    }
    if v["is_error"].as_bool().unwrap_or(false) || subtype.is_some_and(|s| s != "success") {
        return Outcome::Failed(format!(
            "the run failed ({}): {}",
            subtype.unwrap_or("?"),
            excerpt(result)
        ));
    }
    Outcome::Failed("the run gave no structured answer".into())
}

/// Whether a run's text is the CLI's usage limit: "You've hit your session limit", "Claude AI
/// usage limit reached", or another wording of it.
fn is_limit(text: &str) -> bool {
    let t = text.to_lowercase();
    t.contains("limit")
        && (t.contains("usage")
            || t.contains("reached")
            || t.contains("hit your")
            || t.contains("resets"))
}

/// When a usage limit's text says it resets: an epoch after a `|`, or a clock time after
/// `resets`, with the zone in brackets after it if there is one.
fn reset(text: &str) -> Option<Reset> {
    if let Some((_, after)) = text.rsplit_once('|') {
        let digits: String = after
            .trim()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if let Ok(secs) = digits.parse::<u64>() {
            return Some(Reset::At(UNIX_EPOCH + Duration::from_secs(secs)));
        }
    }
    let secs = clock(text)?;
    let zone = text
        .rsplit_once('(')
        .and_then(|(_, z)| z.split_once(')'))
        .map(|(z, _)| z.trim().to_string())
        .filter(|z| {
            !z.is_empty()
                && z.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "/_-+".contains(c))
        });
    Some(Reset::Clock { secs, zone })
}

/// The first time of day after `resets` in `text`, in seconds: `3pm`, `3:45pm`, `15:00`. A bare
/// number isn't one, so a date's day isn't taken for an hour.
fn clock(text: &str) -> Option<u32> {
    let lower = text.to_lowercase();
    let (_, after) = lower.split_once("resets")?;
    after.split_whitespace().find_map(|word| {
        let word = word.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != ':');
        let (time, half) = match (word.strip_suffix("am"), word.strip_suffix("pm")) {
            (Some(t), _) => (t, Some(0)),
            (_, Some(t)) => (t, Some(12)),
            _ => (word, None),
        };
        let (h, m) = match time.split_once(':') {
            Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
            None if half.is_some() => (time.parse::<u32>().ok()?, 0),
            None => return None,
        };
        let h = match half {
            Some(add) if (1..=12).contains(&h) => h % 12 + add,
            Some(_) => return None,
            None if h < 24 => h,
            None => return None,
        };
        (m < 60).then_some(h * 3600 + m * 60)
    })
}

/// How long from `now` until the time of day `reset` next comes, both in seconds of the day.
fn until_clock(reset: u32, now: u32) -> Duration {
    Duration::from_secs(u64::from((reset + 86_400 - now % 86_400) % 86_400))
}

/// The time of day now, in seconds, in `zone` or the machine's own, from `date`.
fn now_of_day(zone: Option<&str>) -> Option<u32> {
    let mut date = Command::new("date");
    date.arg("+%H:%M:%S");
    if let Some(zone) = zone {
        date.env("TZ", zone);
    }
    let out = date.output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    let mut parts = text.trim().split(':').map(|p| p.parse::<u32>().ok());
    let (h, m, s) = (parts.next()??, parts.next()??, parts.next()??);
    Some(h * 3600 + m * 60 + s)
}

fn excerpt(text: &str) -> String {
    let t = text.trim();
    match t.char_indices().nth(300) {
        Some((i, _)) => format!("{}…", &t[..i]),
        None => t.to_string(),
    }
}

/// A batch line as the checks see it.
fn as_line(l: &BatchLine) -> Line {
    Line {
        fp: l.fp,
        env: String::new(),
        zh: l.zh.clone(),
        en: l.en_old.clone(),
        uses: Vec::new(),
        area: Area::Other,
        tight: l.tight,
    }
}

/// Why a job's answer isn't kept.
enum Failure {
    /// The answer as a whole: not the schema's shape, or not the job's lines.
    Whole(String),
    /// Lines that failed their checks.
    Lines(Vec<QueuedLine>),
}

/// What's wrong with `got` as the answer for `wanted`: each wanted line once, and no other.
fn whole_problems(wanted: &[Fingerprint], got: &[Fingerprint]) -> Option<String> {
    let wanted_set: BTreeSet<&Fingerprint> = wanted.iter().collect();
    let mut seen = BTreeSet::new();
    let mut problems = Vec::new();
    for fp in got {
        if !wanted_set.contains(fp) {
            problems.push(format!("{fp} isn't one of the lines"));
        } else if !seen.insert(fp) {
            problems.push(format!("{fp} is there twice"));
        }
    }
    for fp in wanted {
        if !seen.contains(fp) {
            problems.push(format!("{fp} has no English"));
        }
    }
    (!problems.is_empty()).then(|| problems.join("; "))
}

/// The checks on `english` as `line`'s, as text, within the budget its batch gives it: planning
/// set it from the line's fields, which a batch line doesn't carry (#491).
fn line_problems(
    line: &BatchLine,
    english: &str,
    glossary: &Glossary,
    rules: &Rules,
) -> Vec<String> {
    check_within(&as_line(line), english, glossary, rules, line.budget)
        .iter()
        .map(|p| format!("{p:?}"))
        .collect()
}

/// The work every job shares.
struct Ctx {
    batches: Vec<Batch>,
    glossaries: Vec<Glossary>,
    rules: Rules,
}

/// A job a worker took.
struct Picked {
    batch: usize,
    job: Job,
    disputes: Vec<Dispute>,
    /// What was wrong with the last try, for the next one's prompt.
    retry: Option<String>,
}

/// How a try ended.
enum Step {
    Done,
    /// A usage limit, until it resets.
    Limit(SystemTime),
    /// The API's own trouble: a wait, growing while it lasts, that spends no try.
    Busy(String),
    Failed {
        retry: String,
        queued: Queued,
    },
}

/// What the workers share.
#[derive(Default)]
struct State {
    /// Batches, in order, by index.
    order: Vec<usize>,
    /// Batches with nothing left to do, or queued.
    finished: BTreeSet<usize>,
    busy: BTreeSet<usize>,
    tries: HashMap<(usize, Job), u32>,
    retry: HashMap<(usize, Job), String>,
    wait_until: Option<SystemTime>,
    /// The API's troubles in a row, which lengthen the wait.
    busy_streak: u32,
    error: Option<anyhow::Error>,
}

impl Driver {
    /// Takes every batch through its jobs, at most [`Driver::runs`] at once, until each is done
    /// or queued, or `PAUSE` stops new jobs; then where the work stands.
    pub fn run(&self) -> Result<Summary> {
        if self.runs == 0 || self.runs > MAX_RUNS {
            bail!(
                "{} runs at once: {MAX_RUNS} at most, by the owner's rule",
                self.runs
            );
        }
        std::fs::create_dir_all(&self.cwd).with_context(|| self.cwd.display().to_string())?;
        let cwd = self.cwd.canonicalize()?;
        if let Some(checkout) = cwd.ancestors().find(|d| d.join(".git").exists()) {
            bail!(
                "the runs start outside any checkout, and {} is inside {}",
                cwd.display(),
                checkout.display()
            );
        }
        for dir in [ANSWERS, REVIEWS, SETTLED, QUEUE] {
            let dir = self.work.join(dir);
            std::fs::create_dir_all(&dir).with_context(|| dir.display().to_string())?;
            // A crash's half-written files.
            for entry in std::fs::read_dir(&dir)? {
                let path = entry?.path();
                if path.extension().is_some_and(|e| e == "tmp") {
                    std::fs::remove_file(&path)?;
                }
            }
        }
        let lines = read_lines(&self.work)?;
        let rules = Rules::load(&self.work, &lines)?;
        drop(lines);
        let batches = read_batches(&self.work)?;
        if let Some(only) = &self.only
            && !batches.iter().any(|b| &b.id == only)
        {
            bail!("no batch {only}");
        }
        let rank = |id: &str| {
            [
                Area::Interface,
                Area::Items,
                Area::Skills,
                Area::Quests,
                Area::Dialogue,
                Area::Lore,
                Area::Other,
            ]
            .iter()
            .position(|a| id.split('-').next() == Some(a.name()))
            .unwrap_or(usize::MAX)
        };
        let mut order: Vec<usize> = (0..batches.len())
            .filter(|&i| self.only.as_ref().is_none_or(|o| o == &batches[i].id))
            .collect();
        order.sort_by(|&a, &b| {
            (rank(&batches[a].id), &batches[a].id).cmp(&(rank(&batches[b].id), &batches[b].id))
        });
        let ctx = Ctx {
            glossaries: batches
                .iter()
                .map(|b| Glossary {
                    terms: b.glossary.clone(),
                })
                .collect(),
            batches,
            rules,
        };
        let state = Mutex::new(State {
            order,
            ..State::default()
        });
        std::thread::scope(|s| {
            for _ in 0..self.runs {
                s.spawn(|| self.worker(&ctx, &state));
            }
        });
        let _ = std::fs::remove_file(self.work.join(WAITING));
        let state = state.into_inner().expect("no worker panicked");
        if let Some(e) = state.error {
            return Err(e);
        }
        status(&self.work)
    }

    fn worker(&self, ctx: &Ctx, state: &Mutex<State>) {
        loop {
            let picked = {
                let mut s = state.lock().expect("no worker panicked");
                if s.error.is_some() || self.work.join(PAUSE).exists() {
                    return;
                }
                if let Some(until) = s.wait_until {
                    match until.duration_since(SystemTime::now()) {
                        Ok(left) => {
                            drop(s);
                            std::thread::sleep(left.min(Duration::from_millis(200)));
                            continue;
                        }
                        Err(_) => {
                            s.wait_until = None;
                            let _ = std::fs::remove_file(self.work.join(WAITING));
                        }
                    }
                }
                match self.pick(ctx, &mut s) {
                    Ok(Some(p)) => p,
                    Ok(None) if s.busy.is_empty() => return,
                    Ok(None) => {
                        drop(s);
                        std::thread::sleep(Duration::from_millis(50));
                        continue;
                    }
                    Err(e) => {
                        s.error = Some(e);
                        return;
                    }
                }
            };
            let mut ran = Ran::default();
            let result = self.attempt(ctx, &picked, &mut ran);
            let mut s = state.lock().expect("no worker panicked");
            s.busy.remove(&picked.batch);
            let key = (picked.batch, picked.job);
            if let Ok(step) = &result {
                let outcome = match step {
                    Step::Done => "kept",
                    Step::Limit(_) => "limit",
                    Step::Busy(_) => "busy",
                    Step::Failed { .. } => "failed",
                };
                let record = RunRecord {
                    at: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    batch: &ctx.batches[picked.batch].id,
                    job: picked.job.name(),
                    model: &self.model,
                    effort: &self.effort,
                    outcome,
                    seconds: ran.seconds,
                    input_tokens: ran.input_tokens,
                    output_tokens: ran.output_tokens,
                    cost_usd: ran.cost_usd,
                };
                // Under the workers' lock, so two runs' lines never interleave.
                if let Err(e) = append_line(&self.work.join(RUNS), &record) {
                    s.error = Some(e);
                }
            }
            match result {
                Ok(Step::Done) => {
                    s.tries.remove(&key);
                    s.retry.remove(&key);
                    s.busy_streak = 0;
                }
                Ok(Step::Limit(until)) => self.wait_until(&mut s, until, "a usage limit"),
                Ok(Step::Busy(why)) => {
                    // A minute, doubling while the trouble lasts, up to a usage limit's wait.
                    s.busy_streak += 1;
                    let backoff = Duration::from_secs(60)
                        .saturating_mul(1 << (s.busy_streak - 1).min(10))
                        .min(self.wait);
                    self.wait_until(&mut s, SystemTime::now() + backoff, &why);
                }
                Ok(Step::Failed { retry, mut queued }) => {
                    let tries = s.tries.entry(key).or_default();
                    *tries += 1;
                    if *tries > self.retries {
                        queued.tries = *tries;
                        let id = &ctx.batches[picked.batch].id;
                        let path = self.work.join(QUEUE).join(format!("{id}.json"));
                        if let Err(e) = write_json(&path, &queued) {
                            s.error = Some(e);
                        }
                        s.finished.insert(picked.batch);
                    } else {
                        s.retry.insert(key, retry);
                    }
                }
                Err(e) => s.error = Some(e),
            }
        }
    }

    /// Starts no job before `until`, the later of it and any wait already set, and says so in
    /// [`WAITING`]: when the wait ends, in seconds since the epoch, and why.
    fn wait_until(&self, s: &mut State, until: SystemTime, why: &str) {
        let until = s.wait_until.map_or(until, |w| w.max(until));
        s.wait_until = Some(until);
        let secs = until
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if let Err(e) = std::fs::write(self.work.join(WAITING), format!("{secs} {why}\n")) {
            s.error = Some(e.into());
        }
    }

    /// The next job of the first batch in order that has one and that no other worker holds.
    fn pick(&self, ctx: &Ctx, s: &mut State) -> Result<Option<Picked>> {
        for at in 0..s.order.len() {
            let i = s.order[at];
            if s.finished.contains(&i) || s.busy.contains(&i) {
                continue;
            }
            match self.next_job(&ctx.batches[i])? {
                None => {
                    s.finished.insert(i);
                }
                Some((job, disputes)) => {
                    s.busy.insert(i);
                    return Ok(Some(Picked {
                        batch: i,
                        job,
                        disputes,
                        retry: s.retry.get(&(i, job)).cloned(),
                    }));
                }
            }
        }
        Ok(None)
    }

    /// A batch's next job, from its files: none if it's queued or done.
    fn next_job(&self, b: &Batch) -> Result<Option<(Job, Vec<Dispute>)>> {
        let file = |dir: &str| self.work.join(dir).join(format!("{}.json", b.id));
        if file(QUEUE).exists() {
            return Ok(None);
        }
        if !file(ANSWERS).exists() {
            return Ok(Some((Job::Translate, Vec::new())));
        }
        if !file(REVIEWS).exists() {
            return Ok(Some((Job::Review, Vec::new())));
        }
        let answer: Answer = read_json(&file(ANSWERS))?;
        let review: Review = read_json(&file(REVIEWS))?;
        let first: HashMap<Fingerprint, &str> =
            answer.lines.iter().map(|l| (l.fp, l.en.as_str())).collect();
        let disputes: Vec<Dispute> = review
            .lines
            .iter()
            .filter(|l| !l.keep)
            .filter_map(|l| {
                Some(Dispute {
                    fp: l.fp,
                    first: first.get(&l.fp)?.to_string(),
                    second: l.en.clone()?,
                    reason: l.reason.clone().unwrap_or_default(),
                })
            })
            .collect();
        if disputes.is_empty() || file(SETTLED).exists() {
            return Ok(None);
        }
        Ok(Some((Job::Settle, disputes)))
    }

    /// One try at a job: its prompt, its run, and its answer checked and kept. What the run used
    /// goes in `ran`.
    fn attempt(&self, ctx: &Ctx, p: &Picked, ran: &mut Ran) -> Result<Step> {
        let b = &ctx.batches[p.batch];
        let prompt = match p.job {
            Job::Translate => translate_prompt(b, STYLE),
            Job::Review => {
                let answer: Answer =
                    read_json(&self.work.join(ANSWERS).join(format!("{}.json", b.id)))?;
                review_prompt(b, &answer, STYLE)
            }
            Job::Settle => settle_prompt(b, &p.disputes, STYLE),
        };
        let prompt = match &p.retry {
            Some(retry) => format!("{prompt}\n{retry}"),
            None => prompt,
        };
        // The job's lines: the batch's, or its disputed ones.
        let wanted: Vec<Fingerprint> = match p.job {
            Job::Settle => p.disputes.iter().map(|d| d.fp).collect(),
            _ => b.lines.iter().map(|l| l.fp).collect(),
        };
        let failed = |why: String, answer: Option<Value>| Step::Failed {
            retry: whole_retry(&why),
            queued: Queued {
                job: p.job.name().into(),
                tries: 0,
                lines: wanted
                    .iter()
                    .map(|&fp| QueuedLine {
                        fp,
                        en: None,
                        problems: vec![why.clone()],
                    })
                    .collect(),
                answer,
            },
        };
        let (outcome, used) = self.run_claude(&p.job.schema(), &prompt)?;
        *ran = used;
        let value = match outcome {
            Outcome::Limit(reset) => {
                let now = SystemTime::now();
                let until = match reset {
                    Some(Reset::At(t)) if t > now => t + Duration::from_secs(1),
                    // A minute past the clock time, as the limit lifts on the minute.
                    Some(Reset::Clock { secs, zone }) => match now_of_day(zone.as_deref()) {
                        Some(today) => now + until_clock(secs, today) + Duration::from_secs(60),
                        None => now + self.wait,
                    },
                    _ => now + self.wait,
                };
                return Ok(Step::Limit(until));
            }
            Outcome::Busy(why) => return Ok(Step::Busy(why)),
            Outcome::Failed(why) => return Ok(failed(why, None)),
            Outcome::Answer(value) => value,
        };
        let kept = match self.judge(ctx, p, &wanted, value.clone()) {
            Ok(kept) => kept,
            Err(Failure::Whole(why)) => return Ok(failed(why, Some(value))),
            Err(Failure::Lines(lines)) => {
                return Ok(Step::Failed {
                    retry: lines_retry(&lines),
                    queued: Queued {
                        job: p.job.name().into(),
                        tries: 0,
                        lines,
                        answer: Some(value),
                    },
                });
            }
        };
        let path = self.work.join(p.job.dir()).join(format!("{}.json", b.id));
        write_json(&path, &kept)?;
        Ok(Step::Done)
    }

    /// `value` checked as `p`'s answer: what's kept, or why it isn't.
    fn judge(
        &self,
        ctx: &Ctx,
        p: &Picked,
        wanted: &[Fingerprint],
        value: Value,
    ) -> std::result::Result<Value, Failure> {
        let b = &ctx.batches[p.batch];
        let glossary = &ctx.glossaries[p.batch];
        let rules = &ctx.rules;
        let by_fp: HashMap<Fingerprint, &BatchLine> = b.lines.iter().map(|l| (l.fp, l)).collect();
        let shape = |e: serde_json::Error| {
            Failure::Whole(format!("the answer isn't in the schema's shape: {e}"))
        };
        let mut failed = Vec::new();
        let kept = match p.job {
            Job::Translate => {
                let answer: Answer = serde_json::from_value(value).map_err(shape)?;
                let got: Vec<Fingerprint> = answer.lines.iter().map(|l| l.fp).collect();
                if let Some(why) = whole_problems(wanted, &got) {
                    return Err(Failure::Whole(why));
                }
                for l in &answer.lines {
                    let problems = line_problems(by_fp[&l.fp], &l.en, glossary, rules);
                    if !problems.is_empty() {
                        failed.push(QueuedLine {
                            fp: l.fp,
                            en: Some(l.en.clone()),
                            problems,
                        });
                    }
                }
                serde_json::to_value(answer)
            }
            Job::Review => {
                let review: Review = serde_json::from_value(value).map_err(shape)?;
                let got: Vec<Fingerprint> = review.lines.iter().map(|l| l.fp).collect();
                if let Some(why) = whole_problems(wanted, &got) {
                    return Err(Failure::Whole(why));
                }
                for l in review.lines.iter().filter(|l| !l.keep) {
                    let mut problems = match &l.en {
                        Some(en) => line_problems(by_fp[&l.fp], en, glossary, rules),
                        None => vec!["no English for a line it doesn't keep".into()],
                    };
                    if l.reason.as_deref().is_none_or(|r| r.trim().is_empty()) {
                        problems.push("no reason for a line it doesn't keep".into());
                    }
                    if !problems.is_empty() {
                        failed.push(QueuedLine {
                            fp: l.fp,
                            en: l.en.clone(),
                            problems,
                        });
                    }
                }
                serde_json::to_value(review)
            }
            Job::Settle => {
                let settled: Settled = serde_json::from_value(value).map_err(shape)?;
                let got: Vec<Fingerprint> = settled.lines.iter().map(|l| l.fp).collect();
                if let Some(why) = whole_problems(wanted, &got) {
                    return Err(Failure::Whole(why));
                }
                for l in &settled.lines {
                    let problems = line_problems(by_fp[&l.fp], &l.en, glossary, rules);
                    if !problems.is_empty() {
                        failed.push(QueuedLine {
                            fp: l.fp,
                            en: Some(l.en.clone()),
                            problems,
                        });
                    }
                }
                serde_json::to_value(settled)
            }
        }
        .expect("an answer as JSON");
        if failed.is_empty() {
            Ok(kept)
        } else {
            Err(Failure::Lines(failed))
        }
    }

    /// One headless run: the prompt on stdin, the result on stdout, stopped after
    /// [`RUN_TIMEOUT`]; and what it used. An error only if `claude` can't be started.
    fn run_claude(&self, schema: &str, prompt: &str) -> Result<(Outcome, Ran)> {
        let started = Instant::now();
        let mut child = Command::new(&self.claude)
            .args(["-p", "--output-format", "json", "--json-schema", schema])
            .args([
                "--tools",
                "",
                "--strict-mcp-config",
                "--no-session-persistence",
            ])
            // No CLAUDE.md, skills, plugins, hooks or MCP servers, and no user settings: the
            // folder holds no project's, either. 2,966 tokens of context where the plain
            // command has 5,012 (Task 7's real runs).
            .args(["--safe-mode", "--setting-sources", "project"])
            .args(["--model", &self.model, "--effort", &self.effort])
            .current_dir(&self.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("{}: can't start it", self.claude.display()))?;
        let mut stdin = child.stdin.take().expect("a piped stdin");
        let prompt = prompt.to_owned();
        // A run that stops reading its prompt is a failed run, not an error of ours.
        let writer = std::thread::spawn(move || {
            let _ = stdin.write_all(prompt.as_bytes());
        });
        let drain = |mut pipe: Box<dyn Read + Send>| {
            std::thread::spawn(move || {
                let mut out = Vec::new();
                let _ = pipe.read_to_end(&mut out);
                out
            })
        };
        let stdout = drain(Box::new(child.stdout.take().expect("a piped stdout")));
        let stderr = drain(Box::new(child.stderr.take().expect("a piped stderr")));
        let deadline = Instant::now() + RUN_TIMEOUT;
        let finished = loop {
            if child.try_wait()?.is_some() {
                break true;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let _ = writer.join();
        let out = stdout.join().expect("a reader");
        let err = stderr.join().expect("a reader");
        let ran = Ran {
            seconds: started.elapsed().as_secs_f64(),
            ..usage_of(&out)
        };
        let outcome = if finished {
            parse_result(&out, &err)
        } else {
            Outcome::Failed(format!(
                "the run took longer than {} minutes",
                RUN_TIMEOUT.as_secs() / 60
            ))
        };
        Ok((outcome, ran))
    }
}

/// The next try's word on an answer that failed as a whole.
fn whole_retry(why: &str) -> String {
    format!(
        "## What was wrong with your last answer\n\nIt was refused: {why}. Answer again, in the \
         schema's shape, with every line exactly once.\n"
    )
}

/// The next try's word on lines that failed their checks.
fn lines_retry(lines: &[QueuedLine]) -> String {
    let mut out = String::from(
        "## What was wrong with your last answer\n\nThese lines failed their checks. Answer again \
         with every line, and fix these:\n",
    );
    for l in lines {
        out.push_str(&format!("- {}: {}", l.fp, l.problems.join("; ")));
        if let Some(en) = &l.en {
            out.push_str(&format!(". Your English was {en:?}"));
        }
        out.push('\n');
    }
    out.push_str(
        "\nWhat they mean: MissingToken, a tag, placeholder, code or line break of the source is \
         missing; ExtraToken, one the source doesn't have; OutOfOrder, a tag closes before it \
         opens; Cjk, Chinese is left; Char, a character the font doesn't draw; Term, a glossary \
         name isn't written as the glossary has it; TooLong, longer than the line's budget; \
         British, a British spelling; Empty, nothing is drawn.\n",
    );
    out
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = std::fs::read(path).with_context(|| path.display().to_string())?;
    serde_json::from_slice(&bytes).with_context(|| path.display().to_string())
}

/// Appends `value` to `path` as one line of JSON.
fn append_line(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| path.display().to_string())?;
    file.write_all(&line)
        .with_context(|| path.display().to_string())?;
    Ok(())
}

/// Writes `value` to a temporary file beside `path`, then renames it into place.
fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)
        .with_context(|| tmp.display().to_string())?;
    std::fs::rename(&tmp, path).with_context(|| path.display().to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_checked_within_its_batchs_budget() {
        // Planning set the budget from the line's fields, which a batch line doesn't carry; its
        // English today, three letters of pinyin, would allow four on its own (#491).
        let line = BatchLine {
            fp: fenji_pack::fingerprint("蝎妖".as_bytes()),
            zh: "蝎妖".into(),
            en_old: Some("Xie".into()),
            context: String::new(),
            tight: true,
            budget: Some(13),
        };
        let (glossary, rules) = (Glossary::default(), Rules::from_lines(&[]));
        assert!(line_problems(&line, "Scorpion Yao", &glossary, &rules).is_empty());
        let strict = BatchLine {
            budget: Some(4),
            ..line
        };
        assert_eq!(
            line_problems(&strict, "Scorpion Yao", &glossary, &rules),
            ["TooLong { len: 12, budget: 4 }"]
        );
    }

    fn result(fields: &str) -> Vec<u8> {
        format!(r#"{{"type":"result","session_id":"s",{fields}}}"#).into_bytes()
    }

    #[test]
    fn the_clis_result_gives_the_answer_a_limit_a_busy_api_or_why_it_failed() {
        let answer = Outcome::Answer(serde_json::json!({"lines": []}));
        let ok = result(r#""subtype":"success","is_error":false,"structured_output":{"lines":[]}"#);
        assert_eq!(parse_result(&ok, b""), answer);
        // A success with is_error set, which 2.1.290 fixed: the answer stands.
        let flagged =
            result(r#""subtype":"success","is_error":true,"structured_output":{"lines":[]}"#);
        assert_eq!(parse_result(&flagged, b""), answer);

        // The limit, in the docs' words, with a clock time; in older ones, with an epoch.
        let limit = |text: &str| {
            parse_result(
                &result(&format!(
                    r#""subtype":"success","is_error":true,"result":"{text}""#
                )),
                b"",
            )
        };
        assert_eq!(
            limit("You've hit your session limit · resets 3:45pm"),
            Outcome::Limit(Some(Reset::Clock {
                secs: 15 * 3600 + 45 * 60,
                zone: None
            }))
        );
        assert_eq!(
            limit("You've hit your weekly limit · resets 3pm (Europe/Berlin)"),
            Outcome::Limit(Some(Reset::Clock {
                secs: 15 * 3600,
                zone: Some("Europe/Berlin".into())
            }))
        );
        assert_eq!(
            limit("Claude AI usage limit reached|1760000000"),
            Outcome::Limit(Some(Reset::At(
                UNIX_EPOCH + Duration::from_secs(1_760_000_000)
            )))
        );
        assert_eq!(limit("You've hit your session limit"), Outcome::Limit(None));
        let rate = result(r#""is_error":true,"api_error_status":429,"result":"API Error: 429""#);
        assert_eq!(parse_result(&rate, b""), Outcome::Limit(None));
        assert_eq!(
            parse_result(b"", b"Claude AI usage limit reached|1760000000"),
            Outcome::Limit(Some(Reset::At(
                UNIX_EPOCH + Duration::from_secs(1_760_000_000)
            )))
        );

        // The API's own trouble: a wait, not a failed try.
        let overloaded = result(
            r#""subtype":"success","is_error":true,"api_error_status":529,"result":"API Error: Repeated 529 Overloaded errors""#,
        );
        assert!(
            matches!(parse_result(&overloaded, b""), Outcome::Busy(why) if why.contains("Overloaded"))
        );
        let server = result(r#""is_error":true,"result":"API Error: 500 Internal server error""#);
        assert!(matches!(parse_result(&server, b""), Outcome::Busy(_)));

        let invalid = result(
            r#""subtype":"error_max_structured_output_retries","is_error":true,"errors":["no"]"#,
        );
        assert!(
            matches!(parse_result(&invalid, b""), Outcome::Failed(why) if why.contains("error_max_structured_output_retries"))
        );
        let no_answer = result(r#""subtype":"success","is_error":false,"result":"hi""#);
        assert!(matches!(parse_result(&no_answer, b""), Outcome::Failed(_)));
        assert!(matches!(parse_result(b"Sorry.", b""), Outcome::Failed(_)));
    }

    #[test]
    fn a_reset_clock_time_is_waited_for_until_it_next_comes() {
        assert_eq!(clock("resets 3:45pm"), Some(15 * 3600 + 45 * 60));
        assert_eq!(clock("resets 15:00"), Some(15 * 3600));
        assert_eq!(clock("resets 12am"), Some(0));
        assert_eq!(clock("resets 12:30pm"), Some(12 * 3600 + 30 * 60));
        // A later day's reset: the clock time, and the driver tries again then.
        assert_eq!(clock("resets Oct 20, 3pm"), Some(15 * 3600));
        assert_eq!(clock("resets soon"), None);
        assert_eq!(clock("3pm"), None, "only after resets");
        let h = 3600;
        assert_eq!(until_clock(10 * h, 9 * h + 1800), Duration::from_secs(1800));
        assert_eq!(
            until_clock(9 * h, 10 * h),
            Duration::from_secs(23 * h as u64)
        );
        let now = now_of_day(None).expect("the time of day, from date");
        assert!(now < 24 * h);
    }

    #[test]
    fn an_answers_lines_are_the_jobs_each_once() {
        let [a, b, c] = [[1u8; 16], [2; 16], [3; 16]].map(Fingerprint);
        assert_eq!(whole_problems(&[a, b], &[b, a]), None);
        let why = whole_problems(&[a, b], &[a, a, c]).unwrap();
        for part in ["twice", "isn't one of the lines", "has no English"] {
            assert!(why.contains(part), "{why}");
        }
    }
}
