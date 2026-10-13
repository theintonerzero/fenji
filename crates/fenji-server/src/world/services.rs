//! Work off the tick (Fenji design spec §6.5, §6.7): the world's saves run on the tokio runtime,
//! so a slow disk never holds a tick. Each save is one statement.
use fenji_store::{Place, Store};
use tokio::sync::mpsc;

use super::types::Service;

/// Runs until the world drops its end of `jobs`, after the last job queued.
pub(super) async fn run(store: Store, mut jobs: mpsc::UnboundedReceiver<Service>) {
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
        }
    }
}
