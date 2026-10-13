//! The world's thread and its saves in a running server (M4 world spec §3.9), with the test in a
//! connection task's place on the world's channels.
mod common;

use std::sync::Arc;
use std::time::Duration;

use common::*;
use fenji_server::server::GateLimits;
use fenji_server::world::{KickReason, Location, Look, Player, Pos, WorldIn, WorldOut};
use fenji_store::{Character, NewCharacter, Place, Store};
use tokio::sync::mpsc;

const AT: Location = Location {
    scene: 5,
    pos: Pos {
        x: 1.0,
        y: 2.0,
        z: 3.0,
        yaw: 90.0,
    },
};

const SAVED: Place = Place {
    scene: 5,
    x: 1.0,
    y: 2.0,
    z: 3.0,
    yaw: 90.0,
};

/// A character for a new account `name`.
async fn character(s: &fenji_server::server::RunningServer, name: &str) -> Character {
    account(s, name).await;
    let store = s.accounts().store().clone();
    let a = store.account_by_username(name).await.unwrap().unwrap();
    let new = NewCharacter {
        name: format!("{name}c"),
        vocation: 0,
        skeleton: 2,
        appearance: vec![1],
    };
    store.create_character(a.id, &new, 9, NOW).await.unwrap()
}

fn player(conn: u64, c: &Character) -> Player {
    Player {
        conn,
        account: c.account_id,
        admin: false,
        pid: c.pid,
        look: Look {
            name: c.name.clone(),
            vocation: c.vocation,
            skeleton: c.skeleton,
            level: c.level,
            appearance: Arc::from(c.appearance.as_slice()),
        },
        at: AT,
    }
}

async fn next(rx: &mut mpsc::Receiver<WorldOut>) -> WorldOut {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("within 5 s")
        .expect("the channel is open")
}

/// The character's place once the services task has saved it (5 s at most).
async fn saved(store: &Store, c: &Character) -> Place {
    for _ in 0..100 {
        if let Some(p) = store
            .character(c.account_id, c.pid)
            .await
            .unwrap()
            .unwrap()
            .place
        {
            return p;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("no place saved within 5 s");
}

#[tokio::test]
async fn a_player_enters_and_its_place_is_saved_when_it_leaves() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), "", GateLimits::default()).await;
    let c = character(&s, "tester").await;
    let (tx, mut rx) = mpsc::channel(64);
    assert!(s.world().send(WorldIn::Join {
        player: player(1, &c),
        out: tx
    }));
    assert_eq!(
        next(&mut rx).await,
        WorldOut::LoadScene { pid: c.pid, at: AT }
    );
    s.world().send(WorldIn::Loaded { conn: 1 });
    assert!(matches!(next(&mut rx).await, WorldOut::SelfEnter { pid, .. } if pid == c.pid));
    s.world().send(WorldIn::Leave { conn: 1 });
    assert_eq!(saved(s.accounts().store(), &c).await, SAVED);
    s.shutdown().await;
}

#[tokio::test]
async fn shutdown_saves_everyone_still_in_the_world() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), "", GateLimits::default()).await;
    let a = character(&s, "tester").await;
    let b = character(&s, "other").await;
    let (tx, mut rx) = mpsc::channel(64);
    s.world().send(WorldIn::Join {
        player: player(1, &a),
        out: tx,
    });
    let (tx2, mut rx2) = mpsc::channel(64);
    s.world().send(WorldIn::Join {
        player: player(2, &b),
        out: tx2,
    });
    next(&mut rx).await;
    next(&mut rx2).await;
    s.shutdown().await;
    assert_eq!(next(&mut rx).await, WorldOut::Kicked(KickReason::Shutdown));
    let store = Store::open(&dir.path().join("fenji.db")).await.unwrap();
    for c in [&a, &b] {
        let place = store
            .character(c.account_id, c.pid)
            .await
            .unwrap()
            .unwrap()
            .place;
        assert_eq!(place, Some(SAVED), "saved before the store closed");
    }
}

#[tokio::test]
async fn the_world_ticks_and_reports_its_tick_times() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _clock) = server(dir.path(), "", GateLimits::default()).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let r = s.ticks();
    assert!(r.ticks >= 5, "{r:?}");
    assert!(r.p50 <= r.p99 && r.p99 <= r.max, "{r:?}");
    s.shutdown().await;
}
