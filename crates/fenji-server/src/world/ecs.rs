//! The world's ECS (M4 world spec §3.9; Fenji design spec §6.4): one bevy_ecs `World` with an
//! entity per player in the game. A tick runs its phases in order, each its own `Schedule`, so a
//! slow tick can name its slowest phase.
use std::collections::HashMap;
use std::time::{Duration, Instant};

use bevy_ecs::prelude::*;
use bevy_ecs::system::ScheduleSystem;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;

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
    /// The most messages a connection may send in one tick; one more and it is kicked.
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

/// Leaves at the end of the tick. The reason, if any, goes to its connection.
#[derive(Component, Clone, Copy)]
pub(super) struct Doomed(pub Option<KickReason>);

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

/// What the inbound phase hands to later phases, this tick.
#[derive(Resource, Default)]
pub(super) struct Pending {
    pub moves: Vec<(Entity, MoveStep)>,
    pub pulls: Vec<(Entity, ActorId)>,
    pub lines: Vec<(Entity, ChatIn)>,
    pub orders: Vec<(Entity, Command)>,
}

/// The services task's channel.
#[derive(Resource)]
pub(super) struct Services(pub mpsc::UnboundedSender<Service>);

/// When everyone's place is next saved; set by the first tick.
#[derive(Resource, Default)]
struct NextSave(Option<u64>);

/// The world: its ECS, and the phases a tick runs.
pub struct WorldCore {
    world: World,
    phases: Vec<(&'static str, Schedule)>,
}

fn phase<M>(systems: impl IntoScheduleConfigs<ScheduleSystem, M>) -> Schedule {
    let mut schedule = Schedule::default();
    schedule.add_systems(systems);
    schedule
}

impl WorldCore {
    pub fn new(rules: Rules, services: mpsc::UnboundedSender<Service>) -> Self {
        let mut world = World::new();
        world.insert_resource(NextSave::default());
        world.insert_resource(rules);
        world.insert_resource(Now::default());
        world.insert_resource(Inbox::default());
        world.insert_resource(Index::default());
        world.insert_resource(Pending::default());
        world.insert_resource(Services(services));
        let phases = vec![
            ("inbound", phase(inbound)),
            ("movement", phase(super::movement::system)),
            ("interest", phase(super::interest::system)),
            ("chat", phase(super::chat::system)),
            ("admin", phase(super::admin::system)),
            ("persist", phase(persist)),
            ("cleanup", phase(cleanup)),
        ];
        Self { world, phases }
    }

    /// Runs one tick at `now_ms`, with what arrived since the last one in arrival order, and
    /// returns how long each phase took.
    pub fn step(&mut self, now_ms: u64, inputs: Vec<WorldIn>) -> Vec<(&'static str, Duration)> {
        self.world.resource_mut::<Now>().0 = now_ms;
        self.world.resource_mut::<Inbox>().0 = inputs;
        self.phases
            .iter_mut()
            .map(|(name, schedule)| {
                let began = Instant::now();
                schedule.run(&mut self.world);
                (*name, began.elapsed())
            })
            .collect()
    }

    /// The players in the world, loading or not.
    pub fn players(&self) -> usize {
        self.world.resource::<Index>().conns.len()
    }
}

/// A name as the index keeps it. Names are unique without regard to ASCII case, as the store's
/// `COLLATE NOCASE` compares them.
pub(super) fn name_key(name: &str) -> String {
    name.to_ascii_lowercase()
}

/// Applies the tick's inputs in arrival order. Moves, pulls and chat lines wait in [`Pending`] for
/// their phases. A connection that sends more than `max_inputs_per_tick` is kicked.
fn inbound(world: &mut World) {
    let inputs = std::mem::take(&mut world.resource_mut::<Inbox>().0);
    let max = world.resource::<Rules>().max_inputs_per_tick;
    let mut counts: HashMap<ConnId, usize> = HashMap::new();
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
            WorldIn::Loaded { conn } => {
                if let Some(e) = counted(world, &mut counts, max, conn) {
                    loaded(world, e);
                }
            }
            WorldIn::Pull { conn, pid } => {
                if let Some(e) = counted(world, &mut counts, max, conn) {
                    world.resource_mut::<Pending>().pulls.push((e, pid));
                }
            }
            WorldIn::Move { conn, step } => {
                if let Some(e) = counted(world, &mut counts, max, conn) {
                    world.resource_mut::<Pending>().moves.push((e, step));
                }
            }
            WorldIn::Chat { conn, line } => {
                if let Some(e) = counted(world, &mut counts, max, conn) {
                    world.resource_mut::<Pending>().lines.push((e, line));
                }
            }
        }
    }
}

/// The connection's player, unless it leaves this tick.
pub(super) fn live(world: &World, conn: ConnId) -> Option<Entity> {
    let e = *world.resource::<Index>().conns.get(&conn)?;
    world.get::<Doomed>(e).is_none().then_some(e)
}

/// [`live`], counting the message against the connection's limit for the tick.
fn counted(
    world: &mut World,
    counts: &mut HashMap<ConnId, usize>,
    max: usize,
    conn: ConnId,
) -> Option<Entity> {
    let e = live(world, conn)?;
    let n = counts.entry(conn).or_insert(0);
    *n += 1;
    if *n > max {
        doom(world, e, Some(KickReason::Flood));
        return None;
    }
    Some(e)
}

