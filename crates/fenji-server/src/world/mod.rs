//! The world (M4 world spec §3.9; Fenji design spec §6.4): a headless bevy_ecs world on its own
//! thread at `[server] tick_hz`, fed over channels by the connection tasks. It speaks only the
//! protocol-neutral events of [`types`]; the connection side maps them to and from the wire.
mod admin;
mod chat;
mod ecs;
mod interest;
mod movement;
mod services;
mod thread;
pub mod types;

pub use ecs::{Rules, WorldCore};
pub use thread::{RunningWorld, SAVE_WAIT, TickReport, WorldHandle, start};
pub use types::*;

use crate::config::Config;

/// The most messages a connection may send in one tick period before it is kicked: far above what
/// a client sends (a few moves, a pull for each player coming into view, a line of chat).
pub const MAX_INPUTS_PER_TICK: usize = 128;

/// The longest stall between ticks whose inputs the flood check takes as spread over the periods
/// it missed: a longer one counts as this long (#222). A stall is a gap the world's monotonic
/// clock sees: its thread or process paused, or a VM paused while that clock runs on; a suspend of
/// the host isn't one, since `Instant` doesn't advance through it.
pub const MAX_STALL: std::time::Duration = std::time::Duration::from_secs(5);

/// How long the world keeps where a character left. Its save is written off the tick, so a login
/// that reads the store before it lands would come back at an older place (#222); a minute is far
/// longer than a save takes.
pub const KEEP_LEFT: std::time::Duration = std::time::Duration::from_secs(60);

impl Rules {
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            tick_hz: cfg.server.tick_hz,
            move_tolerance: cfg.world.move_tolerance,
            save_every_ms: cfg.world.save_every_secs.saturating_mul(1000),
            max_inputs_per_tick: MAX_INPUTS_PER_TICK,
        }
    }
}

/// Where a character that has never entered starts: `[world] start_scene` and `start_position`.
pub fn start_location(cfg: &Config) -> Location {
    let [x, y, z, yaw] = cfg.world.start_position;
    Location {
        scene: cfg.world.start_scene,
        pos: Pos { x, y, z, yaw },
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn the_rules_and_the_start_come_from_the_config() {
        let cfg = Config::parse(
            "[server]\nname = \"F\"\ntick_hz = 30\n[world]\nstart_scene = 7\n\
             start_position = [1.0, 2.0, 3.0, 4.0]\nmove_tolerance = 2.0\nsave_every_secs = 60\n",
            Path::new("/"),
        )
        .unwrap();
        let r = Rules::from_config(&cfg);
        assert_eq!(
            (
                r.tick_hz,
                r.move_tolerance,
                r.save_every_ms,
                r.max_inputs_per_tick
            ),
            (30, 2.0, 60_000, MAX_INPUTS_PER_TICK)
        );
        assert_eq!(
            start_location(&cfg),
            Location {
                scene: 7,
                pos: Pos {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                    yaw: 4.0
                }
            }
        );
    }
}
