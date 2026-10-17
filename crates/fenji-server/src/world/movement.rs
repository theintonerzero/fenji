//! The speed check (Fenji design spec §6.4; world spec §3.4), lenient by design. A move passes
//! when the distance it covers fits a budget that fills at the mover's top speed times the
//! tolerance and holds at most a second of it, so a burst after a network stall passes too.
//! Horizontal distance and climbing count; falling doesn't, so drops and glides pass.
//!
//! A tick tells whoever has a mover in view one thing of it: its last accepted move (13), or, when
//! the tick refused any of its moves, that it stops where it then stands (13 with direction 0), and
//! answers the mover itself once, with that place (20): the M4 world plan's Amendment C, with #265's
//! stop, without which a viewer's client runs the mover on from its last relay, up to 1.5 s past
//! where the world has it (docs/protocol/packets/world.md 16.9, 16.17). A client sends at most one
//! move per 50 ms besides starting and stopping, and each relay places the mover afresh (16.6,
//! 16.8), so at 20 ticks a second this drops nothing a client means to show, and no client can make
//! the world send anyone more than one 13 of its own a tick. A mover that sent nothing in a tick is
//! relayed nothing. Only direction commands (type 8) reach the world in M4; once skill casts and
//! jumps do (types 9 and 15), only direction commands may coalesce: a newer move must never replace
//! a cast.
use std::collections::HashMap;

use bevy_ecs::prelude::*;

use super::ecs::{Actor, Conn, Doomed, Here, Now, Pending, Rules, Stage, deliver};
use super::interest::{Seen, Sight};
use super::types::*;
use crate::config::MOVE_TOLERANCE;

/// Units per second at speed rate 100 (world spec §2.3: about 500 cm/s).
pub const RUN: f64 = 500.0;
/// Sprinting and gliding add 70% (world spec §2.3).
pub const SPRINT: f64 = 1.7;
/// The highest speed rate the client allows (world spec §2.3).
pub const MAX_RATE: i64 = 340;
/// The farthest a coordinate may be from 0, and the largest yaw: config's, so that every start
/// it takes is a point the world takes.
pub use crate::config::{COORDINATE_LIMIT as LIMIT, YAW_LIMIT};
/// The most movement a player can save up, in seconds.
pub const BURST_SECS: f64 = 1.0;

/// The fastest a player at speed `rate` may go, in units per second. The check doesn't know the
/// mode, so this is the sprint.
pub fn top_speed(rate: i64) -> f64 {
    rate.clamp(0, MAX_RATE) as f64 / 100.0 * RUN * SPRINT
}

/// `tolerance` held to config's range, a non-finite one as the least. Config parsing keeps others
/// out, but [`Rules`] can be built without it, and an infinite tolerance would make the budget
/// infinite (∞ × 0 is NaN, which `min` drops).
fn bounded(tolerance: f64) -> f64 {
    let (least, most) = (*MOVE_TOLERANCE.start(), *MOVE_TOLERANCE.end());
    if tolerance.is_finite() {
        tolerance.clamp(least, most)
    } else {
        least
    }
}

/// A point the world takes: finite, within [`LIMIT`], with a yaw within [`YAW_LIMIT`].
pub fn valid(p: &Pos) -> bool {
    [p.x, p.y, p.z]
        .iter()
        .all(|v| v.is_finite() && v.abs() <= LIMIT)
        && p.yaw.is_finite()
        && p.yaw.abs() <= YAW_LIMIT
}

/// A player's movement budget.
#[derive(Component, Debug, Clone, Copy)]
pub struct Gait {
    budget: f64,
    at_ms: u64,
}

impl Gait {
    /// An empty budget that starts filling at `now_ms`.
    pub fn new(now_ms: u64) -> Self {
        Self {
            budget: 0.0,
            at_ms: now_ms,
        }
    }

