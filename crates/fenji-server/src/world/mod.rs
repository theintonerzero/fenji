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

// The most bytes of a chat line's text, after the client's own escaping (chat.md 1.5). The link
// holds the codec's limit to it: the codec refuses a longer line before it reaches the world,
// which checks again (#280).
pub(crate) use chat::MAX_BYTES as MAX_LINE_BYTES;
// The most characters in a line, as the client counts them: the console's maintenance
// announcement is cut to it.
pub(crate) use chat::MAX_CHARS as MAX_LINE_CHARS;
/// What a thread logs, for the tests of the console's commands and the links too.
#[cfg(test)]
pub(crate) use ecs::test_util::{logged, logged_at};
pub use ecs::{Rules, WorldCore};
pub use thread::{
    Failure, HUNG, RunningWorld, SAVE_WAIT, THREAD_WAIT, TickReport, WorldHandle, start,
};
pub use types::*;

use crate::config::Config;

/// The most messages a connection may send the world a tick, kept up, before it is kicked for
/// flooding: far above what a client sends (a move a tick at most, a pull for each player coming
/// into view, a line of chat). Each tick gives this many back to the connection's allowance.
pub const MAX_INPUTS_PER_TICK: usize = 128;

/// How long a stall a connection's allowance covers: it holds [`MAX_INPUTS_PER_TICK`] for each
/// tick period in this long, up to [`MAX_BURST`], so what a client sent during a stall, which then
/// arrives at once, passes. That's so whether the player's network stalled (#281), which the world never sees, or
/// the world itself did, its thread or process paused (#222). An honest client sends far less
/// than the limit, and its allowance stays full.
pub const MAX_STALL: std::time::Duration = std::time::Duration::from_secs(5);

/// The most a connection's allowance holds, in messages, whatever the tick rate (#281). Without it
/// [`MAX_STALL`]'s worth grew with `tick_hz`: 12,800 messages at 20 Hz, and 39,936 at 60.
///
/// An honest client's backlog after a five-second stall is far less: a move each 50 ms at most,
/// 100; a pull each 250 ms at most, 20 (world.md 14.3); the sync filter's type 5 for each player
/// coming into view, 200 at most (`[server] max_players`; 14.12); and a few lines of chat. So this
/// many still passes every stall [`MAX_STALL`] covers, at every tick rate. The worst tick, with
/// every player bursting at once, takes this many from each: 40,000 messages with 20 players,
/// where it was 256,000 at 20 Hz and 798,720 at 60. With 200 players, the most `max_players` may
/// be, it takes 400,000.
pub const MAX_BURST: usize = 2_000;

/// How long the world keeps where a character left. Its save is written off the tick, so a login
/// that reads the store before it lands would come back at an older place (#222); a minute is far
/// longer than a save takes. It keeps the account's chat and input allowances as long, which is
/// longer than either takes to fill again, so a client gains nothing by going back to character
/// select, or by reconnecting, to start them afresh (#394).
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
