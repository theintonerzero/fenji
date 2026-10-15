//! Work off the tick (Fenji design spec §6.5, §6.7): the world's saves, and the invites `!invite`
//! and the console's `invite` ask for, run on the tokio runtime, so a slow disk never holds a tick.
//! Each save is one statement.
use std::sync::Arc;

use fenji_store::{Place, Store};
use tokio::sync::mpsc;

use super::types::{ConsoleReply, Service, WorldIn};
use crate::clock::Clock;
use crate::invite::InviteBase;

/// Runs until the world drops its end of `jobs`, after the last job queued. `world` is weak, so
/// this task never keeps the world from stopping.
pub(super) async fn run(
    store: Store,
    invite_base: Option<InviteBase>,
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
                let base = invite_base.as_ref();
                let made = invite(&store, base, label.as_deref(), &*clock, "!invite").await;
                if let Some(world) = world.upgrade() {
                    let _ = world.send(WorldIn::Tell {
                        conn,
                        text: made.text,
                    });
                }
            }
            Service::CreateInviteFor { reply, label } => {
                let base = invite_base.as_ref();
                let made = invite(&store, base, label.as_deref(), &*clock, "console").await;
                let _ = reply.send(made);
            }
        }
    }
}

/// Makes an invite with `label` for `by` (`!invite` or the console), and says what came of it: its
/// link or its code, or that it couldn't be made, with why in the log alone.
async fn invite(
    store: &Store,
    base: Option<&InviteBase>,
    label: Option<&str>,
    clock: &dyn Clock,
    by: &'static str,
) -> ConsoleReply {
    match crate::invite::create(store, base, label, clock.now_ms(), None).await {
        Ok(made) => ConsoleReply {
            ok: true,
            text: made.to_string(),
        },
        Err(e) => {
            tracing::error!(error = %e, by, "making an invite");
            ConsoleReply {
                ok: false,
                text: "the invite couldn't be made; the server's log says why".into(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::oneshot;

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

    #[tokio::test]
    async fn the_consoles_invite_is_made_and_answered_on_its_channel() {
        // M4 hosting spec §3.3: the console's `invite` goes through here as `!invite` does, and
        // its answer goes back on the console's channel, not to a player.
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("fenji.db")).await.unwrap();
        let (jobs_tx, jobs) = mpsc::unbounded_channel();
        let (world, mut inbox) = mpsc::unbounded_channel::<WorldIn>();
        let (reply, made) = oneshot::channel();
        let label = Some("for Lin".to_string());
        jobs_tx
            .send(Service::CreateInviteFor { reply, label })
            .unwrap();
        // A label `invite` refuses: nothing is stored, and the answer is the plain one.
        let (reply, refused) = oneshot::channel();
        let label = Some("a\u{202E}b".to_string());
        jobs_tx
            .send(Service::CreateInviteFor { reply, label })
            .unwrap();
        drop(jobs_tx);
        let base = InviteBase {
            origin: "https://play.example.net".into(),
            pin: None,
        };
        let clock = Arc::new(TestClock::at(0));
        run(store.clone(), Some(base), clock, jobs, world.downgrade()).await;
        let made = made.await.unwrap();
        assert!(
            made.ok
                && made
                    .text
                    .starts_with("invite link: https://play.example.net/#invite="),
            "{made:?}"
        );
        assert_eq!(
            refused.await.unwrap(),
            ConsoleReply {
                ok: false,
                text: "the invite couldn't be made; the server's log says why".into()
            }
        );
        let invites = store.list_invites().await.unwrap();
        assert_eq!(invites.len(), 1, "{invites:?}");
        assert_eq!(invites[0].label.as_deref(), Some("for Lin"));
        assert!(inbox.try_recv().is_err(), "no player was told");
    }
}
