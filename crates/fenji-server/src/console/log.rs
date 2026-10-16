//! The log hub (M4 hosting spec §3.4, H10): one place every log line goes through. Each event
//! becomes a [`LogLine`], every part of it cleaned of hidden characters ([`clean`]), for the sinks
//! [`init`] is given:
//! - [`Sink::Stderr`], as before the console, which systemd's journal keeps;
//! - [`Sink::File`], a file a day, which a host attaches to a bug report;
//! - [`Sink::Hub`], the [`LogHub`]: the last lines for a console that attaches, then each line as
//!   it's logged.
//!
//! Logging never waits on a console: one that falls behind loses lines, and is told how many.
use std::cell::Cell;
use std::collections::VecDeque;
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::Context as _;
use fenji_console::text::clean;
use fenji_console::wire::{Level, LogLine, Part};
use tokio::sync::broadcast;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;

use crate::clock::{Clock, SystemClock, Utc};
use crate::config::private_folder;

/// The lines a console gets when it attaches.
const BACKLOG: usize = 500;
/// The lines a subscriber may fall behind by before it loses some: a console's queue.
pub(crate) const QUEUE: usize = 1024;
/// The log files kept, one a day.
const FILES_KEPT: usize = 14;
/// The fields whose values name someone, a character or an account: consoles show them in gold
/// (spec §3.2). The server's own fields name an account as `account`, and give its id, which isn't
/// a name, as `account_id`.
const NAMES: [&str; 5] = ["character", "player", "username", "name", "account"];

/// Where the full-screen console's log files go: `logs` in the data folder, the directory of
/// `[store] path`, which the server keeps owner-only. The files name players and their addresses.
pub fn folder(store_path: &Path) -> PathBuf {
    store_path.parent().unwrap_or(Path::new(".")).join("logs")
}

/// Sets up the process's log: the levels `RUST_LOG` names, or `info`, to `sinks`. Returns the hub,
/// which gets the lines when [`Sink::Hub`] is one of them.
///
/// # Errors
/// If a file sink's folder can't be made or its file opened, or if the process's log is set up
/// already.
pub fn init(sinks: &[Sink]) -> anyhow::Result<LogHub> {
    let hub = LogHub::new(BACKLOG);
    let levels = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(levels)
        .with(LogLayer::to(sinks, &hub)?)
        .try_init()?;
    Ok(hub)
}

/// Where the log's lines go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sink {
    /// The process's stderr, a line each in the console's columns: `HH:MM:SS LEVEL message`.
    Stderr,
    /// A file a day in this folder, `fenji.<date>.log` in UTC, each line with its date in front;
    /// the last 14 are kept. The folder is made owner-only if it's missing, as the log names
    /// players and their addresses.
    File(PathBuf),
    /// The hub [`init`] returns, for consoles.
    Hub,
}

/// Logs a panic on a thread other than the full-screen console's, at ERROR, as the console's hook
/// gives it ([`fenji_console::PanicLog`]): cleaned, as every line is. Turned away, with nothing
/// logged, when the panic is the log's own, this thread being in the log's code: it may hold the
/// log's locks, and logging would wait on them for ever. The console then ends as it did (the
/// hosting plan's final review B, Minor 6).
pub fn log_panic(text: &str) -> bool {
    if IN_LOG.with(Cell::get) {
        return false;
    }
    tracing::error!("{text}");
    true
}

thread_local! {
    /// Whether this thread is in the log's own code, perhaps holding its locks: a panic here is
    /// the log's own, which [`log_panic`] turns away.
    static IN_LOG: Cell<bool> = const { Cell::new(false) };
}

/// This thread marked as in the log's own code ([`IN_LOG`]) until dropped.
struct InLog {
    was: bool,
}

impl InLog {
    fn enter() -> InLog {
        InLog {
            was: IN_LOG.replace(true),
        }
    }
}

impl Drop for InLog {
    fn drop(&mut self) {
        IN_LOG.set(self.was);
    }
}

/// The log for consoles: the last lines, for a console that attaches, and each line as it's
/// logged. Clones share one hub.
#[derive(Debug, Clone)]
pub struct LogHub {
    inner: Arc<Mutex<Inner>>,
}

/// One lock for the backlog and the channel, so that a subscriber gets each line once: in its
/// backlog or from the channel.
#[derive(Debug)]
struct Inner {
    /// The last `keep` lines, oldest first.
    backlog: VecDeque<LogLine>,
    keep: usize,
    lines: broadcast::Sender<LogLine>,
}

