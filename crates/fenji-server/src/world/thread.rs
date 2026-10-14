//! The world's thread (Fenji design spec §6.4, §6.7). It ticks at `tick_hz`, takes in what arrived
//! since the last tick, and keeps the last minute of tick times. A tick over its budget is logged
//! with its slowest phase, at most once every 10 seconds.
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fenji_store::Store;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TryRecvError;

use super::ecs::{Rules, WorldCore};
use super::types::WorldIn;
use crate::clock::Clock;

/// How long stopping waits for the last saves (Fenji design spec §6.7: 20 characters in 2 s).
pub const SAVE_WAIT: Duration = Duration::from_secs(5);

/// How connections reach the world. Clones share the channel.
#[derive(Clone)]
pub struct WorldHandle(mpsc::UnboundedSender<WorldIn>);

impl WorldHandle {
    /// Sends `msg` to the world. False once the world has stopped.
    pub fn send(&self, msg: WorldIn) -> bool {
        self.0.send(msg).is_ok()
    }
}

/// Tick work over the last minute.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TickReport {
    /// Ticks since the world started.
    pub ticks: u64,
    pub p50: Duration,
    pub p99: Duration,
    pub max: Duration,
    /// Ticks since the start whose work took longer than a tick.
    pub over_budget: u64,
}

struct Stats {
    window: VecDeque<Duration>,
    size: usize,
    ticks: u64,
    over: u64,
    warned: Option<Instant>,
}

impl Stats {
    fn new(size: usize) -> Self {
        Self {
            window: VecDeque::with_capacity(size),
            size,
            ticks: 0,
            over: 0,
            warned: None,
        }
    }

    fn record(&mut self, work: Duration, phases: &[(&'static str, Duration)], budget: Duration) {
        self.ticks += 1;
        if self.window.len() == self.size {
            self.window.pop_front();
        }
        self.window.push_back(work);
        if work <= budget {
            return;
        }
        self.over += 1;
        if self
            .warned
            .is_some_and(|t| t.elapsed() < Duration::from_secs(10))
        {
            return;
        }
        self.warned = Some(Instant::now());
        let (slowest, took) = phases
            .iter()
            .copied()
            .max_by_key(|(_, d)| *d)
            .unwrap_or(("none", Duration::ZERO));
        tracing::warn!(
            work_ms = work.as_millis() as u64,
            budget_ms = budget.as_millis() as u64,
            slowest,
            slowest_ms = took.as_millis() as u64,
            over_budget = self.over,
            "a world tick ran over its budget"
        );
    }

    fn report(&self) -> TickReport {
        let mut sorted: Vec<Duration> = self.window.iter().copied().collect();
        sorted.sort();
        let at = |q: f64| {
            let i = (sorted.len().saturating_sub(1) as f64 * q).round() as usize;
            sorted.get(i).copied().unwrap_or_default()
        };
        TickReport {
            ticks: self.ticks,
            p50: at(0.5),
            p99: at(0.99),
            max: sorted.last().copied().unwrap_or_default(),
            over_budget: self.over,
        }
    }
}

/// A running world: its thread, and its services task.
pub struct RunningWorld {
    handle: WorldHandle,
    thread: std::thread::JoinHandle<()>,
    services: tokio::task::JoinHandle<()>,
    stats: Arc<Mutex<Stats>>,
}

/// Starts the world's thread and its services task, on the current tokio runtime. `invite_base`
/// and `clock` are for `!invite`.
pub fn start(
    rules: Rules,
    store: Store,
    invite_base: Option<String>,
    clock: Arc<dyn Clock>,
) -> anyhow::Result<RunningWorld> {
    let (tx, rx) = mpsc::unbounded_channel();
    let (services_tx, services_rx) = mpsc::unbounded_channel();
    let hz = rules.tick_hz.max(1);
    let tick = Duration::from_secs(1) / hz;
    let stats = Arc::new(Mutex::new(Stats::new(hz as usize * 60)));
    let core = WorldCore::new(rules, services_tx);
    let thread = std::thread::Builder::new()
        .name("fenji-world".into())
        .spawn({
            let stats = Arc::clone(&stats);
            move || run(core, rx, tick, &stats)
        })?;
    let services = tokio::spawn(super::services::run(
        store,
        invite_base,
        clock,
        services_rx,
        tx.downgrade(),
    ));
    Ok(RunningWorld {
        handle: WorldHandle(tx),
        thread,
        services,
        stats,
    })
}

impl RunningWorld {
    pub fn handle(&self) -> WorldHandle {
        self.handle.clone()
    }

    pub fn report(&self) -> TickReport {
        self.stats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .report()
    }

    /// Saves everyone and stops: the thread ends after its shutdown tick, and the services task
    /// once it has written the last saves (waiting at most [`SAVE_WAIT`]).
    pub async fn stop(self) {
        self.handle.send(WorldIn::Shutdown);
        let thread = self.thread;
        if !matches!(
            tokio::task::spawn_blocking(move || thread.join()).await,
            Ok(Ok(()))
        ) {
            tracing::error!("the world's thread panicked");
        }
        if tokio::time::timeout(SAVE_WAIT, self.services)
            .await
            .is_err()
        {
            tracing::error!("the world's last saves took over {SAVE_WAIT:?}");
        }
    }
}

/// The tick loop. It ends after the tick that takes `Shutdown`, which comes when the server stops
/// or when every handle has gone.
fn run(
    mut core: WorldCore,
    mut rx: mpsc::UnboundedReceiver<WorldIn>,
    tick: Duration,
    stats: &Mutex<Stats>,
) {
    let start = Instant::now();
    let mut next = start;
    loop {
        let mut inputs = Vec::new();
        let mut stopping = false;
        loop {
            match rx.try_recv() {
                Ok(WorldIn::Shutdown) | Err(TryRecvError::Disconnected) => {
                    inputs.push(WorldIn::Shutdown);
                    stopping = true;
                    break;
                }
                Ok(msg) => inputs.push(msg),
                Err(TryRecvError::Empty) => break,
            }
        }
        let began = Instant::now();
        let now_ms = u64::try_from(began.duration_since(start).as_millis()).unwrap_or(u64::MAX);
        let phases = core.step(now_ms, inputs);
        stats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .record(began.elapsed(), &phases, tick);
        if stopping {
            return;
        }
        next += tick;
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else {
            // Behind: start again from now rather than run a burst of ticks to catch up.
            next = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn the_report_takes_the_last_minute_and_counts_ticks_over_budget() {
        let mut s = Stats::new(100);
        for i in 1..=200u32 {
            s.record(MS * i, &[("inbound", MS * i)], MS * 150);
        }
        let r = s.report();
        assert_eq!(r.ticks, 200);
        assert_eq!(r.over_budget, 50, "151 ms to 200 ms");
        // The window holds the last 100: 101 ms to 200 ms.
        assert_eq!(r.max, MS * 200);
        assert_eq!(r.p50, MS * 151);
        assert_eq!(r.p99, MS * 199);
        assert_eq!(Stats::new(10).report(), TickReport::default());
    }
}
