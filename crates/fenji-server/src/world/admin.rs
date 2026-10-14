//! Admin commands in the game (world spec §3.6): what `!tp`, `!kick`, `!announce` and `!invite`
//! do. The chat phase parses an admin's `!` line with [`crate::commands`] and queues it here; the
//! answer is a line only the admin sees.
use bevy_ecs::prelude::*;

use super::chat;
use super::ecs::{
    Actor, Conn, Doomed, Here, Index, Now, Pending, Services, Stage, doom, hide, name_key, send,
};
use super::interest::Sight;
use super::movement::Gait;
use super::types::*;
use crate::commands::Command;

/// Runs the tick's commands, in order.
pub(super) fn system(world: &mut World) {
    for (admin, order) in std::mem::take(&mut world.resource_mut::<Pending>().orders) {
        if world.get::<Doomed>(admin).is_some() {
            continue;
        }
        if let Some(text) = run(world, admin, order) {
            send(world, admin, WorldOut::Chat(ChatOut::System { text }));
        }
    }
}

/// Does `order` for `admin`, and returns the answer, if there is one now.
fn run(world: &mut World, admin: Entity, order: Command) -> Option<String> {
    match order {
        Command::Tp { who, to } => {
            let mover = match &who {
                Some(name) => find(world, name),
                None => Some(admin),
            };
            let Some(mover) = mover else {
                return Some(absent(who.as_deref().unwrap_or_default()));
            };
            let Some(target) = find(world, &to) else {
                return Some(absent(&to));
            };
            if mover == target {
                return Some(format!("{} is already there", name_of(world, mover)));
            }
            let at = world.get::<Here>(target)?.0;
            teleport(world, mover, at);
            Some(format!(
                "{} is with {}",
                name_of(world, mover),
                name_of(world, target)
            ))
        }
        Command::Kick { name } => {
            let Some(e) = find(world, &name) else {
                return Some(absent(&name));
            };
            doom(world, e, Some(KickReason::Kicked));
            Some(format!("{} was kicked", name_of(world, e)))
        }
        Command::Announce { text } => {
            if let Err(why) = chat::check(&text) {
                return Some(match why {
                    ChatRefusal::TooLong => "not announced: over 140 characters".into(),
                    _ => "not announced: control characters or markup".into(),
                });
            }
            let everyone: Vec<Entity> = world.resource::<Index>().conns.values().copied().collect();
            for e in everyone {
                let text = text.clone();
                send(world, e, WorldOut::Chat(ChatOut::Announce { text }));
            }
            None
        }
        Command::Invite { label } => {
            let conn = world.get::<Conn>(admin)?.id;
            let _ = world
                .resource::<Services>()
                .0
                .send(Service::CreateInvite { conn, label });
            None
        }
    }
}

/// The player called `name` (any ASCII case), unless it leaves this tick.
fn find(world: &World, name: &str) -> Option<Entity> {
    let e = *world.resource::<Index>().names.get(&name_key(name))?;
    world.get::<Doomed>(e).is_none().then_some(e)
}

fn name_of(world: &World, e: Entity) -> String {
    world
        .get::<Actor>(e)
        .map(|a| a.look.name.clone())
        .unwrap_or_default()
}

fn absent(name: &str) -> String {
    format!("no one called {name} is in the game")
}

/// Moves `e` to `at`. Its viewers forget it, and see it again where it lands at their next
/// interest pass. Within its scene it is moved (`Teleport`); another scene is loaded as on
/// entering.
fn teleport(world: &mut World, e: Entity, at: Location) {
    let now = world.resource::<Now>().0;
    let (Some(actor), Some(here), Some(stage)) = (
        world.get::<Actor>(e),
        world.get::<Here>(e),
        world.get::<Stage>(e),
    ) else {
        return;
    };
    let (pid, same_scene) = (actor.pid, here.0.scene == at.scene);
    let in_world = *stage == Stage::InWorld;
    hide(world, pid);
    if let Some(mut here) = world.get_mut::<Here>(e) {
        here.0 = at;
    }
    if let Some(mut gait) = world.get_mut::<Gait>(e) {
        *gait = Gait::new(now);
    }
    if same_scene && in_world {
        send(world, e, WorldOut::Teleport { pos: at.pos });
        return;
    }
    if let Some(mut stage) = world.get_mut::<Stage>(e) {
        *stage = Stage::Loading;
    }
    if let Some(mut sight) = world.get_mut::<Sight>(e) {
        sight.0.clear();
    }
    send(world, e, WorldOut::LoadScene { pid, at });
}

#[cfg(test)]
mod tests {
    use super::super::ecs::test_util::*;
    use super::*;

    fn bang(conn: ConnId, text: &str) -> WorldIn {
        WorldIn::Chat {
            conn,
            line: ChatIn {
                channel: ChatChannel::Nearby,
                text: text.into(),
                to: None,
            },
        }
    }

    fn system(text: &str) -> WorldOut {
        WorldOut::Chat(ChatOut::System { text: text.into() })
    }

