//! The world's thread (Fenji design spec §6.4, §6.7). It ticks at `tick_hz`, takes in what arrived
//! since the last tick, and keeps the last minute of tick times. A tick over its budget is logged
//! with its slowest phase, at most once every 10 seconds. A tick that panics ends the world, and
//! with it the server (Ruling R9), and so does one that makes no progress for [`HUNG`], which a
//! watch outside the thread notices (#281).
use std::any::Any;
use std::collections::VecDeque;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::anyhow;
use fenji_store::Store;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinError;

use super::ecs::{Abandoned, Rules, WorldCore};
use super::types::{ConnId, KickReason, Service, WorldIn, WorldOut};
use crate::clock::Clock;
use crate::invite::InviteBase;

/// How long stopping waits for the last saves (Fenji design spec §6.7: 20 characters in 2 s).
pub const SAVE_WAIT: Duration = Duration::from_secs(5);

/// How long stopping waits for the world's thread to end after its shutdown tick: [`SAVE_WAIT`]
/// and a second. A thread still running then has a tick that hangs.
pub const THREAD_WAIT: Duration = Duration::from_secs(SAVE_WAIT.as_secs() + 1);

/// How long the world's tick count may stand still while the server serves before the world is
/// taken for hung, and the server stops (#281). The world ticks at least once a second, and a
/// tick's work takes far less (the 20-bot load test's slowest took 2.3 ms; world.md §13), so only
/// a tick that never returns comes near it. A pause of the whole process isn't one ([`TickWatch`]).
pub const HUNG: Duration = Duration::from_secs(30);

/// How many times the watch reads the tick count over [`HUNG`]: once a second.
const SAMPLES: u32 = 30;

/// How the world failed while the server served.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// Its thread ended: a tick panicked, and the world let everyone go (Ruling R9).
    Thread,
    /// Its services task ended, so its saves have nowhere to go (Ruling R13).
    Services,
    /// Its tick count stood still this long: a tick hangs (#281).
    Hung(Duration),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Thread => f.write_str("the world's thread has ended"),
            Self::Services => f.write_str("the world's services task has ended"),
            Self::Hung(still) => write!(f, "the world's tick has made no progress for {still:?}"),
        }
    }
}

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
    /// How the services task ended, once [`RunningWorld::failed`] has seen it end before the world
    /// (Ruling R13).
    services_ended: Option<Result<(), JoinError>>,
    stats: Arc<Mutex<Stats>>,
    /// How long the tick count may stand still before the world is taken for hung: [`HUNG`].
    hang: Duration,
}

/// Starts the world's thread and its services task, on the current tokio runtime. `invite_base`
/// and `clock` are for admins' `!invite`. The world sends the sorted names of
/// its characters on `names` when one enters or leaves, and greets each player with the MOTD
/// `motd` holds (`Status::names_sender` and `Status::watch_motd`; M4 hosting spec §3.8, §8 HA1).
pub fn start(
    rules: Rules,
    store: Store,
    invite_base: Option<InviteBase>,
    clock: Arc<dyn Clock>,
    names: watch::Sender<Arc<[String]>>,
    motd: watch::Receiver<String>,
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
    launch(rules, (tx, rx), services_tx, services, names, motd)
}

/// The world's channel: the handles' end, and the thread's.
type Inbox = (
    mpsc::UnboundedSender<WorldIn>,
    mpsc::UnboundedReceiver<WorldIn>,
);

