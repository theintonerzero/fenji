//! Interest management (Fenji design spec §6.4; world spec §3.3): a grid of 5,000-unit cells (50 m
//! if the units are centimetres), where each player sees the players in their own cell and the
//! eight around it, in the same scene. A player coming into view is announced (6); the client
//! pulls it (5), and the world sends it whole (7). Going out of view, or out of the game, is 9.
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use bevy_ecs::prelude::*;

use super::ecs::{Actor, Conn, Doomed, Here, Index, Pending, Stage, deliver};
use super::types::*;

/// A cell's side, in the client's units (world spec §3.3).
pub const CELL: f64 = 5_000.0;

/// A cell: the scene, then x and y counted in cells. z is up and doesn't count.
pub type Cell = (i64, i64, i64);

pub fn cell(at: &Location) -> Cell {
    (
        at.scene,
        (at.pos.x / CELL).floor() as i64,
        (at.pos.y / CELL).floor() as i64,
    )
}

/// How far a client has been told of another player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seen {
    /// Announced (6), not yet pulled.
    Announced,
    /// Sent whole (7), after the client's pull.
    Entered,
}

/// The players a player's client knows of.
#[derive(Component, Debug, Default)]
pub struct Sight(pub BTreeMap<ActorId, Seen>);

/// A player as a viewer: who, where, whether loaded, its connection, and what it knows of.
type Viewer = (
    Entity,
    &'static Actor,
    &'static Here,
    &'static Stage,
    &'static Conn,
    &'static mut Sight,
);

/// Each tick: tells every player in the world of the players coming into and going out of its
/// view, then answers the pulls. Players still loading neither see nor are seen, since the client
/// drops actors announced before its own (world spec §2.2).
pub(super) fn system(
    mut viewers: Query<Viewer, Without<Doomed>>,
    others: Query<(&Actor, &Here, &Stage), Without<Doomed>>,
    index: Res<Index>,
    mut pending: ResMut<Pending>,
    mut commands: Commands,
) {
    let mut grid: HashMap<Cell, Vec<ActorId>> = HashMap::new();
    let mut spots: HashMap<ActorId, Pos> = HashMap::new();
    for (actor, here, stage) in &others {
        if *stage == Stage::InWorld {
            grid.entry(cell(&here.0)).or_default().push(actor.pid);
            spots.insert(actor.pid, here.0.pos);
        }
    }
    for (e, actor, here, stage, conn, mut sight) in &mut viewers {
        if *stage != Stage::InWorld {
            continue;
        }
        let (scene, cx, cy) = cell(&here.0);
        let mut view = BTreeSet::new();
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(pids) = grid.get(&(scene, cx + dx, cy + dy)) {
                    view.extend(pids.iter().copied());
                }
            }
        }
        view.remove(&actor.pid);
        let gone: Vec<ActorId> = sight
            .0
            .keys()
            .filter(|pid| !view.contains(pid))
            .copied()
            .collect();
        if !gone.is_empty() {
            for pid in &gone {
                sight.0.remove(pid);
            }
            deliver(conn, e, WorldOut::Leave { pids: gone }, &mut commands);
        }
        for pid in view {
            if let Entry::Vacant(slot) = sight.0.entry(pid) {
                slot.insert(Seen::Announced);
                let pos = spots[&pid];
                deliver(conn, e, WorldOut::Announce { pid, pos }, &mut commands);
            }
        }
    }
    for (e, pid) in std::mem::take(&mut pending.pulls) {
        let Ok((_, _, _, _, conn, mut sight)) = viewers.get_mut(e) else {
            continue;
        };
        // Only an actor the client was told of, and only once.
        if sight.0.get(&pid) != Some(&Seen::Announced) {
            continue;
        }
        let Some(target) = index.pids.get(&pid) else {
            continue;
        };
        let Ok((actor, here, _)) = others.get(*target) else {
            continue;
        };
        sight.0.insert(pid, Seen::Entered);
        let msg = WorldOut::Enter {
            pid,
            look: actor.look.clone(),
            pos: here.0.pos,
            speed: SPEED_RATE,
        };
        deliver(conn, e, msg, &mut commands);
    }
}

#[cfg(test)]
mod tests {
    use super::super::ecs::test_util::*;
    use super::*;

