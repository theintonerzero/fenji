//! The world's ECS (M4 world spec §3.9; Fenji design spec §6.4): one bevy_ecs `World` with an
//! entity per player in the game. A tick runs its phases in order, each its own `Schedule`, so a
//! slow tick can name its slowest phase.
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bevy_ecs::prelude::*;
use bevy_ecs::system::ScheduleSystem;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, watch};

use super::admin::Caller;
use super::chat::Bucket;
use super::interest::Sight;
use super::movement::Gait;
use super::types::*;
use crate::commands::Command;

/// The world's settings, from fenji.toml.
#[derive(Debug, Clone, Resource)]
pub struct Rules {
    /// Ticks per second (`[server] tick_hz`).
    pub tick_hz: u32,
    /// The speed check's leniency (`[world] move_tolerance`).
    pub move_tolerance: f64,
    /// How often everyone's place is saved, in milliseconds (`[world] save_every_secs`).
    pub save_every_ms: u64,
    /// The most messages a connection may send a tick, kept up: each tick gives this many back
    /// to its allowance, which holds [`MAX_STALL`]'s worth ([`Inputs`]).
    ///
    /// [`MAX_STALL`]: super::MAX_STALL
    pub max_inputs_per_tick: usize,
}

/// A player's connection.
#[derive(Component)]
pub(super) struct Conn {
    pub id: ConnId,
    pub out: mpsc::Sender<WorldOut>,
}

/// The account a player belongs to.
#[derive(Component)]
pub(super) struct Account {
    pub id: i64,
    pub admin: bool,
}

/// The character: its pid, and what others see.
#[derive(Component)]
pub(super) struct Actor {
    pub pid: ActorId,
    pub look: Look,
}

/// Where the player stands.
#[derive(Component, Clone, Copy)]
pub(super) struct Here(pub Location);

/// Whether the player's client has loaded its scene.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Stage {
    Loading,
    InWorld,
}

/// Its own actor has entered once, and it has had the MOTD if there was one: a later load, as a
/// teleport to another scene would bring (#280), doesn't greet it again. No load comes later yet,
/// so no test reaches that branch until #280's teleport does.
#[derive(Component)]
struct Greeted;

/// Leaves at the end of the tick. The reason, if any, goes to its connection.
#[derive(Component, Clone, Copy)]
pub(super) struct Doomed(pub Option<KickReason>);

/// A connection's allowance of messages to the world, a token bucket (#281). It starts full,
/// each tick gives `max_inputs_per_tick` back, and it holds [`MAX_STALL`]'s worth of them
/// ([`input_cap`]). Each message spends one, and one with none left is a flood, which is kicked.
///
/// So a client is held to the limit over time, while what it sent during a stall, which arrives
/// at once, passes within the cap: a stall of the player's network, which the world never sees,
/// as one of the world itself (#222). A tick gives back the same however long it took, so a flood
/// can't buy itself more by making ticks slow (Ruling R10).
///
/// [`MAX_STALL`]: super::MAX_STALL
#[derive(Component, Debug, Clone, Copy)]
struct Inputs(usize);

impl Inputs {
    /// Spends one message's worth, if there is one left.
    fn take(&mut self) -> bool {
        match self.0.checked_sub(1) {
            Some(left) => {
                self.0 = left;
                true
            }
            None => false,
        }
    }
}

/// The tick's time, in milliseconds of a monotonic clock.
#[derive(Resource, Default)]
pub(super) struct Now(pub u64);

#[derive(Resource, Default)]
struct Inbox(Vec<WorldIn>);

/// Players by connection, pid, account and name (as [`name_key`] folds it).
#[derive(Resource, Default)]
pub(super) struct Index {
    pub conns: HashMap<ConnId, Entity>,
    pub pids: HashMap<ActorId, Entity>,
    pub accounts: HashMap<i64, Entity>,
    pub names: HashMap<String, Entity>,
}

/// What the inbound and chat phases hand to later phases, this tick.
#[derive(Resource, Default)]
pub(super) struct Pending {
    pub moves: Vec<(Entity, MoveStep)>,
    pub pulls: Vec<(Entity, ActorId)>,
    pub lines: Vec<(Entity, ChatIn)>,
    /// Commands for the admin phase: the console's, then the admins'.
    pub orders: Vec<(Caller, Command)>,
}

/// The services task's channel.
#[derive(Resource)]
pub(super) struct Services(pub mpsc::UnboundedSender<Service>);

/// Where the world sends the names of the characters it holds (M4 hosting spec §3.8), and whether
/// one entered or left this tick.
#[derive(Resource)]
struct Names {
    out: watch::Sender<Arc<[String]>>,
    changed: bool,
}

/// The MOTD in effect, which each player gets once its own actor is in (M4 hosting spec §8, HA1).
#[derive(Resource)]
struct Motd(watch::Receiver<String>);

/// The last `SetAdmin` for each account, until the account's next join, which takes it over the
/// flag its link read: a link reads the flag before it joins, so a change that reaches the world
/// in between is the newer one, whether it finds the account out of the world or on an older
/// connection. One entry for each account changed since it last joined; account ids are never
/// handed out again, so an entry can't reach another account.
#[derive(Resource, Default)]
struct AdminChanges(HashMap<i64, bool>);

/// When everyone's place is next saved; set by the first tick.
#[derive(Resource, Default)]
struct NextSave(Option<u64>);

/// Where characters that left the game a moment ago stood, and until when that is kept
/// ([`KEEP_LEFT`]). A join uses it over the place the store gave, which holds only while nothing
/// but the world writes places: any future tool that moves an offline character (a command to
/// free a stuck one, say) must clear its entry here, or the world's copy wins for up to a minute.
///
/// [`KEEP_LEFT`]: super::KEEP_LEFT
#[derive(Resource, Default)]
struct Left(HashMap<ActorId, (Location, u64)>);

