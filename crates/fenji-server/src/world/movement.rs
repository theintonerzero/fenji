//! The speed check (Fenji design spec §6.4; world spec §3.4), lenient by design. A move passes
//! when the distance it covers fits a budget that fills at the mover's top speed times the
//! tolerance and holds at most a second of it, so a burst after a network stall passes too.
//! Horizontal distance and climbing count; falling doesn't, so drops and glides pass. A move that
//! passes goes to whoever has the mover in view; one that doesn't is answered with where the mover
//! stands (20).
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

/// Applies the tick's moves in order. Players still loading can't move.
pub(super) fn system(
    now: Res<Now>,
    rules: Res<Rules>,
    mut pending: ResMut<Pending>,
    mut movers: Query<(&Actor, &mut Here, &mut Gait, &Stage, &Conn), Without<Doomed>>,
    viewers: Query<(Entity, &Sight, &Conn), Without<Doomed>>,
    mut commands: Commands,
) {
    for (e, step) in std::mem::take(&mut pending.moves) {
        let Ok((actor, mut here, mut gait, stage, conn)) = movers.get_mut(e) else {
            continue;
        };
        if *stage != Stage::InWorld {
            continue;
        }
        let from = here.0.pos;
        if !gait.check(&from, &step.pos, now.0, SPEED_RATE, rules.move_tolerance) {
            deliver(conn, e, WorldOut::Correct { pos: from }, &mut commands);
            continue;
        }
        here.0.pos = step.pos;
        for (v, sight, to) in &viewers {
            if sight.0.get(&actor.pid) == Some(&Seen::Entered) {
                let msg = WorldOut::Moved {
                    pid: actor.pid,
                    step: step.clone(),
                };
                deliver(to, v, msg, &mut commands);
            }
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
        assert!(h.take(2).is_empty());
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
        let relayed: Vec<WorldOut> = xs[..7]
            .iter()
            .map(|&x| WorldOut::Moved {
                pid: 1,
                step: step(x, 0.0),
            })
            .collect();
        assert_eq!(h.take(2), relayed, "seven, in order");
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
        assert!(h.take(2).is_empty());
        assert_eq!(
            h.take(1),
            vec![WorldOut::Correct {
                pos: at(700.0, 0.0).pos
            }]
        );
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
