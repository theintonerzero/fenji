//! A connection in the world (M4 world plan, the local slices; docs/protocol/packets/world.md).
//! The gate hands its connection here when the client's 10 names one of the account's characters.
//! The link joins the world as that character, maps the client's messages to [`WorldIn`] and each
//! [`WorldOut`] back to messages, and leaves the world however it ends. Slice 1 (Amendment A) maps
//! entering: 772, the time syncs, type 3, then the player's own 7 and 8. Slice 2 (Amendment B)
//! maps seeing: another player announced (6), pulled (5), sent whole (7 and 8) and taken away
//! (9). Slice 3 (Amendment C) maps moving: the client's direction command (8) to the world, its
//! relay to others (13), and a correction or a teleport to the player itself (20). Slice 4 (Task
//! 16; docs/protocol/packets/chat.md) maps chat: a line (115) to the world, the lines the world
//! sends (378), and refusals (270); the admins' commands arrive as lines. Script messages go out
//! through the gate's `script_out`, which refuses `NEVER_SEND`.
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use fenji_proto::chat;
use fenji_proto::codec::StoredCodec;
use fenji_proto::handshake::{Event, Server};
use fenji_proto::lobby::{self, Appearance, code};
use fenji_proto::opcode::{Cts, Stc};
use fenji_proto::packet::Up;
use fenji_proto::pickle::{Table, Value};
use fenji_proto::world::{self, PlayerActor};
use fenji_store::{Account, Character};
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, MissedTickBehavior};

use crate::accounts;
use crate::gate::{self, Gate};
use crate::lobby::stored_appearance;
use crate::online::Closing;
use crate::world::{
    ActorId, ChatChannel, ChatIn, ChatOut, ChatRefusal, ConnId, KickReason, Location, Look,
    MAX_LINE_BYTES, MoveStep, Player, Pos, Speaker, WhisperTarget, WorldHandle, WorldIn, WorldOut,
};

/// The world's messages a connection may hold unwritten; one more and the world lets go of it
/// (`KickReason::SlowReader`). A tick relays at most one move of each mover in view (Amendment C),
/// so a viewer of 20 movers gets 20 relays a tick, besides what comes into or goes out of view: 256
/// holds more than ten ticks of that.
pub(crate) const OUTBOUND: usize = 256;
// No later slice may shrink it: it is the slack for a client that pauses its reads for a moment.
const _: () = assert!(OUTBOUND >= 256);

// The codec refuses a text over the client's own 2,048 bytes before it reaches the world, which
// checks the same limit again (#280): one limit, on both sides.
const _: () = assert!(chat::MAX_TEXT_BYTES == MAX_LINE_BYTES);

/// Whether byte `i` of type 3 describes the player's machine (world.md §3): the Windows version
/// (9–12) or the parent-process flag (17). The trace log masks them.
fn enter_machine(i: usize) -> bool {
    (9..13).contains(&i) || i == 17
}

/// Whether byte `i` of a time sync describes the player's machine (world.md 7.2): the two
/// counters after the clock and its byte 1.
fn sync_machine(i: usize) -> bool {
    i >= 5
}

/// Whether byte `i` of type 5 describes the player's machine (world.md 14.4): the last byte, from
/// a query about the client's own process. The trace log masks it.
fn filter_machine(i: usize) -> bool {
    i == 17
}

/// Plays `character` in the world on the gate's connection, until the client leaves, the world
/// lets go of it, or the online registry tells it to close: a newer login replaces it, or the
/// account is banned. The lobby has checked that the account isn't banned (`gate::entry`).
#[allow(clippy::too_many_arguments)] // the connection's state, as the lobby loop holds it
pub(crate) async fn play(
    sock: &mut TcpStream,
    peer: SocketAddr,
    gate: &Gate,
    server: &mut Server,
    account: &Account,
    conn: ConnId,
    character: Character,
    replaced: &mut oneshot::Receiver<Closing>,
) -> anyhow::Result<()> {
    let deadline = gate.limits.write_deadline;
    let pid = character.pid;
    let (tx, mut rx) = mpsc::channel(OUTBOUND);
    // `[access].admins` or an op (M4 hosting spec §8, HA5). A store that can't say enters the
    // player without an admin's commands.
    let admin = match accounts::is_admin(gate.accounts.store(), &gate.admins, account).await {
        Ok(admin) => admin,
        Err(e) => {
            tracing::error!(error = %e, account = %account.username, "checking for an op: entering without an admin's commands");
            false
        }
    };
    let player = player(gate.start, admin, account, conn, character);
    if !gate.world.send(WorldIn::Join { player, out: tx }) {
        tracing::warn!(%peer, account = %account.username, "the world has stopped: entry refused");
        let bytes = gate::script_out(server, &lobby::error_code(code::SERVER_NOT_RESP))?;
        gate::send(sock, &bytes, deadline).await?;
        return Ok(());
    }
    // The world holds the player from here: it hears of the end however this ends, a panic
    // included, since it notices a closed channel only at its next send to it.
    let _leave = Leave {
        world: &gate.world,
        conn,
    };
    tracing::info!(%peer, account = %account.username, pid, "entering the world");
    // A player who only moves sends no script message for the lobby's account check (Ruling R5).
    let every = gate.limits.account_check.max(Duration::from_millis(10));
    let mut check = tokio::time::interval_at(Instant::now() + every, every);
    check.set_missed_tick_behavior(MissedTickBehavior::Delay);
    // One idle deadline, which only the client's reads move (Ruling R6): the other arms complete
    // without restarting it, so a client gone silent without a FIN still ends.
    let mut idle_at = Instant::now() + gate.limits.idle;
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        tokio::select! {
            read = tokio::time::timeout_at(idle_at, sock.read(&mut buf)) => {
                let n = match read {
                    Ok(n) => n?,
                    Err(_) => {
                        tracing::info!(%peer, account = %account.username, "idle in the world: closing");
                        return Ok(());
                    }
                };
                if n == 0 {
                    tracing::info!(%peer, account = %account.username, "left the world");
                    return Ok(());
                }
                idle_at = Instant::now() + gate.limits.idle;
                for event in server.receive(&buf[..n])? {
                    let m = match event {
                        Event::Message(m) => m,
                        // The client's ping display reads only what a server time sync sets: its
                        // clock less the one echoed (transport.md 4.4, #269).
                        Event::TimeSync(ms) => {
                            if !gate::send_or_replaced(sock, &server.time_sync(ms), deadline, replaced).await? {
                                tracing::info!(%peer, account = %account.username, "replaced by a newer login, or banned, while a reply was blocked: closing");
                                return Ok(());
                            }
                            continue;
                        }
                        _ => continue,
                    };
                    let reply = match on_message(gate, conn, m.ty, &m.body) {
                        Step::Go => None,
                        Step::Reply(ty, body) => Some(server.send(ty, &body, &StoredCodec)?),
                        Step::Script(body) => Some(gate::script_out(server, &body)?),
                        Step::End(why) => {
                            tracing::info!(%peer, account = %account.username, why, "left the world");
                            return Ok(());
                        }
                    };
                    if let Some(bytes) = reply
                        && !gate::send_or_replaced(sock, &bytes, deadline, replaced).await?
                    {
                        tracing::info!(%peer, account = %account.username, "replaced by a newer login, or banned, while a reply was blocked: closing");
                        return Ok(());
                    }
                }
            }
            out = rx.recv() => match out {
                Some(WorldOut::Kicked(reason)) => {
                    return kicked(sock, peer, gate, server, account, reason).await;
                }
                Some(msg) => {
                    let now = u64::try_from(gate.clock.now_ms()).unwrap_or(0);
                    let Some(bytes) = encode(server, pid, now, msg)? else { continue };
                    if !gate::send_or_replaced(sock, &bytes, deadline, replaced).await? {
                        tracing::info!(%peer, account = %account.username, "replaced by a newer login, or banned, while a write was blocked: closing");
                        return Ok(());
                    }
                }
                None => {
                    // A full channel, which the world drops without a word (its SlowReader kick
                    // didn't fit either), or the world has stopped.
                    tracing::info!(%peer, account = %account.username, "the world let go: closing");
                    return Ok(());
                }
            },
            // A newer login, or a ban: the world's own `Ban` may come too, and finds it gone.
            Ok(closing) = &mut *replaced => {
                return gate::close_told(sock, peer, gate, server, account, closing).await;
            }
            _ = check.tick() => match gate.accounts.store().account_by_id(account.id).await {
                Ok(Some(_)) => {}
                Ok(None) => {
                    tracing::info!(%peer, account = %account.username, "the account was deleted: closing");
                    return gate::close_with(sock, peer, gate, server, gate::AUTH_FAILED).await;
                }
                // As in the lobby: a store that fails now may answer at the next check.
                Err(e) => tracing::error!(error = %e, "account check failed"),
            },
        }
    }
}