/// The world: its ECS, and the phases a tick runs.
pub struct WorldCore {
    world: World,
    phases: Vec<(&'static str, Schedule)>,
    /// The phase running, while a tick runs: after a panic, the one it was in.
    running: Option<&'static str>,
}

/// What [`WorldCore::abandon`] did.
#[derive(Debug, Default)]
pub(crate) struct Abandoned {
    /// The connections it told why they were let go.
    pub told: Vec<mpsc::Sender<WorldOut>>,
    /// The accounts of those it was asked about that were in the world.
    pub accounts: Vec<i64>,
}

fn phase<M>(systems: impl IntoScheduleConfigs<ScheduleSystem, M>) -> Schedule {
    let mut schedule = Schedule::default();
    schedule.add_systems(systems);
    schedule
}

impl WorldCore {
    /// A world under `rules`, whose saves and invites go to `services`. It sends the sorted names
    /// of its characters on `names` when one enters or leaves, and greets each player with the
    /// MOTD `motd` holds when it enters.
    pub fn new(
        rules: Rules,
        services: mpsc::UnboundedSender<Service>,
        names: watch::Sender<Arc<[String]>>,
        motd: watch::Receiver<String>,
    ) -> Self {
        let mut world = World::new();
        world.insert_resource(NextSave::default());
        world.insert_resource(rules);
        world.insert_resource(Now::default());
        world.insert_resource(Inbox::default());
        world.insert_resource(Index::default());
        world.insert_resource(Pending::default());
        world.insert_resource(Services(services));
        world.insert_resource(Left::default());
        world.insert_resource(Names {
            out: names,
            changed: false,
        });
        world.insert_resource(Motd(motd));
        world.insert_resource(AdminChanges::default());
        let phases = vec![
            ("inbound", phase((refill, inbound).chain())),
            ("movement", phase(super::movement::system)),
            ("interest", phase(super::interest::system)),
            ("chat", phase(super::chat::system)),
            ("admin", phase(super::admin::system)),
            ("persist", phase(persist)),
            ("cleanup", phase(cleanup)),
        ];
        Self {
            world,
            phases,
            running: None,
        }
    }

    /// Runs one tick at `now_ms` with what arrived since the last one, in arrival order, and
    /// returns how long each phase took.
    pub fn step(&mut self, now_ms: u64, inputs: Vec<WorldIn>) -> Vec<(&'static str, Duration)> {
        self.world.resource_mut::<Now>().0 = now_ms;
        self.world.resource_mut::<Inbox>().0 = inputs;
        let phases = self
            .phases
            .iter_mut()
            .map(|(name, schedule)| {
                self.running = Some(*name);
                let began = Instant::now();
                schedule.run(&mut self.world);
                (*name, began.elapsed())
            })
            .collect();
        self.running = None;
        phases
    }

    /// The phase a tick was in when it panicked, if it was in one.
    pub(crate) fn running(&self) -> Option<&'static str> {
        self.running
    }

    /// The players in the world, loading or not.
    pub fn players(&self) -> usize {
        self.world.resource::<Index>().conns.len()
    }

    /// After a tick that panicked (Ruling R9): saves where each player stands and lets each
    /// connection go with the shutdown's reason. It reads the players' components alone, since
    /// the panic may have left the index and the tick's other state half-done, and the world is
    /// never ticked again. It also names the accounts of `senders`, the connections whose clients
    /// sent that tick's inputs, for the log.
    pub(crate) fn abandon(&mut self, senders: &[ConnId]) -> Abandoned {
        let mut players = self.world.query::<(
            Option<&Actor>,
            Option<&Here>,
            Option<&Conn>,
            Option<&Account>,
        )>();
        let services = self.world.resource::<Services>();
        let mut done = Abandoned::default();
        for (actor, here, conn, account) in players.iter(&self.world) {
            if let (Some(actor), Some(here)) = (actor, here) {
                save(services, actor.pid, here.0);
            }
            if let Some(conn) = conn {
                let _ = conn.out.try_send(WorldOut::Kicked(KickReason::Shutdown));
                done.told.push(conn.out.clone());
                if let Some(account) = account
                    && senders.contains(&conn.id)
                {
                    done.accounts.push(account.id);
                }
            }
        }
        done
    }
}

/// A name as the index keeps it. Names are unique without regard to ASCII case, as the store's
/// `COLLATE NOCASE` compares them.
pub(super) fn name_key(name: &str) -> String {
    name.to_ascii_lowercase()
}

/// The most a connection's [`Inputs`] holds: `max_inputs_per_tick` for each tick period in
/// [`MAX_STALL`], and at least one period's.
///
/// [`MAX_STALL`]: super::MAX_STALL
fn input_cap(rules: &Rules) -> usize {
    let period_ms = (1_000 / u64::from(rules.tick_hz.max(1))).max(1);
    let stall_ms = u64::try_from(super::MAX_STALL.as_millis()).unwrap_or(u64::MAX);
    let periods = usize::try_from((stall_ms / period_ms).max(1)).unwrap_or(usize::MAX);
    rules.max_inputs_per_tick.saturating_mul(periods)
}

/// Gives each connection the tick's `max_inputs_per_tick` back, up to its [`input_cap`], before
/// [`inbound`] spends it.
fn refill(rules: Res<Rules>, mut allowances: Query<&mut Inputs>) {
    let (per_tick, cap) = (rules.max_inputs_per_tick, input_cap(&rules));
    for mut left in &mut allowances {
        left.0 = left.0.saturating_add(per_tick).min(cap);
    }
}

