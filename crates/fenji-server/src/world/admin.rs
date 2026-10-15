//! Admin commands in the game (world spec §3.6): what `!tp`, `!kick`, `!announce` and `!invite`
//! do. The chat phase spends a chat token on an admin's `!` line, parses it with
//! [`crate::commands`] and queues it here; the answer is a line only the admin sees. Each command
//! run is logged, so a host can see what its admins did.
use bevy_ecs::prelude::*;

use super::chat;
use super::ecs::{
    Account, Actor, Conn, Doomed, Here, Index, Now, Pending, Services, Stage, doom, hide, name_key,
    send,
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
        // Commands are plain text (`commands::parse`), so they log as they are.
        let account = world.get::<Account>(admin).map_or(0, |a| a.id);
        let character = name_of(world, admin);
        tracing::info!(account, character, command = ?order, "an admin's command");
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
            // Its client is loading a scene, and its one report would finish both that scene and
            // another sent now.
            if world.get::<Stage>(mover) != Some(&Stage::InWorld) {
                return Some(format!("{} is still loading", name_of(world, mover)));
            }
            let at = world.get::<Here>(target)?.0;
            // A teleport to another scene waits for a local run (#280, Ruling R24; `teleport`).
            if world.get::<Here>(mover)?.0.scene != at.scene {
                return Some(format!("{} is in another scene", name_of(world, target)));
            }
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

/// Moves `e`, which stands in the world, to `at`. Its viewers forget it, and see it again where it
/// lands at their next interest pass. Within its scene it is moved (`Teleport`), near or far.
/// Another scene would be loaded as on entering, but [`run`] refuses that for now (#280).
fn teleport(world: &mut World, e: Entity, at: Location) {
    let now = world.resource::<Now>().0;
    let (Some(actor), Some(here)) = (world.get::<Actor>(e), world.get::<Here>(e)) else {
        return;
    };
    let (pid, same_scene) = (actor.pid, here.0.scene == at.scene);
    hide(world, pid);
    if let Some(mut here) = world.get_mut::<Here>(e) {
        here.0 = at;
    }
    if let Some(mut gait) = world.get_mut::<Gait>(e) {
        *gait = Gait::new(now);
    }
    if same_scene {
        send(world, e, WorldOut::Teleport { pos: at.pos });
        return;
    }
    // Unreachable for now: `run` refuses a teleport to another scene (#280, Ruling R24), since no
    // run has tried this path. A local run must verify it first: a 772 to a client already in the
    // world, then, on its type 3, a second 7 and 8 for the player's own pid, where world.md 1.1
    // reads that a later 7 for an existing id only resets that actor's properties. And before the
    // 772 this branch must send the mover a 9 for each player in its view (`Sight`), which it now
    // clears without telling the client.
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

    /// The admin's line of nearby chat.
    fn lin_says(text: &str) -> WorldOut {
        WorldOut::Chat(ChatOut::Said {
            channel: ChatChannel::Nearby,
            from: Speaker {
                pid: 1,
                name: "Lin".into(),
                vocation: 0,
            },
            to: None,
            text: text.into(),
        })
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
    fn tp_across_scenes_is_refused_and_nothing_moves() {
        // #280, Ruling R24: no run has tried the client's side of a teleport to another scene, so
        // the world refuses one, whoever moves, until a local run has.
        let mut h = admins_world();
        let elsewhere = Location {
            scene: SCENE + 1,
            ..at(0.0, 0.0)
        };
        h.enter(player(4, "Kai", elsewhere));
        h.push(bang(1, "!tp Kai"));
        h.push(bang(1, "!tp Kai Ren"));
        h.push(bang(1, "!tp Mai Kai"));
        h.tick(50);
        assert_eq!(
            h.take(1),
            vec![
                system("Kai is in another scene"),
                system("Ren is in another scene"),
                system("Kai is in another scene")
            ]
        );
        // The next tick's interest pass would show anyone who had moved.
        h.tick(50);
        for c in 1..=4 {
            assert!(h.take(c).is_empty(), "{c}: nobody moved");
        }
        // Within its scene, the admin still goes as before.
        h.push(bang(1, "!tp Ren"));
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
    }

    #[test]
    fn tp_of_a_player_still_loading_answers_so_and_changes_nothing() {
        let mut h = admins_world();
        let elsewhere = Location {
            scene: SCENE + 1,
            ..at(0.0, 0.0)
        };
        h.join(player(4, "Kai", elsewhere));
        h.tick(50);
        h.take(4);
        h.push(bang(1, "!tp Kai Ren"));
        h.tick(50);
        assert_eq!(h.take(1), vec![system("Kai is still loading")]);
        assert!(h.take(4).is_empty(), "no second scene to load");
        h.push(WorldIn::Loaded { conn: 4 });
        h.tick(50);
        assert_eq!(
            h.take(4),
            vec![WorldOut::SelfEnter {
                pid: 4,
                look: look("Kai"),
                pos: elsewhere.pos,
                speed: SPEED_RATE
            }],
            "where it was going"
        );
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
    fn an_announcement_may_have_140_characters_and_no_more() {
        let mut h = admins_world();
        let longest = "a".repeat(140);
        h.push(bang(1, &format!("!announce {longest}")));
        h.push(bang(1, &format!("!announce {}", "a".repeat(141))));
        h.tick(50);
        let note = WorldOut::Chat(ChatOut::Announce { text: longest });
        assert_eq!(
            h.take(1),
            vec![note.clone(), system("not announced: over 140 characters")]
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
    fn commands_spend_the_admins_chat_tokens_and_one_too_fast_does_not_run() {
        let mut h = admins_world();
        for _ in 0..4 {
            h.push(bang(1, "hi"));
        }
        h.push(bang(1, "!announce one"));
        h.push(bang(1, "!kick Mai"));
        h.tick(50);
        let announced = |text: &str| WorldOut::Chat(ChatOut::Announce { text: text.into() });
        let mut lin = vec![lin_says("hi"); 4];
        lin.extend([
            WorldOut::ChatRefused(ChatRefusal::TooFast),
            announced("one"),
        ]);
        assert_eq!(h.take(1), lin, "the sixth line, a command, is too fast");
        assert_eq!(h.core.players(), 3, "and didn't run: Mai stays");
        // A second later, one more.
        h.push(bang(1, "!announce two"));
        h.push(bang(1, "!announce three"));
        h.tick(1_000);
        assert_eq!(
            h.take(1),
            vec![
                WorldOut::ChatRefused(ChatRefusal::TooFast),
                announced("two")
            ]
        );
        assert_eq!(h.take(3), vec![announced("one"), announced("two")]);
    }

    #[test]
    fn each_command_run_is_logged_with_its_account_and_arguments() {
        let mut h = admins_world();
        let ((), log) = logged(|| {
            h.push(bang(1, "!kick Mai"));
            h.push(bang(1, "!give sword"));
            h.push(bang(1, "!announce Restart at 20:00"));
            h.tick(50);
        });
        let runs: Vec<&str> = log.lines().filter(|l| l.contains("account=1")).collect();
        assert_eq!(runs.len(), 2, "one line for each command run: {log}");
        assert!(runs[0].contains(r#"Kick { name: "Mai" }"#), "{log}");
        assert!(
            runs[1].contains(r#"Announce { text: "Restart at 20:00" }"#),
            "{log}"
        );
    }

    #[test]
    fn an_admins_bang_and_a_digit_is_plain_chat() {
        let mut h = admins_world();
        h.push(bang(1, "!1"));
        h.tick(50);
        assert_eq!(h.take(1), vec![lin_says("!1")], "echoed, with no answer");
        assert_eq!(h.take(2), vec![lin_says("!1")]);
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
