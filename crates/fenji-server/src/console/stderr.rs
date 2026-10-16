//! The log's stderr, when stderr is one of its sinks (M4 hosting spec §3.4; #359): `--plain`'s and
//! the admin commands'. A thread that logs only queues its line: one thread of the queue's own
//! writes the lines out, so that a stderr that takes no more, a terminal over a frozen link or a
//! pipe nobody reads, holds that thread, never one that logs. Those go on, a runtime's workers
//! among them, one of which sees the signals and runs the console's `stop`: the server serves on,
//! and stops. What else writes to stderr itself, as a panic's message does, still waits for it.
//! - A line the queue has no room for is dropped, and counted. When the writer next gets a line
//!   through, it first says how many it skipped, at WARN.
//! - As the process exits, [`Stderr::finish`] waits a while for the writer, then leaves it.
use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use fenji_console::wire::{Level, LogLine, Part};

use crate::clock::{Clock, SystemClock};

/// The lines that may wait for stderr: room for a burst.
const QUEUE: usize = 8192;

/// The log's stderr: a queue, and the thread that writes it out. Clones share both.
#[derive(Debug, Clone)]
pub(crate) struct Stderr {
    shared: Arc<Shared>,
}

#[derive(Debug)]
struct Shared {
    state: Mutex<State>,
    /// Signalled when a line is queued, and when the writer has nothing left to write.
    changed: Condvar,
}

#[derive(Debug)]
struct State {
    queue: VecDeque<String>,
    room: usize,
    /// The lines dropped, as the queue had no room for them, since the writer last said so.
    skipped: u64,
    /// Whether the writer has a line in hand, being written.
    writing: bool,
}

impl Stderr {
    /// The process's stderr, through a queue of [`QUEUE`] lines.
    ///
    /// # Errors
    /// If its writer's thread can't be started.
    pub(crate) fn new() -> io::Result<Stderr> {
        Stderr::to(QUEUE, io::stderr())
    }