/// Applies the tick's inputs in arrival order. Moves, pulls, chat lines and the console's commands
/// wait in [`Pending`] for their phases. Each message from a client spends one of its connection's
/// [`Inputs`], and a connection that sends one with none left is kicked.
fn inbound(world: &mut World) {
    let inputs = std::mem::take(&mut world.resource_mut::<Inbox>().0);
    for input in inputs {
        match input {
            WorldIn::Join { player, out } => join(world, player, out),
            WorldIn::Leave { conn } => {
                if let Some(e) = live(world, conn) {
                    doom(world, e, None);
                }
            }
            WorldIn::Shutdown => {
                let all: Vec<Entity> = world.resource::<Index>().conns.values().copied().collect();
                for e in all {
                    doom(world, e, Some(KickReason::Shutdown));
                }
            }
            WorldIn::Tell { conn, text } => {
                if let Some(e) = live(world, conn) {
                    send(world, e, WorldOut::Chat(ChatOut::System { text }));
                }
            }
            WorldIn::Console { order, reply } => {
                let order = (Caller::Console(reply), order);
                world.resource_mut::<Pending>().orders.push(order);
            }
            WorldIn::SetAdmin { account, admin } => {
                world
                    .resource_mut::<AdminChanges>()
                    .0
                    .insert(account, admin);
                let player = world.resource::<Index>().accounts.get(&account).copied();
                if let Some(mut a) = player.and_then(|e| world.get_mut::<Account>(e)) {
                    a.admin = admin;
                }
            }
            WorldIn::Ban { account } => {
                if let Some(e) = world.resource::<Index>().accounts.get(&account).copied() {
                    doom(world, e, Some(KickReason::Kicked));
                }
            }
            WorldIn::Loaded { conn } => {
                if let Some(e) = counted(world, conn) {
                    loaded(world, e);
                }
            }
            WorldIn::Pull { conn, pid } => {
                if let Some(e) = counted(world, conn) {
                    world.resource_mut::<Pending>().pulls.push((e, pid));
                }
            }
            WorldIn::Move { conn, step } => {
                if let Some(e) = counted(world, conn) {
                    world.resource_mut::<Pending>().moves.push((e, step));
                }
            }
            WorldIn::Chat { conn, line } => {
                if let Some(e) = counted(world, conn) {
                    world.resource_mut::<Pending>().lines.push((e, line));
                }
            }
            #[cfg(test)]
            WorldIn::Panic(message) => panic!("{message}"),
            #[cfg(test)]
            WorldIn::Stall { work, begun } => {
                let _ = begun.send(());
                std::thread::sleep(work);
            }
        }
    }
}

/// The connection's player, unless it leaves this tick.
pub(super) fn live(world: &World, conn: ConnId) -> Option<Entity> {
    let e = *world.resource::<Index>().conns.get(&conn)?;
    world.get::<Doomed>(e).is_none().then_some(e)
}

/// [`live`], spending one of the connection's [`Inputs`]: with none left, it floods, and is
/// kicked.
fn counted(world: &mut World, conn: ConnId) -> Option<Entity> {
    let e = live(world, conn)?;
    if world
        .get_mut::<Inputs>(e)
        .is_some_and(|mut left| left.take())
    {
        return Some(e);
    }
    doom(world, e, Some(KickReason::Flood));
    None
}

/// A character enters. An account already in the game is replaced. A character that was in the
/// game a moment ago, on the connection this replaces or one that left within [`KEEP_LEFT`],
/// enters where it stood: the store's copy, which the login read, may be older, since saves are
/// written off the tick (#222). Likewise, an `op` or a `deop` since the account last joined
/// stands over the flag its link read ([`AdminChanges`]).
///
/// [`KEEP_LEFT`]: super::KEEP_LEFT
fn join(world: &mut World, player: Player, out: mpsc::Sender<WorldOut>) {
    if !(1..=MAX_PLAYER_ID).contains(&player.pid) {
        // The connection side refuses these first; a bad id never gets an actor.
        let _ = out.try_send(WorldOut::Kicked(KickReason::Kicked));
        return;
    }
    let admin = world
        .resource_mut::<AdminChanges>()
        .0
        .remove(&player.account)
        .unwrap_or(player.admin);
    let older = world
        .resource::<Index>()
        .accounts
        .get(&player.account)
        .copied();
    if let Some(old) = older {
        // Its place is kept, as for any character that leaves.
        remove(world, old, Some(KickReason::Replaced));
    }
    let now = world.resource::<Now>().0;
    let at = match world.resource_mut::<Left>().0.remove(&player.pid) {
        Some((stood, until)) if now < until => stood,
        _ => player.at,
    };
    let name = name_key(&player.look.name);
    let allowance = Inputs(input_cap(world.resource::<Rules>()));
    let e = world
        .spawn((
            Conn {
                id: player.conn,
                out,
            },
            Account {
                id: player.account,
                admin,
            },
            Actor {
                pid: player.pid,
                look: player.look,
            },
            Here(at),
            Stage::Loading,
            Sight::default(),
            Gait::new(now),
            Bucket::new(now),
            allowance,
        ))
        .id();
    let mut index = world.resource_mut::<Index>();
    index.conns.insert(player.conn, e);
    index.pids.insert(player.pid, e);
    index.accounts.insert(player.account, e);
    index.names.insert(name, e);
    world.resource_mut::<Names>().changed = true;
    send(
        world,
        e,
        WorldOut::LoadScene {
            pid: player.pid,
            at,
        },
    );
}

/// The client has loaded its scene: it gets its own actor, then, the first time, the MOTD if there
/// is one (M4 hosting spec §8, HA1). A repeated report changes nothing.
fn loaded(world: &mut World, e: Entity) {
    if world.get::<Stage>(e) != Some(&Stage::Loading) {
        return;
    }
    let (Some(actor), Some(here)) = (world.get::<Actor>(e), world.get::<Here>(e)) else {
        return;
    };
    let msg = WorldOut::SelfEnter {
        pid: actor.pid,
        look: actor.look.clone(),
        pos: here.0.pos,
        speed: SPEED_RATE,
    };
    if let Some(mut stage) = world.get_mut::<Stage>(e) {
        *stage = Stage::InWorld;
    }
    send(world, e, msg);
    let Ok(mut entity) = world.get_entity_mut(e) else {
        return;
    };
    if entity.contains::<Greeted>() {
        return;
    }
    entity.insert(Greeted);
    let text = world.resource::<Motd>().0.borrow().clone();
    if !text.is_empty() {
        send(world, e, WorldOut::Chat(ChatOut::System { text }));
    }
}

/// Sends `msg` to `e`'s connection. A full channel means its client stopped reading, so it is
/// kicked; a closed one means the connection has gone, so it leaves.
pub(super) fn send(world: &mut World, e: Entity, msg: WorldOut) {
    let Some(conn) = world.get::<Conn>(e) else {
        return;
    };
    match conn.out.try_send(msg) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => doom(world, e, Some(KickReason::SlowReader)),
        Err(TrySendError::Closed(_)) => doom(world, e, None),
    }
}

