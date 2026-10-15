//! The load harness (M4 world plan, Task 17; Amendment D): twenty protocol bots in one cell for a
//! minute, each moving every 100 ms and saying a line every 5 s, measured against the targets of
//! the Fenji design spec's §6.7: p99 tick work of at most 10 ms with 20 players in the starting
//! zone, and all 20 saved within 2 s of a shutdown. It takes a minute, so it is ignored by default:
//!
//! ```text
//! cargo test --release -p fenji-server --test load -- --ignored --nocapture
//! ```
//!
//! It prints what it measured. With `FENJI_LOAD_ASSERT=1` it also asserts the targets, which are
//! meant for release builds on a host's machine, and that the bots delivered the load: no move
//! refused, and each bot sent at least 95% of the moves and lines it planned.
//!
//! The database goes in a temporary directory under `TMPDIR`, which the output names. Where that
//! is a tmpfs, as `/tmp` is on Fedora, the shutdown's saves never reach a disk, so point `TMPDIR`
//! at a directory on the disk a host's server would use, such as one under `target/`.
mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::*;
use fenji_proto::chat::{self, channel::NEARBY};
use fenji_proto::client::ClientEvent;
use fenji_proto::opcode::Stc;
use fenji_proto::pickle::{self, Value};
use fenji_proto::world;
use fenji_server::gate::SERVER_STOPPING;
use fenji_server::server::GateLimits;
use fenji_store::{Place, Store};
use tokio::time::MissedTickBehavior;

/// The design capacity, and the gate's default `max_players` (design spec §6.7).
const BOTS: usize = 20;
/// How long the bots play.
const PLAY: Duration = Duration::from_secs(60);
/// How often each bot moves (Amendment D).
const MOVE_EVERY: Duration = Duration::from_millis(100);
/// How often each bot says a line; the bots' lines are spread over it by their index.
const SAY_EVERY: Duration = Duration::from_secs(5);
/// A small circle of steps of about 40 units (28² + 28² is about 40²) that ends where it began:
/// each bot walks it around the start, so every move passes the speed rule.
const CIRCLE: [[i32; 2]; 8] = [
    [40, 0],
    [28, 28],
    [0, 40],
    [-28, 28],
    [-40, 0],
    [-28, -28],
    [0, -40],
    [28, -28],
];
/// The moves each bot plans: one every [`MOVE_EVERY`] from [`MOVE_EVERY`] in, the last before
/// [`PLAY`] ends (599).
const PLANNED_MOVES: u64 = (PLAY.as_millis() / MOVE_EVERY.as_millis()) as u64 - 1;
/// The lines each bot plans: one every [`SAY_EVERY`] from its offset, which is under
/// [`SAY_EVERY`], the last before [`PLAY`] ends (12).
const PLANNED_LINES: u64 = (PLAY.as_millis() / SAY_EVERY.as_millis()) as u64;
/// The targets (design spec §6.7).
const TICK_P99: Duration = Duration::from_millis(10);
const SAVE_ALL: Duration = Duration::from_secs(2);
/// The share of its planned moves and lines, in percent, that each bot must have sent for the
/// targets to count: a host that stalls the bots lightens the load the world carries.
const DELIVERED: u64 = 95;
/// How long the last moves may take to reach the others once the bots stop.
const SETTLE: Duration = Duration::from_secs(5);

/// The least a bot may send of `planned` messages: [`DELIVERED`] percent, rounded up.
const fn delivered(planned: u64) -> u64 {
    (planned * DELIVERED).div_ceil(100)
}

/// The name of bot `i`'s character: the Western rule allows letters, but no digits.
fn name(i: usize) -> String {
    format!("Bot{}", char::from(b'A' + i as u8))
}

/// How a bot's connection ended.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Ended {
    /// The server closed it, with this reason.
    Closed(Vec<u8>),
    /// The stream ended, or failed, without a reason.
    Lost,
}

/// What one bot's reader counted.
#[derive(Debug, Default)]
struct Tally {
    /// 13s, by mover.
    moves: HashMap<i64, u64>,
    /// The last 13 from each mover.
    last_move: HashMap<i64, Vec<u8>>,
    /// 378s, by speaker.
    lines: HashMap<i64, u64>,
    /// 20s: moves of its own the world refused.
    corrections: u64,
    /// Anything else, which no bot should get while they play.
    other: Vec<String>,
    /// How the connection ended, once it has.
    ended: Option<Ended>,
}

