//! The full-screen console's keys on Linux and macOS (#359), read by the console itself rather than
//! by crossterm's event source. On a terminal that has hung up, crossterm 0.29's read loop never
//! returns: every read ends at once, and it reads again, a core spinning
//! (`src/event/source/unix/mio.rs`). Its event stream keeps a thread of its own in that loop
//! whenever no key is waiting, so a console can't stop it by no longer asking for keys. Here a
//! thread reads the terminal and hands the console each read, and a read that ends, as a hung-up
//! terminal's do, ends the thread and tells the console its terminal has gone ([`Read::Gone`]).
//! [`Parser`] makes keys of the bytes as crossterm's parser does, for the keys the console uses.
use std::io;
use std::os::fd::AsFd;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rustix::io::Errno;
use tokio::sync::mpsc;

/// What the console's reading of its terminal gives it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Read {
    /// Bytes the terminal sent: keys, which [`Parser`] reads.
    Bytes(Vec<u8>),
    /// The terminal has gone: a read ended, as one does once the terminal has hung up (its window
    /// closed, or its SSH connection was lost), or failed.
    Gone,
}

/// The reads that may wait for the console.
const QUEUE: usize = 64;

/// How long the reading waits before it reads again a terminal that had nothing and said so at
/// once, as one that another program left non-blocking does: never a spin.
const NOTHING_YET: Duration = Duration::from_millis(20);

/// Reads `terminal` on a thread of its own, handing the console each read, until a read ends or
/// fails ([`Read::Gone`]) or the console takes no more. The thread isn't waited for: once the
/// console has ended, it ends at its next read, or with the process.
pub(crate) fn read(terminal: impl AsFd + Send + 'static) -> io::Result<mpsc::Receiver<Read>> {
    let (to_console, reads) = mpsc::channel(QUEUE);
    std::thread::Builder::new()
        .name("console-keys".into())
        .spawn(move || {
            let mut buf = [0; 1024];
            loop {
                let read = match rustix::io::read(&terminal, &mut buf) {
                    Ok(0) => Read::Gone,
                    Ok(n) => Read::Bytes(buf[..n].to_vec()),
                    Err(Errno::INTR) => continue,
                    Err(Errno::AGAIN) => {
                        std::thread::sleep(NOTHING_YET);
                        continue;
                    }
                    // EIO, as a hung-up terminal's reads may fail, or anything else: a terminal
                    // that can't be read has gone for the console.
                    Err(_) => Read::Gone,
                };
                let gone = read == Read::Gone;
                if to_console.blocking_send(read).is_err() || gone {
                    return;
                }
            }
        })?;
    Ok(reads)
}

/// Makes keys of what a terminal sends, as crossterm 0.29's parser does
/// (`src/event/sys/unix/parse.rs`), for the keys the console tells apart: characters, Enter, Tab,
/// Backspace, Esc, Ctrl with a letter, the arrows, Home, End, PgUp, PgDn and Delete, each with the
/// modifiers it comes with. A sequence for any other key, a function key say, is skipped whole.
#[derive(Debug, Default)]
pub(crate) struct Parser {
    /// The start of a sequence or a character whose rest hasn't come yet.
    pending: Vec<u8>,
}

/// The longest a sequence may grow before the parser gives up on it: a terminal's are a few dozen
/// bytes at most.
const LONGEST: usize = 64;

impl Parser {
    /// The keys in `bytes`, read after what came before. A lone Esc at the end of what has come is
    /// the Esc key, as with crossterm: a terminal sends each sequence in one write, which a read
    /// takes whole.
    pub(crate) fn keys(&mut self, bytes: &[u8]) -> Vec<KeyEvent> {
        self.pending.extend_from_slice(bytes);
        let mut keys = Vec::new();
        let mut at = 0;
        while at < self.pending.len() {
            match next(&self.pending[at..]) {
                Next::Key(key, len) => {
                    keys.push(key);
                    at += len;
                }
                Next::Skip(len) => at += len,
                // One that never ends is dropped, so that what follows it reads as it should.
                Next::More if self.pending.len() - at > LONGEST => at = self.pending.len(),
                Next::More => break,
            }
        }
        self.pending.drain(..at);
        keys
    }
}