/// Starts the world's thread on `inbox`, with `services` as the task that takes its work, `names`
/// where it sends who is in it, and `motd` the MOTD it greets players with.
fn launch(
    rules: Rules,
    (tx, rx): Inbox,
    services_tx: mpsc::UnboundedSender<Service>,
    services: tokio::task::JoinHandle<()>,
    names: watch::Sender<Arc<[String]>>,
    motd: watch::Receiver<String>,
) -> anyhow::Result<RunningWorld> {
    let hz = rules.tick_hz.max(1);
    let tick = Duration::from_secs(1) / hz;
    let stats = Arc::new(Mutex::new(Stats::new(hz as usize * 60)));
    let core = WorldCore::new(rules, services_tx, names, motd);
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
        services_ended: None,
        stats,
        hang: HUNG,
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

    /// The services task, for a test to end it early (Ruling R13).
    #[cfg(test)]
    pub(crate) fn services_task(&self) -> tokio::task::AbortHandle {
        self.services.abort_handle()
    }

    /// Takes the world for hung once its tick count has stood still for `still`, rather than
    /// [`HUNG`] (tests).
    #[cfg(test)]
    pub(crate) fn hang_after(&mut self, still: Duration) {
        self.hang = still;
    }

    /// Resolves once the world fails, and says how. Before [`RunningWorld::stop`] asks it to end,
    /// each of these is a failure: its thread ends, as when a tick panicked and the world let
    /// everyone go (Ruling R9); its services task ends, so its saves have nowhere to go (Ruling
    /// R13); or its tick count stands still for [`HUNG`], since a tick hangs (#281). The watch over
    /// the tick count runs here, off the world's thread, for as long as this is awaited.
    pub async fn failed(&mut self) -> Failure {
        tokio::select! {
            biased;
            // Nothing is sent: `changed` fails once the thread has dropped its end.
            _ = self.ended.changed() => Failure::Thread,
            done = &mut self.services, if self.services_ended.is_none() => {
                let failed = done.is_err();
                self.services_ended = Some(done);
                // It returns only once the world has dropped its end of their channel, as the
                // world's thread ends: that end is the one to report.
                if failed {
                    Failure::Services
                } else {
                    Failure::Thread
                }
            }
            () = hung(&self.stats, self.hang) => Failure::Hung(self.hang),
        }
    }

    /// Saves everyone and stops: the thread ends after its shutdown tick (waiting at most
    /// [`THREAD_WAIT`]), and the services task once it has written the last saves (waiting at
    /// most [`SAVE_WAIT`]). Each failure is logged and returned, the first if there are two: a
    /// tick that panicked (Ruling R9) or hangs, or a thread that failed; or saves lost to a
    /// services task that failed, before the world (Ruling R13) or after, or didn't finish in
    /// time (#222). The server's shutdown passes it on, so `fenji-server run` exits non-zero.
    pub async fn stop(self) -> anyhow::Result<()> {
        self.stop_within(THREAD_WAIT).await
    }

    /// [`RunningWorld::stop`], waiting at most `wait` for the world's thread to end.
    async fn stop_within(mut self, wait: Duration) -> anyhow::Result<()> {
        self.handle.send(WorldIn::Shutdown);
        // A thread whose tick hangs is left behind, never joined: the process can exit without
        // it, but not without a join that waits for it.
        if tokio::time::timeout(wait, self.ended.changed())
            .await
            .is_err()
        {
            tracing::error!(
                "the world's thread didn't end within {wait:?}: a tick hangs, and what it held \
                 is lost"
            );
            return Err(anyhow!(
                "the world's thread didn't end within {wait:?}: a tick hangs"
            ));
        }
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
        let saved = match self.services_ended {
            // It wrote what was queued, then returned once the world had dropped its end.
            Some(Ok(())) => Ok(()),
            // Every save since it failed went nowhere (Ruling R13).
            Some(Err(e)) => {
                tracing::error!(error = %e, "the world's services task failed before the world");
                Err(anyhow!(
                    "the world's services task failed before the world, and saves were lost: {e}"
                ))
            }
            None => Self::saved(self.services).await,
        };
        ended.and(saved)
    }

    /// Waits at most [`SAVE_WAIT`] for the services task to write the last saves and end.
    async fn saved(services: tokio::task::JoinHandle<()>) -> anyhow::Result<()> {
        match tokio::time::timeout(SAVE_WAIT, services).await {
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
        }
    }
}

/// Resolves once the tick count in `stats` has stood still for `hang` (#281), as [`TickWatch`]
/// counts it, reading it [`SAMPLES`] times over that time.
async fn hung(stats: &Mutex<Stats>, hang: Duration) {
    let count = || stats.lock().unwrap_or_else(|e| e.into_inner()).ticks;
    let every = (hang / SAMPLES).max(Duration::from_millis(1));
    let mut watch = TickWatch::new(count(), Instant::now(), every, hang);
    let mut readings = tokio::time::interval_at(tokio::time::Instant::now() + every, every);
    readings.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        readings.tick().await;
        if watch.hung(count(), Instant::now()) {
            return;
        }
    }
}

/// The watch over the world's tick count (#281): how long the count has stood still, as the watch
/// saw it. A reading that comes more than two periods after the last counts as two: the whole
/// process was paused (stopped, or a VM paused while the monotonic clock ran on), the world's
/// thread with it, and that's no hang. A runtime too busy to read on time only makes it slower to
/// notice.
struct TickWatch {
    /// How often it reads the count.
    every: Duration,
    /// How long the count may stand still.
    hang: Duration,
    /// The count at the last reading, and when that was.
    ticks: u64,
    at: Instant,
    /// How long the count has stood still.
    still: Duration,
}