/// Reads until the connection ends, counting into `tally`. `speakers` names the bots by their
/// links.
async fn listen(mut ears: Reader, tally: Arc<Mutex<Tally>>, speakers: Arc<HashMap<String, i64>>) {
    let ended = loop {
        let Some(event) = ears.next().await else {
            break Ended::Lost;
        };
        let mut t = tally.lock().unwrap();
        match event {
            ClientEvent::Closed(why) => break Ended::Closed(why),
            ClientEvent::Message(m) if m.ty == Stc::MoveDir.to_u8() => {
                let pid = i64::from_le_bytes(m.body[..8].try_into().unwrap());
                *t.moves.entry(pid).or_default() += 1;
                t.last_move.insert(pid, m.body);
            }
            ClientEvent::Message(m) if m.ty == Stc::MoveSyncSvr.to_u8() => t.corrections += 1,
            ClientEvent::Message(m) if m.ty == Stc::Script.to_u8() => {
                let values = pickle::decode(&m.body);
                let Ok([Value::Int(chat::SYS_CHAT_MESSAGE), ..]) = values.as_deref() else {
                    t.other.push(format!("{values:?}"));
                    continue;
                };
                let line = Heard::read(&m.body);
                match speakers.get(&line.sender) {
                    Some(&pid) if line.channel == NEARBY => *t.lines.entry(pid).or_default() += 1,
                    _ => t.other.push(format!("{line:?}")),
                }
            }
            other => t.other.push(format!("{other:?}")),
        }
    };
    tally.lock().unwrap().ended = Some(ended);
}

/// What a bot's writer did.
struct Played {
    /// Where it stepped last.
    last: Place,
    moves: u64,
    lines: u64,
}

/// Bot `i` plays from `began` for [`PLAY`]: a step of [`CIRCLE`] every [`MOVE_EVERY`], and a
/// nearby line every [`SAY_EVERY`], the first `i` twentieths of that in. It stops early if its
/// connection can't take a message. Its end of the socket stays open: the caller keeps it.
async fn play(mut mouth: Writer, i: usize, began: tokio::time::Instant) -> (Writer, Played) {
    let mut moves = tokio::time::interval_at(began + MOVE_EVERY, MOVE_EVERY);
    moves.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let offset = SAY_EVERY * u32::try_from(i).unwrap() / u32::try_from(BOTS).unwrap();
    let mut lines = tokio::time::interval_at(began + offset, SAY_EVERY);
    lines.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let end = tokio::time::sleep_until(began + PLAY);
    tokio::pin!(end);
    let from = mouth.place();
    let mut at = [from.x as i32, from.y as i32, from.z as i32];
    let mut played = Played {
        last: from,
        moves: 0,
        lines: 0,
    };
    loop {
        tokio::select! {
            biased;
            () = &mut end => break,
            _ = moves.tick() => {
                let [dx, dy] = CIRCLE[played.moves as usize % CIRCLE.len()];
                at = [at[0] + dx, at[1] + dy, at[2]];
                if !mouth.walk_to(at).await {
                    break;
                }
                played.moves += 1;
                played.last = mouth.place();
            }
            _ = lines.tick() => {
                let text = format!("line {} from bot {i}", played.lines + 1);
                if !mouth.say(NEARBY, &text).await {
                    break;
                }
                played.lines += 1;
            }
        }
    }
    (mouth, played)
}

/// The least and the most of `counts` over every (listener, speaker) pair of two bots.
fn spread(tallies: &[Tally], pids: &[i64], counts: fn(&Tally) -> &HashMap<i64, u64>) -> (u64, u64) {
    let mut all = Vec::new();
    for (i, t) in tallies.iter().enumerate() {
        for (j, pid) in pids.iter().enumerate() {
            if i != j {
                all.push(counts(t).get(pid).copied().unwrap_or(0));
            }
        }
    }
    (*all.iter().min().unwrap(), *all.iter().max().unwrap())
}

