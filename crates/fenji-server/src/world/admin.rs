//! Admin commands (world spec §3.6; M4 hosting spec §3.3): what `tp`, `kick`, `announce` and
//! `invite` do, for an admin in the game, and the first three for the host's console. The chat
//! phase spends a chat token on an admin's `!` line, parses it with [`crate::commands`] and queues
//! it here, and its answer is a line only the admin sees. The console's come as
//! `WorldIn::Console`, from an admin with no character in the world, and its answers go back on its
//! channel; the console service makes the console's invites itself. Each command run is logged, so
//! a host can see what its admins did.
use bevy_ecs::prelude::*;
use tokio::sync::oneshot;

use super::chat;
use super::ecs::{
    Account, Actor, Conn, Doomed, Here, Index, Now, Pending, Services, Stage, doom, hide, name_key,
    send,
};
use super::interest::Sight;
use super::movement::Gait;
use super::types::*;
use crate::commands::{Command, CommandError, TP_CONSOLE};

/// Who runs a command, and so where its answer goes.
pub(super) enum Caller {
    /// An admin in the game: the answer is a line only it sees.
    Admin(Entity),
    /// The host's console, which has no character: the answer goes back on its channel.
    Console(oneshot::Sender<ConsoleReply>),
}

/// What a command came to.
enum Done {
    /// It did what was asked: an answer for its caller.
    Answer(String),
    /// It didn't, and nothing changed: why, for its caller. An admin reads it as any answer; the
    /// console hears it wasn't done.
    Refused(String),
    /// An announcement, sent to this many players. An admin sees it as everyone does, and the
    /// console hears how many it reached.
    Announced(usize),
    /// An invite to make off the tick, with this label: the services task answers.
    Invite(Option<String>),
}

/// Runs the tick's commands, in order.
pub(super) fn system(world: &mut World) {
    for (caller, order) in std::mem::take(&mut world.resource_mut::<Pending>().orders) {
        // Commands are plain text (`commands::parse`), so they log as they are, as typed.
        let me = match &caller {
            Caller::Admin(admin) if world.get::<Doomed>(*admin).is_some() => continue,
            Caller::Admin(admin) => {
                // By its id, which isn't a name: the log shows `account` as one (#359).
                let account_id = world.get::<Account>(*admin).map_or(0, |a| a.id);
                let character = name_of(world, *admin);
                tracing::info!(account_id, character, command = %order, "an admin's command");
                Some(*admin)
            }
            Caller::Console(_) => {
                tracing::info!(by = "console", command = %order, "an admin's command");
                None
            }
        };
        if let Some(done) = run(world, me, order) {
            answer(world, caller, done);
        }
    }
}

/// Does `order` for the admin `me`, or for the console (`None`), and says what it came to, if
/// anything.
fn run(world: &mut World, me: Option<Entity>, order: Command) -> Option<Done> {
    match order {
        Command::Tp { who, to } => tp(world, me, who, &to),
        Command::Kick { name } => {
            let Some(e) = find(world, &name) else {
                return Some(Done::Refused(absent(&name)));
            };
            doom(world, e, Some(KickReason::Kicked));
            Some(Done::Answer(format!("{} was kicked", name_of(world, e))))
        }
        Command::Announce { text } => Some(announce(world, &text)),
        Command::Invite { label } => Some(Done::Invite(label)),
    }
}

/// Takes `who`, or the admin `me` when it names one player, to the player `to`, and answers: done
/// once it's moved, refused otherwise.
fn tp(world: &mut World, me: Option<Entity>, who: Option<String>, to: &str) -> Option<Done> {
    let mover = match (&who, me) {
        (Some(name), _) => find(world, name),
        (None, Some(me)) => Some(me),
        // The console has no character to take anywhere.
        (None, None) => {
            return Some(Done::Refused(CommandError::Usage(TP_CONSOLE).to_string()));
        }
    };
    let Some(mover) = mover else {
        return Some(Done::Refused(absent(who.as_deref().unwrap_or_default())));
    };
    let Some(target) = find(world, to) else {
        return Some(Done::Refused(absent(to)));
    };
    if mover == target {
        let mover = name_of(world, mover);
        return Some(Done::Refused(format!("{mover} is already there")));
    }
    // Its client is loading a scene, and its one report would finish both that scene and
    // another sent now.
    if world.get::<Stage>(mover) != Some(&Stage::InWorld) {
        let mover = name_of(world, mover);
        return Some(Done::Refused(format!("{mover} is still loading")));
    }
    let at = world.get::<Here>(target)?.0;
    // A teleport to another scene waits for a local run (#280, Ruling R24; `teleport`). With one
    // name the admin is the mover, so the answer names the target alone.
    if world.get::<Here>(mover)?.0.scene != at.scene {
        return Some(Done::Refused(match who {
            None => format!("{} is in another scene", name_of(world, target)),
            Some(_) => format!(
                "{} and {} are in different scenes",
                name_of(world, mover),
                name_of(world, target)
            ),
        }));
    }
    teleport(world, mover, at);
    Some(Done::Answer(format!(
        "{} is with {}",
        name_of(world, mover),
        name_of(world, target)
    )))
}