/// The player `c` enters as: its look, whether its account is an admin's
/// ([`accounts::is_admin`]), and where it last stood, or `start` for a character that never
/// entered.
fn player(start: Location, admin: bool, account: &Account, conn: ConnId, c: Character) -> Player {
    let at = c.place.map_or(start, |p| Location {
        scene: p.scene,
        pos: Pos {
            x: p.x,
            y: p.y,
            z: p.z,
            yaw: p.yaw,
        },
    });
    Player {
        conn,
        account: account.id,
        admin,
        pid: c.pid,
        look: Look {
            name: c.name,
            vocation: c.vocation,
            skeleton: c.skeleton,
            level: c.level,
            appearance: Arc::from(c.appearance),
        },
        at,
    }
}

/// Tells the world the connection has ended, when dropped.
struct Leave<'a> {
    world: &'a WorldHandle,
    conn: ConnId,
}

impl Drop for Leave<'_> {
    fn drop(&mut self) {
        self.world.send(WorldIn::Leave { conn: self.conn });
    }
}

/// What the link does after one message from the client.
#[derive(Debug, PartialEq)]
enum Step {
    /// Nothing to send: go on.
    Go,
    /// Send this message (its type and body) at once.
    Reply(u8, Vec<u8>),
    /// Send this script message at once, through the gate's `script_out`.
    Script(Vec<u8>),
    /// End the session, for this reason (logged).
    End(&'static str),
}

/// The step for one message from a client in the world: the client's half of the slices' mappings
/// (Amendments A to C, and chat.md §6 for slice 4). It decodes only what the world takes: the time
/// syncs, type 3, a pull (5), a direction command (8), a line of chat (115), and the ways out (4 and
/// 392); everything else is dropped with a debug line.
fn on_message(gate: &Gate, conn: ConnId, ty: u8, body: &[u8]) -> Step {
    match Cts::from_u8(ty) {
        // From 772 on, once a second; answered at once (world.md §7).
        Cts::TimeSync => match world::time_sync_request(body) {
            Ok(echo) => {
                tracing::trace!(body = %masked(body, sync_machine), "time sync");
                let now = u64::try_from(gate.clock.now_ms()).unwrap_or(0);
                Step::Reply(
                    Stc::TimeSync.to_u8(),
                    world::time_sync_reply(echo, now).to_vec(),
                )
            }
            Err(e) => {
                tracing::debug!(error = %e, "time sync dropped");
                Step::Go
            }
        },
        // The scene has loaded (world.md §3). Only its length is checked.
        Cts::ClientEnter => match world::client_enter(body) {
            Ok(()) => {
                tracing::trace!(body = %masked(body, enter_machine), "loaded (type 3)");
                gate.world.send(WorldIn::Loaded { conn });
                Step::Go
            }
            Err(_) => Step::End("a type-3 body that doesn't fit its layout"),
        },
        // "Back to character select" waits for 330, which slice 1 doesn't send (world.md 1.5).
        Cts::ClientLeave => Step::End("back to character select (type 4)"),
        // A pull of an announced player, or the sync filter's value for an actor (world.md
        // 14.3, 14.12). Only a pull goes to the world, which answers it only for a pid it
        // announced and hasn't sent (14.13); M4 has no combat or threat for the filter's values.
        Cts::SyncFilterSet => match world::sync_filter_set(body) {
            Ok(f) => {
                tracing::trace!(body = %masked(body, filter_machine), "sync filter (type 5)");
                if f.value == world::PULL {
                    gate.world.send(WorldIn::Pull { conn, pid: f.pid });
                } else {
                    tracing::debug!(
                        pid = f.pid,
                        value = f.value,
                        "a sync filter value: not for the world"
                    );
                }
                Step::Go
            }
            Err(e) => {
                tracing::debug!(len = body.len(), error = %e, "type 5 dropped");
                Step::Go
            }
        },
        // A direction command (world.md 16.3): the position goes to the world as it is, which
        // checks it, and the rest bounded against the server's clock (16.15). The trace shows the
        // decoded move, never the body: its last bytes describe the player's machine. The other
        // move types (6, 7, 9, 10, 11) are ignored below, as is 419 with the script messages
        // (16.11, 16.14, 16.16).
        Cts::MoveDir => {
            let now = u64::try_from(gate.clock.now_ms()).unwrap_or(0);
            match world::move_dir(body, now) {
                Ok(m) => {
                    tracing::trace!(pos = ?m.pos, detail = ?m.detail, dir_yaw = m.dir_yaw, "move (type 8)");
                    let [x, y, z, yaw] = m.pos;
                    let step = MoveStep {
                        pos: Pos { x, y, z, yaw },
                        detail: Arc::from(m.detail.to_bytes()),
                    };
                    gate.world.send(WorldIn::Move { conn, step });
                }
                Err(e) => tracing::debug!(ty, len = body.len(), error = %e, "type 8 dropped"),
            }
            Step::Go
        }
        Cts::GateScript | Cts::Script => match Up::decode(ty, body) {
            Ok(Up::Script {
                id: Value::Int(lobby::CLIENT_EXIT),
                ..
            }) => Step::End("the client is quitting (392)"),
            // A line of chat, which an admin's command rides on (chat.md §1). The world checks the
            // text and routes it; the link refuses at once, as it answers a time sync, only what
            // the world can't take: a channel M4 doesn't carry, a "line" that isn't text, and a
            // text or a whisper's name too long for the world, which the codec refuses before it
            // copies them (#280). Its log says that a line came and on which channel, never what
            // it said or to whom.
            Ok(Up::Script {
                id: Value::Int(chat::CHAT_TRANSMIT),
                args,
                ..
            }) => match chat::parse_transmit(&args) {
                Ok(t) => {
                    tracing::debug!(
                        channel = t.channel.number(),
                        bytes = t.text.len(),
                        "115: a line for the world"
                    );
                    gate.world.send(WorldIn::Chat {
                        conn,
                        line: chat_in(t),
                    });
                    Step::Go
                }
                Err(why) => {
                    tracing::debug!(?why, "115: a line refused at the link");
                    Step::Script(chat::refusal(why))
                }
            },
            Ok(Up::Script { id, gateway, .. }) => {
                // The scripts' logs and fetches after type 8 need no answer (world.md §1, step 8).
                tracing::debug!(id = ?id, gateway, "script message ignored in the world");
                Step::Go
            }
            Ok(Up::Unknown { .. }) => Step::Go,
            Err(e) => {
                tracing::debug!(ty, len = body.len(), error = %e, "undecodable message skipped");
                Step::Go
            }
        },
        _ => {
            tracing::debug!(ty, len = body.len(), "message ignored in the world");
            Step::Go
        }
    }
}

/// `body` in hex with the bytes `machine` picks shown as `..`: what the trace log may show of a
/// message that describes the player's machine (world.md 3.2, 7.2).
fn masked(body: &[u8], machine: fn(usize) -> bool) -> String {
    body.iter()
        .enumerate()
        .map(|(i, b)| {
            if machine(i) {
                "..".to_string()
            } else {
                format!("{b:02x}")
            }
        })
        .collect()
}

/// The bytes for one message from the world to the player `own`, with the server's clock at
/// `now_ms`, or `None` for what isn't sent: what names `own` itself (world.md 14.11: a 9 naming it
/// would make the client's own player leave, and a 6 would queue its own id; 16.8: a 13 would place
/// the player's own actor; interest leaves a viewer out of its own view, so this guards against a
/// bug in the world), and a move whose detail isn't [`world::MoveDetail::LEN`] bytes, which only a
/// bug in the world makes. `play` closes on a kick before it gets here.
fn encode(
    server: &mut Server,
    own: ActorId,
    now_ms: u64,
    msg: WorldOut,
) -> anyhow::Result<Option<Vec<u8>>> {
    match msg {
        WorldOut::LoadScene { pid, at } => {
            let p = at.pos;
            tracing::debug!(
                pid,
                scene = at.scene,
                x = p.x,
                y = p.y,
                z = p.z,
                yaw = p.yaw,
                "772: load the scene"
            );
            let body = world::load_scene(pid, at.scene, [p.x, p.y, p.z, p.yaw]);
            Ok(Some(gate::script_out(server, &body)?))
        }
        WorldOut::SelfEnter {
            pid,
            look,
            pos,
            speed,
        } => {
            // The speed is the blob's 1325 alone: without type 23, run m4-e3 ran as m4-e2 did
            // with it (world.md 6.5).
            tracing::debug!(pid, speed, "the player's own actor: 7 and 8");
            Ok(Some(enter_and_finish(server, pid, &look, pos, speed)?))
        }
        WorldOut::Announce { pid, pos } => {
            if pid == own {
                tracing::warn!(pid, "an announce of the player's own actor: not sent");
                return Ok(None);
            }
            tracing::debug!(
                pid,
                x = pos.x,
                y = pos.y,
                z = pos.z,
                "6: another player in view"
            );
            let body = world::sync_id_enter(pid, [pos.x, pos.y, pos.z]);
            Ok(Some(server.send(
                Stc::SyncIdEnter.to_u8(),
                &body,
                &StoredCodec,
            )?))
        }
        WorldOut::Enter {
            pid,
            look,
            pos,
            speed,
        } => {
            if pid == own {
                tracing::warn!(
                    pid,
                    "another player's 7 for the player's own actor: not sent"
                );
                return Ok(None);
            }
            // The player's own 7 for that character, then 8 (world.md 14.9, 14.10).
            tracing::debug!(pid, name = %look.name, speed, "another player: 7 and 8");
            Ok(Some(enter_and_finish(server, pid, &look, pos, speed)?))
        }
        WorldOut::Leave { pids } => {
            let gone: Vec<ActorId> = pids.iter().copied().filter(|p| *p != own).collect();
            if gone.len() != pids.len() {
                tracing::warn!(pid = own, "a 9 naming the player's own actor: left out");
            }
            if gone.is_empty() {
                return Ok(None);
            }
            tracing::debug!(pids = ?gone, "9: players out of view");
            Ok(Some(server.send(
                Stc::SyncLeave.to_u8(),
                &world::sync_leave(&gone),
                &StoredCodec,
            )?))
        }
        WorldOut::Moved { pid, step } => {
            if pid == own {
                tracing::warn!(pid, "a move of the player's own actor: not sent");
                return Ok(None);
            }
            // The detail is bounded again here, against this link's clock: the relay is built from
            // the position the world checked and these fields alone (world.md 16.15).
            let Some(detail) = world::MoveDetail::from_bytes(&step.detail, now_ms) else {
                tracing::warn!(
                    pid,
                    len = step.detail.len(),
                    "a move whose detail doesn't fit: not sent"
                );
                return Ok(None);
            };
            let p = step.pos;
            // At trace: a viewer of 20 movers gets about 400 a second (Task 15's review).
            tracing::trace!(
                pid,
                x = p.x,
                y = p.y,
                z = p.z,
                yaw = p.yaw,
                time = detail.time,
                dir = detail.dir,
                yaw_offset = detail.yaw_offset,
                rate = detail.rate,
                "13: another player moved"
            );
            let body = world::move_dir_relay(pid, [p.x, p.y, p.z, p.yaw], &detail);
            Ok(Some(server.send(
                Stc::MoveDir.to_u8(),
                &body,
                &StoredCodec,
            )?))
        }
        WorldOut::Correct { pos } => Ok(Some(stand_here(server, own, now_ms, pos, "correct")?)),
        WorldOut::Teleport { pos } => Ok(Some(stand_here(server, own, now_ms, pos, "teleport")?)),
        // A line, on the channel chat.md §3 gives it; the log never holds what it says, or whom.
        WorldOut::Chat(out) => {
            let line = wire_line(&out);
            tracing::debug!(channel = line.channel(), "378: a line");
            Ok(Some(gate::script_out(server, &chat::say(&line))?))
        }
        WorldOut::ChatRefused(why) => {
            tracing::debug!(?why, "270: a line refused");
            let body = chat::refusal(wire_refusal(why));
            Ok(Some(gate::script_out(server, &body)?))
        }
        WorldOut::Kicked(reason) => {
            tracing::warn!(?reason, "a kick reached the encoder: not sent");
            Ok(None)
        }
    }
}

/// The world's line for a 115 the codec read (chat.md §1).
fn chat_in(t: chat::Transmit) -> ChatIn {
    ChatIn {
        channel: match t.channel {
            chat::Channel::Nearby => ChatChannel::Nearby,
            chat::Channel::Whisper => ChatChannel::Whisper,
            chat::Channel::World => ChatChannel::World,
        },
        text: t.text,
        to: t.to.map(|to| match to {
            chat::Target::Pid(pid) => WhisperTarget::Pid(pid),
            chat::Target::Name(name) => WhisperTarget::Name(name),
        }),
    }
}

/// The codec's player for one the world names.
fn wire_player(s: &Speaker) -> chat::Player<'_> {
    chat::Player {
        pid: s.pid,
        name: &s.name,
        vocation: s.vocation,
    }
}