#[test]
fn each_bot_plans_599_moves_and_12_lines_and_must_send_95_percent() {
    assert_eq!((PLANNED_MOVES, PLANNED_LINES), (599, 12));
    assert_eq!(delivered(PLANNED_MOVES), 570, "569 is under 95% of 599");
    assert_eq!(delivered(PLANNED_LINES), 12, "11 is under 95% of 12");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "a minute of load: run it with --release -- --ignored --nocapture"]
async fn twenty_bots_for_a_minute() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), "", GateLimits::default()).await;
    let start = start_of(&config(dir.path(), ""));
    // Twenty bots, each with its own account and character, enter one after another at
    // `[world]`'s start: one cell. Each already in is told of each newcomer, and each newcomer of
    // those already in, in pid order.
    let mut bots: Vec<Bot> = Vec::with_capacity(BOTS);
    for i in 0..BOTS {
        let mut bot = signed_in(&s, &format!("bot{i:02}")).await;
        bot.create(&name(i), 7, 3).await;
        assert_eq!(bot.select(bot.pid()).await, start);
        bot.load().await;
        for other in &mut bots {
            other.expect_announced(bot.pid(), start).await;
        }
        let mut earlier: Vec<i64> = bots.iter().map(Bot::pid).collect();
        earlier.sort_unstable();
        for pid in earlier {
            bot.expect_announced(pid, start).await;
        }
        bots.push(bot);
    }
    // Each pulls the other 19 before the clock starts.
    let pids: Vec<i64> = bots.iter().map(Bot::pid).collect();
    for bot in &mut bots {
        let others: Vec<i64> = pids.iter().copied().filter(|&p| p != bot.pid()).collect();
        bot.pull_all(&others).await;
    }
    let speakers: Arc<HashMap<String, i64>> =
        Arc::new(bots.iter().map(|b| (b.made().link(), b.pid())).collect());

    // A minute of play: each bot reads its socket the whole time, so none is closed as a slow
    // reader, while it moves and speaks.
    let began = tokio::time::Instant::now();
    let mut shared = Vec::with_capacity(BOTS);
    let mut readers = Vec::with_capacity(BOTS);
    let mut writers = Vec::with_capacity(BOTS);
    for (i, bot) in bots.into_iter().enumerate() {
        let (ears, mouth) = bot.split();
        let tally = Arc::new(Mutex::new(Tally::default()));
        let counting = listen(ears, Arc::clone(&tally), Arc::clone(&speakers));
        readers.push(tokio::spawn(counting));
        writers.push(tokio::spawn(play(mouth, i, began)));
        shared.push(tally);
    }
    let mut mouths = Vec::with_capacity(BOTS);
    let mut played = Vec::with_capacity(BOTS);
    for w in writers {
        let (mouth, p) = w.await.unwrap();
        mouths.push(mouth);
        played.push(p);
    }
    // Every bot's last step reaches the bot after it, as a 13 for that place.
    let settling = Instant::now();
    for (i, (pid, p)) in pids.iter().zip(&played).enumerate() {
        let next = (i + 1) % BOTS;
        let at = [p.last.x, p.last.y, p.last.z, p.last.yaw];
        let last = world::move_dir_relay(*pid, at, &walking()).to_vec();
        while shared[next].lock().unwrap().last_move.get(pid) != Some(&last) {
            let waited = settling.elapsed();
            assert!(
                waited < SETTLE,
                "bot {i}'s last step didn't reach bot {next}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    let ticks = s.ticks();

    // Every bot is still connected, saw every other bot move and heard it speak, and got nothing
    // else. What the readers count from here on is the shutdown's.
    let tallies: Vec<Tally> = shared
        .iter()
        .map(|t| std::mem::take(&mut *t.lock().unwrap()))
        .collect();
    for (i, (t, reader)) in tallies.iter().zip(&readers).enumerate() {
        assert_eq!(t.ended, None, "bot {i}'s connection ended while it played");
        assert!(!reader.is_finished(), "bot {i} stopped reading");
        assert!(t.other.is_empty(), "bot {i} got {:?}", t.other);
        for (j, pid) in pids.iter().enumerate().filter(|&(j, _)| j != i) {
            let moved = t.moves.get(pid).is_some_and(|&n| n > 0);
            assert!(moved, "bot {i} never saw bot {j} move");
            let spoke = t.lines.get(pid).is_some_and(|&n| n > 0);
            assert!(spoke, "bot {i} never heard bot {j}");
        }
    }
    let sent_moves: u64 = played.iter().map(|p| p.moves).sum();
    let sent_lines: u64 = played.iter().map(|p| p.lines).sum();
    let fewest_moves = played.iter().map(|p| p.moves).min().unwrap();
    let fewest_lines = played.iter().map(|p| p.lines).min().unwrap();
    let (least_moves, most_moves) = spread(&tallies, &pids, |t| &t.moves);
    let (least_lines, most_lines) = spread(&tallies, &pids, |t| &t.lines);
    let refused: u64 = tallies.iter().map(|t| t.corrections).sum();
    // The logical CPUs the process may run on: hardware threads, not cores.
    let threads = std::thread::available_parallelism().map_or(0, usize::from);
    println!(
        "twenty_bots_for_a_minute: {BOTS} bots in one cell for {PLAY:?}, each moving every \
         {MOVE_EVERY:?} and speaking every {SAY_EVERY:?}, on {threads} threads, with the \
         database in {}",
        dir.path().display()
    );
    println!(
        "  tick work over the last minute: p50 {:?}, p99 {:?}, max {:?}; {} ticks since the \
         start, {} over budget (target: p99 at most {TICK_P99:?})",
        ticks.p50, ticks.p99, ticks.max, ticks.ticks, ticks.over_budget
    );
    println!(
        "  sent {sent_moves} moves and {sent_lines} lines, each bot at least {fewest_moves} of \
         its {PLANNED_MOVES} moves and {fewest_lines} of its {PLANNED_LINES} lines; each bot got \
         {least_moves} to {most_moves} 13s and {least_lines} to {most_lines} 378s from each \
         other bot; {refused} moves refused (20)"
    );

    // The shutdown, with all twenty in the world: each bot hangs up once it has read why, as the
    // real client does, and the shutdown saves each one.
    let stopping = Instant::now();
    let hang_ups: Vec<_> = readers
        .into_iter()
        .zip(mouths)
        .map(|(reader, mouth)| {
            tokio::spawn(async move {
                reader.await.unwrap();
                drop(mouth);
            })
        })
        .collect();
    s.shutdown().await.expect("everyone was saved");
    let took = stopping.elapsed();
    println!("  shutdown with {BOTS} in the world: {took:?} (target: at most {SAVE_ALL:?})");
    for h in hang_ups {
        h.await.unwrap();
    }
    for (i, t) in shared.iter().enumerate() {
        let t = t.lock().unwrap();
        let why = Some(Ended::Closed(SERVER_STOPPING.as_bytes().to_vec()));
        assert_eq!(t.ended, why, "bot {i} read why it was closed");
        assert!(t.other.is_empty(), "bot {i} got {:?}", t.other);
    }
    // Each was saved where it last stepped.
    let store = Store::open(&config(dir.path(), "").store.path)
        .await
        .unwrap();
    for (i, (pid, p)) in pids.iter().zip(&played).enumerate() {
        let account = store.account_by_username(&format!("bot{i:02}")).await;
        let id = account.unwrap().expect("its account").id;
        let character = store.character(id, *pid).await.unwrap().unwrap();
        assert_eq!(character.place, Some(p.last), "bot {i}'s place");
    }

    if std::env::var("FENJI_LOAD_ASSERT").as_deref() == Ok("1") {
        assert!(ticks.p99 <= TICK_P99, "p99 tick work {:?}", ticks.p99);
        assert!(took <= SAVE_ALL, "the shutdown took {took:?}");
        // The targets count only under the full load: every move taken, and no bot held back.
        assert_eq!(refused, 0, "moves refused");
        for (i, p) in played.iter().enumerate() {
            assert!(
                p.moves >= delivered(PLANNED_MOVES),
                "bot {i} sent {} of its {PLANNED_MOVES} moves",
                p.moves
            );
            assert!(
                p.lines >= delivered(PLANNED_LINES),
                "bot {i} sent {} of its {PLANNED_LINES} lines",
                p.lines
            );
        }
    }
}