    #[test]
    fn cells_are_5000_units_of_x_and_y_in_a_scene() {
        let c = |x, y| cell(&at(x, y));
        assert_eq!(c(0.0, 0.0), (SCENE, 0, 0));
        assert_eq!(c(4_999.0, -1.0), (SCENE, 0, -1));
        assert_eq!(c(10_000.0, -10_001.0), (SCENE, 2, -3));
        let high = Location {
            pos: Pos {
                z: 1.0e6,
                ..at(0.0, 0.0).pos
            },
            ..at(0.0, 0.0)
        };
        assert_eq!(cell(&high), (SCENE, 0, 0), "z doesn't count");
        let other_scene = Location {
            scene: SCENE + 1,
            ..at(0.0, 0.0)
        };
        assert_eq!(cell(&other_scene), (SCENE + 1, 0, 0));
    }

    #[test]
    fn the_eight_cells_around_are_in_view_and_no_further_nor_another_scene() {
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(100.0, 100.0)));
        h.enter(player(2, "Mai", at(-4_000.0, 9_999.0)));
        h.enter(player(3, "Ren", at(10_000.0, 100.0)));
        h.enter(Player {
            at: Location {
                scene: SCENE + 1,
                ..at(100.0, 100.0)
            },
            ..player(4, "Kai", at(0.0, 0.0))
        });
        h.tick(50);
        assert!(matches!(
            h.take(1).as_slice(),
            [WorldOut::Announce { pid: 2, .. }]
        ));
        assert!(h.take(3).is_empty());
        assert!(h.take(4).is_empty());
    }

    #[test]
    fn players_in_view_are_announced_then_sent_when_pulled() {
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        h.enter(player(2, "Mai", at(6_000.0, 0.0)));
        h.enter(player(3, "Far", at(20_000.0, 0.0)));
        assert!(h.take(3).is_empty(), "no one near");
        h.tick(50);
        // 1 was told of 2 when 2 entered; `enter` emptied 2's channel.
        assert_eq!(
            h.take(1),
            vec![WorldOut::Announce {
                pid: 2,
                pos: at(6_000.0, 0.0).pos
            }]
        );
        h.push(WorldIn::Pull { conn: 1, pid: 2 });
        h.push(WorldIn::Pull { conn: 1, pid: 2 });
        h.push(WorldIn::Pull { conn: 1, pid: 3 });
        h.tick(50);
        assert_eq!(
            h.take(1),
            vec![WorldOut::Enter {
                pid: 2,
                look: look("Mai"),
                pos: at(6_000.0, 0.0).pos,
                speed: SPEED_RATE
            }],
            "once, and never an actor it wasn't told of"
        );
    }

    #[test]
    fn a_player_still_loading_neither_sees_nor_is_seen() {
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        h.join(player(2, "Mai", at(0.0, 0.0)));
        h.tick(50);
        h.tick(50);
        assert!(h.take(1).is_empty());
        assert_eq!(h.take(2).len(), 1, "only its LoadScene");
        h.push(WorldIn::Loaded { conn: 2 });
        h.tick(50);
        assert!(matches!(
            h.take(2).as_slice(),
            [
                WorldOut::SelfEnter { .. },
                WorldOut::Announce { pid: 1, .. }
            ]
        ));
        assert!(matches!(
            h.take(1).as_slice(),
            [WorldOut::Announce { pid: 2, .. }]
        ));
    }

    #[test]
    fn whoever_knew_of_a_player_hears_it_left_however_it_left() {
        for how in ["closed", "kicked while loading others", "replaced"] {
            let mut h = Harness::new();
            h.enter(player(1, "Lin", at(0.0, 0.0)));
            h.enter(player(2, "Mai", at(0.0, 0.0)));
            h.enter(player(3, "Ren", at(0.0, 0.0)));
            h.push(WorldIn::Pull { conn: 1, pid: 2 });
            h.tick(50);
            for c in 1..=3 {
                h.take(c);
            }
            match how {
                "closed" => h.push(WorldIn::Leave { conn: 2 }),
                "kicked while loading others" => {
                    for _ in 0..flood() {
                        h.push(WorldIn::Pull { conn: 2, pid: 1 });
                    }
                }
                _ => h.join(Player {
                    conn: 9,
                    ..player(2, "Mai", at(100.0, 100.0))
                }),
            }
            h.tick(50);
            // 1 had pulled 2, and 3 had only been told of it: both hear it left.
            assert_eq!(h.take(1), vec![WorldOut::Leave { pids: vec![2] }], "{how}");
            assert_eq!(h.take(3), vec![WorldOut::Leave { pids: vec![2] }], "{how}");
            h.tick(50);
            assert!(h.take(1).is_empty(), "{how}: told once");
        }
    }

    #[test]
    fn a_replaced_player_is_seen_again_where_it_stood_once_its_new_client_loads() {
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        h.enter(player(2, "Mai", at(300.0, 0.0)));
        h.take(1);
        h.join(Player {
            conn: 9,
            ..player(2, "Mai", at(-40_000.0, 0.0))
        });
        h.tick(50);
        assert_eq!(h.take(1), vec![WorldOut::Leave { pids: vec![2] }]);
        h.push(WorldIn::Loaded { conn: 9 });
        h.tick(50);
        assert_eq!(
            h.take(1),
            vec![WorldOut::Announce {
                pid: 2,
                pos: at(300.0, 0.0).pos
            }]
        );
    }

    #[test]
    fn whoever_knew_of_a_player_that_stops_reading_hears_it_left_that_tick() {
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        // LoadScene, SelfEnter and the Announce of Lin fill Mai's channel, which it never reads.
        h.join_with(player(2, "Mai", at(0.0, 0.0)), 3);
        h.push(WorldIn::Loaded { conn: 2 });
        h.tick(50);
        assert!(matches!(
            h.take(1).as_slice(),
            [WorldOut::Announce { pid: 2, .. }]
        ));
        // Ren's Announce finds Mai's channel full in the interest phase, so Mai leaves at the end
        // of the tick, after interest has run: only `hide` can tell Lin in time.
        h.enter(player(3, "Ren", at(0.0, 0.0)));
        assert_eq!(h.core.players(), 2, "Mai is let go");
        assert_eq!(
            h.take(1),
            vec![
                WorldOut::Announce {
                    pid: 3,
                    pos: at(0.0, 0.0).pos
                },
                WorldOut::Leave { pids: vec![2] }
            ],
            "in the same tick"
        );
        h.tick(50);
        assert!(h.take(1).is_empty(), "told once");
    }

    #[test]
    fn whoever_knew_of_a_player_whose_connection_closed_hears_it_left_that_tick() {
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        // Mai's channel is the test's own, with room to spare, so only its closing stops a send.
        let (out, mai) = tokio::sync::mpsc::channel(8);
        h.push(WorldIn::Join {
            player: player(2, "Mai", at(0.0, 0.0)),
            out,
        });
        h.push(WorldIn::Loaded { conn: 2 });
        h.tick(50);
        assert!(matches!(
            h.take(1).as_slice(),
            [WorldOut::Announce { pid: 2, .. }]
        ));
        // Ren is still loading, so no one is told of it yet.
        h.join(player(3, "Ren", at(0.0, 0.0)));
        h.tick(50);
        assert_eq!(h.core.players(), 3);
        // Mai's connection goes without a word to the world. Ren's load brings its Announce, which
        // finds Mai's channel closed in the interest phase.
        drop(mai);
        h.push(WorldIn::Loaded { conn: 3 });
        h.tick(50);
        assert_eq!(h.core.players(), 2, "Mai is let go");
        assert_eq!(
            h.take(1),
            vec![
                WorldOut::Announce {
                    pid: 3,
                    pos: at(0.0, 0.0).pos
                },
                WorldOut::Leave { pids: vec![2] }
            ],
            "in the same tick"
        );
        h.tick(50);
        assert!(h.take(1).is_empty(), "told once");
    }

    #[test]
    fn a_replacement_that_loads_in_the_tick_it_joins_is_a_leave_then_an_announce() {
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        h.enter(player(2, "Mai", at(300.0, 0.0)));
        h.take(1);
        // The newer client joins and loads in one tick, so the new actor is in the world before
        // interest runs and stands in Lin's view under the same pid: only `hide` tells Lin.
        h.join(Player {
            conn: 9,
            ..player(2, "Mai", at(-40_000.0, 0.0))
        });
        h.push(WorldIn::Loaded { conn: 9 });
        h.tick(50);
        assert_eq!(
            h.take(1),
            vec![
                WorldOut::Leave { pids: vec![2] },
                WorldOut::Announce {
                    pid: 2,
                    pos: at(300.0, 0.0).pos
                }
            ]
        );
    }
}
