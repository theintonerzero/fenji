//! Text the server passes on for others to see: chat lines, announcements and command arguments.
//! The list of hidden characters is the console's (`fenji_console::text`), so the server's checks,
//! the host console and the launcher read text alike (M4 hosting plan, Ruling HP3).
pub use fenji_console::text::{escape_hidden, hidden, plain};

/// `word` as one word for a POSIX shell: as it is when it holds nothing the shell reads, quoted
/// otherwise. For the commands a message gives a host to copy.
pub(crate) fn shell_word(word: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "/._-+,:@%=".contains(c);
    if !word.is_empty() && word.chars().all(plain) {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}
