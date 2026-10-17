//! Chat (world spec §3.5): nearby to whoever sees the speaker, world to everyone in the game, and
//! whispers to their target by pid or by name. Every line is echoed to its speaker too, since the
//! client shows nothing of its own. The server enforces the client's own limits again, and a rate
//! (world spec §2.4).
use bevy_ecs::prelude::*;

use super::admin::Caller;
use super::ecs::{Account, Actor, Conn, Doomed, Index, Now, Pending, Stage, deliver, name_key};
use super::interest::Sight;
use super::types::*;
use crate::commands::from_chat;
use crate::text::plain;

/// The most characters in a line, as the client counts them.
pub const MAX_CHARS: usize = 140;
/// The most bytes in a line as it arrives, after the client's own escaping.
pub const MAX_BYTES: usize = 2048;
/// The rate: a burst of this many lines…
pub const BURST: f64 = 5.0;
/// …then this many a second.
pub const PER_SEC: f64 = 1.0;

/// The entities the client's input writes, for `&`, `<` and `>`; it writes no others.
const ENTITIES: [(char, &str); 3] = [('&', "&amp;"), ('<', "&lt;"), ('>', "&gt;")];

/// Whether `text` has anything a client might read as markup:
/// - an `&` that doesn't start one of [`ENTITIES`], which might decode to a tag;
/// - a `\\`, which link escapes start with;
/// - a `[` whose next character, past any spaces, isn't a digit or `]`, or that has none: the
///   client renders `[link …]`, `[q_sound …]`, `[duration …]` and `[teamshout]`, which could
///   forge a sender, a link or a sound in other players' clients.
///
/// The client's own input escapes `&`, `<` and `>` and builds links itself, so a player typing
/// loses only words in square brackets and backslashes.
fn markup(text: &str) -> bool {
    let stray = text
        .match_indices('&')
        .any(|(i, _)| !ENTITIES.iter().any(|(_, e)| text[i..].starts_with(e)));
    if stray || text.contains('\\') {
        return true;
    }
    let mut open = false;
    for c in text.chars() {
        if open && !c.is_whitespace() {
            if !(c.is_ascii_digit() || c == ']') {
                return true;
            }
            open = false;
        }
        if c == '[' {
            open = true;
        }
    }
    // A `[` that ends the line, spaces aside, has no digit or `]` after it.
    open
}

/// A line's length as the client counts it: characters, with each entity its input writes for
/// `&`, `<` and `>` counted as one.
pub fn length(text: &str) -> usize {
    decode(text).chars().count()
}

/// `text` as the player typed it: each of [`ENTITIES`] back as its character, in one pass, so
/// that `&amp;lt;` is the `&lt;` the player typed. A command's arguments mean this text: a name to
/// find and echo, an invite's label.
pub(super) fn decode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let (c, len) = ENTITIES
            .iter()
            .find(|(_, entity)| tail.starts_with(entity))
            .map_or(('&', 1), |(c, entity)| (*c, entity.len()));
        out.push(c);
        rest = &tail[len..];
    }
    out.push_str(rest);
    out
}

/// `text` as the client's input would write it: `&`, `<` and `>` as [`ENTITIES`]. Text the server
/// is given as typed, such as the console's announcements, goes out so, and [`check`] takes it.
pub(super) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match ENTITIES.iter().find(|(raw, _)| *raw == c) {
            Some((_, entity)) => out.push_str(entity),
            None => out.push(c),
        }
    }
    out
}

/// Whether the server takes a line's text. Its size in bytes comes first, so that nothing else
/// walks a line longer than the client could send (#280).
pub fn check(text: &str) -> Result<(), ChatRefusal> {
    if text.len() > MAX_BYTES {
        return Err(ChatRefusal::TooLong);
    }
    if text.trim().is_empty() || !plain(text) || text.contains(['<', '>']) || markup(text) {
        return Err(ChatRefusal::Invalid);
    }
    if length(text) > MAX_CHARS {
        return Err(ChatRefusal::TooLong);
    }
    Ok(())
}

/// A player's chat allowance: a token bucket.
#[derive(Component, Debug, Clone, Copy)]
pub struct Bucket {
    tokens: f64,
    at_ms: u64,
}

impl Bucket {
    /// A full bucket at `now_ms`.
    pub fn new(now_ms: u64) -> Self {
        Self {
            tokens: BURST,
            at_ms: now_ms,
        }
    }

