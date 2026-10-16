//! The log's stderr, when stderr is one of its sinks (M4 hosting spec §3.4; #359): `--plain`'s and
//! the admin commands'. Lines go through a bounded queue, which one thread of its own writes out,
//! so that a stderr that takes no more, a terminal over a frozen link or a pipe nobody reads, holds
//! the server up only while it serves.
//! - While the server serves, a line that finds the queue full waits for room, as a write to
//!   stderr would: none is dropped.
//! - Once it's stopping ([`Stderr::stopping`]), no line waits: those waiting drop theirs, and a line
//!   that finds the queue full is dropped. Each is counted, and once the writer has written what
//!   was queued, it says how many were skipped.
//! - As the process exits, [`Stderr::finish`] waits a while for the writer, then leaves it.
use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// The lines that may wait for stderr.
pub(crate) const QUEUE: usize = 1024;

/// The log's stderr: a queue, and the thread that writes it out. Clones share both.
#[derive(Debug, Clone)]
pub(crate) struct Stderr {
    shared: Arc<Shared>,
}

#[derive(Debug)]
struct Shared {
    state: Mutex<State>,
    /// Signalled at every change: a line queued or taken, a write done, the stop begun.
    changed: Condvar,
}

#[derive(Debug)]
struct State {
    queue: VecDeque<String>,
    room: usize,
    stopping: bool,
    /// The lines dropped since the stop began.
    dropped: u64,
    /// Of those, the ones the writer has said it skipped.
    told: u64,
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
                stopping: false,
                dropped: 0,
                told: 0,
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

    /// Queues `line`, its line ending in it. While the server serves, it waits for room; once it's
    /// stopping, a line there's no room for is dropped.
    pub(crate) fn send(&self, line: String) {
        let mut state = self.shared.lock();
        while state.queue.len() >= state.room {
            if state.stopping {
                state.dropped += 1;
                self.shared.changed.notify_all();
                return;
            }
            state = self.shared.wait(state);
        }
        state.queue.push_back(line);
        self.shared.changed.notify_all();
    }

    /// The server is stopping: from now on no line waits for stderr, and those waiting are
    /// dropped.
    pub(crate) fn stopping(&self) {
        self.shared.lock().stopping = true;
        self.shared.changed.notify_all();
    }

    /// As the process exits: the server is stopping, if it wasn't, and this waits `grace` at most
    /// for the writer to write what's queued, and to say what it skipped.
    pub(crate) fn finish(&self, grace: Duration) {
        let until = Instant::now() + grace;
        let mut state = self.shared.lock();
        state.stopping = true;
        self.shared.changed.notify_all();
        while state.writing || !state.queue.is_empty() || state.dropped > state.told {
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

    #[cfg(test)]
    fn dropped(&self) -> u64 {
        self.shared.lock().dropped
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

/// The writer's thread: writes each line queued to `out`, in order, and once it has written what
/// was queued, how many were dropped since it last said. A write that fails is let go, as the log
/// never stops the server; one that waits holds this thread alone.
fn write_out(shared: &Shared, mut out: impl Write) {
    loop {
        let line = {
            let mut state = shared.lock();
            let line = loop {
                if let Some(line) = state.queue.pop_front() {
                    break line;
                }
                if state.dropped > state.told {
                    let skipped = state.dropped - state.told;
                    state.told = state.dropped;
                    break format!(
                        "… {skipped} lines of the log skipped, as stderr took no more\n"
                    );
                }
                state.writing = false;
                shared.changed.notify_all();
                state = shared.wait(state);
            };
            state.writing = true;
            // Room for a line, for one that waits.
            shared.changed.notify_all();
            line
        };
        let _ = out.write_all(line.as_bytes()).and_then(|()| out.flush());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    use super::*;

    /// A writer whose writes wait until it's opened: a stderr nobody reads, until someone does.
    #[derive(Clone, Default)]
    struct Stalled(Arc<(Mutex<Out>, Condvar)>);

    #[derive(Default)]
    struct Out {
        open: bool,
        /// Whether a write has begun.
        asked: bool,
        written: Vec<u8>,
    }

    impl Stalled {
        fn open(&self) {
            let (out, changed) = &*self.0;
            out.lock().unwrap().open = true;
            changed.notify_all();
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

    /// Waits, 10 s at most, for `thread` to end.
    fn joined(thread: std::thread::JoinHandle<()>) {
        let until = Instant::now() + Duration::from_secs(10);
        while !thread.is_finished() {
            assert!(Instant::now() < until, "still waiting after 10 s");
            std::thread::sleep(Duration::from_millis(10));
        }
        thread.join().unwrap();
    }

    #[test]
    fn a_line_waits_for_room_while_serving_and_is_dropped_and_counted_once_stopping() {
        // #359 (Task 7's review, Important 1): a stderr that takes no more holds a line that
        // finds the queue full while the server serves, as a write to it would, and none once
        // it's stopping.
        let out = Stalled::default();
        let stderr = Stderr::to(2, out.clone()).unwrap();
        stderr.send("a\n".into());
        // The writer has taken it, and waits in its write.
        out.until(|out| out.asked);
        stderr.send("b\n".into());
        stderr.send("c\n".into());
        let waiting = {
            let stderr = stderr.clone();
            std::thread::spawn(move || stderr.send("d\n".into()))
        };
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            !waiting.is_finished(),
            "a line waits for room while serving"
        );
        stderr.stopping();
        joined(waiting);
        assert_eq!(stderr.dropped(), 1);
        // From now on a line never waits: one the queue has no room for is dropped at once.
        stderr.send("e\n".into());
        assert_eq!(stderr.dropped(), 2);
        // Nor does the exit, past its grace.
        let began = Instant::now();
        stderr.finish(Duration::from_millis(200));
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "{:?}",
            began.elapsed()
        );
        // Read again, it gets what was queued, then how many lines were skipped.
        out.open();
        let skipped = "… 2 lines of the log skipped, as stderr took no more\n";
        out.until(|out| out.written.ends_with(skipped.as_bytes()));
        assert_eq!(out.written(), format!("a\nb\nc\n{skipped}"));
    }

    #[test]
    fn every_line_is_written_in_order_and_the_exit_waits_for_them() {
        // While the server serves nothing is dropped, and the exit waits for what's queued.
        let out = Stalled::default();
        out.open();
        let stderr = Stderr::to(4, out.clone()).unwrap();
        let lines: Vec<String> = (0..100).map(|n| format!("line {n}\n")).collect();
        for line in &lines {
            stderr.send(line.clone());
        }
        stderr.finish(Duration::from_secs(10));
        assert_eq!(out.written(), lines.concat());
        assert_eq!(stderr.dropped(), 0);
    }
}