/// What the bytes at the front of the input are.
#[derive(Debug, PartialEq, Eq)]
enum Next {
    /// A key, and how many bytes it took.
    Key(KeyEvent, usize),
    /// How many bytes make no key the console tells apart.
    Skip(usize),
    /// The start of a sequence or a character, whose rest hasn't come yet.
    More,
}

const ESC: u8 = 0x1b;

/// The key at the front of `input`.
fn next(input: &[u8]) -> Next {
    match input {
        [] => Next::More,
        [ESC] => key(KeyCode::Esc, 1),
        [ESC, b'[', rest @ ..] => csi(rest),
        [ESC, b'O', rest @ ..] => ss3(rest),
        [ESC, ESC, ..] => key(KeyCode::Esc, 2),
        // Alt and a key.
        [ESC, rest @ ..] => match next(rest) {
            Next::Key(mut key, len) => {
                key.modifiers |= KeyModifiers::ALT;
                Next::Key(key, len + 1)
            }
            Next::Skip(len) => Next::Skip(len + 1),
            Next::More => Next::More,
        },
        [b'\r', ..] => key(KeyCode::Enter, 1),
        [b'\t', ..] => key(KeyCode::Tab, 1),
        [0x7f, ..] => key(KeyCode::Backspace, 1),
        // Ctrl and a letter. In raw mode \n is Ctrl-J, and ^H Ctrl-H, as crossterm has them.
        [c @ 0x01..=0x1a, ..] => ctrl(char::from(c - 0x01 + b'a')),
        [c @ 0x1c..=0x1f, ..] => ctrl(char::from(c - 0x1c + b'4')),
        [0, ..] => ctrl(' '),
        _ => character(input),
    }
}

fn key(code: KeyCode, len: usize) -> Next {
    Next::Key(KeyEvent::new(code, KeyModifiers::NONE), len)
}

fn ctrl(c: char) -> Next {
    Next::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL), 1)
}

/// The key a control sequence names, `rest` following its `ESC [`: its parameters, then its final
/// byte (ECMA-48 §5.4).
fn csi(rest: &[u8]) -> Next {
    // The Linux console's F1 to F5: `ESC [ [` and a letter.
    if let [b'[', more @ ..] = rest {
        return if more.is_empty() {
            Next::More
        } else {
            Next::Skip(4)
        };
    }
    let Some(end) = rest.iter().position(|b| (0x40..=0x7e).contains(b)) else {
        return Next::More;
    };
    // A byte that has no place in a sequence, such as an Esc, cuts it short: skipped up to it.
    if let Some(cut) = rest[..end].iter().position(|b| !(0x20..=0x3f).contains(b)) {
        return Next::Skip(2 + cut);
    }
    let len = 2 + end + 1;
    let mut params = rest[..end].split(|&b| b == b';');
    let first = params.next().and_then(number);
    let modifiers = params
        .next()
        .and_then(number)
        .map_or(KeyModifiers::NONE, modifiers);
    let code = match rest[end] {
        b'A' => KeyCode::Up,
        b'B' => KeyCode::Down,
        b'C' => KeyCode::Right,
        b'D' => KeyCode::Left,
        b'H' => KeyCode::Home,
        b'F' => KeyCode::End,
        b'~' => match first {
            Some(1 | 7) => KeyCode::Home,
            Some(4 | 8) => KeyCode::End,
            Some(3) => KeyCode::Delete,
            Some(5) => KeyCode::PageUp,
            Some(6) => KeyCode::PageDown,
            _ => return Next::Skip(len),
        },
        _ => return Next::Skip(len),
    };
    Next::Key(KeyEvent::new(code, modifiers), len)
}