/// Sends everyone in the game `text`, as its caller typed it, on the marquee. It goes out as the
/// client's input would write it, `&`, `<` and `>` as entities (chat.md 1.5), and is checked as a
/// player's line is.
fn announce(world: &mut World, text: &str) -> Done {
    let text = chat::escape(text);
    if let Err(why) = chat::check(&text) {
        return Done::Refused(match why {
            ChatRefusal::TooLong => "not announced: over 140 characters".into(),
            _ => "not announced: control characters or markup".into(),
        });
    }
    let everyone: Vec<Entity> = world.resource::<Index>().conns.values().copied().collect();
    for &e in &everyone {
        let text = text.clone();
        send(world, e, WorldOut::Chat(ChatOut::Announce { text }));
    }
    Done::Announced(everyone.len())
}

/// Gives `caller` what its command came to: a line to an admin, the console's answer on its
/// channel, saying whether it was done, and an admin's invite to the services task, which tells
/// the admin its link.
fn answer(world: &mut World, caller: Caller, done: Done) {
    let services = |world: &World, job| {
        let _ = world.resource::<Services>().0.send(job);
    };
    match (caller, done) {
        (Caller::Admin(admin), Done::Answer(text) | Done::Refused(text)) => {
            send(world, admin, WorldOut::Chat(ChatOut::System { text }));
        }
        (Caller::Admin(_), Done::Announced(_)) => {}
        (Caller::Admin(admin), Done::Invite(label)) => {
            if let Some(conn) = world.get::<Conn>(admin).map(|c| c.id) {
                services(world, Service::CreateInvite { conn, label });
            }
        }
        (Caller::Console(reply), Done::Answer(text)) => {
            let _ = reply.send(ConsoleReply { ok: true, text });
        }
        (Caller::Console(reply), Done::Refused(text)) => {
            let _ = reply.send(ConsoleReply { ok: false, text });
        }
        (Caller::Console(reply), Done::Announced(n)) => {
            let text = match n {
                0 => "announced, but no one is in the game".into(),
                1 => "announced to the one player in the game".into(),
                n => format!("announced to the {n} players in the game"),
            };
            let _ = reply.send(ConsoleReply { ok: true, text });
        }
        // The console service makes the console's, with a link for each of the machine's
        // addresses (Amendment A, HA4): one asked of the world is refused.
        (Caller::Console(reply), Done::Invite(_)) => {
            let text = "the console makes its own invites".into();
            let _ = reply.send(ConsoleReply { ok: false, text });
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
/// Another scene would be loaded as on entering, but [`tp`] refuses that for now (#280).
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
    // Unreachable for now: `tp` refuses a teleport to another scene (#280, Ruling R24), since no
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
    use tokio::sync::oneshot;

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
        // One name is the admin's own move; with two, either may be the one elsewhere.
        assert_eq!(
            h.take(1),
            vec![
                system("Kai is in another scene"),
                system("Kai and Ren are in different scenes"),
                system("Mai and Kai are in different scenes")
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
        let runs: Vec<&str> = log.lines().filter(|l| l.contains("account_id=1")).collect();
        assert_eq!(runs.len(), 2, "one line for each command run: {log}");
        // The account by its id, which isn't a name: the console shows `account` in gold, as one
        // (#359, the hosting plan's Task 7).
        assert!(!log.contains("account=1"), "{log}");
        // As the admin typed them, not in Rust's debug form (the hosting plan's final review B,
        // Minor 3).
        assert!(runs[0].contains("command=kick Mai"), "{log}");
        assert!(
            runs[1].contains("command=announce Restart at 20:00"),
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

    #[test]
    fn a_commands_arguments_are_what_the_admin_typed_not_the_clients_entities() {
        // The client's input writes &, < and > as entities (chat.md 1.5). A name is found and
        // echoed as the admin typed it, and a label is checked and kept so: 64 of `&amp;` are 64
        // characters.
        let mut h = admins_world();
        h.saves();
        h.push(bang(1, "!tp Nobody&lt;3"));
        h.push(bang(1, &format!("!invite {}", "&amp;".repeat(64))));
        h.push(bang(1, "!invite Bob &amp; Kai &lt;3"));
        h.tick(50);
        assert_eq!(
            h.take(1),
            vec![system("no one called Nobody<3 is in the game")]
        );
        assert_eq!(
            h.saves(),
            vec![
                Service::CreateInvite {
                    conn: 1,
                    label: Some("&".repeat(64))
                },
                Service::CreateInvite {
                    conn: 1,
                    label: Some("Bob & Kai <3".into())
                }
            ]
        );
    }

    #[test]
    fn an_admins_announcement_goes_out_as_the_client_wrote_it() {
        let mut h = admins_world();
        h.push(bang(1, "!announce Tom &amp; Jerry &lt;3"));
        h.tick(50);
        let note = WorldOut::Chat(ChatOut::Announce {
            text: "Tom &amp; Jerry &lt;3".into(),
        });
        for c in 1..=3 {
            assert_eq!(h.take(c), vec![note.clone()], "{c}");
        }
    }

    // --- The host's console (M4 hosting spec §3.3): an admin with no character.

    /// The console's `order`, and the channel its answer comes back on.
    fn console(order: Command) -> (WorldIn, oneshot::Receiver<ConsoleReply>) {
        let (reply, answer) = oneshot::channel();
        (WorldIn::Console { order, reply }, answer)
    }

    /// The world's answer to the console when the command did what was asked.
    fn done(text: &str) -> ConsoleReply {
        ConsoleReply {
            ok: true,
            text: text.into(),
        }
    }

    /// The world's answer to the console when it refused the command.
    fn refused(text: &str) -> ConsoleReply {
        ConsoleReply {
            ok: false,
            text: text.into(),
        }
    }

    #[test]
    fn the_console_moves_one_player_to_another_and_hears_back() {
        let mut h = Harness::new();
        h.enter(player(1, "Mai", at(0.0, 0.0)));
        h.enter(player(2, "Lin", at(5000.0, 0.0)));
        let (reply, mut answer) = oneshot::channel();
        h.push(WorldIn::Console {
            order: Command::Tp {
                who: Some("Mai".into()),
                to: "Lin".into(),
            },
            reply,
        });
        h.tick(50);
        assert_eq!(answer.try_recv().unwrap(), done("Mai is with Lin"));
        assert!(
            h.take(1)
                .iter()
                .any(|m| matches!(m, WorldOut::Teleport { .. }))
        );
    }

    #[test]
    fn the_console_kicks_and_announces() {
        let mut h = Harness::new();
        h.enter(player(1, "Mai", at(0.0, 0.0)));
        let (reply, mut answer) = oneshot::channel();
        h.push(WorldIn::Console {
            order: Command::Announce {
                text: "Maintenance: Updating".into(),
            },
            reply,
        });
        h.tick(50);
        assert_eq!(
            answer.try_recv().unwrap(),
            done("announced to the one player in the game")
        );
        assert!(
            h.take(1)
                .iter()
                .any(|m| matches!(m, WorldOut::Chat(ChatOut::Announce { .. })))
        );
        let (reply, mut answer) = oneshot::channel();
        h.push(WorldIn::Console {
            order: Command::Kick { name: "mai".into() },
            reply,
        });
        h.tick(50);
        assert_eq!(answer.try_recv().unwrap(), done("Mai was kicked"));
        assert_eq!(h.take(1), vec![WorldOut::Kicked(KickReason::Kicked)]);
        assert!(h.closed(1));
    }

    #[test]
    fn the_console_has_no_character_so_its_tp_takes_two_names() {
        let mut h = admins_world();
        let (order, mut answer) = console(Command::Tp {
            who: None,
            to: "Ren".into(),
        });
        h.push(order);
        h.tick(50);
        assert_eq!(
            answer.try_recv().unwrap(),
            refused("usage: tp <name> <name>")
        );
        h.tick(50);
        for c in 1..=3 {
            assert!(h.take(c).is_empty(), "{c}: nobody moved");
        }
    }

    #[test]
    fn the_consoles_answers_go_to_the_console_alone() {
        // Each is a refusal: nothing the host asked for happened, which the console's `ok` says
        // (review 1, Minor 2).
        let mut h = admins_world();
        let longest = "a".repeat(141);
        for (order, want) in [
            (
                Command::Tp {
                    who: Some("Nobody".into()),
                    to: "Ren".into(),
                },
                "no one called Nobody is in the game",
            ),
            (
                Command::Tp {
                    who: Some("Ren".into()),
                    to: "Nobody".into(),
                },
                "no one called Nobody is in the game",
            ),
            (
                Command::Tp {
                    who: Some("Ren".into()),
                    to: "REN".into(),
                },
                "Ren is already there",
            ),
            (
                Command::Kick {
                    name: "Nobody".into(),
                },
                "no one called Nobody is in the game",
            ),
            (
                Command::Announce {
                    text: "[link type=\"player\"]".into(),
                },
                "not announced: control characters or markup",
            ),
            (
                Command::Announce { text: longest },
                "not announced: over 140 characters",
            ),
        ] {
            let (order, mut answer) = console(order);
            h.push(order);
            h.tick(50);
            assert_eq!(answer.try_recv().unwrap(), refused(want));
        }
        for c in 1..=3 {
            assert!(h.take(c).is_empty(), "{c}: heard the console's answer");
        }
    }

    #[test]
    fn the_console_hears_a_tp_across_scenes_or_of_a_player_loading_refused() {
        let mut h = admins_world();
        let elsewhere = Location {
            scene: SCENE + 1,
            ..at(0.0, 0.0)
        };
        h.enter(player(4, "Kai", elsewhere));
        h.join(player(5, "Ivy", elsewhere));
        h.tick(50);
        for (who, to, want) in [
            ("Kai", "Ren", "Kai and Ren are in different scenes"),
            ("Ivy", "Kai", "Ivy is still loading"),
        ] {
            let (order, mut answer) = console(Command::Tp {
                who: Some(who.into()),
                to: to.into(),
            });
            h.push(order);
            h.tick(50);
            assert_eq!(answer.try_recv().unwrap(), refused(want));
        }
    }

    #[test]
    fn the_consoles_announcement_is_escaped_for_the_client_and_says_who_saw_it() {
        // The console's text is raw, and the client takes &, < and > only as its own input writes
        // them (chat.md 1.5), so the world writes them so first.
        let mut h = Harness::new();
        let (order, mut answer) = console(Command::Announce {
            text: "Hello".into(),
        });
        h.push(order);
        h.tick(50);
        assert_eq!(
            answer.try_recv().unwrap(),
            done("announced, but no one is in the game")
        );
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        let (order, mut answer) = console(Command::Announce {
            text: "Fish & chips <3".into(),
        });
        h.push(order);
        h.tick(50);
        assert_eq!(
            answer.try_recv().unwrap(),
            done("announced to the one player in the game")
        );
        let fish = WorldOut::Chat(ChatOut::Announce {
            text: "Fish &amp; chips &lt;3".into(),
        });
        assert_eq!(h.take(1), vec![fish.clone()]);
        let mut h = admins_world();
        let (order, mut answer) = console(Command::Announce {
            text: "Fish & chips <3".into(),
        });
        h.push(order);
        h.tick(50);
        assert_eq!(
            answer.try_recv().unwrap(),
            done("announced to the 3 players in the game")
        );
        for c in 1..=3 {
            assert_eq!(h.take(c), vec![fish.clone()], "{c}");
        }
    }

    #[test]
    fn the_world_makes_no_invite_for_the_console() {
        // The hosting plan's final review, Minor 5: the console's `invite` runs in the console
        // service, which gives a link for each of the machine's addresses (HA4). An invite asked
        // of the world for the console is refused, and nothing is made.
        let mut h = admins_world();
        h.saves();
        let (order, mut answer) = console(Command::Invite {
            label: Some("for Bob".into()),
        });
        h.push(order);
        h.tick(50);
        assert_eq!(h.saves(), []);
        assert_eq!(
            answer.try_recv().unwrap(),
            refused("the console makes its own invites")
        );
        for c in 1..=3 {
            assert!(h.take(c).is_empty(), "{c}");
        }
    }

    #[test]
    fn each_console_command_is_logged_as_the_consoles() {
        let mut h = admins_world();
        let ((), log) = logged(|| {
            let (order, _answer) = console(Command::Kick { name: "Mai".into() });
            h.push(order);
            h.tick(50);
        });
        let runs: Vec<&str> = log
            .lines()
            .filter(|l| l.contains("an admin's command"))
            .collect();
        assert_eq!(runs.len(), 1, "{log}");
        assert!(runs[0].contains("by=\"console\""), "{log}");
        assert!(runs[0].contains("command=kick Mai"), "{log}");
    }

    // --- Ops and bans from the console (M4 hosting spec §8, HA5).

    #[test]
    fn op_and_deop_take_effect_at_once() {
        let mut h = admins_world();
        // Lin (account 1) is deopped: its `!kick` the next tick is chat, and Mai stays.
        h.push(WorldIn::SetAdmin {
            account: 1,
            admin: false,
        });
        h.tick(50);
        h.push(bang(1, "!kick Mai"));
        h.tick(50);
        assert_eq!(h.take(1), vec![lin_says("!kick Mai")]);
        assert_eq!(h.take(2), vec![lin_says("!kick Mai")]);
        assert_eq!(h.core.players(), 3, "Mai stays");
        // Mai (2) is opped: a command after it in the same tick runs. An account in no one's
        // hands changes nothing.
        h.push(WorldIn::SetAdmin {
            account: 99,
            admin: true,
        });
        h.push(WorldIn::SetAdmin {
            account: 2,
            admin: true,
        });
        h.push(bang(2, "!kick Ren"));
        h.tick(50);
        assert_eq!(h.take(3), vec![WorldOut::Kicked(KickReason::Kicked)]);
        assert_eq!(h.take(2), vec![system("Ren was kicked")]);
        assert_eq!(h.core.players(), 2);
    }

    #[test]
    fn a_change_of_admin_that_reaches_the_world_before_the_join_holds() {
        // The link reads whether the account is an admin, then joins: a `deop` between the two
        // reaches the world first, and the join mustn't bring the old flag back. An `op` likewise.
        let mut h = Harness::new();
        h.enter(player(2, "Mai", at(100.0, 0.0)));
        h.enter(player(3, "Ren", at(50_000.0, 0.0)));
        h.push(WorldIn::SetAdmin {
            account: 1,
            admin: false,
        });
        h.push(WorldIn::SetAdmin {
            account: 2,
            admin: true,
        });
        h.tick(50);
        h.enter(Player {
            admin: true,
            ..player(1, "Lin", at(0.0, 0.0))
        });
        h.push(bang(1, "!kick Mai"));
        h.tick(50);
        assert_eq!(
            h.take(1),
            vec![lin_says("!kick Mai")],
            "chat, not a command"
        );
        assert_eq!(h.core.players(), 3, "Mai stays");
        // Mai was in the world when she was made an admin: that took at once.
        h.take(2);
        h.push(bang(2, "!kick Ren"));
        h.tick(50);
        assert_eq!(h.take(3), vec![WorldOut::Kicked(KickReason::Kicked)]);
        assert_eq!(h.core.players(), 2);
    }

    #[test]
    fn a_change_of_admin_holds_for_one_join_only() {
        let mut h = Harness::new();
        h.enter(player(2, "Mai", at(50_000.0, 0.0)));
        h.push(WorldIn::SetAdmin {
            account: 1,
            admin: true,
        });
        h.tick(50);
        // An `op` before the join, whose link read the account before it: a command runs.
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        h.push(bang(1, "!announce one"));
        h.tick(50);
        let one = WorldOut::Chat(ChatOut::Announce { text: "one".into() });
        assert_eq!(h.take(1), vec![one]);
        // Once used, it's forgotten: the same account's next join brings its own link's reading.
        h.push(WorldIn::Leave { conn: 1 });
        h.tick(50);
        h.enter(Player {
            account: 1,
            ..player(4, "Lin", at(0.0, 0.0))
        });
        h.push(bang(4, "!announce two"));
        h.tick(50);
        let said = WorldOut::Chat(ChatOut::Said {
            channel: ChatChannel::Nearby,
            from: Speaker {
                pid: 4,
                name: "Lin".into(),
                vocation: 0,
            },
            to: None,
            text: "!announce two".into(),
        });
        assert_eq!(h.take(4), vec![said]);
    }

    #[test]
    fn a_deop_while_in_the_world_holds_for_a_newer_login_too() {
        // A newer login's link may read the account before the deop, while the older connection
        // is still in the world, which takes the deop: the newer join mustn't undo it.
        let mut h = admins_world();
        h.push(WorldIn::SetAdmin {
            account: 1,
            admin: false,
        });
        h.tick(50);
        h.enter(Player {
            conn: 4,
            admin: true,
            ..player(1, "Lin", at(0.0, 0.0))
        });
        h.push(bang(4, "!kick Mai"));
        h.tick(50);
        assert_eq!(h.core.players(), 3, "Mai stays");
    }

    #[test]
    fn a_ban_lets_the_accounts_player_go_as_a_kick_does() {
        let mut h = admins_world();
        h.push(WorldIn::Ban { account: 2 });
        h.push(WorldIn::Ban { account: 99 });
        h.tick(50);
        assert_eq!(h.take(2), vec![WorldOut::Kicked(KickReason::Kicked)]);
        assert!(h.closed(2));
        assert_eq!(h.take(1), vec![WorldOut::Leave { pids: vec![2] }]);
        assert!(h.take(3).is_empty());
        assert_eq!(h.core.players(), 2);
    }
}