impl TickWatch {
    fn new(ticks: u64, at: Instant, every: Duration, hang: Duration) -> Self {
        Self {
            every,
            hang,
            ticks,
            at,
            still: Duration::ZERO,
        }
    }

    /// Takes the count `ticks`, read at `now`: true once it has stood still for the watch's
    /// `hang`.
    fn hung(&mut self, ticks: u64, now: Instant) -> bool {
        let since = now.saturating_duration_since(self.at).min(self.every * 2);
        self.at = now;
        if ticks != self.ticks {
            self.ticks = ticks;
            self.still = Duration::ZERO;
            return false;
        }
        self.still += since;
        self.still >= self.hang
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
        let sent = Sent::of(&inputs);
        let began = Instant::now();
        let now_ms = millis(began.duration_since(start));
        let phases = match catch_unwind(AssertUnwindSafe(|| core.step(now_ms, inputs))) {
            Ok(phases) => phases,
            Err(panic) => return Err(abandon(core, &mut rx, &sent, panic)),
        };
        drop(sent);
        let work = began.elapsed();
        stats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .record(work, &phases, tick);
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

fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// What a tick that panics still needs of its inputs, which the panic drops: the channels of the
/// links joining in it, which hear why all the same, and who sent the inputs, for the log.
#[derive(Default)]
struct Sent {
    joining: Vec<mpsc::Sender<WorldOut>>,
    /// The connections whose clients sent inputs, as the world knows them.
    conns: Vec<ConnId>,
    /// The accounts joining, which may have no connection in the world yet.
    accounts: Vec<i64>,
}

impl Sent {
    fn of(inputs: &[WorldIn]) -> Self {
        let mut sent = Self::default();
        for input in inputs {
            match input {
                WorldIn::Join { player, out } => {
                    sent.joining.push(out.clone());
                    sent.accounts.push(player.account);
                }
                WorldIn::Loaded { conn }
                | WorldIn::Pull { conn, .. }
                | WorldIn::Move { conn, .. }
                | WorldIn::Chat { conn, .. }
                | WorldIn::Leave { conn } => sent.conns.push(*conn),
                // Not from a client.
                WorldIn::Tell { .. }
                | WorldIn::Console { .. }
                | WorldIn::SetAdmin { .. }
                | WorldIn::Ban { .. }
                | WorldIn::Shutdown => {}
                #[cfg(test)]
                WorldIn::Panic(_) | WorldIn::Stall { .. } => {}
            }
        }
        sent
    }
}

/// Ends the world after a tick that panicked (Ruling R9). The world saves everyone it can and
/// lets every link go with the shutdown's reason, those joining in that tick too, and takes
/// nothing more. Then it logs the panic's message as an error, with the phase the tick was in
/// and the accounts that sent its inputs, for a host to trace a crash loop. Returns the message,
/// which the server stops with.
fn abandon(
    mut core: WorldCore,
    rx: &mut mpsc::UnboundedReceiver<WorldIn>,
    sent: &Sent,
    panic: Box<dyn Any + Send>,
) -> String {
    let message = message(panic.as_ref());
    let phase = core.running().unwrap_or("none");
    let done = match catch_unwind(AssertUnwindSafe(|| core.abandon(&sent.conns))) {
        Ok(done) => done,
        Err(_) => {
            tracing::error!(
                "saving everyone after the tick's panic panicked too: places since the last \
                 save are lost"
            );
            Abandoned::default()
        }
    };
    for out in &sent.joining {
        if !done.told.iter().any(|t| t.same_channel(out)) {
            let _ = out.try_send(WorldOut::Kicked(KickReason::Shutdown));
        }
    }
    let mut accounts = done.accounts;
    accounts.extend(&sent.accounts);
    accounts.sort_unstable();
    accounts.dedup();
    tracing::error!(
        panic = %message,
        phase = %phase,
        ?accounts,
        "a tick of the world panicked: everyone was saved and let go, and the server is stopping"
    );
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

/// Takes nothing more once the world has stopped: a link's later join fails to send, and it
/// answers 270. A join that came before, after the last tick, hears the shutdown, as everyone in
/// the world did (#219); the rest is dropped. Never call it from an async task: it blocks.
fn turn_away(rx: &mut mpsc::UnboundedReceiver<WorldIn>) {
    rx.close();
    // Once closed, this ends when every message sent before has been read, one that a sender was
    // still writing included, so no join is left without an answer.
    while let Some(msg) = rx.blocking_recv() {
        if let WorldIn::Join { out, .. } = msg {
            let _ = out.try_send(WorldOut::Kicked(KickReason::Shutdown));
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::oneshot;

    use super::super::ecs::test_util::*;
    use super::super::types::ChatOut;
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    /// A world on its own thread, whose services task takes its work and drops it.
    fn world(rules: Rules) -> RunningWorld {
        let (services_tx, mut jobs) = mpsc::unbounded_channel();
        let services = tokio::spawn(async move { while jobs.recv().await.is_some() {} });
        let channels = mpsc::unbounded_channel();
        launch(
            rules,
            channels,
            services_tx,
            services,
            unread_names(),
            no_motd(),
        )
        .unwrap()
    }

    /// The next thing the world tells `link` (5 s at most).
    async fn told(link: &mut mpsc::Receiver<WorldOut>) -> WorldOut {
        tokio::time::timeout(Duration::from_secs(5), link.recv())
            .await
            .expect("within 5 s")
            .expect("the channel is open")
    }

    /// Holds the world's next tick for `work`, and returns once it has begun.
    async fn stall(world: &RunningWorld, work: Duration) {
        let (begun, working) = oneshot::channel();
        world.handle().send(WorldIn::Stall { work, begun });
        tokio::time::timeout(Duration::from_secs(5), working)
            .await
            .expect("the tick begins")
            .unwrap();
    }

    #[tokio::test]
    async fn a_tick_slowed_by_its_own_work_gives_back_no_more_than_one_on_time() {
        // Ruling R10, through the world's own loop: each tick gives a connection back the limit,
        // 8 here, however long it took, so a slow tick buys a flood nothing. At 5 Hz the allowance
        // holds 25 ticks' worth, 200.
        let world = world(Rules {
            tick_hz: 5,
            ..rules()
        });
        let (out, mut link) = mpsc::channel(64);
        let player = player(1, "Lin", at(0.0, 0.0));
        world.handle().send(WorldIn::Join { player, out });
        world.handle().send(WorldIn::Loaded { conn: 1 });
        assert!(matches!(told(&mut link).await, WorldOut::LoadScene { .. }));
        assert!(matches!(told(&mut link).await, WorldOut::SelfEnter { .. }));
        // While a tick is held, the whole allowance, a line to show it was spent without a kick,
        // and a tick whose work takes a second: the next tick takes them all, in that order. The
        // hold is a second too, ample for a busy machine to send them all within it.
        stall(&world, MS * 1_000).await;
        for _ in 0..200 {
            world.handle().send(WorldIn::Pull { conn: 1, pid: 99 });
        }
        let text = String::from("spent");
        world.handle().send(WorldIn::Tell { conn: 1, text });
        stall(&world, MS * 1_000).await;
        let spent = WorldOut::Chat(ChatOut::System {
            text: "spent".into(),
        });
        assert_eq!(told(&mut link).await, spent);
        // Just over a tick's worth, sent during that work: the tick after it takes them.
        for _ in 0..9 {
            world.handle().send(WorldIn::Pull { conn: 1, pid: 99 });
        }
        assert_eq!(told(&mut link).await, WorldOut::Kicked(KickReason::Flood));
        world.stop().await.unwrap();
    }

    #[test]
    fn a_join_after_the_shutdown_hears_the_shutdown() {
        // #219: a link whose join reaches the world after its shutdown tick closes with the
        // shutdown's reason too, not as a world that let go without one.
        let (tx, rx) = mpsc::unbounded_channel();
        let (services, _saves) = mpsc::unbounded_channel();
        let core = WorldCore::new(rules(), services, unread_names(), no_motd());
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
        let core = WorldCore::new(rules(), services, unread_names(), no_motd());
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
        let ren_joins = player(3, "Ren", at(3.0, 0.0));
        tx.send(WorldIn::Join {
            player: ren_joins,
            out,
        })
        .unwrap();
        // A shutdown ends a tick's inputs, so Kai's join reaches the world after that tick.
        tx.send(WorldIn::Shutdown).unwrap();
        let (out, mut kai) = mpsc::channel(8);
        let kai_joins = player(4, "Kai", at(4.0, 0.0));
        tx.send(WorldIn::Join {
            player: kai_joins,
            out,
        })
        .unwrap();
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
        for late in [&mut ren, &mut kai] {
            assert_eq!(late.try_recv(), Ok(WorldOut::Kicked(KickReason::Shutdown)));
            assert!(late.try_recv().is_err(), "told once");
        }
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
    }

    #[test]
    fn a_panicking_tick_names_its_phase_and_the_accounts_that_sent_its_inputs() {
        // For a host tracing a crash loop: where the tick was, and whose inputs it held. Ren
        // (account 3) and Kai (4) entered in an earlier tick; in the one that panics, only Kai
        // sends anything, and Lin (1) joins.
        let (tx, rx) = mpsc::unbounded_channel();
        let (services, _saves) = mpsc::unbounded_channel();
        let mut core = WorldCore::new(rules(), services, unread_names(), no_motd());
        let (ren, _ren) = mpsc::channel(8);
        let (kai, _kai) = mpsc::channel(8);
        let earlier = vec![
            WorldIn::Join {
                player: player(3, "Ren", at(0.0, 0.0)),
                out: ren,
            },
            WorldIn::Join {
                player: player(4, "Kai", at(0.0, 0.0)),
                out: kai,
            },
        ];
        core.step(0, earlier);
        let (lin, _lin) = mpsc::channel(8);
        let lin_joins = player(1, "Lin", at(0.0, 0.0));
        tx.send(WorldIn::Join {
            player: lin_joins,
            out: lin,
        })
        .unwrap();
        tx.send(WorldIn::Loaded { conn: 4 }).unwrap();
        tx.send(WorldIn::Panic("a bug in a tick")).unwrap();
        let (_, log) = logged(|| run(core, rx, MS * 50, &Mutex::new(Stats::new(10))));
        let line = log
            .lines()
            .find(|l| l.contains("ERROR") && l.contains("a bug in a tick"))
            .unwrap_or_else(|| panic!("no error line: {log}"));
        assert!(line.contains("phase=inbound"), "{line}");
        assert!(line.contains("accounts=[1, 4]"), "{line}");
    }

    #[tokio::test]
    async fn a_join_after_a_panicking_tick_is_refused_or_hears_the_shutdown() {
        // Ruling R9: once a tick has panicked, a link that joins either can't (the world's channel
        // is closed, and it answers 270) or hears Kicked(Shutdown): never a world that took its
        // join and let go of it without a word.
        let mut world = world(rules());
        let (out, mut lin) = mpsc::channel(8);
        let lin_joins = player(1, "Lin", at(0.0, 0.0));
        world.handle().send(WorldIn::Join {
            player: lin_joins,
            out,
        });
        assert!(matches!(told(&mut lin).await, WorldOut::LoadScene { .. }));
        world.handle().send(WorldIn::Panic("a bug in a tick"));
        assert_eq!(told(&mut lin).await, WorldOut::Kicked(KickReason::Shutdown));
        let (out, mut mai) = mpsc::channel(8);
        let mai_joins = player(2, "Mai", at(0.0, 0.0));
        if world.handle().send(WorldIn::Join {
            player: mai_joins,
            out,
        }) {
            assert_eq!(told(&mut mai).await, WorldOut::Kicked(KickReason::Shutdown));
        }
        let failed = tokio::time::timeout(Duration::from_secs(5), world.failed()).await;
        assert_eq!(failed, Ok(Failure::Thread));
        assert!(world.stop().await.is_err());
    }

    #[tokio::test]
    async fn a_world_whose_tick_hangs_fails_and_one_that_ticks_doesnt() {
        // #281: a tick that never returns left a world that took everything in and answered
        // nothing, without a word. The watch, outside the world's thread, takes a tick count that
        // stands still for its time, here a second, for a hang; the world ticks every 50 ms.
        let mut world = world(rules());
        world.hang_after(MS * 1_000);
        let ticking = tokio::time::timeout(MS * 2_500, world.failed()).await;
        assert!(ticking.is_err(), "a world that ticks failed: {ticking:?}");
        let (begun, working) = oneshot::channel();
        let work = Duration::from_secs(12);
        world.handle().send(WorldIn::Stall { work, begun });
        tokio::time::timeout(Duration::from_secs(5), working)
            .await
            .expect("the tick begins")
            .unwrap();
        let began = std::time::Instant::now();
        let failed = tokio::time::timeout(Duration::from_secs(5), world.failed()).await;
        assert_eq!(failed, Ok(Failure::Hung(MS * 1_000)));
        let took = began.elapsed();
        assert!(took >= MS * 1_000, "noticed early: {took:?}");
        // The tick still hangs: stopping gives up on it.
        assert!(world.stop_within(MS * 100).await.is_err());
    }

    #[test]
    fn a_count_that_stands_still_for_the_watchs_time_is_a_hang() {
        // #281: read once a second, a count that stands still for 30 s is a hang, and one that
        // moves starts the time again.
        let (every, hang) = (MS * 1_000, MS * 30_000);
        let t = Instant::now();
        let mut w = TickWatch::new(7, t, every, hang);
        for s in 1..30 {
            assert!(!w.hung(7, t + every * s), "{s} s");
        }
        assert!(w.hung(7, t + every * 30), "30 s");
        let mut w = TickWatch::new(7, t, every, hang);
        for s in 1..30 {
            assert!(!w.hung(7, t + every * s), "{s} s");
        }
        assert!(!w.hung(8, t + every * 30), "a tick at 30 s");
        for s in 31..60 {
            assert!(!w.hung(8, t + every * s), "{s} s");
        }
        assert!(w.hung(8, t + every * 60), "30 s after that tick");
    }

    #[test]
    fn a_pause_of_the_whole_process_is_no_hang() {
        // A reading that comes late counts as two periods: the process was stopped, or its VM
        // paused while the monotonic clock ran on, and the world's thread with it. A minute's
        // pause, then a tick, is no hang.
        let (every, hang) = (MS * 1_000, MS * 30_000);
        let t = Instant::now();
        let mut w = TickWatch::new(7, t, every, hang);
        assert!(!w.hung(7, t + every * 60), "a minute's pause");
        assert!(!w.hung(8, t + every * 61), "then a tick");
        // Readings that keep coming late, as from a busy runtime, still add up, two periods each:
        // 14 readings 5 s apart are 28 s, and the 15th is 30.
        let mut w = TickWatch::new(7, t, every, hang);
        for n in 1..15 {
            assert!(!w.hung(7, t + every * 5 * n), "reading {n}");
        }
        assert!(w.hung(7, t + every * 75), "reading 15");
        // A reading from before the last counts nothing.
        let mut w = TickWatch::new(7, t + every * 10, every, hang);
        for _ in 0..40 {
            assert!(!w.hung(7, t));
        }
    }

    #[tokio::test]
    async fn a_tick_that_hangs_fails_the_stop_within_its_bound() {
        // A tick that never returns mustn't hang the shutdown: stopping gives up on the thread,
        // which the process can exit without, and fails.
        let world = world(rules());
        let (begun, working) = oneshot::channel();
        let work = Duration::from_secs(12);
        world.handle().send(WorldIn::Stall { work, begun });
        tokio::time::timeout(Duration::from_secs(5), working)
            .await
            .expect("the tick begins")
            .unwrap();
        let began = std::time::Instant::now();
        let stopped = tokio::time::timeout(Duration::from_secs(8), world.stop_within(MS * 300))
            .await
            .expect("stopping gives up on the thread");
        let e = stopped.expect_err("a tick that hangs fails the stop");
        assert!(format!("{e:#}").contains("hangs"), "{e:#}");
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "{:?}",
            began.elapsed()
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
        let channels = mpsc::unbounded_channel();
        let world = launch(
            rules(),
            channels,
            services_tx,
            services,
            unread_names(),
            no_motd(),
        )
        .unwrap();
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

    #[tokio::test]
    async fn a_services_task_that_ends_well_names_the_worlds_thread_and_loses_no_saves() {
        // #222's fix of the name: the services task returns only once the world's thread has
        // dropped its end of their channel, so a services task that ends well, seen before the
        // thread's end, names the thread, and claims no lost saves. Which end the runtime sees
        // first is its own choice when the thread ends; here the services task's comes first
        // every time, from a stand-in that returns at once while the world still ticks.
        let (services_tx, _jobs) = mpsc::unbounded_channel();
        let services = tokio::spawn(async {});
        let channels = mpsc::unbounded_channel();
        let mut world = launch(
            rules(),
            channels,
            services_tx,
            services,
            unread_names(),
            no_motd(),
        )
        .unwrap();
        let failed = tokio::time::timeout(Duration::from_secs(5), world.failed()).await;
        assert_eq!(failed, Ok(Failure::Thread));
        let stopped = tokio::time::timeout(Duration::from_secs(10), world.stop())
            .await
            .expect("the world stops");
        assert!(stopped.is_ok(), "no saves were lost: {stopped:?}");
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