/// A sequence's parameter as a number: digits alone.
fn number(param: &[u8]) -> Option<u32> {
    std::str::from_utf8(param).ok()?.parse().ok()
}

/// The modifiers a sequence's parameter gives: one more than a bit field of Shift (1), Alt (2) and
/// Ctrl (4).
fn modifiers(param: u32) -> KeyModifiers {
    let bits = param.saturating_sub(1);
    let mut modifiers = KeyModifiers::NONE;
    for (bit, modifier) in [
        (1, KeyModifiers::SHIFT),
        (2, KeyModifiers::ALT),
        (4, KeyModifiers::CONTROL),
    ] {
        if bits & bit != 0 {
            modifiers |= modifier;
        }
    }
    modifiers
}

/// The key `ESC O` and `rest` name: the arrows, Home and End in the cursor keys' application mode.
fn ss3(rest: &[u8]) -> Next {
    let code = match rest.first() {
        None => return Next::More,
        Some(b'A') => KeyCode::Up,
        Some(b'B') => KeyCode::Down,
        Some(b'C') => KeyCode::Right,
        Some(b'D') => KeyCode::Left,
        Some(b'H') => KeyCode::Home,
        Some(b'F') => KeyCode::End,
        // F1 to F4, and the keypad's keys.
        Some(_) => return Next::Skip(3),
    };
    key(code, 3)
}