/// A character enters. An account already in the game is replaced, and the same character keeps
/// where it stood, which may be newer than the store's copy.
fn join(world: &mut World, player: Player, out: mpsc::Sender<WorldOut>) {
    if !(1..=MAX_PLAYER_ID).contains(&player.pid) {
        // The connection side refuses these first; a bad id never gets an actor.
        let _ = out.try_send(WorldOut::Kicked(KickReason::Kicked));
        return;
    }
    let mut at = player.at;
    let older = world
        .resource::<Index>()
        .accounts
        .get(&player.account)
        .copied();
    if let Some(old) = older {
        if world.get::<Actor>(old).is_some_and(|a| a.pid == player.pid)
            && let Some(here) = world.get::<Here>(old)
        {
            at = here.0;
        }
        remove(world, old, Some(KickReason::Replaced));
    }
    let now = world.resource::<Now>().0;
    let name = name_key(&player.look.name);
    let e = world
        .spawn((
            Conn {
                id: player.conn,
                out,
            },
            Account {
                id: player.account,
                admin: player.admin,
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
        ))
        .id();
    let mut index = world.resource_mut::<Index>();
    index.conns.insert(player.conn, e);
    index.pids.insert(player.pid, e);
    index.accounts.insert(player.account, e);
    index.names.insert(name, e);
    send(
        world,
        e,
        WorldOut::LoadScene {
            pid: player.pid,
            at,
        },
    );
}

/// The client has loaded its scene: it gets its own actor. A repeated report changes nothing.
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
pub(super) fn hide(world: &mut World, pid: ActorId) {
    let mut viewers = world.query::<(Entity, &mut Sight)>();
    let told: Vec<Entity> = viewers
        .iter_mut(world)
        .filter_map(|(v, mut sight)| sight.0.remove(&pid).map(|_| v))
        .collect();
    for v in told {
        send(world, v, WorldOut::Leave { pids: vec![pid] });
    }
}

/// Takes `e` out of the game now: its viewers are told, its place is saved, its connection hears
/// why when there is a reason, and the index forgets it.
fn remove(world: &mut World, e: Entity, reason: Option<KickReason>) {
    let (Some(actor), Some(here), Some(conn), Some(account)) = (
        world.get::<Actor>(e),
        world.get::<Here>(e),
        world.get::<Conn>(e),
        world.get::<Account>(e),
    ) else {
        world.despawn(e);
        return;
    };
    let (pid, at, conn_id, account_id) = (actor.pid, here.0, conn.id, account.id);
    let name = name_key(&actor.look.name);
    if let Some(reason) = reason {
        let _ = conn.out.try_send(WorldOut::Kicked(reason));
    }
    let _ = world
        .resource::<Services>()
        .0
        .send(Service::SavePlace { pid, at });
    world.despawn(e);
    let mut index = world.resource_mut::<Index>();
    unindex(&mut index.conns, &conn_id, e);
    unindex(&mut index.pids, &pid, e);
    unindex(&mut index.accounts, &account_id, e);
    unindex(&mut index.names, &name, e);
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
        let _ = services.0.send(Service::SavePlace {
            pid: actor.pid,
            at: here.0,
        });
    }
}

/// Takes the doomed players out. Telling their viewers can doom more (a full channel), so it goes
/// on until none is left.
fn cleanup(world: &mut World) {
    loop {
        let mut doomed = world.query::<(Entity, &Doomed)>();
        let out: Vec<(Entity, Option<KickReason>)> =
            doomed.iter(world).map(|(e, d)| (e, d.0)).collect();
        if out.is_empty() {
            return;
        }
        for (e, reason) in out {
            remove(world, e, reason);
        }
    }
}

#[cfg(test)]
pub(crate) mod test_util {
    use std::collections::HashMap;
    use std::sync::Arc;

    use tokio::sync::mpsc;

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

    /// A world with fake connections: each player's outbound channel is kept here.
    pub struct Harness {
        pub core: WorldCore,
        pub services: mpsc::UnboundedReceiver<Service>,
        pub now: u64,
        outs: HashMap<ConnId, mpsc::Receiver<WorldOut>>,
        queued: Vec<WorldIn>,
    }

    impl Harness {
        pub fn new() -> Self {
            Self::with(rules())
        }

        pub fn with(rules: Rules) -> Self {
            let (tx, rx) = mpsc::unbounded_channel();
            Self {
                core: WorldCore::new(rules, tx),
                services: rx,
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

        /// Runs a tick `ms` after the last one, with what was queued.
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

    #[test]
    fn a_flood_is_kicked() {
        let mut h = Harness::new();
        h.enter(player(1, "Lin", at(0.0, 0.0)));
        for _ in 0..9 {
            h.push(WorldIn::Pull { conn: 1, pid: 99 });
        }
        h.tick(50);
        assert_eq!(h.take(1), vec![WorldOut::Kicked(KickReason::Flood)]);
        assert_eq!(h.core.players(), 0);
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
}