/// The codec's line for one the world sends (chat.md §2, §3).
fn wire_line(out: &ChatOut) -> chat::Line<'_> {
    match out {
        ChatOut::Said {
            channel,
            from,
            to,
            text,
        } => chat::Line::Said {
            channel: match channel {
                ChatChannel::Nearby => chat::Channel::Nearby,
                ChatChannel::Whisper => chat::Channel::Whisper,
                ChatChannel::World => chat::Channel::World,
            },
            from: wire_player(from),
            to: to.as_ref().map(wire_player),
            text,
        },
        ChatOut::Announce { text } => chat::Line::Announce { text },
        ChatOut::System { text } => chat::Line::System { text },
    }
}

/// The codec's refusal for the world's (chat.md §5).
fn wire_refusal(why: ChatRefusal) -> chat::Refusal {
    match why {
        ChatRefusal::Invalid => chat::Refusal::Invalid,
        ChatRefusal::TooLong => chat::Refusal::TooLong,
        ChatRefusal::TooFast => chat::Refusal::TooFast,
        ChatRefusal::NotOnline => chat::Refusal::NotOnline,
        ChatRefusal::ToSelf => chat::Refusal::ToSelf,
        ChatRefusal::Channel => chat::Refusal::Channel,
    }
}

/// Type 20 for the player `own` at `pos` (world.md 16.12), as `why` (a correction or a teleport):
/// T is the server's clock floored to 16 ms, less 160 ms, so the client takes the place into its
/// state of a moment ago and replays its own commands since.
fn stand_here(
    server: &mut Server,
    own: ActorId,
    now_ms: u64,
    pos: Pos,
    why: &'static str,
) -> anyhow::Result<Vec<u8>> {
    let t = world::sync_time(now_ms);
    tracing::debug!(
        pid = own,
        t,
        x = pos.x,
        y = pos.y,
        z = pos.z,
        yaw = pos.yaw,
        why,
        "20: the player stands here"
    );
    let body = world::move_sync_svr(own, t, [pos.x, pos.y, pos.z, pos.yaw]);
    Ok(server.send(Stc::MoveSyncSvr.to_u8(), &body, &StoredCodec)?)
}

/// Type 7 for player `pid`, then type 8 (world.md 4.1, 6.1, 14.9): one write, so both go out
/// within the write deadline. The appearance is the stored one, or the defaults with a warning.
fn enter_and_finish(
    server: &mut Server,
    pid: ActorId,
    look: &Look,
    pos: Pos,
    speed: i64,
) -> anyhow::Result<Vec<u8>> {
    let appearance = stored_appearance(&look.appearance).unwrap_or_else(|| {
        tracing::warn!(
            pid,
            "a stored appearance that doesn't decode: the defaults go out"
        );
        Appearance::from_table(&Table::default())
    });
    let actor = PlayerActor {
        pid,
        name: &look.name,
        vocation: look.vocation,
        skeleton: look.skeleton,
        level: look.level,
        appearance: &appearance,
        pos: [pos.x, pos.y, pos.z, pos.yaw],
        speed,
    };
    let mut out = server.send(
        Stc::SyncEnter.to_u8(),
        &world::sync_enter_player(&actor),
        &StoredCodec,
    )?;
    out.extend(server.send(
        Stc::SyncEnterFinish.to_u8(),
        &world::sync_enter_finish(pid),
        &StoredCodec,
    )?);
    Ok(out)
}

