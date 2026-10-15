//! The host console's side in the server (M4 hosting spec §3): [`log`], the hub every log line goes
//! through; [`service`], which runs the console's commands and feeds its front ends; and which
//! front end `fenji-server run` gives the terminal it started in (§3.1).
use std::path::Path;

use self::log::Sink;

pub mod log;
pub mod service;

/// The console `fenji-server run` shows in the terminal it started in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Front {
    /// The full-screen console, which holds the terminal: the log goes to its pane and to a file.
    Full,
    /// Plain lines: commands from stdin, answers on stdout and the log on stderr, which systemd's
    /// journal keeps.
    Plain,
}

/// The full-screen console only when stdin and stdout are both terminals, `TERM` isn't `dumb`
/// (Windows' consoles set none) and `--plain` wasn't given; plain lines otherwise.
pub fn front(stdin_tty: bool, stdout_tty: bool, term: Option<&str>, plain: bool) -> Front {
    if stdin_tty && stdout_tty && term != Some("dumb") && !plain {
        Front::Full
    } else {
        Front::Plain
    }
}

/// Where `front`'s log goes, for a database at `store_path` (M4 hosting spec §3.4). Both feed the
/// hub, for the consoles. The full-screen console's file is in the data folder's `logs`
/// ([`log::folder`]), as nothing else may write to its terminal; plain lines' log goes to stderr.
pub fn sinks(front: Front, store_path: &Path) -> Vec<Sink> {
    match front {
        Front::Full => vec![Sink::File(log::folder(store_path)), Sink::Hub],
        Front::Plain => vec![Sink::Stderr, Sink::Hub],
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::log::Sink;
    use super::*;

    #[test]
    fn the_full_screen_console_logs_to_a_file_in_the_data_folder_and_plain_lines_to_stderr() {
        // Carry-forward 4: both give attached consoles the hub, and the full-screen console's
        // file stays in the owner-only data folder, beside the database.
        let store = Path::new("/srv/fenji/data/fenji.db");
        assert_eq!(
            sinks(Front::Full, store),
            [Sink::File("/srv/fenji/data/logs".into()), Sink::Hub]
        );
        assert_eq!(sinks(Front::Plain, store), [Sink::Stderr, Sink::Hub]);
        // A database named alone is in the working directory, and so are its logs.
        assert_eq!(
            sinks(Front::Full, Path::new("fenji.db")),
            [Sink::File("logs".into()), Sink::Hub]
        );
    }
}
