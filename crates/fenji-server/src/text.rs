//! Text the server passes on for others to see: chat lines, announcements and command arguments.
//! The list of hidden characters is the console's (`fenji_console::text`), so the server's checks,
//! the host console and the launcher read text alike (M4 hosting plan, Ruling HP3).
pub use fenji_console::text::{escape_hidden, hidden, plain};
