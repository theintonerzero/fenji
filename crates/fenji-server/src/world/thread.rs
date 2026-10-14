//! The world's thread (Fenji design spec §6.4, §6.7). It ticks at `tick_hz`, takes in what arrived
//! since the last tick, and keeps the last minute of tick times. A tick over its budget is logged
//! with its slowest phase, at most once every 10 seconds. A tick that panics ends the world, and
//! with it the server (Ruling R9).
use std::any::Any;
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::anyhow;
use fenji_store::Store;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::{mpsc, watch};

use super::ecs::{Rules, WorldCore};
use super::types::{KickReason, Service, WorldIn, WorldOut};
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

    /// A handle whose other end the caller holds, in a running world's place (tests).
    #[cfg(test)]
    pub(crate) fn fake() -> (Self, mpsc::UnboundedReceiver<WorldIn>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self(tx), rx)
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
    /// The thread: it ends with the message of a tick that panicked (Ruling R9).
    thread: std::thread::JoinHandle<Result<(), String>>,
    /// Closed when the thread ends, however it ends; nothing is sent on it.
    ended: watch::Receiver<()>,
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
    let services = tokio::spawn(super::services::run(
        store,
        invite_base,
        clock,
        services_rx,
        tx.downgrade(),
    ));
    launch(rules, (tx, rx), services_tx, services)
}

/// The world's channel: the handles' end, and the thread's.
type Inbox = (
    mpsc::UnboundedSender<WorldIn>,
    mpsc::UnboundedReceiver<WorldIn>,
);

