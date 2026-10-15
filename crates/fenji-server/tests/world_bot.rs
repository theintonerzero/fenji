//! The protocol bot plays the world (M4 world plan, Task 17), as the plan's Amendment D scripts it:
//! two bots sign in, enter, see each other, walk and chat on all three channels, an admin bot runs
//! each command, and a bot that leaves is a 9 for the bot that saw it. Every reply is checked
//! through fenji-proto's codecs, never by hand-copied bytes. First, without a server, the bot's own
//! messages are checked against the real client's recorded bytes and the server's decoders, and its
//! readings against the codecs that build what it reads. These run in CI; `tests/load.rs` has
//! twenty bots measure the tick.
mod common;

use std::path::Path;
use std::time::Duration;

use common::*;
use fenji_proto::chat::channel::{NEARBY, WHISPER, WORLD};
use fenji_proto::chat::{self, Channel};
use fenji_proto::lobby::{self, CreateRequest};
use fenji_proto::opcode::Cts;
use fenji_proto::pickle::{self, Value};
use fenji_proto::world::{self, MoveDetail, PlayerActor, SyncFilter};
use fenji_server::gate::{KICKED, SERVER_STOPPING};
use fenji_server::server::{GateLimits, RunningServer};
use fenji_store::Place;

// --- The bot's messages and readings, without a server.