/// A character in UTF-8 at the front of `input`, whose rest may be yet to come. Upper case is
/// Shift's, as crossterm has it.
fn character(input: &[u8]) -> Next {
    let Some(&lead) = input.first() else {
        return Next::More;
    };
    let len = match lead {
        0x00..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return Next::Skip(1),
    };
    let Some(bytes) = input.get(..len) else {
        // The rest may come in the next read, unless what has come can't be it.
        let continues = input[1..].iter().all(|b| b & 0xc0 == 0x80);
        return if continues { Next::More } else { Next::Skip(1) };
    };
    match std::str::from_utf8(bytes)
        .ok()
        .and_then(|s| s.chars().next())
    {
        Some(c) => {
            let shift = c.is_uppercase();
            let modifiers = if shift {
                KeyModifiers::SHIFT
            } else {
                KeyModifiers::NONE
            };
            Next::Key(KeyEvent::new(KeyCode::Char(c), modifiers), len)
        }
        None => Next::Skip(1),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crossterm::event::{KeyCode, KeyModifiers};

    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn with(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    /// The keys in `bytes`, read at once.
    fn keys(bytes: &[u8]) -> Vec<KeyEvent> {
        Parser::default().keys(bytes)
    }

    #[test]
    fn characters_and_the_editing_keys_read_as_crossterm_reads_them() {
        assert_eq!(keys(b"a"), [key(KeyCode::Char('a'))]);
        // Upper case is Shift's.
        assert_eq!(keys(b"A"), [with(KeyCode::Char('A'), KeyModifiers::SHIFT)]);
        assert_eq!(
            keys("é世🙂".as_bytes()),
            [
                key(KeyCode::Char('é')),
                key(KeyCode::Char('世')),
                key(KeyCode::Char('🙂')),
            ]
        );
        assert_eq!(keys(b"\r"), [key(KeyCode::Enter)]);
        assert_eq!(keys(b"\t"), [key(KeyCode::Tab)]);
        assert_eq!(keys(b"\x7f"), [key(KeyCode::Backspace)]);
        assert_eq!(keys(b"\x1b"), [key(KeyCode::Esc)]);
        assert_eq!(keys(b"\x1b\x1b"), [key(KeyCode::Esc)]);
        let ctrl = |c| with(KeyCode::Char(c), KeyModifiers::CONTROL);
        assert_eq!(keys(b"\x03\x15"), [ctrl('c'), ctrl('u')]);
        // In raw mode, \n is Ctrl-J and ^H Ctrl-H, as crossterm has them.
        assert_eq!(keys(b"\n\x08"), [ctrl('j'), ctrl('h')]);
        assert_eq!(keys(b"\x00\x1c"), [ctrl(' '), ctrl('4')]);
        // Alt and a key, which the view lets go.
        assert_eq!(
            keys(b"\x1bx"),
            [with(KeyCode::Char('x'), KeyModifiers::ALT)]
        );
    }

    #[test]
    fn the_cursor_keys_read_in_every_form_terminals_send() {
        let forms: [(&[u8], KeyCode); 22] = [
            (b"\x1b[A", KeyCode::Up),
            (b"\x1b[B", KeyCode::Down),
            (b"\x1b[C", KeyCode::Right),
            (b"\x1b[D", KeyCode::Left),
            // The cursor keys' application mode.
            (b"\x1bOA", KeyCode::Up),
            (b"\x1bOB", KeyCode::Down),
            (b"\x1bOC", KeyCode::Right),
            (b"\x1bOD", KeyCode::Left),
            (b"\x1b[H", KeyCode::Home),
            (b"\x1bOH", KeyCode::Home),
            (b"\x1b[1~", KeyCode::Home),
            (b"\x1b[7~", KeyCode::Home),
            (b"\x1b[F", KeyCode::End),
            (b"\x1bOF", KeyCode::End),
            (b"\x1b[4~", KeyCode::End),
            (b"\x1b[8~", KeyCode::End),
            (b"\x1b[5~", KeyCode::PageUp),
            (b"\x1b[6~", KeyCode::PageDown),
            (b"\x1b[3~", KeyCode::Delete),
            (b"\x1b[5;5~", KeyCode::PageUp),
            (b"\x1b[1;2H", KeyCode::Home),
            (b"\x1b[1;5D", KeyCode::Left),
        ];
        for (bytes, code) in forms {
            let read = keys(bytes);
            assert_eq!(read.len(), 1, "{bytes:?}: {read:?}");
            assert_eq!(read[0].code, code, "{bytes:?}");
        }
        // The modifiers come from the parameter: Ctrl-Left, Shift-Home.
        assert_eq!(keys(b"\x1b[1;5D")[0].modifiers, KeyModifiers::CONTROL);
        assert_eq!(keys(b"\x1b[1;2H")[0].modifiers, KeyModifiers::SHIFT);
        // Several in one read.
        assert_eq!(
            keys(b"\x1b[A\x1b[Bx"),
            [
                key(KeyCode::Up),
                key(KeyCode::Down),
                key(KeyCode::Char('x')),
            ]
        );
    }

    #[test]
    fn a_key_the_console_doesnt_use_is_skipped_whole() {
        // F1 to F12 in their forms, Insert, Shift-Tab, a kitty key, the Linux console's F1 and a
        // cursor position report: none types a character.
        for bytes in [
            &b"\x1bOP"[..],
            b"\x1b[15~",
            b"\x1b[24~",
            b"\x1b[2~",
            b"\x1b[Z",
            b"\x1b[97;5u",
            b"\x1b[[A",
            b"\x1b[12;40R",
        ] {
            let mut input = bytes.to_vec();
            input.push(b'x');
            assert_eq!(keys(&input), [key(KeyCode::Char('x'))], "{bytes:?}");
        }
    }

    #[test]
    fn a_sequence_or_a_character_split_between_reads_is_read_whole() {
        let mut parser = Parser::default();
        assert!(parser.keys(b"\x1b[").is_empty());
        assert!(parser.keys(b"5").is_empty());
        assert_eq!(parser.keys(b"~"), [key(KeyCode::PageUp)]);
        assert!(parser.keys(b"\x1bO").is_empty());
        assert_eq!(parser.keys(b"A"), [key(KeyCode::Up)]);
        let e = "é".as_bytes();
        assert!(parser.keys(&e[..1]).is_empty());
        assert_eq!(parser.keys(&e[1..]), [key(KeyCode::Char('é'))]);
    }

    #[test]
    fn what_can_never_be_a_key_is_dropped_and_what_follows_reads() {
        // A sequence that never ends, bytes that aren't UTF-8 and a sequence cut short by another.
        let mut parser = Parser::default();
        let mut endless = b"\x1b[".to_vec();
        endless.extend([b'1'; 100]);
        assert!(parser.keys(&endless).is_empty());
        assert_eq!(parser.keys(b"a"), [key(KeyCode::Char('a'))]);
        assert_eq!(
            keys(b"\xff\xc3(b"),
            [key(KeyCode::Char('(')), key(KeyCode::Char('b')),]
        );
        assert_eq!(keys(b"\x1b[1\x1b[A"), [key(KeyCode::Up)]);
    }

    /// The next thing `reads` gives, within 5 s.
    fn next(reads: &mut mpsc::Receiver<Read>) -> Option<Read> {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match reads.try_recv() {
                Ok(read) => return Some(read),
                Err(mpsc::error::TryRecvError::Disconnected) => return None,
                Err(mpsc::error::TryRecvError::Empty) => {
                    assert!(std::time::Instant::now() < deadline, "nothing within 5 s");
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }

    #[test]
    fn a_terminal_that_hangs_up_ends_the_reading_which_says_so_once() {
        // #359: the read that ends once the terminal has hung up ends the reading's thread, rather
        // than reading again without end.
        use std::io::Write;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::OpenOptionsExt;

        use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
        let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
        grantpt(&master).unwrap();
        unlockpt(&master).unwrap();
        let name = ptsname(&master, Vec::new()).unwrap();
        // Never the test's controlling terminal, whose hang-up would send it SIGHUP.
        let terminal = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(rustix::fs::OFlags::NOCTTY.bits() as i32)
            .open(std::ffi::OsStr::from_bytes(name.as_bytes()))
            .unwrap();
        // Raw, as the console has it: each key as it comes.
        let mut raw = rustix::termios::tcgetattr(&terminal).unwrap();
        raw.make_raw();
        let now = rustix::termios::OptionalActions::Now;
        rustix::termios::tcsetattr(&terminal, now, &raw).unwrap();
        let mut reads = read(terminal).unwrap();
        let mut master = std::fs::File::from(master);
        master.write_all(b"x").unwrap();
        assert_eq!(next(&mut reads), Some(Read::Bytes(b"x".to_vec())));
        drop(master);
        assert_eq!(next(&mut reads), Some(Read::Gone));
        // The thread has ended, and its end of the channel with it.
        assert_eq!(next(&mut reads), None);
    }

    #[test]
    fn the_end_of_the_input_ends_the_reading() {
        let (from, mut to) = std::io::pipe().unwrap();
        let mut reads = read(from).unwrap();
        std::io::Write::write_all(&mut to, b"ab").unwrap();
        assert_eq!(next(&mut reads), Some(Read::Bytes(b"ab".to_vec())));
        drop(to);
        assert_eq!(next(&mut reads), Some(Read::Gone));
        assert_eq!(next(&mut reads), None);
    }

    #[test]
    fn a_console_that_takes_no_more_ends_the_reading_at_its_next_read() {
        use std::io::Write;
        let (from, mut to) = std::io::pipe().unwrap();
        drop(read(from).unwrap());
        // Its next read finds no one to hand it to: the thread ends, and the pipe's reading end
        // with it, so that a write then fails.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match to.write_all(b"a") {
                Err(e) if e.kind() == io::ErrorKind::BrokenPipe => break,
                Err(e) => panic!("{e}"),
                Ok(()) => {
                    let late = std::time::Instant::now() >= deadline;
                    assert!(!late, "something still reads the pipe after 5 s");
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }
}