/// [`send`] for systems, which doom through `commands`.
pub(super) fn deliver(conn: &Conn, e: Entity, msg: WorldOut, commands: &mut Commands) {
    match conn.out.try_send(msg) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {
            commands
                .entity(e)
                .insert_if_new(Doomed(Some(KickReason::SlowReader)));
        }
        Err(TrySendError::Closed(_)) => {
            commands.entity(e).insert_if_new(Doomed(None));
        }
    }
}

/// Marks `e` to leave at the end of the tick. The first reason stands.
pub(super) fn doom(world: &mut World, e: Entity, reason: Option<KickReason>) {
    if let Ok(mut entity) = world.get_entity_mut(e)
        && !entity.contains::<Doomed>()
    {
        entity.insert(Doomed(reason));
    }
}

/// Tells every player whose client knows of `pid` that it has gone (9), and forgets it there.
/// Players leaving this tick aren't told: their connections are closing, and at shutdown, when
/// everyone leaves, telling each of the others would be N² Leaves that could fill a channel before
/// its `Kicked` (#222).
pub(super) fn hide(world: &mut World, pid: ActorId) {
    let mut viewers = world.query_filtered::<(Entity, &mut Sight), Without<Doomed>>();
    let told: Vec<Entity> = viewers
        .iter_mut(world)
        .filter_map(|(v, mut sight)| sight.0.remove(&pid).map(|_| v))
        .collect();
    for v in told {
        send(world, v, WorldOut::Leave { pids: vec![pid] });
    }
}

/// Takes `e` out of the game now: its viewers are told, its place is saved and kept for a while
/// (`Left`), its connection hears why when there is a reason, and the index forgets it.
fn remove(world: &mut World, e: Entity, reason: Option<KickReason>) {
    let (Some(actor), Some(here), Some(conn), Some(account)) = (
        world.get::<Actor>(e),
        world.get::<Here>(e),
        world.get::<Conn>(e),
        world.get::<Account>(e),
    ) else {
        world.despawn(e);
        world.resource_mut::<Names>().changed = true;
        return;
    };
    let (pid, at, conn_id, account_id) = (actor.pid, here.0, conn.id, account.id);
    let name = name_key(&actor.look.name);
    if let Some(reason) = reason {
        let _ = conn.out.try_send(WorldOut::Kicked(reason));
    }
    save(world.resource::<Services>(), pid, at);
    let now = world.resource::<Now>().0;
    let keep = u64::try_from(super::KEEP_LEFT.as_millis()).unwrap_or(u64::MAX);
    let mut left = world.resource_mut::<Left>();
    // Those kept long enough are forgotten as others leave, so the map holds only the last minute.
    left.0.retain(|_, (_, until)| now < *until);
    left.0.insert(pid, (at, now.saturating_add(keep)));
    world.despawn(e);
    let mut index = world.resource_mut::<Index>();
    unindex(&mut index.conns, &conn_id, e);
    unindex(&mut index.pids, &pid, e);
    unindex(&mut index.accounts, &account_id, e);
    unindex(&mut index.names, &name, e);
    world.resource_mut::<Names>().changed = true;
    hide(world, pid);
}

/// Removes `key` from `map` only if it still names `e`.
fn unindex<K: std::hash::Hash + Eq>(map: &mut HashMap<K, Entity>, key: &K, e: Entity) {
    if map.get(key) == Some(&e) {
        map.remove(key);
    }
}

/// Every `save_every_ms`, saves where everyone stands.
fn persist(
    now: Res<Now>,
    rules: Res<Rules>,
    mut next: ResMut<NextSave>,
    players: Query<(&Actor, &Here)>,
    services: Res<Services>,
) {
    let due = now.0.saturating_add(rules.save_every_ms);
    match next.0 {
        Some(at) if now.0 >= at => next.0 = Some(due),
        Some(_) => return,
        None => {
            next.0 = Some(due);
            return;
        }
    }
    for (actor, here) in &players {
        save(&services, actor.pid, here.0);
    }
}

/// Queues a save of where `pid` stands, off the tick. A services task that has stopped loses it,
/// so that is logged as an error with the place, for the host to see what was lost (#222).
fn save(services: &Services, pid: ActorId, at: Location) {
    if services.0.send(Service::SavePlace { pid, at }).is_err() {
        let p = at.pos;
        tracing::error!(
            pid,
            scene = at.scene,
            x = p.x,
            y = p.y,
            z = p.z,
            yaw = p.yaw,
            "a place wasn't saved: the world's services task has stopped"
        );
    }
}

/// Takes the doomed players out. Telling their viewers can doom more (a full channel), so it goes
/// on until none is left. Then, if anyone entered or left this tick, it sends the names.
fn cleanup(world: &mut World) {
    loop {
        let mut doomed = world.query::<(Entity, &Doomed)>();
        let out: Vec<(Entity, Option<KickReason>)> =
            doomed.iter(world).map(|(e, d)| (e, d.0)).collect();
        if out.is_empty() {
            break;
        }
        for (e, reason) in out {
            remove(world, e, reason);
        }
    }
    send_names(world);
}

/// Sends the names of the characters in the world, loading or not, if one entered or left this
/// tick (M4 hosting spec §3.8): the account API lists them for signed-in players. They're sorted as
/// the store sorts names, without regard to ASCII case, and a list like the last wakes no one.
fn send_names(world: &mut World) {
    if !std::mem::take(&mut world.resource_mut::<Names>().changed) {
        return;
    }
    let mut actors = world.query::<&Actor>();
    let mut names: Vec<String> = actors.iter(world).map(|a| a.look.name.clone()).collect();
    names.sort_by_cached_key(|name| (name_key(name), name.clone()));
    let names: Arc<[String]> = names.into();
    world.resource::<Names>().out.send_if_modified(|last| {
        if *last == names {
            return false;
        }
        *last = names;
        true
    });
}

#[cfg(test)]
pub(crate) mod test_util {
    use std::collections::HashMap;
    use std::sync::Arc;

    use tokio::sync::{mpsc, watch};

    use super::*;

    pub const SCENE: i64 = 5;

