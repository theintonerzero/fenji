//! A connection in the world (M4 world plan, the local slices; docs/protocol/packets/world.md).
//! The gate hands its connection here when the client's 10 names one of the account's characters.
//! The link joins the world as that character, maps the client's messages to [`WorldIn`] and each
//! [`WorldOut`] back to messages, and leaves the world however it ends. Slice 1 (Amendment A) maps
//! entering: 772, the time syncs, type 3, then the player's own 7 and 8; later slices add the
//! rest. Script messages go out through the gate's `script_out`, which refuses `NEVER_SEND`.
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use fenji_proto::codec::StoredCodec;
use fenji_proto::handshake::{Event, Server};
use fenji_proto::lobby::{self, Appearance, code};
use fenji_proto::opcode::{Cts, Stc};
use fenji_proto::packet::Up;
use fenji_proto::pickle::{Table, Value};
use fenji_proto::world::{self, SelfActor};
use fenji_store::{Account, Character};
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, MissedTickBehavior};

use crate::gate::{self, Gate};
use crate::lobby::stored_appearance;
use crate::world::{
    ConnId, KickReason, Location, Look, Player, Pos, WorldHandle, WorldIn, WorldOut,
};

/// The world's messages a connection may hold unwritten; one more and the world lets go of it
/// (`KickReason::SlowReader`). One mover can push up to 127 relays a tick, so not fewer.
pub(crate) const OUTBOUND: usize = 256;
// Carry-forward 7: two ticks of one mover's 127 relays must fit, so no later slice may shrink it.
const _: () = assert!(OUTBOUND >= 256);

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