    /// `out`, through a queue of `room` lines.
    fn to(room: usize, out: impl Write + Send + 'static) -> io::Result<Stderr> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                queue: VecDeque::new(),
                room,
                skipped: 0,
                writing: false,
            }),
            changed: Condvar::new(),
        });
        let writing = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("fenji-stderr".into())
            .spawn(move || write_out(&writing, out))?;
        Ok(Stderr { shared })
    }

    /// Queues `line`, its line ending in it, without waiting: one the queue has no room for is
    /// dropped, and counted.
    pub(crate) fn send(&self, line: String) {
        let mut state = self.shared.lock();
        if state.queue.len() < state.room {
            state.queue.push_back(line);
            self.shared.changed.notify_all();
        } else {
            state.skipped = state.skipped.saturating_add(1);
        }
    }

    /// Queues `text`, whatever room is left, without waiting: the last the process writes, the
    /// error it exits with.
    pub(crate) fn last(&self, text: String) {
        self.shared.lock().queue.push_back(text);
        self.shared.changed.notify_all();
    }

    /// As the process exits: waits `grace` at most for the writer to write what's queued, and to
    /// say what it skipped.
    pub(crate) fn finish(&self, grace: Duration) {
        let until = Instant::now() + grace;
        let mut state = self.shared.lock();
        while state.writing || !state.queue.is_empty() || state.skipped > 0 {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return;
            }
            state = self
                .shared
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

impl Shared {
    /// The state, whatever a thread that panicked holding it left: the log goes on.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn wait<'a>(&self, state: MutexGuard<'a, State>) -> MutexGuard<'a, State> {
        self.changed
            .wait(state)
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// The writer's thread: writes each line queued to `out`, in order. When lines were skipped, the
/// next it writes says how many, and the one after that is a queued line, if there's one: counts
/// that come as fast as the writer gets through never hold up what's queued. A write that fails is
/// let go, as the log never stops the server; one that waits holds this thread alone.
fn write_out(shared: &Shared, mut out: impl Write) {
    // Whether the line last written was a count.
    let mut counted = false;
    loop {
        let line = {
            let mut state = shared.lock();
            let line = loop {
                if state.skipped > 0 && (!counted || state.queue.is_empty()) {
                    counted = true;
                    break skipped(std::mem::take(&mut state.skipped), SystemClock.now_ms());
                }
                if let Some(line) = state.queue.pop_front() {
                    counted = false;
                    break line;
                }
                state.writing = false;
                shared.changed.notify_all();
                state = shared.wait(state);
            };
            state.writing = true;
            line
        };
        let _ = out.write_all(line.as_bytes()).and_then(|()| out.flush());
    }
}

/// The line, at WARN as the log writes its own, that says `lines` were skipped, at `at_ms`:
/// `2 lines of the log were skipped: stderr wasn't being read`, and `1 line … was skipped: …`.
fn skipped(lines: u64, at_ms: i64) -> String {
    let (noun, verb) = if lines == 1 {
        ("line", "was")
    } else {
        ("lines", "were")
    };
    let line = LogLine {
        at_ms,
        level: Level::Warn,
        parts: vec![Part {
            text: format!("{lines} {noun} of the log {verb} skipped: stderr wasn't being read"),
            name: false,
        }],
    };
    format!("{line}\n")
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    use super::*;

    /// A writer whose writes wait while it's closed: a stderr nobody reads, until someone does.
    #[derive(Clone, Default)]
    struct Stalled(Arc<(Mutex<Out>, Condvar)>);

    #[derive(Default)]
    struct Out {
        open: bool,
        /// Whether a write has begun since it was last closed.
        asked: bool,
        written: Vec<u8>,
    }

    impl Stalled {
        fn open(&self) {
            let (out, changed) = &*self.0;
            out.lock().unwrap().open = true;
            changed.notify_all();
        }

        fn close(&self) {
            let mut out = self.0.0.lock().unwrap();
            out.open = false;
            out.asked = false;
        }

        /// Waits, 10 s at most, until `done` holds of what it has.
        fn until(&self, done: impl Fn(&Out) -> bool) {
            let (out, changed) = &*self.0;
            let mut out = out.lock().unwrap();
            let until = Instant::now() + Duration::from_secs(10);
            while !done(&out) {
                let left = until.saturating_duration_since(Instant::now());
                assert!(!left.is_zero(), "not within 10 s");
                out = changed.wait_timeout(out, left).unwrap().0;
            }
        }

        fn written(&self) -> String {
            String::from_utf8(self.0.0.lock().unwrap().written.clone()).unwrap()
        }
    }

    impl io::Write for Stalled {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let (out, changed) = &*self.0;
            let mut out = out.lock().unwrap();
            out.asked = true;
            changed.notify_all();
            while !out.open {
                out = changed.wait(out).unwrap();
            }
            out.written.extend_from_slice(buf);
            changed.notify_all();
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// Sends `lines` from a thread of their own, as a runtime's worker would log them: each must
    /// be through at once, whatever the writer is doing. A panic if they aren't after 5 s.
    fn sent(stderr: &Stderr, lines: &[&'static str]) {
        let sending = {
            let stderr = stderr.clone();
            let lines = lines.to_vec();
            std::thread::spawn(move || {
                for line in lines {
                    stderr.send(line.into());
                }
            })
        };
        let until = Instant::now() + Duration::from_secs(5);
        while !sending.is_finished() {
            assert!(
                Instant::now() < until,
                "a line waited for stderr: still sending after 5 s"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        sending.join().unwrap();
    }

    /// `text` with the time each of the log's own lines begins with, `HH:MM:SS`, as those letters.
    fn untimed(text: &str) -> String {
        let time = |word: &str| {
            word.len() == 8
                && word.bytes().enumerate().all(|(at, byte)| match at {
                    2 | 5 => byte == b':',
                    _ => byte.is_ascii_digit(),
                })
        };
        text.split_inclusive('\n')
            .map(|line| match line.split_once(' ') {
                Some((word, rest)) if time(word) => format!("HH:MM:SS {rest}"),
                _ => line.to_owned(),
            })
            .collect()
    }

    #[test]
    fn a_line_never_waits_and_those_with_no_room_are_counted_and_said_first_once_stderr_reads() {
        // #359 (Task 7's re-review, Important 1): the thread that logs may be a runtime's worker,
        // the one that would see a signal or run the console's `stop`. A stderr that takes no
        // more holds the writer alone, while the server serves as after.
        let out = Stalled::default();
        let stderr = Stderr::to(2, out.clone()).unwrap();
        stderr.send("a\n".into());
        // The writer has taken it, and waits in its write.
        out.until(|out| out.asked);
        stderr.send("b\n".into());
        stderr.send("c\n".into());
        // The queue is full: these are dropped at once, and counted.
        sent(&stderr, &["d\n", "e\n"]);
        // Nor does the exit wait for the writer past its grace.
        let began = Instant::now();
        stderr.finish(Duration::from_millis(200));
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "{:?}",
            began.elapsed()
        );
        // Read again: the writer's line gets through, and first of all it says how many it
        // skipped, then writes what was queued, and what comes after.
        out.open();
        out.until(|out| out.written.ends_with(b"c\n"));
        stderr.send("f\n".into());
        out.until(|out| out.written.ends_with(b"f\n"));
        // Stalled again, for one line: the count is of those skipped since it was last said.
        out.close();
        stderr.send("g\n".into());
        out.until(|out| out.asked);
        stderr.send("h\n".into());
        stderr.send("i\n".into());
        sent(&stderr, &["j\n"]);
        out.open();
        stderr.finish(Duration::from_secs(10));
        assert_eq!(
            untimed(&out.written()),
            "a\n\
             HH:MM:SS WARN  2 lines of the log were skipped: stderr wasn't being read\n\
             b\nc\nf\ng\n\
             HH:MM:SS WARN  1 line of the log was skipped: stderr wasn't being read\n\
             h\ni\n"
        );
    }

    #[test]
    fn the_error_the_process_exits_with_is_queued_whatever_room_is_left() {
        // #359 (Task 7's re-review): the process's last line, after those queued.
        let out = Stalled::default();
        let stderr = Stderr::to(1, out.clone()).unwrap();
        stderr.send("a\n".into());
        out.until(|out| out.asked);
        stderr.send("b\n".into());
        sent(&stderr, &["c\n"]);
        stderr.last("Error: the world failed\n".into());
        out.open();
        stderr.finish(Duration::from_secs(10));
        assert_eq!(
            untimed(&out.written()),
            "a\n\
             HH:MM:SS WARN  1 line of the log was skipped: stderr wasn't being read\n\
             b\n\
             Error: the world failed\n"
        );
    }

    #[test]
    fn every_line_is_written_in_order_and_the_exit_waits_for_them() {
        // With room for them all, none is dropped, however far ahead of the writer they come, and
        // the exit waits for what's queued.
        let out = Stalled::default();
        out.open();
        let stderr = Stderr::to(128, out.clone()).unwrap();
        let lines: Vec<String> = (0..100).map(|n| format!("line {n}\n")).collect();
        for line in &lines {
            stderr.send(line.clone());
        }
        stderr.finish(Duration::from_secs(10));
        assert_eq!(out.written(), lines.concat());
    }
}