    pub fn rules() -> Rules {
        Rules {
            tick_hz: 20,
            move_tolerance: 1.5,
            save_every_ms: 300_000,
            max_inputs_per_tick: 8,
        }
    }

    /// One message more than a connection's allowance holds under [`rules`] (800): a flood, all
    /// in one tick.
    pub fn flood() -> usize {
        input_cap(&rules()) + 1
    }

    pub fn at(x: f64, y: f64) -> Location {
        Location {
            scene: SCENE,
            pos: Pos {
                x,
                y,
                z: 100.0,
                yaw: 0.0,
            },
        }
    }

    pub fn look(name: &str) -> Look {
        Look {
            name: name.into(),
            vocation: 0,
            skeleton: 2,
            level: 37,
            appearance: Arc::from(&[1u8, 2, 3][..]),
        }
    }

    /// What a thread logs, as the server's log shows it, caught by the process's own subscriber.
    pub use crate::console::log::caught::{logged, logged_at};

    /// A player for connection `conn`: account `conn`, pid `conn`, named `name`.
    pub fn player(conn: ConnId, name: &str, at: Location) -> Player {
        Player {
            conn,
            account: conn as i64,
            admin: false,
            pid: conn as i64,
            look: look(name),
            at,
        }
    }

    /// A names channel nobody reads, for a world whose names a test doesn't look at.
    pub fn unread_names() -> watch::Sender<Arc<[String]>> {
        watch::channel(Arc::from(Vec::new())).0
    }

    /// No MOTD, and none to come.
    pub fn no_motd() -> watch::Receiver<String> {
        watch::channel(String::new()).1
    }

    /// A world with fake connections: each player's outbound channel is kept here.
    pub struct Harness {
        pub core: WorldCore,
        pub services: mpsc::UnboundedReceiver<Service>,
        /// The MOTD the world greets players with: none until a test sets one.
        pub motd: watch::Sender<String>,
        pub now: u64,
        outs: HashMap<ConnId, mpsc::Receiver<WorldOut>>,
        queued: Vec<WorldIn>,
    }

    impl Harness {
        pub fn new() -> Self {
            Self::with(rules())
        }

        pub fn with(rules: Rules) -> Self {
            Self::built(rules, unread_names())
        }

        /// A world that sends its names on `names`.
        pub fn with_names(names: watch::Sender<Arc<[String]>>) -> Self {
            Self::built(rules(), names)
        }

        fn built(rules: Rules, names: watch::Sender<Arc<[String]>>) -> Self {
            let (tx, rx) = mpsc::unbounded_channel();
            let (motd, follows) = watch::channel(String::new());
            Self {
                core: WorldCore::new(rules, tx, names, follows),
                services: rx,
                motd,
                now: 1_000,
                outs: HashMap::new(),
                queued: Vec::new(),
            }
        }

        /// Queues a join with an outbound channel of `capacity`.
        pub fn join_with(&mut self, player: Player, capacity: usize) {
            let (tx, rx) = mpsc::channel(capacity);
            self.outs.insert(player.conn, rx);
            self.queued.push(WorldIn::Join { player, out: tx });
        }

        pub fn join(&mut self, player: Player) {
            self.join_with(player, 256);
        }

        pub fn push(&mut self, input: WorldIn) {
            self.queued.push(input);
        }

        /// Runs a tick `ms` after the last one began, with what was queued.
        pub fn tick(&mut self, ms: u64) {
            self.now += ms;
            let inputs = std::mem::take(&mut self.queued);
            self.core.step(self.now, inputs);
        }

        /// What `conn` was sent since the last call.
        pub fn take(&mut self, conn: ConnId) -> Vec<WorldOut> {
            let rx = self.outs.get_mut(&conn).expect("a joined connection");
            let mut out = Vec::new();
            while let Ok(m) = rx.try_recv() {
                out.push(m);
            }
            out
        }

        /// Whether `conn`'s channel has closed: the world let go of it.
        pub fn closed(&mut self, conn: ConnId) -> bool {
            let rx = self.outs.get_mut(&conn).expect("a joined connection");
            matches!(rx.try_recv(), Err(mpsc::error::TryRecvError::Disconnected))
        }

        /// The services work queued since the last call.
        pub fn saves(&mut self) -> Vec<Service> {
            let mut out = Vec::new();
            while let Ok(s) = self.services.try_recv() {
                out.push(s);
            }
            out
        }

        /// Joins `player`, loads it and empties its channel: it stands in the world.
        pub fn enter(&mut self, player: Player) {
            let conn = player.conn;
            self.join(player);
            self.push(WorldIn::Loaded { conn });
            self.tick(50);
            self.take(conn);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::sync::watch;

    use super::test_util::*;
    use super::*;

    #[test]
    fn a_join_loads_the_scene_and_the_load_brings_the_actor() {
        let mut h = Harness::new();
        h.join(player(1, "Lin", at(10.0, 20.0)));
        h.tick(50);
        assert_eq!(
            h.take(1),
            vec![WorldOut::LoadScene {
                pid: 1,
                at: at(10.0, 20.0)
            }]
        );
        h.push(WorldIn::Loaded { conn: 1 });
        h.push(WorldIn::Loaded { conn: 1 });
        h.tick(50);
        assert_eq!(
            h.take(1),
            vec![WorldOut::SelfEnter {
                pid: 1,
                look: look("Lin"),
                pos: at(10.0, 20.0).pos,
                speed: SPEED_RATE
            }],
            "once, however often the client reports"
        );
        assert_eq!(h.core.players(), 1);
    }

    #[test]
    fn leaving_saves_the_place_and_forgets_the_player() {
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(10.0, 20.0)));
        h.push(WorldIn::Leave { conn: 1 });
        h.push(WorldIn::Chat {
            conn: 1,
            line: ChatIn {
                channel: ChatChannel::World,
                text: "after leaving".into(),
                to: None,
            },
        });
        h.tick(50);
        assert_eq!(
            h.saves(),
            vec![Service::SavePlace {
                pid: 1,
                at: at(10.0, 20.0)
            }]
        );
        assert_eq!(h.core.players(), 0);
        assert!(h.closed(1), "no message for a connection that left");
    }