/// Plays `character` in the world on the gate's connection, until the client leaves, the world
/// lets go of it, or a newer login replaces it.
#[allow(clippy::too_many_arguments)] // the connection's state, as the lobby loop holds it
pub(crate) async fn play(
    sock: &mut TcpStream,
    peer: SocketAddr,
    gate: &Gate,
    server: &mut Server,
    account: &Account,
    conn: ConnId,
    character: Character,
    replaced: &mut oneshot::Receiver<()>,
) -> anyhow::Result<()> {
    let deadline = gate.limits.write_deadline;
    let pid = character.pid;
    let (tx, mut rx) = mpsc::channel(OUTBOUND);
    let player = player(gate.start, &gate.admins, account, conn, character);
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
                    let Event::Message(m) = event else { continue };
                    match on_message(gate, conn, m.ty, &m.body) {
                        Step::Go => {}
                        Step::Reply(ty, body) => {
                            let bytes = server.send(ty, &body, &StoredCodec)?;
                            if !gate::send_or_replaced(sock, &bytes, deadline, replaced).await? {
                                tracing::info!(%peer, account = %account.username, "replaced by a newer login while a reply was blocked: closing");
                                return Ok(());
                            }
                        }
                        Step::End(why) => {
                            tracing::info!(%peer, account = %account.username, why, "left the world");
                            return Ok(());
                        }
                    }
                }
            }
            out = rx.recv() => match out {
                Some(WorldOut::Kicked(reason)) => {
                    return kicked(sock, peer, gate, server, account, reason).await;
                }
                Some(msg) => {
                    let Some(bytes) = encode(server, msg)? else { continue };
                    if !gate::send_or_replaced(sock, &bytes, deadline, replaced).await? {
                        tracing::info!(%peer, account = %account.username, "replaced by a newer login while a write was blocked: closing");
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
            Ok(()) = &mut *replaced => {
                tracing::info!(%peer, account = %account.username, "replaced by a newer login");
                let bytes = server.send(Stc::Close.to_u8(), gate::DUPLICATE_LOGIN.as_bytes(), &StoredCodec)?;
                let place = gate.waiting.take(peer.ip());
                gate::close_replaced(sock, &bytes, place, gate.limits.linger).await;
                return Ok(());
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

/// The player `c` enters as: its look, `[access].admins`' verdict on the account (without regard
/// to ASCII case), and where it last stood, or `start` for a character that never entered.
fn player(
    start: Location,
    admins: &[String],
    account: &Account,
    conn: ConnId,
    c: Character,
) -> Player {
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
        admin: admins
            .iter()
            .any(|a| a.eq_ignore_ascii_case(&account.username)),
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
    /// End the session, for this reason (logged).
    End(&'static str),
}

/// The step for one message from a client in the world (Amendment A's mapping for slice 1).
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
        Cts::GateScript | Cts::Script => match Up::decode(ty, body) {
            Ok(Up::Script {
                id: Value::Int(lobby::CLIENT_EXIT),
                ..
            }) => Step::End("the client is quitting (392)"),
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

/// The bytes for one message from the world, or `None` for what slice 1 doesn't send.
fn encode(server: &mut Server, msg: WorldOut) -> anyhow::Result<Option<Vec<u8>>> {
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
            let appearance = stored_appearance(&look.appearance).unwrap_or_else(|| {
                tracing::warn!(
                    pid,
                    "a stored appearance that doesn't decode: the defaults go out"
                );
                Appearance::from_table(&Table::default())
            });
            let actor = SelfActor {
                pid,
                name: &look.name,
                vocation: look.vocation,
                skeleton: look.skeleton,
                level: look.level,
                appearance: &appearance,
                pos: [pos.x, pos.y, pos.z, pos.yaw],
                speed,
            };
            // The speed is the blob's 1325 alone: without type 23, run m4-e3 ran as m4-e2 did
            // with it (world.md 6.5).
            tracing::debug!(pid, speed, "the player's own actor: 7 and 8");
            let mut out = server.send(
                Stc::SyncEnter.to_u8(),
                &world::sync_enter_self(&actor),
                &StoredCodec,
            )?;
            out.extend(server.send(
                Stc::SyncEnterFinish.to_u8(),
                &world::sync_enter_finish(pid),
                &StoredCodec,
            )?);
            Ok(Some(out))
        }
        // Types 6 and 9, the moves and chat aren't read yet: encoding them would send unverified
        // bytes, and closing on them would end the two-client run (Amendment A).
        other => {
            tracing::debug!(variant = variant(&other), "not sent in slice 1");
            Ok(None)
        }
    }
}

/// A `WorldOut`'s name, for the log.
fn variant(msg: &WorldOut) -> &'static str {
    match msg {
        WorldOut::LoadScene { .. } => "LoadScene",
        WorldOut::SelfEnter { .. } => "SelfEnter",
        WorldOut::Announce { .. } => "Announce",
        WorldOut::Enter { .. } => "Enter",
        WorldOut::Leave { .. } => "Leave",
        WorldOut::Moved { .. } => "Moved",
        WorldOut::Correct { .. } => "Correct",
        WorldOut::Teleport { .. } => "Teleport",
        WorldOut::Chat(_) => "Chat",
        WorldOut::ChatRefused(_) => "ChatRefused",
        WorldOut::Kicked(_) => "Kicked",
    }
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
    use fenji_proto::handshake::{AuthMessage, Event};
    use fenji_proto::hello::{HELLO_LEN, Variants};
    use fenji_proto::lobby::{self, code};
    use fenji_proto::pickle::{self, Value};
    use fenji_proto::variant::AES128;
    use fenji_store::{Place, Store};
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;
    use tokio::sync::mpsc;

    use super::*;
    use crate::accounts::Accounts;
    use crate::clock::{Clock, TestClock};
    use crate::config::AuthConfig;
    use crate::gate::{GateLimits, Places};
    use crate::lobby::LobbyLimits;
    use crate::online::Online;
    use crate::world::{KickReason, WorldIn, WorldOut};

    const START: Location = Location {
        scene: 5,
        pos: Pos {
            x: 1.0,
            y: 2.0,
            z: 3.0,
            yaw: 4.0,
        },
    };

    /// A gate whose world is the test's: what the link sends arrives on the receiver.
    async fn gate(dir: &Path) -> (Gate, mpsc::UnboundedReceiver<WorldIn>) {
        let store = Store::open(&dir.join("fenji.db")).await.unwrap();
        let clock: Arc<dyn Clock> = Arc::new(TestClock::at(1_800_000_000_000));
        let (world, inbox) = WorldHandle::fake();
        let gate = Gate {
            accounts: Arc::new(Accounts::new(
                store,
                AuthConfig::default(),
                Arc::clone(&clock),
            )),
            online: Online::new(4),
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
    fn the_player_comes_from_the_character_and_the_config() {
        let admins = ["boss".to_string()];
        let p = player(START, &admins, &account("Boss"), 3, lin(None));
        assert_eq!((p.conn, p.account, p.pid), (3, 7, 258));
        assert!(
            p.admin,
            "[access].admins names the account, whatever its case"
        );
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
        let p = player(START, &admins, &account("tester"), 3, lin(Some(place)));
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
}
