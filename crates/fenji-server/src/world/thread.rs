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
    /// How the services task ended, once [`RunningWorld::ended`] has seen it end before the world
    /// (Ruling R13).
    services_ended: Option<Result<(), JoinError>>,
    stats: Arc<Mutex<Stats>>,
}

/// Starts the world's thread and its services task, on the current tokio runtime. `invite_base`
/// and `clock` are for `!invite` and the console's `invite`. The world sends the sorted names of
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

    /// Resolves once the world's thread or its services task has ended, and says which. Before
    /// [`RunningWorld::stop`] asks them to, either is a failure: a tick panicked and the world has
    /// let everyone go (Ruling R9), or the world's saves have nowhere to go (Ruling R13).
    pub async fn ended(&mut self) -> &'static str {
        tokio::select! {
            biased;
            // Nothing is sent: `changed` fails once the thread has dropped its end.
            _ = self.ended.changed() => "the world's thread",
            done = &mut self.services, if self.services_ended.is_none() => {
                let failed = done.is_err();
                self.services_ended = Some(done);
                // It returns only once the world has dropped its end of their channel, as the
                // world's thread ends: that end is the one to report.
                if failed {
                    "the world's services task"
                } else {
                    "the world's thread"
                }
            }
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
    // When the last tick's work ended.
    let mut ended = None;
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
        let gap_ms = gap(ended, began);
        let phases = match catch_unwind(AssertUnwindSafe(|| core.step(now_ms, gap_ms, inputs))) {
            Ok(phases) => phases,
            Err(panic) => return Err(abandon(core, &mut rx, &sent, panic)),
        };
        drop(sent);
        let work = began.elapsed();
        ended = Some(began + work);
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

/// Milliseconds from the end of the last tick's work, `ended`, to a tick that began at `began`:
/// what the flood check takes as a stall. It is 0 for the first tick, and for one that starts as
/// the last one ends because that one's work ran long: a tick slowed by its own inputs must earn
/// the next no more allowance, or a flood would buy itself more tick after tick (Ruling R10).
fn gap(ended: Option<Instant>, began: Instant) -> u64 {
    ended.map_or(0, |ended| millis(began.saturating_duration_since(ended)))
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

    #[tokio::test]
    async fn a_tick_slowed_by_its_own_work_earns_the_next_no_allowance() {
        // Ruling R10, through the world's own loop: the gap that scales the flood check runs from
        // the end of a tick's work, so a slow tick leaves none. At 5 Hz a period is 200 ms, with
        // 8 inputs.
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
        // A tick whose work takes a second: five periods, which as a gap would allow 40.
        let (begun, working) = oneshot::channel();
        let work = MS * 1_000;
        world.handle().send(WorldIn::Stall { work, begun });
        tokio::time::timeout(Duration::from_secs(5), working)
            .await
            .expect("the slow tick begins")
            .unwrap();
        // Just over a period's worth, sent during that work: the tick after it takes them.
        for _ in 0..9 {
            world.handle().send(WorldIn::Pull { conn: 1, pid: 99 });
        }
        assert_eq!(told(&mut link).await, WorldOut::Kicked(KickReason::Flood));
        world.stop().await.unwrap();
    }

    #[test]
    fn the_gap_runs_from_the_end_of_the_last_ticks_work() {
        // Ruling R10: the flood check forgives a stall between ticks, never a tick's own work.
        let t = Instant::now();
        assert_eq!(gap(None, t), 0, "the first tick");
        // A tick that worked 500 ms, then the next as soon as it ended: no gap, though the next
        // began 500 ms after the last one did.
        assert_eq!(gap(Some(t + MS * 500), t + MS * 500), 0);
        // A tick that worked 2 ms, then a pause: the gap is the pause.
        assert_eq!(gap(Some(t + MS * 2), t + MS * 2_002), 2_000);
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
            Service::CreateInvite { .. } | Service::CreateInviteFor { .. } => 0,
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
        core.step(0, 0, earlier);
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
        let ended = tokio::time::timeout(Duration::from_secs(5), world.ended()).await;
        assert_eq!(ended, Ok("the world's thread"));
        assert!(world.stop().await.is_err());
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