    /// Whether a move from `from` to `to` at `now_ms` fits the budget. One that does spends it.
    pub fn check(&mut self, from: &Pos, to: &Pos, now_ms: u64, rate: i64, tolerance: f64) -> bool {
        if !valid(to) {
            return false;
        }
        let per_sec = top_speed(rate) * bounded(tolerance);
        let elapsed = now_ms.saturating_sub(self.at_ms) as f64 / 1000.0;
        self.at_ms = self.at_ms.max(now_ms);
        self.budget = (self.budget + per_sec * elapsed).min(per_sec * BURST_SECS);
        let climb = (to.z - from.z).max(0.0);
        let d = (to.x - from.x).hypot(to.y - from.y).hypot(climb);
        if d <= self.budget {
            self.budget -= d;
            true
        } else {
            false
        }
    }
}

/// What a tick did with one mover's moves.
#[derive(Default)]
struct Outcome {
    /// The last move the check accepted, which the tick relays unless it refused one too.
    relay: Option<MoveStep>,
    /// Whether the check refused any, so that the tick corrects the mover, and stops it for its
    /// viewers.
    refused: bool,
}

/// Checks the tick's moves in the order they arrived, each from where the last accepted one put its
/// mover. Then, for each mover in the order its first move of the tick arrived, tells each viewer
/// that has it its last accepted move (Moved), or, if any move was refused, that it stops where it
/// then stands (Stopped), and puts the mover there once (Correct). Players still loading can't
/// move.
pub(super) fn system(
    now: Res<Now>,
    rules: Res<Rules>,
    mut pending: ResMut<Pending>,
    mut movers: Query<(&Actor, &mut Here, &mut Gait, &Stage, &Conn), Without<Doomed>>,
    viewers: Query<(Entity, &Sight, &Conn), Without<Doomed>>,
    mut commands: Commands,
) {
    let mut outcomes: Vec<(Entity, Outcome)> = Vec::new();
    let mut slot: HashMap<Entity, usize> = HashMap::new();
    for (e, step) in std::mem::take(&mut pending.moves) {
        let Ok((_, mut here, mut gait, stage, _)) = movers.get_mut(e) else {
            continue;
        };
        if *stage != Stage::InWorld {
            continue;
        }
        let i = *slot.entry(e).or_insert_with(|| {
            outcomes.push((e, Outcome::default()));
            outcomes.len() - 1
        });
        let outcome = &mut outcomes[i].1;
        if gait.check(
            &here.0.pos,
            &step.pos,
            now.0,
            SPEED_RATE,
            rules.move_tolerance,
        ) {
            here.0.pos = step.pos;
            outcome.relay = Some(step);
        } else {
            outcome.refused = true;
        }
    }
    for (e, outcome) in outcomes {
        let Ok((actor, here, _, _, conn)) = movers.get(e) else {
            continue;
        };
        let (pid, pos) = (actor.pid, here.0.pos);
        // The stop takes the place of the last accepted move, which put the mover where it stops,
        // so that a viewer still hears one thing of each mover a tick (#265).
        let news = match (outcome.refused, outcome.relay) {
            (true, _) => WorldOut::Stopped { pid, pos },
            (false, Some(step)) => WorldOut::Moved { pid, step },
            (false, None) => continue,
        };
        for (v, sight, to) in &viewers {
            if sight.0.get(&pid) == Some(&Seen::Entered) {
                deliver(to, v, news.clone(), &mut commands);
            }
        }
        if outcome.refused {
            deliver(conn, e, WorldOut::Correct { pos }, &mut commands);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::ecs::test_util::*;
    use super::*;

    fn step(x: f64, y: f64) -> MoveStep {
        MoveStep {
            pos: at(x, y).pos,
            detail: Arc::from(&[7u8][..]),
        }
    }

    fn mv(conn: ConnId, x: f64, y: f64) -> WorldIn {
        WorldIn::Move {
            conn,
            step: step(x, y),
        }
    }

    /// What a viewer whose client has `pid` hears when the world corrects it at (x, y): it stops
    /// there (#265).
    fn stopped(pid: ActorId, x: f64, y: f64) -> WorldOut {
        WorldOut::Stopped {
            pid,
            pos: at(x, y).pos,
        }
    }

    #[test]
    fn top_speed_is_a_sprint_at_the_rate_capped_at_340() {
        assert_eq!(top_speed(100), 850.0);
        assert_eq!(top_speed(340), top_speed(1_000));
        assert_eq!(top_speed(-5), 0.0);
    }

    #[test]
    fn the_budget_fills_with_time_up_to_a_second() {
        let o = at(0.0, 0.0).pos;
        let mut g = Gait::new(0);
        // 1.5 × 850 = 1275 units a second.
        assert!(g.check(&o, &at(127.0, 0.0).pos, 100, 100, 1.5));
        assert!(!g.check(&o, &at(10.0, 0.0).pos, 100, 100, 1.5), "spent");
        let mut g = Gait::new(0);
        assert!(
            !g.check(&o, &at(1_300.0, 0.0).pos, 10_000, 100, 1.5),
            "a second at most"
        );
        assert!(g.check(&o, &at(1_275.0, 0.0).pos, 10_000, 100, 1.5));
    }

    #[test]
    fn hostile_points_and_times_fail_without_harm() {
        let o = at(0.0, 0.0).pos;
        for bad in [
            Pos { x: f64::NAN, ..o },
            Pos {
                y: f64::INFINITY,
                ..o
            },
            Pos {
                z: f64::NEG_INFINITY,
                ..o
            },
            Pos { yaw: f64::NAN, ..o },
            Pos { yaw: 361.0, ..o },
            Pos { x: 1.0e9, ..o },
            // Falling is free, so only the limit refuses a drop this far.
            Pos { z: -1.0e9, ..o },
        ] {
            let mut g = Gait::new(0);
            assert!(!g.check(&o, &bad, 1_000, 100, 1.5), "{bad:?}");
        }
        let mut g = Gait::new(5_000);
        assert!(
            !g.check(&o, &at(1.0, 0.0).pos, 1_000, 100, 1.5),
            "time going back adds nothing"
        );
        assert!(
            g.check(&o, &at(0.0, 0.0).pos, 1_000, 100, 1.5),
            "standing still always passes"
        );
        // Only the 10 ms since 5,000 count: 12.75 units, not the 4 s since the time that went back.
        assert!(
            !g.check(&o, &at(13.0, 0.0).pos, 5_010, 100, 1.5),
            "the budget fills from the latest time"
        );
        assert!(
            g.check(&o, &at(12.0, 0.0).pos, 5_010, 100, 1.5),
            "and the budget fills again"
        );
    }

    #[test]
    fn a_tolerance_beyond_the_configs_range_is_held_to_it() {
        let o = at(0.0, 0.0).pos;
        // Whether a new player may move `x` units a second after joining.
        let in_a_second = |tolerance: f64, x: f64| {
            Gait::new(1_000).check(&o, &at(x, 0.0).pos, 2_000, 100, tolerance)
        };
        // A non-finite one counts as 1.0: 850 units a second, and nothing at once.
        for tolerance in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            let mut g = Gait::new(1_000);
            assert!(
                !g.check(&o, &at(100_000.0, 0.0).pos, 1_000, 100, tolerance),
                "{tolerance}: at once"
            );
            assert!(in_a_second(tolerance, 850.0), "{tolerance}");
            assert!(!in_a_second(tolerance, 851.0), "{tolerance}");
        }
        // A finite one is held to 1.0 to 5.0: 850 to 4,250 units a second.
        assert!(in_a_second(1.0e9, 4_250.0));
        assert!(!in_a_second(1.0e9, 4_251.0));
        assert!(in_a_second(0.0, 850.0));
        assert!(!in_a_second(0.0, 851.0));
    }

    #[test]
    fn the_world_takes_exactly_the_points_its_config_does() {
        use crate::config::{COORDINATE_LIMIT, YAW_LIMIT};
        let edge = Pos {
            x: COORDINATE_LIMIT,
            y: -COORDINATE_LIMIT,
            z: COORDINATE_LIMIT,
            yaw: -YAW_LIMIT,
        };
        assert!(valid(&edge));
        for beyond in [
            Pos {
                x: COORDINATE_LIMIT.next_up(),
                ..edge
            },
            Pos {
                y: (-COORDINATE_LIMIT).next_down(),
                ..edge
            },
            Pos {
                z: COORDINATE_LIMIT.next_up(),
                ..edge
            },
            Pos {
                yaw: YAW_LIMIT.next_up(),
                ..edge
            },
            Pos {
                yaw: (-YAW_LIMIT).next_down(),
                ..edge
            },
        ] {
            assert!(!valid(&beyond), "{beyond:?}");
        }
    }

    #[test]
    fn climbing_counts_and_falling_does_not() {
        let o = at(0.0, 0.0).pos;
        let up = Pos {
            z: o.z + 1_300.0,
            ..o
        };
        let down = Pos {
            z: o.z - 50_000.0,
            ..o
        };
        let mut g = Gait::new(0);
        assert!(!g.check(&o, &up, 1_000, 100, 1.5), "1,300 up in a second");
        assert!(g.check(&o, &down, 1_000, 100, 1.5), "a long drop");
        assert!(g.check(
            &down,
            &Pos {
                z: down.z + 1_000.0,
                ..down
            },
            1_000,
            100,
            1.5
        ));
    }

    #[test]
    fn a_move_reaches_whoever_pulled_the_mover_and_a_jump_is_corrected() {
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        h.enter(player(2, "Mai", at(0.0, 0.0)));
        h.enter(player(3, "Ren", at(0.0, 0.0)));
        h.push(WorldIn::Pull { conn: 2, pid: 1 });
        h.tick(50);
        for c in 1..=3 {
            h.take(c);
        }
        h.tick(200);
        h.push(mv(1, 100.0, 0.0));
        h.tick(50);
        assert!(h.take(1).is_empty());
        assert_eq!(
            h.take(2),
            vec![WorldOut::Moved {
                pid: 1,
                step: step(100.0, 0.0)
            }]
        );
        assert!(h.take(3).is_empty(), "3 never pulled 1");
        h.push(mv(1, 9_000.0, 0.0));
        h.tick(50);
        assert_eq!(
            h.take(1),
            vec![WorldOut::Correct {
                pos: at(100.0, 0.0).pos
            }]
        );
        // Mai's client would run Lin on from that relay: it hears she stops where the world has
        // her (#265), and nothing of the jump. Ren's client never had her.
        assert_eq!(h.take(2), vec![stopped(1, 100.0, 0.0)]);
        assert!(h.take(3).is_empty(), "3 never pulled 1");
    }

    #[test]
    fn a_burst_after_a_stall_passes_and_hostile_moves_never_reach_others() {
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        h.enter(player(2, "Mai", at(0.0, 0.0)));
        h.push(WorldIn::Pull { conn: 2, pid: 1 });
        h.tick(50);
        h.take(1);
        h.take(2);
        h.tick(400);
        // Eight moves of 100 arrive together after a stall. Lin joined 0.55 s before this tick,
        // so its budget holds 0.55 × 1,275 = 701.25 units, and the tick's moves share it: seven
        // fit and the eighth is refused.
        let xs = [100.0, 200.0, 300.0, 400.0, 500.0, 600.0, 700.0, 800.0];
        for x in xs {
            h.push(mv(1, x, 0.0));
        }
        h.tick(50);
        // The eighth was refused, so the tick tells Mai, in place of Lin's last accepted move, that
        // Lin stops where the world has her (#265): where the seventh put her, at 700. That it is
        // 700, and that Lin is corrected there, pins the speed rule: had the budget held eight,
        // Mai would get the eighth's move, and no stop or correction would come.
        assert_eq!(
            h.take(2),
            vec![stopped(1, 700.0, 0.0)],
            "one stop, where the last move that fit put her"
        );
        assert_eq!(
            h.take(1),
            vec![WorldOut::Correct {
                pos: at(700.0, 0.0).pos
            }]
        );
        // A ninth message in that tick would have been a flood (the harness allows eight), so the
        // hostile move comes in the next one.
        h.push(WorldIn::Move {
            conn: 1,
            step: MoveStep {
                pos: Pos {
                    x: f64::NAN,
                    ..at(0.0, 0.0).pos
                },
                detail: Arc::from(&[][..]),
            },
        });
        h.tick(50);
        // Nothing of the hostile move reaches Mai: only the world's own place for Lin.
        assert_eq!(h.take(2), vec![stopped(1, 700.0, 0.0)]);
        assert_eq!(
            h.take(1),
            vec![WorldOut::Correct {
                pos: at(700.0, 0.0).pos
            }]
        );
    }

    #[test]
    fn a_tick_relays_each_movers_last_move_once() {
        // Amendment C: one relay per mover per tick, its last accepted move, to each viewer that
        // has it, the movers in the order their first moves of the tick arrived (world.md 16.6).
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        h.enter(player(2, "Mai", at(0.0, 0.0)));
        h.enter(player(3, "Ren", at(0.0, 0.0)));
        h.push(WorldIn::Pull { conn: 3, pid: 2 });
        h.push(WorldIn::Pull { conn: 3, pid: 1 });
        h.tick(50);
        for c in 1..=3 {
            h.take(c);
        }
        h.tick(500);
        // Lin moves twice and Mai once, Mai's move arriving between Lin's two.
        h.push(mv(1, 100.0, 0.0));
        h.push(mv(2, 0.0, 100.0));
        h.push(mv(1, 200.0, 0.0));
        h.tick(50);
        assert_eq!(
            h.take(3),
            vec![
                WorldOut::Moved {
                    pid: 1,
                    step: step(200.0, 0.0)
                },
                WorldOut::Moved {
                    pid: 2,
                    step: step(0.0, 100.0)
                },
            ]
        );
        // Lin and Mai were only told of each other: neither has the other, so neither hears of it.
        assert!(h.take(1).is_empty());
        assert!(h.take(2).is_empty());
        // Each move still counts where it was accepted: Lin stands at 200, Mai at (0, 100).
        h.push(WorldIn::Leave { conn: 1 });
        h.push(WorldIn::Leave { conn: 2 });
        h.tick(50);
        let saves = h.saves();
        for (pid, place) in [(1, at(200.0, 0.0)), (2, at(0.0, 100.0))] {
            let save = Service::SavePlace { pid, at: place };
            assert!(saves.contains(&save), "{saves:?}");
        }
    }

    #[test]
    fn a_tick_corrects_a_mover_once() {
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        h.enter(player(2, "Mai", at(0.0, 0.0)));
        h.push(WorldIn::Pull { conn: 2, pid: 1 });
        h.tick(50);
        h.take(1);
        h.take(2);
        h.tick(200);
        // Two jumps too far in one tick: one Correct, with where Lin stands, and Mai, who has Lin,
        // one stop there (#265), and no move: neither jump was accepted.
        h.push(mv(1, 9_000.0, 0.0));
        h.push(mv(1, 0.0, 9_000.0));
        h.tick(50);
        assert_eq!(
            h.take(1),
            vec![WorldOut::Correct {
                pos: at(0.0, 0.0).pos
            }]
        );
        assert_eq!(h.take(2), vec![stopped(1, 0.0, 0.0)], "one stop, no relay");
        // A refused move around an accepted one: the mover gets one Correct, with where it stands
        // after the tick's moves, and the viewer the stop there, in place of the accepted move,
        // which put the mover there: a tick tells a viewer one thing of each mover at most.
        h.push(mv(1, 9_000.0, 0.0));
        h.push(mv(1, 50.0, 0.0));
        h.push(mv(1, 0.0, 9_000.0));
        h.tick(50);
        assert_eq!(h.take(2), vec![stopped(1, 50.0, 0.0)]);
        assert_eq!(
            h.take(1),
            vec![WorldOut::Correct {
                pos: at(50.0, 0.0).pos
            }]
        );
    }

    #[test]
    fn a_corrected_movers_stop_comes_in_its_turn_among_the_ticks_relays() {
        // #265: each viewer hears of each mover once a tick, in the order the movers' first moves
        // of the tick arrived: its last accepted move, or, when the tick refused any of its moves,
        // a stop where the world has it.
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        h.enter(player(2, "Mai", at(0.0, 0.0)));
        h.enter(player(3, "Ren", at(0.0, 0.0)));
        h.push(WorldIn::Pull { conn: 3, pid: 2 });
        h.push(WorldIn::Pull { conn: 3, pid: 1 });
        h.push(WorldIn::Pull { conn: 2, pid: 1 });
        h.tick(50);
        for c in 1..=3 {
            h.take(c);
        }
        h.tick(500);
        // Mai's move arrives first, then Lin's jump too far.
        h.push(mv(2, 0.0, 100.0));
        h.push(mv(1, 9_000.0, 0.0));
        h.tick(50);
        assert_eq!(
            h.take(3),
            vec![
                WorldOut::Moved {
                    pid: 2,
                    step: step(0.0, 100.0)
                },
                stopped(1, 0.0, 0.0),
            ]
        );
        assert_eq!(h.take(2), vec![stopped(1, 0.0, 0.0)]);
        // Lin hears her 20 and nothing else: no stop of her own, and nothing of Mai's move, since
        // her client was only told of Mai.
        assert_eq!(
            h.take(1),
            vec![WorldOut::Correct {
                pos: at(0.0, 0.0).pos
            }]
        );
        // Lin's jump first this time: her stop comes first.
        h.push(mv(1, 9_000.0, 0.0));
        h.push(mv(2, 0.0, 200.0));
        h.tick(50);
        assert_eq!(
            h.take(3),
            vec![
                stopped(1, 0.0, 0.0),
                WorldOut::Moved {
                    pid: 2,
                    step: step(0.0, 200.0)
                },
            ]
        );
    }

    #[test]
    fn a_correction_with_no_viewer_is_the_movers_20_alone() {
        // #265: a stop goes only where a relay would. Mai's client was only told of Lin, so it has
        // no actor to stop (world.md 16.8), and Far is out of Lin's view: neither hears anything.
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        h.enter(player(2, "Mai", at(0.0, 0.0)));
        h.enter(player(3, "Far", at(20_000.0, 0.0)));
        h.tick(200);
        for c in 1..=3 {
            h.take(c);
        }
        h.push(mv(1, 9_000.0, 0.0));
        h.tick(50);
        assert_eq!(
            h.take(1),
            vec![WorldOut::Correct {
                pos: at(0.0, 0.0).pos
            }]
        );
        assert!(h.take(2).is_empty(), "Mai's client was only told of Lin");
        assert!(h.take(3).is_empty(), "Far's client knows nothing of Lin");
        // Once Mai's client has pulled Lin, Lin's next correction reaches it.
        h.push(WorldIn::Pull { conn: 2, pid: 1 });
        h.tick(50);
        h.take(2);
        h.push(mv(1, 9_000.0, 0.0));
        h.tick(50);
        assert_eq!(
            h.take(1),
            vec![WorldOut::Correct {
                pos: at(0.0, 0.0).pos
            }]
        );
        assert_eq!(h.take(2), vec![stopped(1, 0.0, 0.0)]);
        assert!(h.take(3).is_empty());
    }

    #[test]
    fn moves_while_loading_are_dropped() {
        let mut h = Harness::new();
        h.join(player(1, "Lin", at(0.0, 0.0)));
        h.tick(500);
        h.take(1);
        h.push(mv(1, 10.0, 0.0));
        h.tick(50);
        assert!(h.take(1).is_empty());
        h.push(WorldIn::Leave { conn: 1 });
        h.tick(50);
        assert_eq!(
            h.saves(),
            vec![Service::SavePlace {
                pid: 1,
                at: at(0.0, 0.0)
            }]
        );
    }

    #[test]
    fn walking_out_of_view_and_back_is_a_leave_then_an_announce() {
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        h.enter(player(2, "Mai", at(9_000.0, 0.0)));
        h.take(1);
        // 2 walks east, 1,000 units a second, out of 1's cells (x ≥ 10,000) and back.
        let mut x = 9_000.0;
        for _ in 0..12 {
            x += 1_000.0;
            h.push(mv(2, x, 0.0));
            h.tick(1_000);
        }
        assert_eq!(h.take(1), vec![WorldOut::Leave { pids: vec![2] }]);
        for _ in 0..12 {
            x -= 1_000.0;
            h.push(mv(2, x, 0.0));
            h.tick(1_000);
        }
        assert!(matches!(
            h.take(1).as_slice(),
            [WorldOut::Announce { pid: 2, .. }]
        ));
    }
}