/// A body as the fixtures keep it, in hex.
fn fixture(hex: &str) -> Vec<u8> {
    let hex = hex.trim();
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

/// `[world]`'s default start (the plan's Global Constraints): a place for these checks to use.
const START: Place = Place {
    scene: 5,
    x: 2_366.0,
    y: -71_735.0,
    z: 64_854.0,
    yaw: 345.0,
};

fn lin() -> Made {
    Made {
        pid: 1,
        name: "Lin".into(),
        vocation: 7,
        skeleton: 3,
    }
}

fn mai() -> Made {
    Made {
        pid: 2,
        name: "Mai".into(),
        vocation: 2,
        skeleton: 1,
    }
}

#[test]
fn the_bots_lines_are_the_real_clients() {
    // Run t16-c1's 115s, as the real client sent them (chat.md §7): a line with a nil `extra` is
    // the client's to the byte.
    for (channel, text, recorded) in [
        (
            NEARBY,
            "hello from Lin",
            include_str!("fixtures/t16-c1-nearby.hex"),
        ),
        (
            WORLD,
            "hi world from Mai",
            include_str!("fixtures/t16-c1-world.hex"),
        ),
        (2, "team line", include_str!("fixtures/t16-c1-team.hex")),
    ] {
        assert_eq!(
            transmit(channel, text, Value::Nil),
            fixture(recorded),
            "{text}"
        );
    }
    // A whisper holds the client's values in the client's order. Only the header differs: the
    // client's packer counts the tables in it (payloads.md P.3), which `script_body` doesn't, and
    // the server reads both the same.
    let values = |body: &[u8]| pickle::decode(body).unwrap();
    let typed = transmit(WHISPER, "psst Lin from Mai", Whom::Typed("Lin").extra());
    let recorded = fixture(include_str!("fixtures/t16-c1-whisper-typed.hex"));
    assert_eq!(values(&typed), values(&recorded));
    let clicked = Whom::Clicked {
        pid: 2,
        name: "Mai",
    };
    let sent = transmit(WHISPER, "psst Mai from Lin", clicked.extra());
    let recorded = fixture(include_str!("fixtures/t16-c1-whisper-clicked.hex"));
    assert_eq!(values(&sent), values(&recorded));
}

#[test]
fn the_bots_pull_is_the_real_clients() {
    // Run m4-s4's pull of Mai, pid 2 (world.md 14.4), with its machine byte zero.
    let recorded = fixture(include_str!("fixtures/m4-s4-pull.hex"));
    assert_eq!(pull(2), recorded);
    assert_eq!(
        world::sync_filter_set(&pull(2)).unwrap(),
        SyncFilter {
            pid: 2,
            value: world::PULL
        }
    );
}

#[test]
fn a_walk_is_a_type_8_the_link_reads_as_sent_and_viewers_get_its_detail() {
    let (body, to) = walk(START, [2_406, -71_735, 64_854]);
    assert_eq!(
        to,
        Place {
            x: 2_406.0,
            ..START
        },
        "40 units east, facing as before"
    );
    // What the link takes from it: the place as sent, and the detail every viewer's 13 carries.
    let m = world::move_dir(&body, NOW_MS).unwrap();
    assert_eq!(m.pos, [2_406.0, -71_735.0, 64_854.0, 345.0]);
    let sent = MoveDetail {
        time: NOW_MS,
        dir: 1,
        yaw_offset: 0,
        rate: 100,
        pitch: 0,
        pitch2: 0,
    };
    assert_eq!(m.detail, sent);
    assert_eq!(walking(), sent);
}

#[test]
#[should_panic(expected = "at most 50 units")]
fn a_bot_walks_at_most_50_units_a_step() {
    walk(START, [2_366 + 51, -71_735, 64_854]);
}

#[test]
fn the_bot_reads_772_as_load_scene_builds_it() {
    let body = world::load_scene(1, 5, [2_366.0, -71_735.0, 64_854.0, 345.0]);
    assert_eq!(loads_at(1, &body), START);
}

#[test]
fn the_bot_reads_a_378_as_say_builds_it() {
    let whisper = chat::say(&chat::Line::Said {
        channel: Channel::Whisper,
        from: chat::Player {
            pid: 1,
            name: "Lin",
            vocation: 7,
        },
        to: Some(chat::Player {
            pid: 2,
            name: "Mai",
            vocation: 2,
        }),
        text: "psst",
    });
    let want = Heard {
        channel: 1,
        sender: r#" [link type="player" pid="1" name="Lin" vocation="7" gid="0"] "#.into(),
        content: "psst".into(),
        receiver: r#" [link type="player" pid="2" name="Mai" vocation="2" gid="0"] "#.into(),
    };
    assert_eq!(Heard::read(&whisper), want);
    assert_eq!(Heard::said(WHISPER, &lin(), "psst", Some(&mai())), want);
    let told = chat::say(&chat::Line::System {
        text: "Lin is with Kai",
    });
    assert_eq!(Heard::read(&told), Heard::system("Lin is with Kai"));
    assert_eq!(Heard::system("x").channel, 64);
    let note = chat::say(&chat::Line::Announce { text: "Restart" });
    assert_eq!(Heard::read(&note), Heard::marquee("Restart"));
    assert_eq!(Heard::marquee("x").channel, 66);
}

/// Lin's own 7 as `sync_enter_player` builds it, at `level`, standing at `pos`, at `speed`, with
/// `common::request`'s look.
fn lins_seven(level: i64, pos: [f64; 4], speed: i64) -> Vec<u8> {
    let look = CreateRequest::parse(&request("Lin", 7, 3)).unwrap();
    world::sync_enter_player(&PlayerActor {
        pid: 1,
        name: "Lin",
        vocation: 7,
        skeleton: 3,
        level,
        appearance: &look.appearance,
        pos,
        speed,
    })
}

#[test]
fn the_bot_takes_its_own_7_as_sync_enter_player_builds_it() {
    let at = [2_366.0, -71_735.0, 64_854.0, 345.0];
    assert_own_seven(&lin(), START, &lins_seven(37, at, 100));
}

#[test]
#[should_panic(expected = "level")]
fn a_7_at_another_level_is_not_the_bots_own() {
    let at = [2_366.0, -71_735.0, 64_854.0, 345.0];
    assert_own_seven(&lin(), START, &lins_seven(36, at, 100));
}

#[test]
#[should_panic(expected = "where 772 put it")]
fn a_7_somewhere_else_is_not_the_bots_own() {
    let at = [2_367.0, -71_735.0, 64_854.0, 345.0];
    assert_own_seven(&lin(), START, &lins_seven(37, at, 100));
}

#[test]
#[should_panic(expected = "speed")]
fn a_7_at_another_speed_is_not_the_bots_own() {
    let at = [2_366.0, -71_735.0, 64_854.0, 345.0];
    assert_own_seven(&lin(), START, &lins_seven(37, at, 120));
}

// --- The script against a server (Amendment D's steps 1 to 8).

/// A server in `dir` with `extra` in its config, and `[world]`'s start there: where a character
/// that never entered stands.
async fn world_server(dir: &Path, extra: &str) -> (RunningServer, Place) {
    let (s, _clock) = server(dir, extra, GateLimits::default()).await;
    (s, start_of(&config(dir, extra)))
}

/// A bot signed in as the new account `account` (step 1) that has made `name` (step 2), with
/// `common::request`'s look: vocation 7 with skeleton 3, or 2 with 1.
async fn player(s: &RunningServer, account: &str, name: &str, look: (i64, i64)) -> Bot {
    let mut bot = signed_in(s, account).await;
    bot.create(name, look.0, look.1).await;
    bot
}

/// `bot` enters at `at` (step 3): its 10 brings 772 for that place, and its type 3 its own 7 and
/// 8. The body of its 7.
async fn enters(bot: &mut Bot, at: Place) -> Vec<u8> {
    assert_eq!(bot.select(bot.pid()).await, at);
    bot.load().await
}

/// Lin (aaa, a Fox Mage: 7 and 3) and Mai (bbb: 2 and 1) at `[world]`'s start (steps 1 to 4): each
/// was told of the other where it stands (6), and has pulled it (5, then its 7 and 8).
async fn two_who_see_each_other(s: &RunningServer, start: Place) -> (Bot, Bot) {
    let mut a = player(s, "aaa", "Lin", (7, 3)).await;
    let mut b = player(s, "bbb", "Mai", (2, 1)).await;
    enters(&mut a, start).await;
    enters(&mut b, start).await;
    a.expect_announced(b.pid(), start).await;
    b.expect_announced(a.pid(), start).await;
    a.pull(b.pid()).await;
    b.pull(a.pid()).await;
    (a, b)
}

/// Kai (ccc: 7 and 3), whose character last stood at `far`: in the world there, told of no one.
async fn kai_at(s: &RunningServer, far: Place) -> Bot {
    let mut c = player(s, "ccc", "Kai", (7, 3)).await;
    assert!(
        s.accounts()
            .store()
            .save_place(c.pid(), &far)
            .await
            .unwrap()
    );
    enters(&mut c, far).await;
    c
}

/// 1 km east of `start`, in its scene: out of view of whoever stands at the start.
fn far_from(start: Place) -> Place {
    Place {
        x: start.x + 100_000.0,
        ..start
    }
}

/// Nothing else is on its way to `speaker` or `others`: `speaker` says a world line, and it is the
/// next message each of them gets. The world sends each player its messages in the order it makes
/// them, so anything made for one of them before the line would come first.
async fn nothing_else(speaker: &mut Bot, others: &mut [&mut Bot]) {
    speaker.say(WORLD, "that is all").await;
    let line = Heard::said(WORLD, speaker.made(), "that is all", None);
    assert_eq!(speaker.heard().await, line, "{}", speaker.made().name);
    for bot in others.iter_mut() {
        assert_eq!(bot.heard().await, line, "{}", bot.made().name);
    }
}

/// Stops the server with `bots` in the world: each reads why its connection closed (world.md
/// 10.3), which is its next message, and hangs up, as the real client does, so the shutdown waits
/// out no linger.
async fn stop(s: RunningServer, bots: Vec<Bot>) {
    let stopping = tokio::spawn(s.shutdown());
    for mut bot in bots {
        let why = bot.expect_closed().await;
        assert_eq!(why, SERVER_STOPPING.as_bytes(), "{}", bot.made().name);
    }
    stopping.await.unwrap().expect("everyone was saved");
}

#[tokio::test]
async fn two_bots_enter_and_see_each_other() {
    let dir = tempfile::tempdir().unwrap();
    let (s, start) = world_server(dir.path(), "").await;
    // Steps 1 and 2: each signs in and makes a character, in the two forms step 2 names.
    let mut a = player(&s, "aaa", "Lin", (7, 3)).await;
    let mut b = player(&s, "bbb", "Mai", (2, 1)).await;
    // Step 3: each enters at `[world]`'s start, since neither has stood in the world before.
    let lins_own = enters(&mut a, start).await;
    let mais_own = enters(&mut b, start).await;
    // Step 4: in the tick Mai's client loaded, each was told of the other where it stands...
    a.expect_announced(b.pid(), start).await;
    b.expect_announced(a.pid(), start).await;
    // ...and its pull brings the 7 the other's own client got, byte for byte (world.md 14.9).
    assert_eq!(
        a.pull(b.pid()).await,
        mais_own,
        "Lin sees Mai as Mai's client has her"
    );
    assert_eq!(
        b.pull(a.pid()).await,
        lins_own,
        "Mai sees Lin as Lin's client has her"
    );
    nothing_else(&mut a, &mut [&mut b]).await;
    stop(s, vec![a, b]).await;
}

/// `mover` walks five steps of 40 units east, one at a time: each reaches `viewer` as a 13 for the
/// place the world checked, in order. A tick relays a mover's last move alone (Amendment C), so
/// the mover waits for each relay before its next step. Then the mover's next message is its own
/// world line: it got no 13 for itself.
async fn walks_five_steps(mover: &mut Bot, viewer: &mut Bot) {
    let from = mover.place();
    for step in 1..=5 {
        let at = [from.x as i32 + 40 * step, from.y as i32, from.z as i32];
        mover.walk_to(at).await;
        viewer.expect_moved(mover.pid(), mover.place()).await;
    }
    nothing_else(mover, &mut [viewer]).await;
}

#[tokio::test]
async fn a_bot_walks_and_the_other_sees_it() {
    let dir = tempfile::tempdir().unwrap();
    let (s, start) = world_server(dir.path(), "").await;
    let (mut a, mut b) = two_who_see_each_other(&s, start).await;
    // Step 5, each way.
    walks_five_steps(&mut a, &mut b).await;
    walks_five_steps(&mut b, &mut a).await;
    stop(s, vec![a, b]).await;
}

#[tokio::test]
async fn the_bots_chat_on_all_three_channels() {
    let dir = tempfile::tempdir().unwrap();
    let (s, start) = world_server(dir.path(), "").await;
    let (mut a, mut b) = two_who_see_each_other(&s, start).await;
    let mut c = kai_at(&s, far_from(start)).await;
    // Step 6. Nearby (0): Lin hears her own line back, since her client shows nothing of its own
    // (chat.md 1.1), and Mai, who was told of her, hears it.
    a.say(NEARBY, "hi").await;
    let hi = Heard::said(NEARBY, a.made(), "hi", None);
    assert_eq!(a.heard().await, hi);
    assert_eq!(b.heard().await, hi);
    // World (5): everyone in the game hears Kai from 1 km off. It is Kai's first line: he never
    // heard Lin's, whose view he is out of.
    c.say(WORLD, "all &lt;3").await;
    let all = Heard::said(WORLD, c.made(), "all &lt;3", None);
    for bot in [&mut a, &mut b, &mut c] {
        assert_eq!(bot.heard().await, all);
    }
    // A whisper typed with `/w Kai`: both ends hear it, with Kai's link as its receiver.
    a.whisper(Whom::Typed("Kai"), "psst").await;
    let psst = Heard::said(WHISPER, a.made(), "psst", Some(c.made()));
    assert_eq!(c.heard().await, psst);
    assert_eq!(a.heard().await, psst);
    // One set by clicking Lin's name names her by her pid.
    let lin = Whom::Clicked {
        pid: a.pid(),
        name: "Lin",
    };
    c.whisper(lin, "back").await;
    let back = Heard::said(WHISPER, c.made(), "back", Some(a.made()));
    assert_eq!(a.heard().await, back);
    assert_eq!(c.heard().await, back);
    // Team chat (2) isn't carried: Lin alone gets a 270.
    a.say(2, "team").await;
    a.expect_refused(chat::code::CHAT_INVALID_CHANNEL).await;
    // Mai heard neither whisper, and nobody heard the team line.
    nothing_else(&mut b, &mut [&mut a, &mut c]).await;
    stop(s, vec![a, b, c]).await;
}

#[tokio::test]
async fn an_admin_bot_runs_each_command() {
    let dir = tempfile::tempdir().unwrap();
    // Step 7: `[access] admins` names Lin's account.
    let (s, start) = world_server(dir.path(), "[access]\nadmins = [\"aaa\"]\n").await;
    let (mut a, mut b) = two_who_see_each_other(&s, start).await;
    let far = far_from(start);
    let mut c = kai_at(&s, far).await;
    // A player's `!` line is ordinary chat.
    b.say(NEARBY, "!kick Lin").await;
    let bang = Heard::said(NEARBY, b.made(), "!kick Lin", None);
    assert_eq!(b.heard().await, bang);
    assert_eq!(a.heard().await, bang);
    // `!announce`: the marquee (66), for everyone.
    a.say(NEARBY, "!announce Restart at 20:00").await;
    for bot in [&mut a, &mut b, &mut c] {
        assert_eq!(bot.heard().await, Heard::marquee("Restart at 20:00"));
    }
    // `!invite`: a link Lin alone gets (64), whose code registers an account under its label.
    a.say(NEARBY, "!invite for Bob").await;
    let invite = a.heard().await;
    let base = "invite link: http://127.0.0.1:0/#invite=";
    let code = invite
        .content
        .strip_prefix(base)
        .unwrap_or_else(|| panic!("{invite:?}"));
    assert_eq!(invite, Heard::system(&format!("{base}{code}")));
    let accounts = s.accounts();
    accounts
        .register(code, "dan", "a long password", LOCAL)
        .await
        .unwrap();
    let invites = accounts.store().list_invites().await.unwrap();
    let redeemed = |i: &fenji_store::InviteRow| {
        i.label.as_deref() == Some("for Bob") && i.redeemed_by.as_deref() == Some("dan")
    };
    assert!(invites.iter().any(redeemed), "{invites:?}");
    // `!tp Kai`: Lin's client stands her at Kai's place (20) and hears so; Mai's, which saw her,
    // hears she left (9); then, in the next tick, Lin's forgets Mai, and Lin's and Kai's are told
    // of each other where they now stand (6).
    a.say(NEARBY, "!tp Kai").await;
    a.expect_stand(far).await;
    assert_eq!(a.heard().await, Heard::system("Lin is with Kai"));
    b.expect_left(&[a.pid()]).await;
    a.expect_left(&[b.pid()]).await;
    a.expect_announced(c.pid(), far).await;
    c.expect_announced(a.pid(), far).await;
    // `!kick Kai`: Kai's client reads why it was closed, and Lin, who was told of Kai, hears he
    // left, after the answer.
    a.say(NEARBY, "!kick Kai").await;
    assert_eq!(c.expect_closed().await, KICKED.as_bytes());
    assert_eq!(a.heard().await, Heard::system("Kai was kicked"));
    a.expect_left(&[c.pid()]).await;
    drop(c);
    nothing_else(&mut b, &mut [&mut a]).await;
    stop(s, vec![a, b]).await;
}

/// The ways a client leaves the world (Amendment D, step 8).
#[derive(Debug, Clone, Copy)]
enum Leaving {
    /// 392 through `send_gateway` (0xFF), as `RequestQuitGame` sends it in the world.
    Quit,
    /// Type 4: back to character select.
    BackToSelect,
    /// The socket closes.
    HangUp,
}

impl Leaving {
    /// `bot` leaves this way, and the server ends its connection.
    async fn leave(self, mut bot: Bot) {
        match self {
            Self::Quit => bot.send(lobby::CLIENT_EXIT, &[]).await,
            Self::BackToSelect => bot.send_native(Cts::ClientLeave.to_u8(), &[]).await,
            Self::HangUp => return,
        }
        // The idle timeout is a minute, so an end within 2 s is the server's answer.
        let ends = bot.ends_within(Duration::from_secs(2)).await;
        assert!(ends, "the server ends the connection after {self:?}");
    }
}

#[tokio::test]
async fn a_bot_leaving_is_a_9_for_the_other() {
    let dir = tempfile::tempdir().unwrap();
    let (s, start) = world_server(dir.path(), "").await;
    let mut a = player(&s, "aaa", "Lin", (7, 3)).await;
    enters(&mut a, start).await;
    // Step 8, each way in turn: Lin was told of the one leaving and pulled it, and hears it left.
    for (account, name, way) in [
        ("bbb", "Mai", Leaving::Quit),
        ("ccc", "Kai", Leaving::BackToSelect),
        ("ddd", "Ren", Leaving::HangUp),
    ] {
        let mut b = player(&s, account, name, (2, 1)).await;
        enters(&mut b, start).await;
        a.expect_announced(b.pid(), start).await;
        b.expect_announced(a.pid(), start).await;
        a.pull(b.pid()).await;
        let pid = b.pid();
        way.leave(b).await;
        a.expect_left(&[pid]).await;
    }
    nothing_else(&mut a, &mut []).await;
    stop(s, vec![a]).await;
}