/// Starts the world's thread on `inbox`, with `services` as the task that takes its work.
fn launch(
    rules: Rules,
    (tx, rx): Inbox,
    services_tx: mpsc::UnboundedSender<Service>,
    services: tokio::task::JoinHandle<()>,
) -> anyhow::Result<RunningWorld> {
    let hz = rules.tick_hz.max(1);
    let tick = Duration::from_secs(1) / hz;
    let stats = Arc::new(Mutex::new(Stats::new(hz as usize * 60)));
    let core = WorldCore::new(rules, services_tx);
    let (closes, ended) = watch::channel(());
    let thread = std::thread::Builder::new()
        .name("fenji-world".into())
        .spawn({
            let stats = Arc::clone(&stats);
            move || {
                let _ended = closes;
                run(core, rx, tick, &stats)
            }
        })?;
    Ok(RunningWorld {
        handle: WorldHandle(tx),
        thread,
        ended,
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

    /// Resolves once the world's thread has ended. Before [`RunningWorld::stop`] asks it to, that
    /// is a failure: a tick panicked, and the world has let everyone go (Ruling R9).
    pub async fn ended(&mut self) {
        // Nothing is sent: `changed` fails once the thread has dropped its end.
        let _ = self.ended.changed().await;
    }

    /// Saves everyone and stops: the thread ends after its shutdown tick, and the services task
    /// once it has written the last saves (waiting at most [`SAVE_WAIT`]). Each failure is logged
    /// and returned, the first if there are two: a tick that panicked (Ruling R9) or a thread that
    /// failed, or saves lost to a services task that failed or didn't finish in time (#222). The
    /// server's shutdown passes it on, so `fenji-server run` exits non-zero.
    pub async fn stop(self) -> anyhow::Result<()> {
        self.handle.send(WorldIn::Shutdown);
        let thread = self.thread;
        let ended = match tokio::task::spawn_blocking(move || thread.join()).await {
            Ok(Ok(Ok(()))) => Ok(()),
            // The thread has logged it.
            Ok(Ok(Err(panicked))) => Err(anyhow!("a tick of the world panicked: {panicked}")),
            _ => {
                tracing::error!("the world's thread panicked");
                Err(anyhow!("the world's thread panicked"))
            }
        };
        let saved = match tokio::time::timeout(SAVE_WAIT, self.services).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => {
                tracing::error!(error = %e, "the world's services task failed: saves were lost");
                Err(anyhow!(
                    "the world's services task failed, and saves were lost: {e}"
                ))
            }
            Err(_) => {
                tracing::error!("the world's last saves took over {SAVE_WAIT:?}");
                Err(anyhow!("the world's last saves took over {SAVE_WAIT:?}"))
            }
        };
        ended.and(saved)
    }
}

/// The tick loop. It ends after the tick that takes `Shutdown`, which comes when the server stops
/// or when every handle has gone, or with the message of a tick that panicked: then the world
/// saves everyone it can and lets them go first (Ruling R9).
fn run(
    mut core: WorldCore,
    mut rx: mpsc::UnboundedReceiver<WorldIn>,
    tick: Duration,
    stats: &Mutex<Stats>,
) -> Result<(), String> {
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
        let joining = joining(&inputs);
        let began = Instant::now();
        let now_ms = u64::try_from(began.duration_since(start).as_millis()).unwrap_or(u64::MAX);
        let phases = match catch_unwind(AssertUnwindSafe(|| core.step(now_ms, inputs))) {
            Ok(phases) => phases,
            Err(panic) => return Err(abandon(core, &mut rx, &joining, panic)),
        };
        stats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .record(began.elapsed(), &phases, tick);
        if stopping {
            turn_away(&mut rx);
            return Ok(());
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

/// The channels of the links joining in a tick: a tick that panics may drop them before their
/// players have actors, and they hear why all the same.
fn joining(inputs: &[WorldIn]) -> Vec<mpsc::Sender<WorldOut>> {
    inputs
        .iter()
        .filter_map(|input| match input {
            WorldIn::Join { out, .. } => Some(out.clone()),
            _ => None,
        })
        .collect()
}

/// Ends the world after a tick that panicked (Ruling R9). The panic's message is logged as an
/// error; the world saves everyone it can and lets every link go with the shutdown's reason, those
/// joining in that tick too, and takes nothing more. Returns the message, which the server stops
/// with.
fn abandon(
    mut core: WorldCore,
    rx: &mut mpsc::UnboundedReceiver<WorldIn>,
    joining: &[mpsc::Sender<WorldOut>],
    panic: Box<dyn Any + Send>,
) -> String {
    let message = message(panic.as_ref());
    tracing::error!(
        panic = %message,
        "a tick of the world panicked: saving everyone, letting them go and stopping the server"
    );
    let told = match catch_unwind(AssertUnwindSafe(|| core.abandon())) {
        Ok(told) => told,
        Err(_) => {
            tracing::error!(
                "saving everyone after the tick's panic panicked too: places since the last \
                 save are lost"
            );
            Vec::new()
        }
    };
    for out in joining {
        if !told.iter().any(|t| t.same_channel(out)) {
            let _ = out.try_send(WorldOut::Kicked(KickReason::Shutdown));
        }
    }
    turn_away(rx);
    message
}

/// A panic's message, as `panic!` makes it.
fn message(panic: &(dyn Any + Send)) -> String {
    if let Some(s) = panic.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = panic.downcast_ref::<String>() {
        s.clone()
    } else {
        "a panic without a message".into()
    }
}

/// Takes nothing more once the world has stopped. A join that came after the last tick hears the
/// shutdown, as everyone in the world did (#219); the rest is dropped.
fn turn_away(rx: &mut mpsc::UnboundedReceiver<WorldIn>) {
    rx.close();
    while let Ok(msg) = rx.try_recv() {
        if let WorldIn::Join { out, .. } = msg {
            let _ = out.try_send(WorldOut::Kicked(KickReason::Shutdown));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::ecs::test_util::*;
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn a_join_after_the_shutdown_hears_the_shutdown() {
        // #219: a link whose join reaches the world after its shutdown tick closes with the
        // shutdown's reason too, not as a world that let go without one.
        let (tx, rx) = mpsc::unbounded_channel();
        let (services, _saves) = mpsc::unbounded_channel();
        let core = WorldCore::new(rules(), services);
        let (out, mut late) = mpsc::channel(4);
        tx.send(WorldIn::Shutdown).unwrap();
        tx.send(WorldIn::Join {
            player: player(1, "Lin", at(0.0, 0.0)),
            out,
        })
        .unwrap();
        assert_eq!(run(core, rx, MS * 50, &Mutex::new(Stats::new(10))), Ok(()));
        assert_eq!(late.try_recv(), Ok(WorldOut::Kicked(KickReason::Shutdown)));
    }

    #[test]
    fn a_panicking_tick_saves_everyone_lets_everyone_go_and_ends_the_world() {
        // Ruling R9 (#222): the loop catches a tick's panic, logs its message as an error, saves
        // everyone it can, tells every link Kicked(Shutdown), turns away what comes after, and
        // ends with the message.
        let (tx, rx) = mpsc::unbounded_channel();
        let (services, mut saves) = mpsc::unbounded_channel();
        let core = WorldCore::new(rules(), services);
        let mut links = Vec::new();
        for (conn, name) in [(1, "Lin"), (2, "Mai")] {
            let (out, link) = mpsc::channel(8);
            let player = player(conn, name, at(conn as f64, 0.0));
            tx.send(WorldIn::Join { player, out }).unwrap();
            links.push(link);
        }
        tx.send(WorldIn::Panic("a bug in a tick")).unwrap();
        // Joining in the same tick, after the bug: it never got an actor.
        let (out, mut ren) = mpsc::channel(8);
        let player = player(3, "Ren", at(3.0, 0.0));
        tx.send(WorldIn::Join { player, out }).unwrap();
        let (ended, log) = logged(|| run(core, rx, MS * 50, &Mutex::new(Stats::new(10))));
        assert_eq!(ended, Err("a bug in a tick".to_string()));
        assert!(
            log.lines()
                .any(|l| l.contains("ERROR") && l.contains("a bug in a tick")),
            "{log}"
        );
        for link in &mut links {
            assert!(matches!(link.try_recv(), Ok(WorldOut::LoadScene { .. })));
            assert_eq!(link.try_recv(), Ok(WorldOut::Kicked(KickReason::Shutdown)));
            assert!(link.try_recv().is_err(), "told once");
        }
        assert_eq!(ren.try_recv(), Ok(WorldOut::Kicked(KickReason::Shutdown)));
        assert!(ren.try_recv().is_err(), "told once");
        let mut saved = Vec::new();
        while let Ok(s) = saves.try_recv() {
            saved.push(s);
        }
        saved.sort_by_key(|s| match s {
            Service::SavePlace { pid, .. } => *pid,
            Service::CreateInvite { .. } => 0,
        });
        assert_eq!(
            saved,
            [
                Service::SavePlace {
                    pid: 1,
                    at: at(1.0, 0.0)
                },
                Service::SavePlace {
                    pid: 2,
                    at: at(2.0, 0.0)
                }
            ]
        );
        assert!(
            !WorldHandle(tx).send(WorldIn::Shutdown),
            "the world takes nothing more"
        );
    }

    #[tokio::test]
    async fn stop_reports_a_services_task_that_failed() {
        // #222: a services task that panicked loses every save from then on, here the shutdown's.
        // `stop` says so, and the server's shutdown passes it on to `fenji-server run`.
        let (services_tx, jobs) = mpsc::unbounded_channel();
        let services = tokio::spawn(async move {
            drop(jobs);
            panic!("a bug in the services");
        });
        let world = launch(rules(), mpsc::unbounded_channel(), services_tx, services).unwrap();
        let (out, _rx) = mpsc::channel(4);
        world.handle().send(WorldIn::Join {
            player: player(1, "Lin", at(0.0, 0.0)),
            out,
        });
        let stopped = tokio::time::timeout(Duration::from_secs(10), world.stop())
            .await
            .expect("the world stops");
        let e = stopped.expect_err("the failed services task is reported");
        assert!(format!("{e:#}").contains("services task"), "{e:#}");
    }

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