    /// Takes a line's token at `now_ms`, if there is one.
    pub fn take(&mut self, now_ms: u64) -> bool {
        let elapsed = now_ms.saturating_sub(self.at_ms) as f64 / 1000.0;
        self.at_ms = self.at_ms.max(now_ms);
        self.tokens = (self.tokens + elapsed * PER_SEC).min(BURST);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Routes the tick's lines. Each spends a token of its speaker's [`Bucket`], an admin's command
/// too, so commands keep the rate as chat does. An admin's `!` line then goes to the admin phase
/// as a command, read as the admin typed it ([`decode`]).
pub(super) fn system(
    now: Res<Now>,
    index: Res<Index>,
    mut pending: ResMut<Pending>,
    mut speakers: Query<(&Actor, &Account, &Stage, &mut Bucket, &Conn), Without<Doomed>>,
    listeners: Query<(Entity, &Actor, &Sight, &Conn), Without<Doomed>>,
    mut commands: Commands,
) {
    for (e, line) in std::mem::take(&mut pending.lines) {
        let Ok((actor, account, stage, mut bucket, conn)) = speakers.get_mut(e) else {
            continue;
        };
        if *stage != Stage::InWorld {
            continue;
        }
        if !bucket.take(now.0) {
            let refused = WorldOut::ChatRefused(ChatRefusal::TooFast);
            deliver(conn, e, refused, &mut commands);
            continue;
        }
        if account.admin
            && let Some(parsed) = from_chat(&decode(&line.text))
        {
            match parsed {
                Ok(order) => pending.orders.push((Caller::Admin(e), order)),
                Err(why) => {
                    let text = why.to_string();
                    deliver(
                        conn,
                        e,
                        WorldOut::Chat(ChatOut::System { text }),
                        &mut commands,
                    );
                }
            }
            continue;
        }
        if let Err(why) = check(&line.text) {
            deliver(conn, e, WorldOut::ChatRefused(why), &mut commands);
            continue;
        }
        let from = Speaker {
            pid: actor.pid,
            name: actor.look.name.clone(),
            vocation: actor.look.vocation,
        };
        match line.channel {
            ChatChannel::Nearby => {
                let said = WorldOut::Chat(ChatOut::Said {
                    channel: ChatChannel::Nearby,
                    from,
                    to: None,
                    text: line.text,
                });
                deliver(conn, e, said.clone(), &mut commands);
                for (v, _, sight, to) in &listeners {
                    if v != e && sight.0.contains_key(&actor.pid) {
                        deliver(to, v, said.clone(), &mut commands);
                    }
                }
            }
            ChatChannel::World => {
                let said = WorldOut::Chat(ChatOut::Said {
                    channel: ChatChannel::World,
                    from,
                    to: None,
                    text: line.text,
                });
                for (v, _, _, to) in &listeners {
                    deliver(to, v, said.clone(), &mut commands);
                }
            }
            ChatChannel::Whisper => {
                let target = match &line.to {
                    Some(WhisperTarget::Pid(pid)) => index.pids.get(pid).copied(),
                    Some(WhisperTarget::Name(name)) => {
                        // A typed name comes as `Name@<server>`; the server part is the client's.
                        let name = name.split('@').next().unwrap_or_default().trim();
                        index.names.get(&name_key(name)).copied()
                    }
                    None => None,
                };
                let Some(t) = target else {
                    let refused = WorldOut::ChatRefused(ChatRefusal::NotOnline);
                    deliver(conn, e, refused, &mut commands);
                    continue;
                };
                if t == e {
                    let refused = WorldOut::ChatRefused(ChatRefusal::ToSelf);
                    deliver(conn, e, refused, &mut commands);
                    continue;
                }
                let Ok((_, target, _, to)) = listeners.get(t) else {
                    let refused = WorldOut::ChatRefused(ChatRefusal::NotOnline);
                    deliver(conn, e, refused, &mut commands);
                    continue;
                };
                let said = WorldOut::Chat(ChatOut::Said {
                    channel: ChatChannel::Whisper,
                    from,
                    to: Some(Speaker {
                        pid: target.pid,
                        name: target.look.name.clone(),
                        vocation: target.look.vocation,
                    }),
                    text: line.text,
                });
                deliver(to, t, said.clone(), &mut commands);
                deliver(conn, e, said, &mut commands);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::ecs::test_util::*;
    use super::*;

    fn say(conn: ConnId, channel: ChatChannel, text: &str) -> WorldIn {
        WorldIn::Chat {
            conn,
            line: ChatIn {
                channel,
                text: text.into(),
                to: None,
            },
        }
    }

    fn whisper(conn: ConnId, to: WhisperTarget, text: &str) -> WorldIn {
        WorldIn::Chat {
            conn,
            line: ChatIn {
                channel: ChatChannel::Whisper,
                text: text.into(),
                to: Some(to),
            },
        }
    }

    fn speaker(pid: ActorId, name: &str) -> Speaker {
        Speaker {
            pid,
            name: name.into(),
            vocation: 0,
        }
    }

    fn said(channel: ChatChannel, from: Speaker, to: Option<Speaker>, text: &str) -> WorldOut {
        WorldOut::Chat(ChatOut::Said {
            channel,
            from,
            to,
            text: text.into(),
        })
    }

    /// 1 and 2 stand together, 3 far away.
    fn three() -> Harness {
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        h.enter(player(2, "Mai", at(100.0, 0.0)));
        h.enter(player(3, "Ren", at(50_000.0, 0.0)));
        h.tick(50);
        for c in 1..=3 {
            h.take(c);
        }
        h
    }

    #[test]
    fn the_client_limits_hold_and_markup_is_refused() {
        assert_eq!(check("hello #12 &lt;3 &amp; &gt; [1] a [ 2 ] []"), Ok(()));
        assert_eq!(
            check(&"&lt;".repeat(140)),
            Ok(()),
            "140 as the client counts"
        );
        assert_eq!(check(&"a".repeat(141)), Err(ChatRefusal::TooLong));
        assert_eq!(check(&"é".repeat(140)), Ok(()));
        for bad in [
            "",
            "   ",
            "a\u{0}b",
            "line\nbreak",
            "\u{202E}evil",
            "<IMG SRC=\"x\">",
            "[link type=\"player\" pid=\"1\" name=\"Admin\" gm=\"1\"]",
            "[LINK type=\"item\"]",
            "[q_sound event=1 sequence=2]",
            "[duration 5]",
            "[teamshout]",
            "[ link type=\"player\"]",
            "[AFK]",
            "[[1]",
            "\\091link type=\"player\"]",
            "\\o/",
            "&#60;IMG SRC=\"x\"&#62;",
            "&#x3c;b&#x3e;",
            "&quot;",
            "AT&T",
        ] {
            assert_eq!(check(bad), Err(ChatRefusal::Invalid), "{bad:?}");
        }
    }

    #[test]
    fn a_line_over_the_byte_limit_is_too_long_whatever_else_it_holds() {
        // #280: the length in bytes comes first, so nothing else walks a line longer than the
        // client could send; one that also holds markup or a control character is too long.
        for long in [
            "a".repeat(MAX_BYTES + 1),
            "é".repeat(MAX_BYTES / 2 + 1),
            "\\".repeat(MAX_BYTES + 1),
            format!("{}\u{0}", "a".repeat(MAX_BYTES)),
            format!("[link {}", "a".repeat(MAX_BYTES)),
        ] {
            assert_eq!(
                check(&long),
                Err(ChatRefusal::TooLong),
                "{} bytes",
                long.len()
            );
        }
    }

    #[test]
    fn a_bracket_with_nothing_after_it_is_refused() {
        // Task 7's rule: a `[` only before a digit or `]`, past any spaces. At the end of a line
        // there is neither.
        for bad in ["[", "hi [", "hi [  ", "[1] and ["] {
            assert_eq!(check(bad), Err(ChatRefusal::Invalid), "{bad:?}");
        }
        for good in ["[1]", "[]", "hi [ 2 ] ok"] {
            assert_eq!(check(good), Ok(()), "{good:?}");
        }
    }

    #[test]
    fn each_entity_counts_as_one_character() {
        for entity in ["&lt;", "&gt;", "&amp;"] {
            assert_eq!(check(&entity.repeat(140)), Ok(()), "{entity}");
            assert_eq!(
                check(&entity.repeat(141)),
                Err(ChatRefusal::TooLong),
                "{entity}"
            );
        }
    }

    #[test]
    fn entities_read_back_as_typed_in_one_pass_and_escaping_writes_them_again() {
        // The client's input writes only these three (chat.md 1.5), so a typed `&lt;` arrives as
        // `&amp;lt;` and reads back as `&lt;`, and anything else with a `&` stays as it is.
        for (wire, typed) in [
            ("a &amp; b", "a & b"),
            ("&lt;3 &gt;", "<3 >"),
            ("&amp;lt;", "&lt;"),
            ("&amp;&amp;", "&&"),
            ("AT&T", "AT&T"),
            ("&#60;", "&#60;"),
            ("&amp", "&amp"),
            ("&", "&"),
            ("", ""),
        ] {
            assert_eq!(decode(wire), typed, "{wire:?}");
        }
        assert_eq!(escape("Fish & chips <3 >"), "Fish &amp; chips &lt;3 &gt;");
        for typed in ["a & b", "<3 >", "&lt;", "é &amp; [1] \\"] {
            assert_eq!(decode(&escape(typed)), typed, "{typed:?}");
        }
    }

    #[test]
    fn the_rate_is_a_burst_of_five_then_one_a_second() {
        let mut b = Bucket::new(0);
        for _ in 0..5 {
            assert!(b.take(0));
        }
        assert!(!b.take(500));
        assert!(b.take(1_000));
        assert!(!b.take(1_000));
        assert!(!b.take(10), "time going back adds nothing");
    }

    #[test]
    fn an_idle_hour_refills_a_burst_and_no_more() {
        let mut b = Bucket::new(0);
        for _ in 0..5 {
            assert!(b.take(0));
        }
        // An hour at one a second would be 3,600 lines; the bucket holds five.
        for _ in 0..5 {
            assert!(b.take(3_600_000));
        }
        assert!(!b.take(3_600_000), "a sixth at once");
    }

    #[test]
    fn time_going_back_refills_nothing() {
        let mut b = Bucket::new(10_000);
        for _ in 0..5 {
            assert!(b.take(10_000));
        }
        assert!(!b.take(0), "a time before the last");
        // The time up to 10 s was counted once; coming back to it adds nothing.
        assert!(!b.take(10_000), "back at the last time");
        assert!(b.take(11_000), "a second past it");
    }

    #[test]
    fn nearby_reaches_whoever_sees_the_speaker_and_world_reaches_everyone() {
        let mut h = three();
        h.push(say(1, ChatChannel::Nearby, "hi"));
        h.tick(50);
        let hi = said(ChatChannel::Nearby, speaker(1, "Lin"), None, "hi");
        assert_eq!(h.take(1), vec![hi.clone()], "echoed");
        assert_eq!(h.take(2), vec![hi]);
        assert!(h.take(3).is_empty());
        h.push(say(3, ChatChannel::World, "all"));
        h.tick(50);
        let all = said(ChatChannel::World, speaker(3, "Ren"), None, "all");
        for c in 1..=3 {
            assert_eq!(h.take(c), vec![all.clone()]);
        }
    }

    #[test]
    fn whispers_find_their_target_by_pid_or_name() {
        let mut h = three();
        h.push(whisper(
            1,
            WhisperTarget::Name("ren@Unknown".into()),
            "psst",
        ));
        h.push(whisper(3, WhisperTarget::Pid(1), "back"));
        h.tick(50);
        let psst = said(
            ChatChannel::Whisper,
            speaker(1, "Lin"),
            Some(speaker(3, "Ren")),
            "psst",
        );
        let back = said(
            ChatChannel::Whisper,
            speaker(3, "Ren"),
            Some(speaker(1, "Lin")),
            "back",
        );
        assert_eq!(h.take(1), vec![psst.clone(), back.clone()]);
        assert_eq!(h.take(3), vec![psst, back]);
        assert!(h.take(2).is_empty());
    }

    #[test]
    fn whispers_to_oneself_or_to_no_one_are_refused() {
        let mut h = three();
        h.push(whisper(1, WhisperTarget::Pid(1), "me"));
        h.push(whisper(1, WhisperTarget::Name("Nobody".into()), "you"));
        h.push(whisper(1, WhisperTarget::Pid(999), "you"));
        h.tick(50);
        assert_eq!(
            h.take(1),
            vec![
                WorldOut::ChatRefused(ChatRefusal::ToSelf),
                WorldOut::ChatRefused(ChatRefusal::NotOnline),
                WorldOut::ChatRefused(ChatRefusal::NotOnline)
            ]
        );
        assert!(h.take(2).is_empty(), "Mai stands by Lin and hears nothing");
        assert!(h.take(3).is_empty());
    }

    #[test]
    fn a_whisper_to_a_player_leaving_this_tick_is_refused_and_reaches_no_one() {
        let mut h = three();
        h.push(whisper(1, WhisperTarget::Name("Ren".into()), "bye"));
        h.push(whisper(1, WhisperTarget::Pid(3), "bye"));
        h.push(WorldIn::Leave { conn: 3 });
        h.tick(50);
        assert_eq!(
            h.take(1),
            vec![
                WorldOut::ChatRefused(ChatRefusal::NotOnline),
                WorldOut::ChatRefused(ChatRefusal::NotOnline)
            ]
        );
        assert!(h.take(3).is_empty(), "nothing reached Ren's connection");
        assert!(h.closed(3));
        assert!(h.take(2).is_empty());
    }

    #[test]
    fn a_forged_line_or_a_sixth_in_a_second_is_refused_and_reaches_no_one() {
        let mut h = three();
        h.push(say(
            1,
            ChatChannel::World,
            "[link type=\"player\" gm=\"1\"]",
        ));
        for _ in 0..5 {
            h.push(say(2, ChatChannel::Nearby, "spam"));
        }
        h.tick(50);
        let spam = said(ChatChannel::Nearby, speaker(2, "Mai"), None, "spam");
        let mut lin = vec![WorldOut::ChatRefused(ChatRefusal::Invalid)];
        lin.extend(std::iter::repeat_n(spam.clone(), 5));
        assert_eq!(h.take(1), lin, "Lin's refusal, then Mai's lines");
        assert_eq!(h.take(2), vec![spam; 5], "the forged line reached no one");
        h.push(say(2, ChatChannel::Nearby, "spam"));
        h.tick(50);
        assert_eq!(h.take(2), vec![WorldOut::ChatRefused(ChatRefusal::TooFast)]);
        assert!(h.take(1).is_empty());
        assert!(h.take(3).is_empty());
    }

    #[test]
    fn going_back_to_character_select_brings_no_fresh_burst() {
        // #394: each join spawned a full bucket, so four rounds of six lines, with back to
        // character select between them, put 20 lines in front of another player at once. The
        // account keeps its bucket, whichever character it comes back as: five, as staying would.
        let mut h = Harness::new();
        h.enter(player(2, "Mai", at(1.0, 1.0)));
        let mut heard = 0;
        for round in 0..4 {
            h.enter(Player {
                pid: 10 + round,
                look: look(&format!("Lin{round}")),
                ..player(1, "Lin", at(0.0, 0.0))
            });
            for _ in 0..6 {
                h.push(say(1, ChatChannel::World, "hi"));
            }
            h.tick(1);
            h.push(WorldIn::Leave { conn: 1 });
            h.tick(1);
            heard += h
                .take(2)
                .iter()
                .filter(|m| matches!(m, WorldOut::Chat(ChatOut::Said { .. })))
                .count();
        }
        assert_eq!(heard, 5);
    }

    #[test]
    fn the_kept_bucket_refills_with_the_time_away() {
        // #394: the bucket an account keeps refills while it's away, as staying would have: five
        // lines, about three seconds at character select, then three pass and a fourth is too
        // fast. Kept any shorter than the five seconds a burst takes to come back, it would be
        // forgotten here, and the account would have five again.
        let mut h = Harness::new();
        h.enter(player(2, "Mai", at(1.0, 1.0)));
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        for _ in 0..5 {
            h.push(say(1, ChatChannel::World, "hi"));
        }
        h.tick(50);
        h.push(WorldIn::Leave { conn: 1 });
        h.tick(50);
        for _ in 0..58 {
            h.tick(50);
        }
        h.enter(Player {
            pid: 7,
            look: look("Ren"),
            ..player(1, "Lin", at(0.0, 0.0))
        });
        h.take(2);
        for _ in 0..4 {
            h.push(say(1, ChatChannel::World, "hi"));
        }
        h.tick(50);
        let heard = h
            .take(2)
            .iter()
            .filter(|m| matches!(m, WorldOut::Chat(ChatOut::Said { .. })))
            .count();
        assert_eq!(heard, 3);
        assert_eq!(
            h.take(1).last(),
            Some(&WorldOut::ChatRefused(ChatRefusal::TooFast))
        );
    }

    #[test]
    fn reconnecting_brings_no_fresh_burst() {
        // #394's other path: a newer connection replaces the older in the tick it joins, and its
        // account's spent bucket comes with it.
        let mut h = Harness::new();
        h.enter(player(2, "Mai", at(1.0, 1.0)));
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        for _ in 0..5 {
            h.push(say(1, ChatChannel::World, "hi"));
        }
        h.tick(1);
        h.enter(Player {
            conn: 3,
            ..player(1, "Lin", at(0.0, 0.0))
        });
        h.take(2);
        for _ in 0..5 {
            h.push(say(3, ChatChannel::World, "hi"));
        }
        h.tick(1);
        let heard = h
            .take(2)
            .iter()
            .filter(|m| matches!(m, WorldOut::Chat(ChatOut::Said { .. })))
            .count();
        assert_eq!(heard, 0);
    }

    #[test]
    fn a_non_admins_bang_line_is_ordinary_chat() {
        let mut h = three();
        h.push(say(1, ChatChannel::Nearby, "!kick Mai"));
        h.tick(50);
        let line = said(ChatChannel::Nearby, speaker(1, "Lin"), None, "!kick Mai");
        assert_eq!(h.take(2), vec![line]);
        assert_eq!(h.core.players(), 3);
    }
}