impl LogHub {
    /// A hub that keeps the last `backlog` lines.
    pub fn new(backlog: usize) -> LogHub {
        LogHub {
            inner: Arc::new(Mutex::new(Inner {
                backlog: VecDeque::new(),
                keep: backlog,
                lines: broadcast::Sender::new(QUEUE),
            })),
        }
    }

    /// A [`Layer`] that gives this hub each event, as a [`LogLine`].
    pub fn layer(&self) -> LogLayer {
        LogLayer {
            writers: Vec::new(),
            hub: Some(self.clone()),
        }
    }

    /// The last lines logged, then each line as it's logged: none missed, and none twice.
    pub fn subscribe(&self) -> LogSubscription {
        let _in_log = InLog::enter();
        let inner = self.lock();
        LogSubscription {
            backlog: inner.backlog.iter().cloned().collect(),
            lines: inner.lines.subscribe(),
        }
    }

    /// Keeps `line` and sends it to the subscribers, without waiting for any.
    pub(crate) fn publish(&self, line: LogLine) {
        let _in_log = InLog::enter();
        let mut inner = self.lock();
        // With no subscriber, the line is the backlog's alone.
        let _ = inner.lines.send(line.clone());
        inner.backlog.push_back(line);
        if inner.backlog.len() > inner.keep {
            inner.backlog.pop_front();
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// A subscriber to the log: the lines logged before it subscribed, then each line as it's logged.
#[derive(Debug)]
pub struct LogSubscription {
    /// The last lines logged before it subscribed, oldest first.
    pub backlog: Vec<LogLine>,
    lines: broadcast::Receiver<LogLine>,
}

/// What a subscriber gets next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogItem {
    /// A line, as it's logged.
    Line(LogLine),
    /// How many lines it lost by falling behind: `… 37 lines skipped`.
    Skipped(u64),
}

impl LogSubscription {
    /// The next line, or how many it lost by falling behind, once there is one; `None` once the hub
    /// is gone. Cancel-safe: cancelled, it has taken nothing.
    pub async fn next(&mut self) -> Option<LogItem> {
        match self.lines.recv().await {
            Ok(line) => Some(LogItem::Line(line)),
            Err(broadcast::error::RecvError::Lagged(lost)) => Some(LogItem::Skipped(lost)),
            Err(broadcast::error::RecvError::Closed) => None,
        }
    }

    /// [`LogSubscription::next`] without waiting: `None` when no line is waiting, or once the hub
    /// is gone.
    pub fn try_next(&mut self) -> Option<LogItem> {
        match self.lines.try_recv() {
            Ok(line) => Some(LogItem::Line(line)),
            Err(broadcast::error::TryRecvError::Lagged(lost)) => Some(LogItem::Skipped(lost)),
            Err(_) => None,
        }
    }
}

/// The [`Layer`] that makes each event a [`LogLine`] and gives it to its sinks.
#[derive(Debug)]
pub struct LogLayer {
    /// Stderr and the files, in the order they were given.
    writers: Vec<Writer>,
    /// The hub, when it's a sink.
    hub: Option<LogHub>,
}

/// A sink the layer writes text to.
#[derive(Debug)]
enum Writer {
    Stderr,
    File(RollingFileAppender),
}

impl LogLayer {
    /// A layer that gives each line to `sinks`, of which [`Sink::Hub`] is `hub`.
    fn to(sinks: &[Sink], hub: &LogHub) -> anyhow::Result<LogLayer> {
        let mut writers = Vec::new();
        for sink in sinks {
            match sink {
                Sink::Stderr => writers.push(Writer::Stderr),
                Sink::File(folder) => writers.push(Writer::File(daily_file(folder)?)),
                Sink::Hub => {}
            }
        }
        Ok(LogLayer {
            writers,
            hub: sinks.contains(&Sink::Hub).then(|| hub.clone()),
        })
    }
}

impl<S: Subscriber> Layer<S> for LogLayer {
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        // A panic from here on, a field's value written out or a sink's write, is the log's own.
        let _in_log = InLog::enter();
        let line = line_of(event, SystemClock.now_ms());
        for writer in &self.writers {
            writer.write(&line);
        }
        if let Some(hub) = &self.hub {
            hub.publish(line);
        }
    }
}

impl Writer {
    /// Writes `line` as this sink shows it. A write that fails is dropped: the log never stops the
    /// server, and under the full-screen console nothing else may write to the terminal.
    fn write(&self, line: &LogLine) {
        let _ = match self {
            Writer::Stderr => std::io::stderr().write_all(format!("{line}\n").as_bytes()),
            Writer::File(file) => file.make_writer().write_all(dated(line).as_bytes()),
        };
    }
}

/// A file a day in `folder`, the last [`FILES_KEPT`] kept, the folder made owner-only if it's
/// missing.
fn daily_file(folder: &Path) -> anyhow::Result<RollingFileAppender> {
    private_folder(folder)
        .with_context(|| format!("can't make the log folder {}", folder.display()))?;
    RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("fenji")
        .filename_suffix("log")
        .max_log_files(FILES_KEPT)
        .build(folder)
        .with_context(|| format!("can't open a log file in {}", folder.display()))
}

/// `event` as a line logged at `at_ms`: its message, then `key=value` for each field. Every part
/// is cleaned, and the values of the [`NAMES`] fields are names.
fn line_of(event: &Event<'_>, at_ms: i64) -> LogLine {
    let mut fields = Fields::default();
    event.record(&mut fields);
    let mut parts = Parts::default();
    parts.push(&fields.message, false);
    for (key, value) in &fields.rest {
        let space = if parts.0.is_empty() { "" } else { " " };
        parts.push(&format!("{space}{key}="), false);
        parts.push(value, NAMES.contains(key));
    }
    LogLine {
        at_ms,
        level: level_of(*event.metadata().level()),
        parts: parts.0,
    }
}

/// An event's message, and its other fields' values as text, in order.
#[derive(Default)]
struct Fields {
    message: String,
    rest: Vec<(&'static str, String)>,
}

impl Fields {
    fn add(&mut self, field: &Field, value: String) {
        match field.name() {
            "message" => self.message = value,
            // Where a `log` crate's record came from, which tracing-log adds: rustls logs that way.
            name if name.starts_with("log.") => {}
            name => self.rest.push((name, value)),
        }
    }
}

impl Visit for Fields {
    /// Text as it is, rather than quoted as `Debug` has it.
    fn record_str(&mut self, field: &Field, value: &str) {
        self.add(field, value.to_owned());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.add(field, format!("{value:?}"));
    }
}

/// A line's parts as they're built: cleaned, and text that names no one joins the part before it
/// when that names no one either.
#[derive(Default)]
struct Parts(Vec<Part>);

impl Parts {
    fn push(&mut self, text: &str, name: bool) {
        if text.is_empty() {
            return;
        }
        let text = clean(text);
        match self.0.last_mut() {
            Some(last) if !last.name && !name => last.text.push_str(&text),
            _ => self.0.push(Part {
                text: text.into_owned(),
                name,
            }),
        }
    }
}

fn level_of(level: tracing::Level) -> Level {
    match level {
        tracing::Level::ERROR => Level::Error,
        tracing::Level::WARN => Level::Warn,
        tracing::Level::INFO => Level::Info,
        tracing::Level::DEBUG => Level::Debug,
        _ => Level::Trace,
    }
}

/// `line` as a file has it: its date in UTC, then the console's columns, `YYYY-MM-DD HH:MM:SS
/// LEVEL message`.
fn dated(line: &LogLine) -> String {
    let Utc {
        year, month, day, ..
    } = Utc::from_ms(line.at_ms);
    format!("{year:04}-{month:02}-{day:02} {line}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The line's text: its parts joined as they are.
    fn text_of(line: &LogLine) -> String {
        line.parts.iter().map(|p| p.text.as_str()).collect()
    }

    #[test]
    fn the_backlog_keeps_the_last_lines_and_subscribers_see_the_rest() {
        let hub = LogHub::new(500);
        let subscriber = tracing_subscriber::registry().with(hub.layer());
        tracing::subscriber::with_default(subscriber, || {
            for n in 0..600 {
                tracing::info!(n, "line");
            }
        });
        let sub = hub.subscribe();
        assert_eq!(sub.backlog.len(), 500);
        assert!(text_of(&sub.backlog[0]).contains("n=100"));
    }

    #[tokio::test]
    async fn a_subscriber_that_falls_behind_is_told_how_many_it_lost() {
        let hub = LogHub::new(500);
        let mut sub = hub.subscribe();
        let subscriber = tracing_subscriber::registry().with(hub.layer());
        tracing::subscriber::with_default(subscriber, || {
            for n in 0..1100 {
                tracing::info!(n, "line");
            }
        });
        assert_eq!(sub.next().await, Some(LogItem::Skipped(76)));
    }

    /// A line of `text` alone, for the hub.
    fn line(text: &str) -> LogLine {
        LogLine {
            at_ms: 0,
            level: Level::Info,
            parts: vec![Part {
                text: text.into(),
                name: false,
            }],
        }
    }

    #[test]
    fn with_publishers_on_many_threads_each_subscriber_gets_every_line_once() {
        // The hosting plan's Task 7 (#359): the backlog and the channel share one lock, so that a
        // subscriber gets each line once, in its backlog or from the channel, whichever thread
        // logged it and whenever it subscribed. 400 lines in all, within a subscriber's queue
        // (`QUEUE`): none can fall behind and lose lines here.
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::Duration;
        const PUBLISHERS: usize = 4;
        const EACH: usize = 100;
        const SUBSCRIBERS: usize = 8;
        const SUBSCRIPTIONS: usize = 25;
        let hub = LogHub::new(PUBLISHERS * EACH);
        let start = Arc::new(std::sync::Barrier::new(PUBLISHERS + SUBSCRIBERS));
        let published = Arc::new(AtomicBool::new(false));
        let publishers: Vec<_> = (0..PUBLISHERS)
            .map(|p| {
                let (hub, start) = (hub.clone(), Arc::clone(&start));
                std::thread::spawn(move || {
                    start.wait();
                    for n in 0..EACH {
                        hub.publish(line(&format!("{p}:{n}")));
                        std::thread::sleep(Duration::from_micros(20));
                    }
                })
            })
            .collect();
        // Subscribing again and again while the lines come.
        let subscribers: Vec<_> = (0..SUBSCRIBERS)
            .map(|_| {
                let (hub, start) = (hub.clone(), Arc::clone(&start));
                let published = Arc::clone(&published);
                std::thread::spawn(move || {
                    start.wait();
                    let mut subscriptions = Vec::new();
                    while subscriptions.len() < SUBSCRIPTIONS && !published.load(Ordering::SeqCst) {
                        subscriptions.push(hub.subscribe());
                        std::thread::sleep(Duration::from_micros(100));
                    }
                    subscriptions
                })
            })
            .collect();
        for publisher in publishers {
            publisher.join().unwrap();
        }
        published.store(true, Ordering::SeqCst);
        let all: Vec<String> = hub.subscribe().backlog.iter().map(text_of).collect();
        assert_eq!(all.len(), PUBLISHERS * EACH);
        let mut checked = 0;
        for subscriber in subscribers {
            for mut sub in subscriber.join().unwrap() {
                let kept = sub.backlog.len();
                let mut seen: Vec<String> = sub.backlog.iter().map(text_of).collect();
                while let Some(item) = sub.try_next() {
                    match item {
                        LogItem::Line(line) => seen.push(text_of(&line)),
                        LogItem::Skipped(lost) => panic!("{lost} lines lost"),
                    }
                }
                assert_eq!(
                    seen, all,
                    "{kept} lines from its backlog, then the channel's"
                );
                checked += 1;
            }
        }
        assert!(checked >= SUBSCRIBERS, "{checked} subscriptions");
    }

    #[test]
    fn an_account_is_a_name_which_the_console_shows_in_gold_and_its_id_is_not() {
        // The hosting plan's Task 7 (#359): the console draws a name part in gold, and these
        // fields' values are names; an account's id is a number.
        let hub = LogHub::new(10);
        let subscriber = tracing_subscriber::registry().with(hub.layer());
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(account = "mai", account_id = 7, "signed in");
            tracing::info!(
                character = "Mai",
                player = "Lin",
                username = "wen",
                name = "Ren",
                "named"
            );
        });
        let backlog = hub.subscribe().backlog;
        let parts = |line: &LogLine| -> Vec<(String, bool)> {
            line.parts
                .iter()
                .map(|p| (p.text.clone(), p.name))
                .collect()
        };
        let part = |text: &str, name| (text.to_owned(), name);
        assert_eq!(
            parts(&backlog[0]),
            [
                part("signed in account=", false),
                part("mai", true),
                part(" account_id=7", false)
            ]
        );
        assert_eq!(
            parts(&backlog[1]),
            [
                part("named character=", false),
                part("Mai", true),
                part(" player=", false),
                part("Lin", true),
                part(" username=", false),
                part("wen", true),
                part(" name=", false),
                part("Ren", true)
            ]
        );
    }

