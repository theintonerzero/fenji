//! Work off the tick (Fenji design spec §6.5, §6.7): the world's saves, and the invites `!invite`
//! asks for, run on the tokio runtime, so a slow disk never holds a tick. Each save is one
//! statement.
use std::sync::Arc;

use fenji_store::{Place, Store};
use tokio::sync::mpsc;

use super::types::{Service, WorldIn};
use crate::clock::Clock;

/// Runs until the world drops its end of `jobs`, after the last job queued. `world` is weak, so
/// this task never keeps the world from stopping.
pub(super) async fn run(
    store: Store,
    invite_base: Option<String>,
    clock: Arc<dyn Clock>,
    mut jobs: mpsc::UnboundedReceiver<Service>,
    world: mpsc::WeakUnboundedSender<WorldIn>,
) {
    while let Some(job) = jobs.recv().await {
        match job {
            Service::SavePlace { pid, at } => {
                let place = Place {
                    scene: at.scene,
                    x: at.pos.x,
                    y: at.pos.y,
                    z: at.pos.z,
                    yaw: at.pos.yaw,
                };
                match store.save_place(pid, &place).await {
                    Ok(true) => {}
                    Ok(false) => tracing::debug!(pid, "a place not saved: the character is gone"),
                    // Lost as surely as a save the world couldn't queue, so logged as it is, with
                    // the place, for the host to see what was lost.
                    Err(e) => tracing::error!(
                        pid,
                        scene = place.scene,
                        x = place.x,
                        y = place.y,
                        z = place.z,
                        yaw = place.yaw,
                        error = %e,
                        "a place wasn't saved: the store refused it"
                    ),
                }
            }
            Service::CreateInvite { conn, label } => {
                let made = crate::invite::create(
                    &store,
                    invite_base.as_deref(),
                    label.as_deref(),
                    clock.now_ms(),
                    None,
                )
                .await;
                let text = match made {
                    Ok(made) => made.to_string(),
                    Err(e) => {
                        tracing::error!(error = %e, "making an invite for !invite");
                        "the invite couldn't be made; the server's log says why".into()
                    }
                };
                if let Some(world) = world.upgrade() {
                    let _ = world.send(WorldIn::Tell { conn, text });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::ecs::test_util::logged;
    use super::super::types::{Location, Pos};
    use super::*;
    use crate::clock::TestClock;

    #[test]
    fn a_save_the_store_refuses_is_logged_as_an_error_with_the_place() {
        // A save the store refuses loses the place as surely as one the world couldn't queue, so
        // the log says so as loudly, with what was lost.
        let ((), log) = logged(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                let dir = tempfile::tempdir().unwrap();
                let store = Store::open(&dir.path().join("fenji.db")).await.unwrap();
                store.close().await;
                let (jobs_tx, jobs) = mpsc::unbounded_channel();
                let (world, _inbox) = mpsc::unbounded_channel::<WorldIn>();
                let pos = Pos {
                    x: 1.5,
                    y: -2.0,
                    z: 3.0,
                    yaw: 90.0,
                };
                let at = Location { scene: 5, pos };
                jobs_tx.send(Service::SavePlace { pid: 7, at }).unwrap();
                drop(jobs_tx);
                let clock = Arc::new(TestClock::at(0));
                run(store, None, clock, jobs, world.downgrade()).await;
            });
        });
        let lost: Vec<&str> = log
            .lines()
            .filter(|l| l.contains("ERROR") && l.contains("pid=7 "))
            .collect();
        assert_eq!(lost.len(), 1, "{log}");
        assert!(
            lost[0].contains("scene=5 x=1.5 y=-2.0 z=3.0 yaw=90.0"),
            "{log}"
        );
    }
}
