//! The host console's front ends (M4 hosting spec §3.1, §3.2): live logs above and a command line
//! below, like a Minecraft server's.
//! - [`wire`]: what a server and a console say to each other, in-process or over the console
//!   socket.
//! - [`text`]: the one list of hidden characters. Players' names, chat, invite labels and messages
//!   reach the host's terminal, so everything a front end draws or prints goes through
//!   [`text::clean`].
//! - With the `tui` feature (the default): [`run_tui`], the full-screen console, drawn by
//!   [`view::View`] in [`theme`]'s colours, and [`run_plain`], the console as plain lines.
//!
//! Without `tui`, the crate is [`wire`] and [`text`] alone, as the launcher takes it.
pub mod text;
pub mod wire;

#[cfg(all(feature = "tui", unix))]
mod keys;
#[cfg(feature = "tui")]
mod plain;
#[cfg(feature = "tui")]
pub mod theme;
#[cfg(feature = "tui")]
mod tui;
#[cfg(feature = "tui")]
pub mod view;

#[cfg(feature = "tui")]
pub use plain::run_plain;
#[cfg(feature = "tui")]
pub use theme::Colours;
#[cfg(feature = "tui")]
pub use tui::run_tui;
#[cfg(feature = "tui")]
pub use view::Mode;
pub use wire::Hello;

#[cfg(feature = "tui")]
use tokio::sync::mpsc;

/// A console's link to its server: what the server said first, and a channel each way. In-process
/// the server holds the other ends; attached, a task bridges them to the console socket.
#[cfg(feature = "tui")]
#[derive(Debug)]
pub struct Session {
    pub hello: Hello,
    pub from_server: mpsc::Receiver<wire::ServerMsg>,
    pub to_server: mpsc::Sender<wire::ConsoleMsg>,
}

#[cfg(feature = "tui")]
impl Session {
    /// A session with a server named `Test` that takes `commands`.
    #[cfg(test)]
    pub(crate) fn for_test(
        commands: Vec<wire::CommandHelp>,
        from_server: mpsc::Receiver<wire::ServerMsg>,
        to_server: mpsc::Sender<wire::ConsoleMsg>,
    ) -> Session {
        Session {
            hello: Hello {
                version: "0.0.0".into(),
                proto: wire::PROTO,
                server: "Test".into(),
                commands,
            },
            from_server,
            to_server,
        }
    }
}

/// How a console ended.
#[cfg(feature = "tui")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ending {
    /// The host left an attached console, and the server runs on.
    Detached,
    /// The server said goodbye, with its reason.
    ServerStopped(String),
    /// The server's side closed without a goodbye.
    Lost,
    /// The console's input ended: stdin for plain lines, the terminal's keys for full-screen.
    InputClosed,
    /// The full-screen console's terminal has gone: it hung up, as when its window closes or its
    /// SSH connection is lost, ending its reads and failing its writes.
    HungUp,
}