    #[test]
    fn where_a_log_crates_record_came_from_is_left_out() {
        // The hosting plan's Task 7 (#359): tracing-log gives a `log` crate's record, as rustls
        // logs, fields for where it came from. The line is the record's message and its own
        // fields; a field whose name merely starts with "log" stays.
        let hub = LogHub::new(10);
        let subscriber = tracing_subscriber::registry().with(hub.layer());
        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!(
                log.target = "rustls::conn",
                log.module_path = "rustls::conn",
                log.file = "src/conn.rs",
                log.line = 42,
                logins = 3,
                "a TLS alert"
            );
        });
        assert_eq!(text_of(&hub.subscribe().backlog[0]), "a TLS alert logins=3");
    }

    #[test]
    fn names_are_marked_and_hidden_characters_are_escaped() {
        // Review Focus 1: the file and stderr get the same cleaned text.
        let hub = LogHub::new(10);
        let subscriber = tracing_subscriber::registry().with(hub.layer());
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(character = "Mai\u{1b}[2J", "entered");
        });
        let line = &hub.subscribe().backlog[0];
        assert!(
            line.parts
                .iter()
                .any(|p| p.name && p.text == r"Mai\u{1b}[2J"),
            "{line:?}"
        );
    }

    #[test]
    fn a_panic_elsewhere_is_logged_at_error_cleaned() {
        // The hosting plan's final review B, Minor 6: while the full-screen console holds the
        // terminal, stderr points away from it, so another thread's panic reaches the host through
        // the log, cleaned as every line is.
        let hub = LogHub::new(10);
        let subscriber = tracing_subscriber::registry().with(hub.layer());
        let logged = tracing::subscriber::with_default(subscriber, || {
            log_panic("a thread panicked at src/x.rs:1:2: \u{1b}[2Jboom")
        });
        assert!(logged);
        let line = &hub.subscribe().backlog[0];
        assert_eq!(line.level, Level::Error);
        assert_eq!(
            text_of(line),
            "a thread panicked at src/x.rs:1:2: \\u{1b}[2Jboom"
        );
    }

    #[test]
    fn a_panic_in_the_logs_own_code_is_turned_away() {
        // The panicking thread may hold the log's locks, and logging its panic would wait on them
        // for ever: it's turned away, and the console ends as it did. Here the log is asked from
        // inside the layer, as a field's value is written out there.
        struct Inside(Arc<Mutex<Option<bool>>>);
        impl fmt::Debug for Inside {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                *self.0.lock().unwrap() = Some(log_panic("a thread panicked in the log"));
                f.write_str("inside")
            }
        }
        let hub = LogHub::new(10);
        let asked = Arc::new(Mutex::new(None));
        let subscriber = tracing_subscriber::registry().with(hub.layer());
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(value = ?Inside(Arc::clone(&asked)), "a line");
        });
        assert_eq!(*asked.lock().unwrap(), Some(false), "not turned away");
        // Outside the log's code, it's taken again.
        let subscriber = tracing_subscriber::registry().with(hub.layer());
        let logged = tracing::subscriber::with_default(subscriber, || log_panic("later"));
        assert!(logged);
        let backlog = hub.subscribe().backlog;
        let texts: Vec<String> = backlog.iter().map(text_of).collect();
        assert_eq!(texts, ["a line value=inside", "later"]);
    }

    #[test]
    fn a_file_sink_writes_the_hubs_line_with_its_date_in_a_file_a_day() {
        // Review Focus 1: the file shows the name's escape, never the escape itself.
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("logs");
        let hub = LogHub::new(10);
        let layer = LogLayer::to(&[Sink::File(logs.clone()), Sink::Hub], &hub).unwrap();
        tracing::subscriber::with_default(tracing_subscriber::registry().with(layer), || {
            tracing::info!(character = "Mai\u{1b}[2J", "entered");
        });
        let names: Vec<String> = std::fs::read_dir(&logs)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        let [name] = names.as_slice() else {
            panic!("one file: {names:?}")
        };
        assert!(name.starts_with("fenji") && name.ends_with("log"), "{name}");
        let text = std::fs::read_to_string(logs.join(name)).unwrap();
        assert!(
            text.ends_with(" INFO  entered character=Mai\\u{1b}[2J\n"),
            "{text:?}"
        );
        // The hub's line, the date in front: one line, as stderr has it.
        assert_eq!(text, dated(&hub.subscribe().backlog[0]));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&logs).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700, "the log names players: owner-only");
        }
    }

    #[test]
    fn a_files_line_is_the_consoles_with_the_date_in_front() {
        // 2026-10-14 05:02:03 UTC.
        let line = LogLine {
            at_ms: 1_791_950_400_000 + 3_723_000,
            level: Level::Warn,
            parts: vec![
                Part {
                    text: "chat from ".into(),
                    name: false,
                },
                Part {
                    text: "Mai\u{1b}[2J".into(),
                    name: true,
                },
            ],
        };
        assert_eq!(
            dated(&line),
            "2026-10-14 05:02:03 WARN  chat from Mai\\u{1b}[2J\n"
        );
    }
}