    #[test]
    fn a_newer_login_replaces_the_account_and_keeps_where_it_stood() {
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(10.0, 20.0)));
        // The same account and character on a newer connection, with an older place from the store.
        let newer = Player {
            conn: 2,
            ..player(1, "Lin", at(-500.0, -500.0))
        };
        h.join(newer);
        h.tick(50);
        assert_eq!(h.take(1), vec![WorldOut::Kicked(KickReason::Replaced)]);
        assert!(h.closed(1));
        assert_eq!(
            h.take(2),
            vec![WorldOut::LoadScene {
                pid: 1,
                at: at(10.0, 20.0)
            }]
        );
        assert_eq!(h.core.players(), 1);
    }

    #[test]
    fn a_character_that_left_a_moment_ago_comes_back_where_it_left() {
        // #222: the save on leaving is written off the tick, so a newer login that read the store
        // first brings an older place. The world keeps where it left for a minute.
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(10.0, 20.0)));
        h.push(WorldIn::Leave { conn: 1 });
        h.tick(50);
        let older = |conn| Player {
            conn,
            ..player(1, "Lin", at(-500.0, -500.0))
        };
        h.join(older(2));
        h.tick(50);
        assert_eq!(
            h.take(2),
            vec![WorldOut::LoadScene {
                pid: 1,
                at: at(10.0, 20.0)
            }],
            "where it left, not the older place"
        );
        // Once a minute has passed, the store has had its save: the login's place stands.
        h.push(WorldIn::Leave { conn: 2 });
        h.tick(50);
        h.tick(60_001);
        h.join(older(3));
        h.tick(50);
        assert_eq!(
            h.take(3),
            vec![WorldOut::LoadScene {
                pid: 1,
                at: at(-500.0, -500.0)
            }]
        );
    }

    #[test]
    fn a_newer_login_with_another_character_enters_where_that_one_was_saved() {
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(10.0, 20.0)));
        let other = Player {
            conn: 2,
            pid: 7,
            look: look("Mai"),
            ..player(1, "Lin", at(-500.0, -500.0))
        };
        h.join(other);
        h.tick(50);
        assert_eq!(h.take(1), vec![WorldOut::Kicked(KickReason::Replaced)]);
        assert_eq!(
            h.take(2),
            vec![WorldOut::LoadScene {
                pid: 7,
                at: at(-500.0, -500.0)
            }]
        );
        assert_eq!(
            h.saves(),
            vec![Service::SavePlace {
                pid: 1,
                at: at(10.0, 20.0)
            }]
        );
    }

    #[test]
    fn an_id_a_player_cannot_have_gets_no_actor() {
        let mut h = Harness::new();
        for (conn, pid) in [(1, 0), (2, -5), (3, MAX_PLAYER_ID + 1)] {
            h.join(Player {
                pid,
                ..player(conn, "Lin", at(0.0, 0.0))
            });
        }
        h.tick(50);
        for conn in 1..=3 {
            assert_eq!(h.take(conn), vec![WorldOut::Kicked(KickReason::Kicked)]);
        }
        assert_eq!(h.core.players(), 0);
    }

    /// `n` pulls from `conn` of a player who isn't there: inputs that send nothing back.
    fn pulls(h: &mut Harness, conn: ConnId, n: usize) {
        for _ in 0..n {
            h.push(WorldIn::Pull { conn, pid: 99 });
        }
    }

    #[test]
    fn a_flood_is_kicked() {
        // #281: each tick gives a connection back the limit, 8 here, and its allowance holds
        // MAX_STALL's worth, 100 ticks' (800). Sixteen a tick, twice the limit, spend it 8 a tick
        // faster than it comes back, and the hundredth such tick finds 8 for its 16.
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        for t in 1..100 {
            pulls(&mut h, 1, 16);
            h.tick(50);
            assert_eq!(h.take(1), Vec::new(), "tick {t}");
        }
        pulls(&mut h, 1, 16);
        h.tick(50);
        assert_eq!(h.take(1), vec![WorldOut::Kicked(KickReason::Flood)]);
        assert_eq!(h.core.players(), 0);
    }

    #[test]
    fn a_burst_after_the_players_network_stalls_is_no_flood() {
        // #281: the player's network stalls, and what its client sent meanwhile arrives at once.
        // The world ticked on time throughout and saw no stall, but the burst passes within the
        // allowance: up to MAX_STALL's worth at the limit, 800 here.
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        for _ in 0..60 {
            h.tick(50);
        }
        pulls(&mut h, 1, 800);
        h.tick(50);
        assert_eq!(h.take(1), Vec::new(), "not kicked");
        assert_eq!(h.core.players(), 1);
    }

    #[test]
    fn a_spent_allowance_comes_back_a_tick_at_a_time() {
        // Once a burst has spent it, each tick gives back the limit and no more: 8 a tick pass,
        // and 9 are a flood.
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        pulls(&mut h, 1, 800);
        h.tick(50);
        assert_eq!(h.take(1), Vec::new(), "the whole allowance");
        for t in 1..=3 {
            pulls(&mut h, 1, 8);
            h.tick(50);
            assert_eq!(h.take(1), Vec::new(), "tick {t}: the limit");
        }
        pulls(&mut h, 1, 9);
        h.tick(50);
        assert_eq!(h.take(1), vec![WorldOut::Kicked(KickReason::Flood)]);
    }

    #[test]
    fn a_slow_tick_gives_back_no_more_than_one_on_time() {
        // Ruling R10: a tick slowed by its own work (many connections each sending near the limit,
        // say) mustn't give back more than one on time, or a flood would buy itself more by making
        // ticks slow. Each tick gives back the limit, however long since the last.
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        pulls(&mut h, 1, 800);
        h.tick(50);
        assert_eq!(h.take(1), Vec::new(), "the whole allowance");
        pulls(&mut h, 1, 9);
        h.tick(500);
        assert_eq!(h.take(1), vec![WorldOut::Kicked(KickReason::Flood)]);
    }

    #[test]
    fn a_stall_of_the_world_is_no_flood_however_its_backlog_arrives() {
        // #222: after a stall of the world's thread or its process, what each connection sent in
        // the meantime arrives over that tick and the next, since the links read it from their
        // sockets only after the resume.
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        pulls(&mut h, 1, 40);
        h.tick(500);
        pulls(&mut h, 1, 40);
        h.tick(50);
        assert_eq!(h.take(1), Vec::new(), "not kicked");
        assert_eq!(h.core.players(), 1);
    }

    #[test]
    fn a_long_stall_counts_as_a_few_seconds() {
        // However long a stall, the allowance holds MAX_STALL's five seconds' worth: 100 ticks
        // of 8.
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        h.enter(player(2, "Mai", at(50_000.0, 0.0)));
        for (conn, n) in [(1, 800), (2, 801)] {
            pulls(&mut h, conn, n);
        }
        h.tick(600_000);
        assert_eq!(h.take(1), Vec::new(), "within five seconds' worth");
        assert_eq!(h.take(2), vec![WorldOut::Kicked(KickReason::Flood)]);
    }

    #[test]
    fn the_allowance_holds_the_limit_for_each_tick_period_in_max_stall() {
        let at_hz = |tick_hz| Rules {
            tick_hz,
            max_inputs_per_tick: 10,
            ..rules()
        };
        // 20 Hz: 100 periods of 50 ms; 1 Hz: 5 of a second; 60 Hz: 312 of 16 ms.
        assert_eq!(input_cap(&at_hz(20)), 1_000);
        assert_eq!(input_cap(&at_hz(1)), 50);
        assert_eq!(input_cap(&at_hz(60)), 3_120);
        // The server's own: 128 a tick at 20 Hz, five seconds' worth.
        let server = Rules {
            max_inputs_per_tick: crate::world::MAX_INPUTS_PER_TICK,
            ..rules()
        };
        assert_eq!(input_cap(&server), 12_800);
    }

    #[test]
    fn a_client_that_stops_reading_is_kicked_without_stalling_the_tick() {
        let mut h = Harness::new();
        h.join_with(player(1, "Lin", at(0.0, 0.0)), 1);
        h.tick(50);
        // The channel holds LoadScene, unread; SelfEnter finds it full.
        h.push(WorldIn::Loaded { conn: 1 });
        h.tick(50);
        assert_eq!(h.core.players(), 0);
        assert_eq!(h.saves().len(), 1, "its place is saved");
        assert_eq!(
            h.take(1),
            vec![WorldOut::LoadScene {
                pid: 1,
                at: at(0.0, 0.0)
            }]
        );
    }

    #[test]
    fn a_connection_that_has_gone_leaves() {
        let mut h = Harness::new();
        let (tx, rx) = mpsc::channel(4);
        drop(rx);
        h.push(WorldIn::Join {
            player: player(1, "Lin", at(0.0, 0.0)),
            out: tx,
        });
        h.tick(50);
        assert_eq!(h.core.players(), 0);
    }

    #[test]
    fn everyone_is_saved_every_save_period() {
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(1.0, 2.0)));
        h.enter(player(2, "Mai", at(3.0, 4.0)));
        h.saves();
        h.tick(299_000);
        assert!(h.saves().is_empty());
        h.tick(2_000);
        let saves = h.saves();
        assert_eq!(saves.len(), 2, "{saves:?}");
        for (pid, place) in [(1, at(1.0, 2.0)), (2, at(3.0, 4.0))] {
            let save = Service::SavePlace { pid, at: place };
            assert!(saves.contains(&save), "{saves:?}");
        }
    }

    #[test]
    fn shutdown_saves_and_lets_go_of_everyone() {
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(1.0, 2.0)));
        h.join(player(2, "Mai", at(3.0, 4.0)));
        h.tick(50);
        h.take(2);
        h.saves();
        h.push(WorldIn::Shutdown);
        h.tick(50);
        assert_eq!(h.take(1), vec![WorldOut::Kicked(KickReason::Shutdown)]);
        assert_eq!(h.take(2), vec![WorldOut::Kicked(KickReason::Shutdown)]);
        assert_eq!(h.saves().len(), 2);
        assert_eq!(h.core.players(), 0);
    }

    #[test]
    fn at_shutdown_no_leaver_hears_of_another_and_each_hears_why() {
        // #222: telling leavers that the others left is N² Leaves, and one that fills a channel
        // pushes out its Kicked(Shutdown). Each channel here has room for one more message.
        const N: u64 = 4;
        let mut h = Harness::new();
        for c in 1..=N {
            // LoadScene, SelfEnter, and an Announce of each of the others, all unread.
            let room = N as usize + 2;
            h.join_with(player(c, &format!("P{c}"), at(0.0, 0.0)), room);
            h.push(WorldIn::Loaded { conn: c });
        }
        h.tick(50);
        h.push(WorldIn::Shutdown);
        h.tick(50);
        for c in 1..=N {
            let got = h.take(c);
            let leaves = got
                .iter()
                .filter(|m| matches!(m, WorldOut::Leave { .. }))
                .count();
            assert_eq!(leaves, 0, "{c}: {got:?}");
            assert_eq!(
                got.last(),
                Some(&WorldOut::Kicked(KickReason::Shutdown)),
                "{c}: {got:?}"
            );
        }
        assert_eq!(h.saves().len(), N as usize, "everyone is saved");
        assert_eq!(h.core.players(), 0);
    }

    #[test]
    fn a_save_the_services_cannot_take_is_logged_as_an_error() {
        // #222: a services task that has died loses every save from then on, and each one lost
        // says so, with the place, rather than going without a word.
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(10.0, 20.0)));
        h.enter(player(2, "Mai", at(30.0, 40.0)));
        h.services.close();
        let errors = |log: &str, pid: &str| {
            log.lines()
                .filter(|l| l.contains("ERROR") && l.contains(&format!("pid={pid} ")))
                .count()
        };
        // The periodic save, then a leave.
        let ((), log) = logged(|| h.tick(300_000));
        assert_eq!((errors(&log, "1"), errors(&log, "2")), (1, 1), "{log}");
        let ((), log) = logged(|| {
            h.push(WorldIn::Leave { conn: 2 });
            h.tick(50);
        });
        assert_eq!((errors(&log, "1"), errors(&log, "2")), (0, 1), "{log}");
        assert!(log.contains("x=30.0 y=40.0"), "the place lost: {log}");
    }

    #[test]
    fn a_told_line_reaches_its_player_only() {
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        h.enter(player(2, "Mai", at(0.0, 0.0)));
        h.take(1);
        h.take(2);
        h.push(WorldIn::Tell {
            conn: 1,
            text: "hello".into(),
        });
        h.tick(50);
        assert_eq!(
            h.take(1),
            vec![WorldOut::Chat(ChatOut::System {
                text: "hello".into()
            })]
        );
        assert!(h.take(2).is_empty());
    }

    #[test]
    fn the_names_follow_who_is_in_the_world() {
        let (names, mut seen) = watch::channel(Arc::<[String]>::from(Vec::new()));
        let mut h = Harness::with_names(names);
        h.enter(player(1, "Mai", at(0.0, 0.0)));
        h.enter(player(2, "Lin", at(0.0, 0.0)));
        assert_eq!(&*seen.borrow_and_update().clone(), ["Lin", "Mai"]);
        h.push(WorldIn::Leave { conn: 1 });
        h.tick(50);
        assert_eq!(&*seen.borrow_and_update().clone(), ["Lin"]);
    }

    #[test]
    fn the_names_are_sent_only_when_someone_enters_or_leaves() {
        // M4 hosting spec §3.8: not every tick. A player still loading is in the world, and the
        // names are sorted as the store sorts them, without regard to ASCII case.
        let (names, mut seen) = watch::channel(Arc::<[String]>::from(Vec::new()));
        let mut h = Harness::with_names(names);
        h.enter(player(1, "mai", at(0.0, 0.0)));
        h.enter(player(2, "Zed", at(0.0, 0.0)));
        h.join(player(3, "Lin", at(0.0, 0.0)));
        h.tick(50);
        assert_eq!(&*seen.borrow_and_update().clone(), ["Lin", "mai", "Zed"]);
        // Ticks with a move and a line of chat, but no one coming or going.
        h.push(WorldIn::Move {
            conn: 1,
            step: MoveStep {
                pos: at(10.0, 0.0).pos,
                detail: Arc::from(&[0u8; 17][..]),
            },
        });
        h.push(WorldIn::Chat {
            conn: 2,
            line: ChatIn {
                channel: ChatChannel::World,
                text: "hi".into(),
                to: None,
            },
        });
        h.tick(50);
        h.tick(50);
        assert!(!seen.has_changed().unwrap(), "nothing new to hear");
        for c in 1..=3 {
            h.take(c);
        }
        // A newer login of a character already in: the same names, so nothing to hear either.
        h.join(Player {
            conn: 4,
            ..player(1, "mai", at(0.0, 0.0))
        });
        h.tick(50);
        assert_eq!(h.take(1), vec![WorldOut::Kicked(KickReason::Replaced)]);
        assert!(!seen.has_changed().unwrap(), "the same names");
        // Everyone leaving at shutdown leaves none.
        h.push(WorldIn::Shutdown);
        h.tick(50);
        assert!(seen.borrow_and_update().is_empty());
    }

    #[test]
    fn a_player_hears_the_motd_once_its_own_actor_is_in() {
        // M4 hosting spec §8, HA1: a system line, after the player's own 7 and 8.
        let mut h = Harness::new();
        h.motd.send_replace("Welcome to Moonward".into());
        h.join(player(1, "Lin", at(0.0, 0.0)));
        h.tick(50);
        assert_eq!(
            h.take(1),
            vec![WorldOut::LoadScene {
                pid: 1,
                at: at(0.0, 0.0)
            }],
            "nothing before its actor"
        );
        h.push(WorldIn::Loaded { conn: 1 });
        h.push(WorldIn::Loaded { conn: 1 });
        h.tick(50);
        h.tick(50);
        let motd = |text: &str| WorldOut::Chat(ChatOut::System { text: text.into() });
        assert_eq!(
            h.take(1),
            vec![
                WorldOut::SelfEnter {
                    pid: 1,
                    look: look("Lin"),
                    pos: at(0.0, 0.0).pos,
                    speed: SPEED_RATE
                },
                motd("Welcome to Moonward")
            ],
            "once, however often the client reports"
        );
        // A new MOTD is for whoever enters next: those in the world don't hear it.
        h.motd.send_replace("Doors close at 23:00".into());
        h.join(player(2, "Mai", at(50_000.0, 0.0)));
        h.push(WorldIn::Loaded { conn: 2 });
        h.tick(50);
        let got = h.take(2);
        assert_eq!(got.last(), Some(&motd("Doors close at 23:00")), "{got:?}");
        assert!(h.take(1).is_empty());
    }

    #[test]
    fn with_no_motd_a_player_hears_nothing() {
        let mut h = Harness::new();
        h.motd.send_replace("Welcome".into());
        h.motd.send_replace(String::new());
        h.join(player(1, "Lin", at(0.0, 0.0)));
        h.push(WorldIn::Loaded { conn: 1 });
        h.tick(50);
        h.tick(50);
        assert_eq!(
            h.take(1),
            vec![
                WorldOut::LoadScene {
                    pid: 1,
                    at: at(0.0, 0.0)
                },
                WorldOut::SelfEnter {
                    pid: 1,
                    look: look("Lin"),
                    pos: at(0.0, 0.0).pos,
                    speed: SPEED_RATE
                }
            ]
        );
    }

    #[test]
    fn a_told_line_for_a_connection_that_has_left_reaches_no_one() {
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        h.enter(player(2, "Mai", at(0.0, 0.0)));
        h.take(1);
        h.take(2);
        // The services answer late: in the tick the connection leaves, and in a later one.
        h.push(WorldIn::Leave { conn: 1 });
        h.push(WorldIn::Tell {
            conn: 1,
            text: "invite link: A".into(),
        });
        h.tick(50);
        h.push(WorldIn::Tell {
            conn: 1,
            text: "invite link: B".into(),
        });
        h.tick(50);
        assert!(h.take(1).is_empty());
        assert!(h.closed(1));
        assert_eq!(
            h.take(2),
            vec![WorldOut::Leave { pids: vec![1] }],
            "Mai hears only that Lin left"
        );
    }
}
