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
                    Err(e) => tracing::warn!(pid, error = %e, "saving a place"),
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