/// Closes a connection the world let go of, with the reason its client shows (world.md 10.3): a
/// replaced login as M3's, through `close_replaced`; a stopping server and an admin's kick with
/// their keys, lingering within a waiting place; a client that stopped reading or flooded with
/// none, so it shows the maintenance text.
async fn kicked(
    sock: &mut TcpStream,
    peer: SocketAddr,
    gate: &Gate,
    server: &mut Server,
    account: &Account,
    reason: KickReason,
) -> anyhow::Result<()> {
    tracing::info!(%peer, account = %account.username, ?reason, "the world let go");
    let why = match reason {
        KickReason::Replaced => gate::DUPLICATE_LOGIN,
        KickReason::Shutdown => gate::SERVER_STOPPING,
        KickReason::Kicked => gate::KICKED,
        KickReason::SlowReader | KickReason::Flood => return Ok(()),
    };
    if reason != KickReason::Replaced {
        return gate::close_with(sock, peer, gate, server, why).await;
    }
    // A replaced login no longer holds an online place: it says why and lingers only within a
    // waiting place, or closes at once without a reason when none is free (#174).
    let bytes = server.send(Stc::Close.to_u8(), why.as_bytes(), &StoredCodec)?;
    let place = gate.waiting.take(peer.ip());
    gate::close_replaced(sock, &bytes, place, gate.limits.linger).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::panic::AssertUnwindSafe;
    use std::path::Path;
    use std::time::Duration;

    use fenji_proto::client::{Client, ClientEvent};
    use fenji_proto::codec::StoredCodec;
    use fenji_proto::handshake::{AuthMessage, Event, Message};
    use fenji_proto::hello::{HELLO_LEN, Variants};
    use fenji_proto::lobby::{self, code};
    use fenji_proto::pickle::{self, Value};
    use fenji_proto::variant::AES128;
    use fenji_proto::world::BlobValue;
    use fenji_store::{Place, Store};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::mpsc;

    use super::*;
    use crate::accounts::Accounts;
    use crate::clock::{Clock, TestClock};
    use crate::config::AuthConfig;
    use crate::gate::{GateLimits, Places};
    use crate::lobby::LobbyLimits;
    use crate::online::Online;
    use crate::world::{
        ChatChannel, ChatIn, ChatOut, ChatRefusal, KickReason, Speaker, WhisperTarget, WorldIn,
        WorldOut,
    };

    const START: Location = Location {
        scene: 5,
        pos: Pos {
            x: 1.0,
            y: 2.0,
            z: 3.0,
            yaw: 4.0,
        },
    };

    /// The test gate's clock, in Unix ms: a multiple of 16.
    const NOW: i64 = 1_800_000_000_000;

    /// A gate whose world is the test's: what the link sends arrives on the receiver.
    async fn gate(dir: &Path) -> (Gate, mpsc::UnboundedReceiver<WorldIn>) {
        gate_at(dir, NOW).await
    }

    /// [`gate`] with its clock at `now_ms`.
    async fn gate_at(dir: &Path, now_ms: i64) -> (Gate, mpsc::UnboundedReceiver<WorldIn>) {
        let store = Store::open(&dir.join("fenji.db")).await.unwrap();
        let clock: Arc<dyn Clock> = Arc::new(TestClock::at(now_ms));
        let (world, inbox) = WorldHandle::fake();
        let gate = Gate {
            accounts: Arc::new(Accounts::new(
                store,
                AuthConfig::default(),
                Arc::clone(&clock),
            )),
            online: Arc::new(Online::new(4)),
            lobby: LobbyLimits { max_characters: 9 },
            limits: GateLimits {
                linger: Duration::from_millis(200),
                ..GateLimits::default()
            },
            waiting: Places::new(8, 8),
            clock,
            world,
            start: START,
            admins: vec!["boss".into()],
            stopping: tokio::sync::watch::Sender::new(false),
        };
        (gate, inbox)
    }

    fn account(name: &str) -> Account {
        Account {
            id: 7,
            username: name.into(),
            password_phc: String::new(),
            created_at: 0,
            last_sign_in_at: None,
        }
    }

    fn lin(place: Option<Place>) -> Character {
        Character {
            pid: 258,
            account_id: 7,
            name: "Lin".into(),
            vocation: 7,
            skeleton: 3,
            level: 37,
            appearance: vec![1, 2, 3],
            created_at: 0,
            place,
        }
    }

    /// Both ends of a loopback connection in `WORK`: the gate's socket and session, and the
    /// client's socket and transport.
    async fn session() -> (TcpStream, SocketAddr, Server, TcpStream, Client) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let theirs = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (ours, peer) = listener.accept().await.unwrap();
        let variants = Variants {
            down: AES128,
            up: AES128,
        };
        let (mut server, hello) = Server::start([7; HELLO_LEN], variants).unwrap();
        let mut client = Client::new(AuthMessage {
            dict_id: [0; 16],
            machine: [2, 0, 0, 0, 0, 1],
            payload: vec![1],
        });
        let (_, auth) = client.receive(&hello).unwrap();
        assert!(matches!(
            server.receive(&auth).unwrap()[..],
            [Event::Auth(_)]
        ));
        let mut work = server.accept(&StoredCodec).unwrap();
        work.extend(server.begin_work(&StoredCodec).unwrap());
        let (events, _) = client.receive(&work).unwrap();
        assert_eq!(events.len(), 2, "ok, then 414: {events:?}");
        (ours, peer, server, theirs, client)
    }

    /// What the client reads until the server closes the connection (5 s at most).
    async fn rest(sock: &mut TcpStream, client: &mut Client) -> Vec<ClientEvent> {
        let mut events = Vec::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let read = tokio::time::timeout(Duration::from_secs(5), sock.read(&mut buf))
                .await
                .expect("the server closes within 5 s");
            let n = read.unwrap_or(0);
            if n == 0 {
                return events;
            }
            match client.receive(&buf[..n]) {
                Ok((got, _)) => events.extend(got),
                Err(_) => return events,
            }
        }
    }

    /// The next thing the link tells the world (5 s at most).
    async fn told(inbox: &mut mpsc::UnboundedReceiver<WorldIn>) -> WorldIn {
        tokio::time::timeout(Duration::from_secs(5), inbox.recv())
            .await
            .expect("the link tells the world within 5 s")
            .expect("the link's handle is open")
    }

    #[test]
    fn the_player_comes_from_the_character_and_the_account() {
        let p = player(START, true, &account("Boss"), 3, lin(None));
        assert_eq!((p.conn, p.account, p.pid), (3, 7, 258));
        assert!(p.admin, "an admin's account");
        assert_eq!(
            p.at, START,
            "[world]'s start for a character that never entered"
        );
        assert_eq!(
            p.look,
            Look {
                name: "Lin".into(),
                vocation: 7,
                skeleton: 3,
                level: 37,
                appearance: Arc::from(&[1u8, 2, 3][..]),
            }
        );
        let place = Place {
            scene: 12,
            x: -1.5,
            y: 2.25,
            z: 30.0,
            yaw: -45.0,
        };
        let p = player(START, false, &account("tester"), 3, lin(Some(place)));
        assert!(!p.admin);
        assert_eq!(
            p.at,
            Location {
                scene: 12,
                pos: Pos {
                    x: -1.5,
                    y: 2.25,
                    z: 30.0,
                    yaw: -45.0
                }
            },
            "where it last stood"
        );
    }

    #[test]
    fn the_trace_log_masks_the_players_machine() {
        // Type 3 with a Windows version and the parent flag set: both masked (world.md §3).
        let enter = [
            1, 0, 0, 0, 0, 0, 0, 0, 0, 0x65, 0x4a, 0x00, 0x0a, 0, 0, 0, 0, 1, 0, 0, 0,
        ];
        assert_eq!(
            masked(&enter, enter_machine),
            "010000000000000000........00000000..000000"
        );
        // A time sync: its clock and byte 1 show, its two counters don't (7.2).
        let sync = [0x78, 0x56, 0x34, 0x12, 1, 9, 9, 9, 9, 8, 8, 8, 8];
        assert_eq!(masked(&sync, sync_machine), "7856341201................");
        // Type 5 with its last byte set: that byte is masked, the rest shows (14.4).
        let mut pull = filter(300, 32);
        pull[17] = 1;
        assert_eq!(
            masked(&pull, filter_machine),
            "2c01000000000000200000000100000000.."
        );
    }

    #[test]
    fn the_world_hears_of_the_end_however_play_ends() {
        let (world, mut inbox) = WorldHandle::fake();
        drop(Leave {
            world: &world,
            conn: 3,
        });
        assert!(matches!(inbox.try_recv(), Ok(WorldIn::Leave { conn: 3 })));
        let panicked = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _leave = Leave {
                world: &world,
                conn: 4,
            };
            panic!("a bug in the link");
        }));
        assert!(panicked.is_err());
        assert!(matches!(inbox.try_recv(), Ok(WorldIn::Leave { conn: 4 })));
    }

    #[tokio::test]
    async fn a_stopped_world_answers_270_and_ends() {
        let dir = tempfile::tempdir().unwrap();
        let (gate, inbox) = gate(dir.path()).await;
        drop(inbox);
        let (mut ours, peer, mut server, mut theirs, mut client) = session().await;
        let (_newer, mut replaced) = oneshot::channel();
        let tester = account("tester");
        let played = play(
            &mut ours,
            peer,
            &gate,
            &mut server,
            &tester,
            1,
            lin(None),
            &mut replaced,
        );
        tokio::time::timeout(Duration::from_secs(5), played)
            .await
            .expect("at once")
            .unwrap();
        drop(ours);
        let events = rest(&mut theirs, &mut client).await;
        let [ClientEvent::Message(m)] = &events[..] else {
            panic!("one message: {events:?}")
        };
        assert_eq!(m.ty, 0);
        assert_eq!(
            pickle::decode(&m.body).unwrap(),
            [
                Value::Int(lobby::ERROR_CODE),
                Value::Int(code::SERVER_NOT_RESP)
            ]
        );
    }

    #[tokio::test]
    async fn kicks_close_with_their_reasons() {
        // world.md 10.3: the client's own close-reason keys, or none for a client that stopped
        // reading or flooded, which then sees the maintenance text (10.1).
        for (reason, why) in [
            (KickReason::Shutdown, Some("error server-not-running")),
            (KickReason::Kicked, Some("error kick-by-user")),
            (KickReason::Replaced, Some("error duplicate-login")),
            (KickReason::SlowReader, None),
            (KickReason::Flood, None),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let (gate, mut inbox) = gate(dir.path()).await;
            let (mut ours, peer, mut server, mut theirs, mut client) = session().await;
            let (_newer, mut replaced) = oneshot::channel();
            let tester = account("tester");
            let played = play(
                &mut ours,
                peer,
                &gate,
                &mut server,
                &tester,
                1,
                lin(None),
                &mut replaced,
            );
            let world = async {
                let WorldIn::Join { player, out } = told(&mut inbox).await else {
                    panic!("a join first")
                };
                assert_eq!((player.conn, player.pid), (1, 258));
                out.send(WorldOut::Kicked(reason)).await.unwrap();
            };
            let (ended, ()) = tokio::time::timeout(Duration::from_secs(5), async {
                tokio::join!(played, world)
            })
            .await
            .expect("within 5 s");
            ended.unwrap();
            assert!(
                matches!(told(&mut inbox).await, WorldIn::Leave { conn: 1 }),
                "{reason:?}"
            );
            drop(ours);
            let events = rest(&mut theirs, &mut client).await;
            let want: Vec<ClientEvent> = why
                .map(|w| ClientEvent::Closed(w.as_bytes().to_vec()))
                .into_iter()
                .collect();
            assert_eq!(events, want, "{reason:?}");
        }
    }

    /// A real account named `name` in the gate's store, made through an invite.
    async fn registered(gate: &Gate, name: &str) -> Account {
        let code = fenji_auth::invite::InviteCode::generate().unwrap();
        let store = gate.accounts.store();
        store
            .create_invite(&code.hash(), None, 0, None)
            .await
            .unwrap();
        let local = std::net::IpAddr::from([127, 0, 0, 1]);
        gate.accounts
            .register(&code.display(), name, "a long password", local)
            .await
            .unwrap();
        store.account_by_username(name).await.unwrap().unwrap()
    }

    #[tokio::test]
    async fn a_deleted_account_is_closed_and_leaves_the_world() {
        // Ruling R5: a player who only moves sends no script message for the lobby's check, so
        // the link checks the account every `account_check`, and closes as the lobby does.
        let dir = tempfile::tempdir().unwrap();
        let (mut gate, mut inbox) = gate(dir.path()).await;
        gate.limits.account_check = Duration::from_millis(100);
        let tester = registered(&gate, "tester").await;
        let (mut ours, peer, mut server, mut theirs, mut client) = session().await;
        let (_newer, mut replaced) = oneshot::channel();
        let played = play(
            &mut ours,
            peer,
            &gate,
            &mut server,
            &tester,
            1,
            lin(None),
            &mut replaced,
        );
        let world = async {
            let WorldIn::Join { out, .. } = told(&mut inbox).await else {
                panic!("a join first")
            };
            assert!(
                gate.accounts
                    .store()
                    .delete_account("tester")
                    .await
                    .unwrap()
            );
            // Kept open: the close must come from the link's check, not from the world.
            out
        };
        let (ended, _out) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(played, world)
        })
        .await
        .expect("closed within 5 s");
        ended.unwrap();
        assert!(matches!(told(&mut inbox).await, WorldIn::Leave { conn: 1 }));
        drop(ours);
        assert_eq!(
            rest(&mut theirs, &mut client).await,
            [ClientEvent::Closed(b"error auth-failed".to_vec())]
        );
    }

    #[tokio::test]
    async fn a_ban_the_registry_tells_closes_a_player_in_the_world() {
        // What the console's `ban` does besides `WorldIn::Ban` (HA5): the online registry tells
        // the connection to close, as it tells one a newer login replaced, and the link closes
        // with the ban's reason. The world hears that it left.
        let dir = tempfile::tempdir().unwrap();
        let (gate, mut inbox) = gate(dir.path()).await;
        let (mut ours, peer, mut server, mut theirs, mut client) = session().await;
        let (registry, mut closing) = oneshot::channel();
        let tester = account("tester");
        let played = play(
            &mut ours,
            peer,
            &gate,
            &mut server,
            &tester,
            1,
            lin(None),
            &mut closing,
        );
        let world = async {
            let WorldIn::Join { out, .. } = told(&mut inbox).await else {
                panic!("a join first")
            };
            registry.send(Closing::Banned).unwrap();
            // Kept open: the close must come from the registry, not from the world.
            out
        };
        let (ended, _out) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(played, world)
        })
        .await
        .expect("closed within 5 s");
        ended.unwrap();
        assert!(matches!(told(&mut inbox).await, WorldIn::Leave { conn: 1 }));
        drop(ours);
        assert_eq!(
            rest(&mut theirs, &mut client).await,
            [ClientEvent::Closed(gate::BANNED.as_bytes().to_vec())]
        );
    }

    #[tokio::test]
    async fn the_world_letting_go_closes_the_connection() {
        // The world drops a full channel without a Kicked (carry-forward 3).
        let dir = tempfile::tempdir().unwrap();
        let (gate, mut inbox) = gate(dir.path()).await;
        let (mut ours, peer, mut server, mut theirs, mut client) = session().await;
        let (_newer, mut replaced) = oneshot::channel();
        let tester = account("tester");
        let played = play(
            &mut ours,
            peer,
            &gate,
            &mut server,
            &tester,
            1,
            lin(None),
            &mut replaced,
        );
        let world = async {
            let WorldIn::Join { out, .. } = told(&mut inbox).await else {
                panic!("a join first")
            };
            drop(out);
        };
        let (ended, ()) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(played, world)
        })
        .await
        .expect("within 5 s");
        ended.unwrap();
        assert!(matches!(told(&mut inbox).await, WorldIn::Leave { conn: 1 }));
        drop(ours);
        assert_eq!(rest(&mut theirs, &mut client).await, []);
    }

    #[tokio::test]
    async fn the_worlds_writes_dont_keep_a_silent_client() {
        // Ruling R6: only the client's reads move the idle deadline, not what the link writes.
        let dir = tempfile::tempdir().unwrap();
        let (mut gate, mut inbox) = gate(dir.path()).await;
        let idle = Duration::from_millis(300);
        gate.limits.idle = idle;
        let (mut ours, peer, mut server, mut theirs, mut client) = session().await;
        let (_newer, mut replaced) = oneshot::channel();
        let tester = account("tester");
        let began = std::time::Instant::now();
        let played = play(
            &mut ours,
            peer,
            &gate,
            &mut server,
            &tester,
            1,
            lin(None),
            &mut replaced,
        );
        let world = async {
            let WorldIn::Join { out, .. } = told(&mut inbox).await else {
                panic!("a join first")
            };
            // A 772 every 100 ms, which the link writes, until it lets go of its end.
            let mut every = tokio::time::interval(Duration::from_millis(100));
            let at = START;
            while out.send(WorldOut::LoadScene { pid: 258, at }).await.is_ok() {
                every.tick().await;
            }
        };
        let (ended, ()) = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(played, world)
        })
        .await
        .expect("closed within 2 s");
        ended.unwrap();
        assert!(began.elapsed() >= idle, "{:?}", began.elapsed());
        assert!(matches!(told(&mut inbox).await, WorldIn::Leave { conn: 1 }));
        drop(ours);
        let events = rest(&mut theirs, &mut client).await;
        assert!(events.len() >= 2, "the 772s went out: {events:?}");
    }

    /// What this thread logs while the subscriber is its default, as text.
    #[derive(Clone, Default)]
    struct Logs(Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for Logs {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Logs {
        type Writer = Logs;
        fn make_writer(&'a self) -> Logs {
            self.clone()
        }
    }

    #[tokio::test]
    async fn a_store_error_in_the_account_check_keeps_the_player() {
        // Ruling R5, as in the lobby: a store that fails now may answer at the next check, so the
        // check logs the error and keeps the connection.
        let logs = Logs::default();
        let log = tracing_subscriber::fmt()
            .with_writer(logs.clone())
            .with_ansi(false);
        let _logging = tracing::subscriber::set_default(log.finish());
        let dir = tempfile::tempdir().unwrap();
        let (mut gate, mut inbox) = gate(dir.path()).await;
        gate.limits.account_check = Duration::from_millis(50);
        gate.accounts.store().close().await;
        let (mut ours, peer, mut server, _theirs, _client) = session().await;
        let (_newer, mut replaced) = oneshot::channel();
        let tester = account("tester");
        let played = play(
            &mut ours,
            peer,
            &gate,
            &mut server,
            &tester,
            1,
            lin(None),
            &mut replaced,
        );
        tokio::pin!(played);
        let open = tokio::time::timeout(Duration::from_millis(400), &mut played).await;
        assert!(open.is_err(), "the connection stays open: {open:?}");
        let text = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        assert!(text.matches("account check failed").count() >= 3, "{text}");
        let WorldIn::Join { out, .. } = told(&mut inbox).await else {
            panic!("a join first")
        };
        drop(out);
        let ended = tokio::time::timeout(Duration::from_secs(5), played).await;
        ended.expect("the world letting go ends it").unwrap();
    }

    // --- Slice 2 (Amendment B): seeing each other.

    /// A type-5 body as the client builds it (world.md 14.4): the actor, the value, a byte 1, a
    /// zero `u32` and the machine byte, 0 here.
    fn filter(pid: i64, value: u32) -> Vec<u8> {
        let mut b = pid.to_le_bytes().to_vec();
        b.extend(value.to_le_bytes());
        b.push(1);
        b.extend([0; 5]);
        b
    }

    /// Another player as the world sends it: Mai, a Spellsword, with a look the lobby stored.
    fn mai() -> (Look, Appearance) {
        let appearance = Appearance {
            voice: Some(2),
            base_face: 2,
            face: Some("v:5;".into()),
            equips_id: 1,
            portrait: 3 << 16,
        };
        let stored = pickle::encode(&[Value::Table(appearance.to_table())]).unwrap();
        let look = Look {
            name: "Mai".into(),
            vocation: 2,
            skeleton: 1,
            level: 37,
            appearance: Arc::from(stored),
        };
        (look, appearance)
    }

    /// The test's end of a connection in the world: the world's sender for it, and the client.
    struct Linked {
        out: mpsc::Sender<WorldOut>,
        theirs: TcpStream,
        client: Client,
        inbox: mpsc::UnboundedReceiver<WorldIn>,
    }

    impl Linked {
        /// One message from the client, as its transport writes it.
        async fn send(&mut self, ty: u8, body: &[u8]) {
            let bytes = self.client.send(ty, body).unwrap();
            self.theirs.write_all(&bytes).await.unwrap();
        }

        /// What the client reads until `n` messages have come (5 s at most); a read that brings
        /// more brings them all.
        async fn messages(&mut self, n: usize) -> Vec<Message> {
            let mut got = Vec::new();
            let mut buf = vec![0u8; 64 * 1024];
            while got.len() < n {
                let read = tokio::time::timeout(Duration::from_secs(5), self.theirs.read(&mut buf))
                    .await
                    .expect("a message within 5 s")
                    .unwrap();
                assert!(read > 0, "the connection closed after {got:?}");
                let (events, _) = self.client.receive(&buf[..read]).unwrap();
                for e in events {
                    match e {
                        ClientEvent::Message(m) => got.push(m),
                        other => panic!("{other:?}"),
                    }
                }
            }
            got
        }
    }

    /// Lin (pid 258) in a world that is `test`'s: it gets the link's channel, the client and the
    /// link's messages to the world. When `test` ends, its channel goes, and the link with it.
    async fn linked<F, Fut>(test: F)
    where
        F: FnOnce(Linked) -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        linked_at(NOW, test).await;
    }

    /// [`linked`] with the gate's clock at `now_ms`.
    async fn linked_at<F, Fut>(now_ms: i64, test: F)
    where
        F: FnOnce(Linked) -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        let dir = tempfile::tempdir().unwrap();
        let (gate, mut inbox) = gate_at(dir.path(), now_ms).await;
        let (mut ours, peer, mut server, theirs, client) = session().await;
        let (_newer, mut replaced) = oneshot::channel();
        let tester = account("tester");
        let played = play(
            &mut ours,
            peer,
            &gate,
            &mut server,
            &tester,
            1,
            lin(None),
            &mut replaced,
        );
        let world = async move {
            let WorldIn::Join { player, out } = told(&mut inbox).await else {
                panic!("a join first")
            };
            assert_eq!((player.conn, player.pid), (1, 258));
            test(Linked {
                out,
                theirs,
                client,
                inbox,
            })
            .await;
        };
        let (ended, ()) = tokio::time::timeout(Duration::from_secs(10), async {
            tokio::join!(played, world)
        })
        .await
        .expect("within 10 s");
        ended.unwrap();
    }

    const AT: Pos = Pos {
        x: 2431.0,
        y: -71493.5,
        z: 64854.4,
        yaw: 345.0,
    };

    #[tokio::test]
    async fn announce_sends_type_6() {
        linked(|mut l| async move {
            l.out
                .send(WorldOut::Announce { pid: 300, pos: AT })
                .await
                .unwrap();
            let got = l.messages(1).await;
            assert_eq!(got.len(), 1, "{got:?}");
            assert_eq!(got[0].ty, Stc::SyncIdEnter.to_u8());
            // The pid, x, y and z rounded half away from zero, then the flag and the priority 0.
            let mut want = 300i64.to_le_bytes().to_vec();
            for c in [2431i32, -71494, 64854] {
                want.extend(c.to_le_bytes());
            }
            want.extend([0, 0]);
            assert_eq!(got[0].body, want);
        })
        .await;
    }

    #[tokio::test]
    async fn a_pull_reaches_the_world() {
        linked(|mut l| async move {
            l.send(Cts::SyncFilterSet.to_u8(), &filter(300, world::PULL))
                .await;
            assert!(matches!(
                told(&mut l.inbox).await,
                WorldIn::Pull { conn: 1, pid: 300 }
            ));
        })
        .await;
    }

    #[tokio::test]
    async fn a_transport_time_sync_is_echoed_with_the_clients_own_clock() {
        // #269: the client's ping display reads only what a transport-level time sync from the
        // server sets, the client's clock less the echoed one (transport.md 4.4). Unanswered, it
        // stays at its first value, 50 ms.
        linked(|l| async move {
            // The whole of `l`: the world's channel in it must live until the test ends, or the
            // link sees the world let go and ends before it reads the sync.
            let mut l = l;
            let bytes = l.client.time_sync(1_791_950_400_123);
            l.theirs.write_all(&bytes).await.unwrap();
            let mut buf = vec![0u8; 1024];
            let read = tokio::time::timeout(Duration::from_secs(5), l.theirs.read(&mut buf))
                .await
                .expect("an answer within 5 s")
                .unwrap();
            let (events, _) = l.client.receive(&buf[..read]).unwrap();
            assert_eq!(events, [ClientEvent::TimeSync(1_791_950_400_123)]);
        })
        .await;
    }

    #[tokio::test]
    async fn filter_values_stay_out_of_the_world() {
        linked(|mut l| async move {
            // The sync filter's 32, and a body a byte short: nothing for the world.
            l.send(Cts::SyncFilterSet.to_u8(), &filter(300, 32)).await;
            l.send(Cts::SyncFilterSet.to_u8(), &filter(300, 0)[..17])
                .await;
            // The connection stays open: a time sync after them is answered.
            let mut sync = 7u32.to_le_bytes().to_vec();
            sync.push(1);
            sync.extend([0; 8]);
            l.send(Cts::TimeSync.to_u8(), &sync).await;
            let got = l.messages(1).await;
            assert_eq!(got.len(), 1, "{got:?}");
            assert_eq!(got[0].ty, Stc::TimeSync.to_u8());
            // The link handles a read's messages in order, so anything for the world is there.
            assert!(l.inbox.try_recv().is_err(), "the world got something");
        })
        .await;
    }

    #[tokio::test]
    async fn enter_sends_7_then_8() {
        linked(|mut l| async move {
            let (look, appearance) = mai();
            l.out
                .send(WorldOut::Enter {
                    pid: 300,
                    look: look.clone(),
                    pos: AT,
                    speed: 100,
                })
                .await
                .unwrap();
            let got = l.messages(2).await;
            assert_eq!(got.len(), 2, "{got:?}");
            assert_eq!(got[0].ty, Stc::SyncEnter.to_u8());
            // A player (1), difficulty 255, template vocation 2 × 10000 (world.md 5.4).
            assert_eq!(got[0].body[..8], 300i64.to_le_bytes());
            assert_eq!(got[0].body[8..14], [1, 255, 0x20, 0x4e, 0, 0]);
            let blob = world::decode_blob(&got[0].body[14..]).unwrap();
            let get = |i: u16| blob.iter().find(|(x, _)| *x == i).map(|(_, v)| v.clone());
            assert_eq!(get(1), Some(BlobValue::Str(b"Mai".to_vec())));
            assert_eq!(get(5), Some(BlobValue::Int(2)));
            assert_eq!(get(6), Some(BlobValue::Int(1)));
            assert_eq!(get(9), Some(BlobValue::Int(37)));
            assert_eq!(
                get(16),
                Some(BlobValue::Vec([2431.0, -71493.5, 64854.4, 345.0]))
            );
            assert_eq!(get(1325), Some(BlobValue::Int(100)));
            // The player's own 7 for that character (world.md 14.9).
            let own = world::sync_enter_player(&PlayerActor {
                pid: 300,
                name: "Mai",
                vocation: 2,
                skeleton: 1,
                level: 37,
                appearance: &appearance,
                pos: [AT.x, AT.y, AT.z, AT.yaw],
                speed: 100,
            });
            assert_eq!(got[0].body, own);
            assert_eq!(
                (got[1].ty, &got[1].body[..]),
                (
                    Stc::SyncEnterFinish.to_u8(),
                    &world::sync_enter_finish(300)[..]
                )
            );
        })
        .await;
    }

    #[tokio::test]
    async fn leave_sends_type_9() {
        linked(|mut l| async move {
            let far = (1 << 32) + 1;
            l.out
                .send(WorldOut::Leave {
                    pids: vec![300, far],
                })
                .await
                .unwrap();
            let got = l.messages(1).await;
            assert_eq!(got.len(), 1, "{got:?}");
            assert_eq!(got[0].ty, Stc::SyncLeave.to_u8());
            let mut want = 300i64.to_le_bytes().to_vec();
            want.extend(far.to_le_bytes());
            assert_eq!(got[0].body, want);
        })
        .await;
    }

    #[tokio::test]
    async fn the_players_own_pid_is_never_sent_to_it() {
        // world.md 14.11: a 9 naming it makes the client's own player leave, and a 6 would queue
        // its own id. Lin is pid 258. The 9 for 300 comes last, so whatever went out before it
        // would be read first.
        linked(|mut l| async move {
            let (look, _) = mai();
            for msg in [
                WorldOut::Announce { pid: 258, pos: AT },
                WorldOut::Enter {
                    pid: 258,
                    look,
                    pos: AT,
                    speed: 100,
                },
                WorldOut::Leave { pids: vec![258] },
                WorldOut::Leave {
                    pids: vec![258, 300],
                },
            ] {
                l.out.send(msg).await.unwrap();
            }
            let got = l.messages(1).await;
            assert_eq!(got.len(), 1, "{got:?}");
            assert_eq!(
                (got[0].ty, got[0].body.clone()),
                (Stc::SyncLeave.to_u8(), 300i64.to_le_bytes().to_vec())
            );
        })
        .await;
    }

    // --- Slice 3 (Amendment C): moving.

    /// The plan's T, 1,791,936,000,000 ms: a multiple of 16.
    const T: u64 = 1_791_936_000_000;

    /// A 200-byte type 8 as the client builds it (world.md §16, the plan's V11 layout): the time,
    /// x, y and z, the yaw, the state blocks filled with `fill`, the direction part `part` (the
    /// direction, the yaw again, the yaw offset, the rate and the pitches), and the 24 bytes after
    /// it with the facts about the machine 0x5A.
    fn move_body(time: u64, at: [i32; 3], yaw: i16, fill: u8, part: [u8; 11]) -> Vec<u8> {
        let mut b = time.to_le_bytes().to_vec();
        for c in at {
            b.extend(c.to_le_bytes());
        }
        b.extend([0; 5]);
        b.extend(yaw.to_le_bytes());
        b.extend([0; 4]);
        b.extend([fill; 134]);
        b.extend(part);
        b.extend([
            0, 0, 0, 0x80, 0x3e, 0x5a, 0, 0, 0, 0x3f, 0x14, 0, 0, 0, 0, 0, 0, 0,
        ]);
        b.extend([0x5a, 0x5a, 0x5a, 0x5a, 0x5a, 0]);
        assert_eq!(b.len(), world::MOVE_DIR_LEN);
        b
    }

    /// The default start, as the plan's vectors use it.
    const START_XYZ: [i32; 3] = [2366, -71735, 64854];

    #[tokio::test]
    async fn a_move_reaches_the_world_bounded() {
        // The plan's V14 with the clock at T + 100: the position as it is, the rest bounded, and
        // nothing else of the body (Review Focus 3).
        linked_at(T as i64 + 100, |mut l| async move {
            let hostile = [
                0xc8, 0xe7, 0x03, 0xe8, 0x03, 0xff, 0xff, 0x00, 0x80, 0xff, 0x7f,
            ];
            l.send(
                Cts::MoveDir.to_u8(),
                &move_body(0, START_XYZ, -15, 0xff, hostile),
            )
            .await;
            let WorldIn::Move { conn, step } = told(&mut l.inbox).await else {
                panic!("a move")
            };
            assert_eq!(conn, 1);
            assert_eq!(
                step.pos,
                Pos {
                    x: 2366.0,
                    y: -71735.0,
                    z: 64854.0,
                    yaw: -15.0
                }
            );
            // V14's bounded detail: T − 4,912, direction 8, offset −80, rate 340, pitches ±90.
            let want = [
                0xd0, 0x5c, 0xb5, 0x37, 0xa1, 0x01, 0x00, 0x00, 0x08, 0xb0, 0xff, 0x54, 0x01, 0xa6,
                0xff, 0x5a, 0x00,
            ];
            assert_eq!(step.detail[..], want);
            // A body of another length goes nowhere, and the connection stays: a time sync after
            // it is answered.
            l.send(Cts::MoveDir.to_u8(), &[0; 150]).await;
            let mut sync = 7u32.to_le_bytes().to_vec();
            sync.push(1);
            sync.extend([0; 8]);
            l.send(Cts::TimeSync.to_u8(), &sync).await;
            let got = l.messages(1).await;
            assert_eq!(got.len(), 1, "{got:?}");
            assert_eq!(got[0].ty, Stc::TimeSync.to_u8());
            assert!(l.inbox.try_recv().is_err(), "the world got something");
        })
        .await;
    }

    /// A detail for the test gate's clock: direction 1, rate 100, at [`NOW`].
    fn running() -> world::MoveDetail {
        world::MoveDetail {
            time: NOW as u64,
            dir: 1,
            yaw_offset: 0,
            rate: 100,
            pitch: -10,
            pitch2: 10,
        }
    }

    #[tokio::test]
    async fn moved_sends_type_13() {
        linked(|mut l| async move {
            let step = |detail: &[u8]| crate::world::MoveStep {
                pos: AT,
                detail: Arc::from(detail),
            };
            let bytes = running().to_bytes();
            // Lin's own move, and Kai's with a detail a byte short, send nothing; then Mai's move
            // goes out, the only 13, so a 13 for either of the others would come first.
            for msg in [
                WorldOut::Moved {
                    pid: 258,
                    step: step(&bytes),
                },
                WorldOut::Moved {
                    pid: 301,
                    step: step(&bytes[..16]),
                },
                WorldOut::Moved {
                    pid: 300,
                    step: step(&bytes),
                },
            ] {
                l.out.send(msg).await.unwrap();
            }
            let got = l.messages(1).await;
            assert_eq!(got.len(), 1, "{got:?}");
            assert_eq!(got[0].ty, Stc::MoveDir.to_u8());
            let want = world::move_dir_relay(300, [AT.x, AT.y, AT.z, AT.yaw], &running());
            assert_eq!(got[0].body, want);
            assert_eq!(got[0].body[..8], 300i64.to_le_bytes());
        })
        .await;
    }

    #[tokio::test]
    async fn correct_and_teleport_send_type_20() {
        linked(|mut l| async move {
            let there = Pos {
                x: -1.5,
                y: 2.25,
                z: 30.0,
                yaw: -45.0,
            };
            l.out.send(WorldOut::Correct { pos: AT }).await.unwrap();
            l.out.send(WorldOut::Teleport { pos: there }).await.unwrap();
            let got = l.messages(2).await;
            assert_eq!(got.len(), 2, "{got:?}");
            let t = world::sync_time(NOW as u64);
            for (m, pos) in got.iter().zip([AT, there]) {
                assert_eq!(m.ty, Stc::MoveSyncSvr.to_u8());
                assert_eq!(
                    m.body,
                    world::move_sync_svr(258, t, [pos.x, pos.y, pos.z, pos.yaw])
                );
                // Its own pid, T, and the mask for the position and the yaw.
                assert_eq!(m.body[..8], 258i64.to_le_bytes());
                assert_eq!(m.body[8..16], (NOW as u64 - 160).to_le_bytes());
                assert_eq!(m.body[16..18], [0xc0, 0x00]);
            }
        })
        .await;
    }

    // --- Slice 4 (Task 16): chat.

    fn s(x: &str) -> Value {
        Value::Str(x.as_bytes().to_vec())
    }

    /// 115 as the client's `net_manager_send` sends it (chat.md 1.1): a type-0 script message.
    fn transmit(args: &[Value]) -> Vec<u8> {
        fenji_proto::packet::script_body(chat::CHAT_TRANSMIT, args).unwrap()
    }

    /// A time sync, then its reply: proof the connection is open and everything before it was read.
    async fn still_open(l: &mut Linked) {
        let mut sync = 7u32.to_le_bytes().to_vec();
        sync.push(1);
        sync.extend([0; 8]);
        l.send(Cts::TimeSync.to_u8(), &sync).await;
        let got = l.messages(1).await;
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].ty, Stc::TimeSync.to_u8());
    }

    #[tokio::test]
    async fn a_line_reaches_the_world() {
        linked(|mut l| async move {
            let whisper = Value::Table(Table {
                array: Vec::new(),
                hash: vec![(s("pid"), Value::Int(300)), (s("name"), s("Mai@Unknown"))],
            });
            let typed = Value::Table(Table {
                array: Vec::new(),
                hash: vec![(s("name"), s("Mai@Unknown"))],
            });
            for (args, want) in [
                (
                    vec![Value::Int(0), s("hi"), Value::Nil],
                    ChatIn {
                        channel: ChatChannel::Nearby,
                        text: "hi".into(),
                        to: None,
                    },
                ),
                (
                    vec![Value::Int(5), s("all &lt;3"), Value::Nil],
                    ChatIn {
                        channel: ChatChannel::World,
                        text: "all &lt;3".into(),
                        to: None,
                    },
                ),
                (
                    vec![Value::Int(1), s("psst"), whisper],
                    ChatIn {
                        channel: ChatChannel::Whisper,
                        text: "psst".into(),
                        to: Some(WhisperTarget::Pid(300)),
                    },
                ),
                (
                    vec![Value::Int(1), s("psst"), typed],
                    ChatIn {
                        channel: ChatChannel::Whisper,
                        text: "psst".into(),
                        to: Some(WhisperTarget::Name("Mai@Unknown".into())),
                    },
                ),
            ] {
                l.send(Cts::Script.to_u8(), &transmit(&args)).await;
                let WorldIn::Chat { conn, line } = told(&mut l.inbox).await else {
                    panic!("a line for the world")
                };
                assert_eq!((conn, line), (1, want));
            }
        })
        .await;
    }

    #[tokio::test]
    async fn a_line_the_link_cant_take_is_a_270_and_the_world_hears_nothing() {
        linked(|mut l| async move {
            // Team chat (2), then a "line" that isn't text.
            for (args, code) in [
                (
                    vec![Value::Int(2), s("hi"), Value::Nil],
                    chat::code::CHAT_INVALID_CHANNEL,
                ),
                (
                    vec![Value::Int(0), Value::Int(7), Value::Nil],
                    chat::code::CHAT_INVALID_CONTENT,
                ),
            ] {
                l.send(Cts::Script.to_u8(), &transmit(&args)).await;
                let got = l.messages(1).await;
                assert_eq!(got.len(), 1, "{got:?}");
                assert_eq!(got[0].ty, Stc::Script.to_u8());
                assert_eq!(
                    pickle::decode(&got[0].body).unwrap(),
                    [Value::Int(lobby::ERROR_CODE), Value::Int(code)]
                );
            }
            still_open(&mut l).await;
            assert!(l.inbox.try_recv().is_err(), "the world got something");
        })
        .await;
    }

    #[tokio::test]
    async fn a_line_or_a_name_too_long_for_the_world_is_a_270_at_the_link() {
        // #280: the link bounds what reaches the world. A text over the client's own 2,048 bytes
        // (chat.md 1.5), or a whisper to a name over 64 bytes, comes back at once as too long;
        // the longest of each still reaches the world, which checks it.
        linked(|mut l| async move {
            let named = |name: &str| {
                Value::Table(Table {
                    array: Vec::new(),
                    hash: vec![(s("name"), s(name))],
                })
            };
            let longest = "a".repeat(2_048);
            let name = "n".repeat(64);
            for (args, want) in [
                (
                    vec![Value::Int(0), s(&longest), Value::Nil],
                    ChatIn {
                        channel: ChatChannel::Nearby,
                        text: longest.clone(),
                        to: None,
                    },
                ),
                (
                    vec![Value::Int(1), s("psst"), named(&name)],
                    ChatIn {
                        channel: ChatChannel::Whisper,
                        text: "psst".into(),
                        to: Some(WhisperTarget::Name(name.clone())),
                    },
                ),
            ] {
                l.send(Cts::Script.to_u8(), &transmit(&args)).await;
                let WorldIn::Chat { conn, line } = told(&mut l.inbox).await else {
                    panic!("a line for the world")
                };
                assert_eq!((conn, line), (1, want));
            }
            // Bytes count, not characters: 1,025 é are 2,050 bytes.
            for args in [
                vec![Value::Int(0), s(&format!("{longest}a")), Value::Nil],
                vec![Value::Int(5), s(&"é".repeat(1_025)), Value::Nil],
                vec![Value::Int(1), s("psst"), named(&format!("{name}n"))],
                vec![Value::Int(1), s("psst"), named(&"é".repeat(33))],
            ] {
                l.send(Cts::Script.to_u8(), &transmit(&args)).await;
                let got = l.messages(1).await;
                assert_eq!(got.len(), 1, "{got:?}");
                assert_eq!(got[0].ty, Stc::Script.to_u8());
                assert_eq!(
                    pickle::decode(&got[0].body).unwrap(),
                    [
                        Value::Int(lobby::ERROR_CODE),
                        Value::Int(chat::code::CHAT_MAX_CHAR)
                    ]
                );
            }
            still_open(&mut l).await;
            assert!(l.inbox.try_recv().is_err(), "the world got something");
        })
        .await;
    }

    #[tokio::test]
    async fn chat_sends_378() {
        linked(|mut l| async move {
            let mai = Speaker {
                pid: 300,
                name: "Mai".into(),
                vocation: 2,
            };
            let lin = Speaker {
                pid: 258,
                name: "Lin".into(),
                vocation: 7,
            };
            let mai_link = chat::player_link(&chat::Player {
                pid: 300,
                name: "Mai",
                vocation: 2,
            });
            let lin_link = chat::player_link(&chat::Player {
                pid: 258,
                name: "Lin",
                vocation: 7,
            });
            for (out, want) in [
                (
                    ChatOut::Said {
                        channel: ChatChannel::Nearby,
                        from: mai.clone(),
                        to: None,
                        text: "hi".into(),
                    },
                    [Value::Int(0), s(&mai_link), s("hi"), s("")],
                ),
                (
                    ChatOut::Said {
                        channel: ChatChannel::World,
                        from: mai.clone(),
                        to: None,
                        text: "all".into(),
                    },
                    [Value::Int(5), s(&mai_link), s("all"), s("")],
                ),
                (
                    ChatOut::Said {
                        channel: ChatChannel::Whisper,
                        from: mai,
                        to: Some(lin),
                        text: "psst".into(),
                    },
                    [Value::Int(1), s(&mai_link), s("psst"), s(&lin_link)],
                ),
                (
                    ChatOut::Announce {
                        text: "Restart at 20:00".into(),
                    },
                    [Value::Int(66), s(""), s("Restart at 20:00"), s("")],
                ),
                (
                    ChatOut::System {
                        text: "usage: kick <name>".into(),
                    },
                    [Value::Int(64), s(""), s("usage: kick &lt;name&gt;"), s("")],
                ),
            ] {
                l.out.send(WorldOut::Chat(out)).await.unwrap();
                let got = l.messages(1).await;
                assert_eq!(got.len(), 1, "{got:?}");
                assert_eq!(got[0].ty, Stc::Script.to_u8());
                let v = pickle::decode(&got[0].body).unwrap();
                assert_eq!(v[0], Value::Int(chat::SYS_CHAT_MESSAGE));
                assert_eq!(v[1..5], want);
                assert_eq!(v[5..], [Value::Nil, Value::Nil]);
            }
        })
        .await;
    }

    #[tokio::test]
    async fn a_refusal_sends_270_with_its_code() {
        linked(|mut l| async move {
            for (why, code) in [
                (ChatRefusal::Invalid, chat::code::CHAT_INVALID_CONTENT),
                (ChatRefusal::TooLong, chat::code::CHAT_MAX_CHAR),
                (ChatRefusal::TooFast, chat::code::CHAT_TOO_FAST),
                (ChatRefusal::NotOnline, chat::code::CHAT_RECEIVER_OFFLINE),
                (ChatRefusal::ToSelf, chat::code::CHAT_TALK_TOSELF),
                (ChatRefusal::Channel, chat::code::CHAT_INVALID_CHANNEL),
            ] {
                l.out.send(WorldOut::ChatRefused(why)).await.unwrap();
                let got = l.messages(1).await;
                assert_eq!(got.len(), 1, "{got:?}");
                assert_eq!(got[0].ty, Stc::Script.to_u8());
                assert_eq!(
                    pickle::decode(&got[0].body).unwrap(),
                    [Value::Int(lobby::ERROR_CODE), Value::Int(code)],
                    "{why:?}"
                );
            }
        })
        .await;
    }

    #[test]
    fn the_logs_never_hold_a_line() {
        // A host's log says that a line came or went, never what it said or whom a whisper named.
        let logs = Logs::default();
        let log = tracing_subscriber::fmt()
            .with_writer(logs.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE);
        let _logging = tracing::subscriber::set_default(log.finish());
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let (gate, _inbox) = gate(dir.path()).await;
            let secret = Value::Table(Table {
                array: Vec::new(),
                hash: vec![(s("name"), s("Zed@Unknown"))],
            });
            for args in [
                vec![Value::Int(0), s("a secret plan"), Value::Nil],
                vec![Value::Int(1), s("a secret plan"), secret],
                vec![Value::Int(2), s("a secret plan"), Value::Nil],
            ] {
                on_message(&gate, 1, Cts::Script.to_u8(), &transmit(&args));
            }
            let (_ours, _peer, mut server, _theirs, _client) = session().await;
            let said = WorldOut::Chat(ChatOut::Said {
                channel: ChatChannel::Whisper,
                from: Speaker {
                    pid: 300,
                    name: "Mai".into(),
                    vocation: 2,
                },
                to: Some(Speaker {
                    pid: 301,
                    name: "Zed".into(),
                    vocation: 2,
                }),
                text: "a secret plan".into(),
            });
            encode(&mut server, 258, NOW as u64, said).unwrap();
        });
        let text = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        assert!(
            text.contains("115"),
            "the lines were logged as arriving: {text}"
        );
        assert!(
            text.contains("378"),
            "the line was logged as going out: {text}"
        );
        for word in ["secret", "Zed", "Mai"] {
            assert!(!text.contains(word), "{word}: {text}");
        }
    }
}