    /// An admin (1) and two players: 2 near the admin, 3 far away.
    fn admins_world() -> Harness {
        let mut h = Harness::new();
        h.enter(Player {
            admin: true,
            ..player(1, "Lin", at(0.0, 0.0))
        });
        h.enter(player(2, "Mai", at(100.0, 0.0)));
        h.enter(player(3, "Ren", at(50_000.0, 0.0)));
        h.push(WorldIn::Pull { conn: 2, pid: 1 });
        h.tick(50);
        for c in 1..=3 {
            h.take(c);
        }
        h
    }

    #[test]
    fn tp_takes_the_admin_to_a_player_and_others_see_it_move() {
        let mut h = admins_world();
        h.push(bang(1, "!tp ren"));
        h.tick(50);
        assert_eq!(
            h.take(1),
            vec![
                WorldOut::Teleport {
                    pos: at(50_000.0, 0.0).pos
                },
                system("Lin is with Ren")
            ]
        );
        assert_eq!(h.take(2), vec![WorldOut::Leave { pids: vec![1] }]);
        h.tick(50);
        assert!(matches!(
            h.take(3).as_slice(),
            [WorldOut::Announce { pid: 1, .. }]
        ));
    }

    #[test]
    fn tp_with_two_names_moves_the_first_and_names_are_checked() {
        let mut h = admins_world();
        h.push(bang(1, "!tp Ren Mai"));
        h.push(bang(1, "!tp Nobody"));
        h.push(bang(1, "!tp Lin Lin"));
        h.tick(50);
        assert_eq!(
            h.take(3),
            vec![WorldOut::Teleport {
                pos: at(100.0, 0.0).pos
            }]
        );
        assert_eq!(
            h.take(1),
            vec![
                system("Ren is with Mai"),
                system("no one called Nobody is in the game"),
                system("Lin is already there")
            ]
        );
    }

    #[test]
    fn tp_to_another_scene_loads_it() {
        let mut h = admins_world();
        let elsewhere = Location {
            scene: SCENE + 1,
            ..at(0.0, 0.0)
        };
        h.enter(player(4, "Kai", elsewhere));
        h.push(bang(1, "!tp Kai"));
        h.tick(50);
        assert_eq!(
            h.take(1),
            vec![
                WorldOut::LoadScene {
                    pid: 1,
                    at: elsewhere
                },
                system("Lin is with Kai")
            ]
        );
        h.push(WorldIn::Loaded { conn: 1 });
        h.tick(50);
        assert!(matches!(
            h.take(1).as_slice(),
            [
                WorldOut::SelfEnter { .. },
                WorldOut::Announce { pid: 4, .. }
            ]
        ));
    }

    #[test]
    fn kick_lets_go_of_the_player_and_its_viewers_hear_it_left() {
        let mut h = admins_world();
        h.push(bang(1, "!kick MAI"));
        h.tick(50);
        assert_eq!(h.take(2), vec![WorldOut::Kicked(KickReason::Kicked)]);
        assert!(h.closed(2));
        assert_eq!(
            h.take(1),
            vec![system("Mai was kicked"), WorldOut::Leave { pids: vec![2] }]
        );
        assert_eq!(h.core.players(), 2);
    }

    #[test]
    fn announce_reaches_everyone_and_markup_is_refused() {
        let mut h = admins_world();
        h.push(bang(1, "!announce Restart at 20:00"));
        h.push(bang(1, "!announce [link type=\"player\"]"));
        h.tick(50);
        let note = WorldOut::Chat(ChatOut::Announce {
            text: "Restart at 20:00".into(),
        });
        assert_eq!(
            h.take(1),
            vec![
                note.clone(),
                system("not announced: control characters or markup")
            ]
        );
        assert_eq!(h.take(2), vec![note.clone()]);
        assert_eq!(h.take(3), vec![note]);
    }

    #[test]
    fn invite_goes_to_the_services_and_the_answer_comes_back_as_a_told_line() {
        let mut h = admins_world();
        h.saves();
        h.push(bang(1, "!invite for Bob"));
        h.tick(50);
        assert_eq!(
            h.saves(),
            vec![Service::CreateInvite {
                conn: 1,
                label: Some("for Bob".into())
            }]
        );
        h.push(WorldIn::Tell {
            conn: 1,
            text: "invite link: http://127.0.0.1:7801/#invite=X".into(),
        });
        h.tick(50);
        assert_eq!(
            h.take(1),
            vec![system("invite link: http://127.0.0.1:7801/#invite=X")]
        );
    }

    #[test]
    fn usage_and_unknown_commands_answer_the_admin_only() {
        let mut h = admins_world();
        h.push(bang(1, "!kick"));
        h.push(bang(1, "!give sword"));
        h.tick(50);
        assert_eq!(
            h.take(1),
            vec![
                system("usage: kick <name>"),
                system("no command give; the commands are tp, kick, announce and invite")
            ]
        );
        assert!(h.take(2).is_empty());
    }
}
