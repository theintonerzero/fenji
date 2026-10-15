//! The full-screen console in the host's terminal (M4 hosting spec §3.1, §3.2). It takes the
//! terminal over and gives it back on every way out, a panic included (Review Focus 2).
use std::io;
use std::panic::{self, PanicHookInfo};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossterm::cursor::{Hide, Show};
use crossterm::event::{Event, EventStream, KeyEvent};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use futures_util::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::{TryRecvError, TrySendError};
use tokio::time::MissedTickBehavior;

use crate::theme::{Colours, Palette};
use crate::view::{Action, Mode, View};
use crate::wire::{ConsoleMsg, ServerMsg};
use crate::{Ending, Session};

/// The most server messages handled between two draws, so a flood of log lines can't keep the
/// keys waiting.
const BATCH: usize = 256;

/// Whether a panic, on any thread, gave the terminal back while the console held it.
static PANICKED: AtomicBool = AtomicBool::new(false);

/// The full-screen console, until the host leaves it or the server goes. It draws on every
/// message and key, and every second for the uptime. Ctrl-C sends `stop` in-process, and leaves
/// when attached.
pub async fn run_tui(session: Session, mode: Mode, colours: Colours) -> io::Result<Ending> {
    let Session {
        hello,
        mut from_server,
        to_server,
    } = session;
    let mut view = View::new(hello.server, hello.commands, mode);
    let palette = Palette::of(colours);
    let _screen = Screen::take()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;
    let mut keys = EventStream::new();
    let mut second = tokio::time::interval(Duration::from_secs(1));
    second.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut last_id = 0;
    loop {
        if PANICKED.load(Ordering::SeqCst) {
            // The panic hook gave the terminal back: drawing now would scribble on the shell.
            return Err(io::Error::other(
                "a panic ended the console; its message is above",
            ));
        }
        terminal.draw(|frame| view.draw(frame, &palette, now_ms()))?;
        tokio::select! {
            msg = from_server.recv() => {
                if let Some(ending) = take_messages(&mut view, msg, &mut from_server) {
                    return Ok(ending);
                }
            }
            event = keys.next() => match event {
                None => return Ok(Ending::InputClosed),
                Some(Err(e)) => return Err(e),
                Some(Ok(Event::Key(key))) => {
                    if let Some(ending) = act(&mut view, key, mode, &to_server, &mut last_id) {
                        return Ok(ending);
                    }
                }
                // A resize and the rest: drawn again.
                Some(Ok(_)) => {}
            },
            _ = second.tick() => {}
        }
    }
}

/// Hands `msg`, and up to [`BATCH`] more already waiting, to the view: the ending, if the server
/// said goodbye or went.
fn take_messages(
    view: &mut View,
    msg: Option<ServerMsg>,
    from_server: &mut mpsc::Receiver<ServerMsg>,
) -> Option<Ending> {
    let mut msg = msg;
    let mut handled = 0;
    loop {
        match msg {
            None => return Some(Ending::Lost),
            Some(ServerMsg::Bye { reason }) => return Some(Ending::ServerStopped(reason)),
            Some(msg) => view.on_server(msg),
        }
        handled += 1;
        if handled == BATCH {
            return None;
        }
        msg = match from_server.try_recv() {
            Ok(next) => Some(next),
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => None,
        };
    }
}

/// What a key does: a command for the server, or the console's end.
fn act(
    view: &mut View,
    key: KeyEvent,
    mode: Mode,
    to_server: &mpsc::Sender<ConsoleMsg>,
    last_id: &mut u64,
) -> Option<Ending> {
    let line = match view.on_key(key)? {
        Action::Run(line) => line,
        Action::Interrupt if mode == Mode::Attached => return Some(Ending::Detached),
        Action::Interrupt => "stop".to_owned(),
    };
    *last_id += 1;
    // Never wait on the server here: the keys and the log would stop with it.
    match to_server.try_send(ConsoleMsg::Run { id: *last_id, line }) {
        Ok(()) => None,
        Err(TrySendError::Full(_)) => {
            view.note("the server is busy with other commands: that one wasn't sent");
            None
        }
        Err(TrySendError::Closed(_)) => Some(Ending::Lost),
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// The terminal, taken over: raw mode, the alternate screen and a hidden cursor. Dropping it
/// gives the terminal back, and so does a panic on any thread while it's held.
struct Screen {
    _hook: PanicHook,
}

impl Screen {
    fn take() -> io::Result<Screen> {
        PANICKED.store(false, Ordering::SeqCst);
        // In place before the first change, so a failure part-way gives back what changed.
        let screen = Screen {
            _hook: PanicHook::install(give_back),
        };
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, Hide)?;
        Ok(screen)
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        give_back();
    }
}

/// Gives the terminal back: the main screen, a cursor and cooked mode. Running it twice is
/// harmless.
fn give_back() {
    let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
    let _ = disable_raw_mode();
}

type Hook = Box<dyn Fn(&PanicHookInfo<'_>) + Sync + Send + 'static>;

/// A panic hook that runs `give_back` before the hook it replaced prints the panic, so the message
/// lands on the main screen, readable. The replaced hook comes back when this is dropped.
struct PanicHook {
    previous: Option<Arc<Hook>>,
}

impl PanicHook {
    fn install(give_back: fn()) -> PanicHook {
        let previous = Arc::new(panic::take_hook());
        let chained = Arc::clone(&previous);
        panic::set_hook(Box::new(move |info| {
            PANICKED.store(true, Ordering::SeqCst);
            give_back();
            (*chained)(info);
        }));
        PanicHook {
            previous: Some(previous),
        }
    }
}

impl Drop for PanicHook {
    fn drop(&mut self) {
        // A panicking thread can't change the hook, and the process is on its way out then.
        if std::thread::panicking() {
            return;
        }
        let Some(previous) = self.previous.take() else {
            return;
        };
        // Ours goes, and with it its share of the previous hook.
        drop(panic::take_hook());
        match Arc::try_unwrap(previous) {
            Ok(hook) => panic::set_hook(hook),
            Err(shared) => panic::set_hook(Box::new(move |info| (*shared)(info))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn a_panic_gives_the_terminal_back_before_the_previous_hook_prints() {
        // Review Focus 2. The panic hook is the process's: no other test sets one.
        static SEEN: Mutex<Vec<&str>> = Mutex::new(Vec::new());
        let before = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {
            SEEN.lock().unwrap().push("the previous hook");
        }));
        {
            let _hook = PanicHook::install(|| {
                SEEN.lock().unwrap().push("the terminal given back");
            });
            assert!(
                std::thread::spawn(|| panic!("in the console"))
                    .join()
                    .is_err()
            );
        }
        // Gone with its guard: the previous hook alone.
        assert!(
            std::thread::spawn(|| panic!("after the console"))
                .join()
                .is_err()
        );
        std::panic::set_hook(before);
        assert_eq!(
            *SEEN.lock().unwrap(),
            [
                "the terminal given back",
                "the previous hook",
                "the previous hook"
            ]
        );
    }
}
