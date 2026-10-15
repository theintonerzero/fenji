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
    use crate::wire::{Level, LogLine, Part};
    use crossterm::event::{KeyCode, KeyModifiers};
    use ratatui::backend::TestBackend;
    use std::sync::Mutex;

    /// The thread the hook test panics on. The panic hook is the process's, and the crate's other
    /// tests run beside it: the test counts only its own thread's panics, and passes every other
    /// one on to the hook that was there before, which prints it as usual.
    const HOOK_TEST: &str = "panic-hook-test";

    fn on_hook_test_thread() -> bool {
        std::thread::current().name() == Some(HOOK_TEST)
    }

    /// Whether `message` panicked on a thread of the hook test's.
    fn panics_on_hook_test_thread(message: &'static str) -> bool {
        std::thread::Builder::new()
            .name(HOOK_TEST.into())
            .spawn(move || panic!("{message}"))
            .unwrap()
            .join()
            .is_err()
    }

    #[test]
    fn a_panic_gives_the_terminal_back_before_the_previous_hook_prints() {
        // Review Focus 2.
        static SEEN: Mutex<Vec<&str>> = Mutex::new(Vec::new());
        let before: Arc<Hook> = Arc::new(panic::take_hook());
        let others = Arc::clone(&before);
        panic::set_hook(Box::new(move |info| {
            if on_hook_test_thread() {
                SEEN.lock().unwrap().push("the previous hook");
            } else {
                (*others)(info);
            }
        }));
        {
            let _hook = PanicHook::install(|| {
                if on_hook_test_thread() {
                    SEEN.lock().unwrap().push("the terminal given back");
                }
            });
            assert!(panics_on_hook_test_thread("in the console"));
        }
        // Gone with its guard: the previous hook alone.
        assert!(panics_on_hook_test_thread("after the console"));
        // The hook the tests had, back.
        drop(panic::take_hook());
        match Arc::try_unwrap(before) {
            Ok(hook) => panic::set_hook(hook),
            Err(shared) => panic::set_hook(Box::new(move |info| (*shared)(info))),
        }
        assert_eq!(
            *SEEN.lock().unwrap(),
            [
                "the terminal given back",
                "the previous hook",
                "the previous hook"
            ]
        );
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl_c() -> KeyEvent {
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
    }

    /// Types `line` and presses Enter through [`act`]: what Enter did.
    fn run(
        view: &mut View,
        line: &str,
        mode: Mode,
        to_server: &mpsc::Sender<ConsoleMsg>,
        last_id: &mut u64,
    ) -> Option<Ending> {
        for c in line.chars() {
            assert_eq!(
                act(view, press(KeyCode::Char(c)), mode, to_server, last_id),
                None
            );
        }
        act(view, press(KeyCode::Enter), mode, to_server, last_id)
    }

    fn log(n: usize) -> ServerMsg {
        ServerMsg::Log {
            line: LogLine {
                at_ms: 0,
                level: Level::Info,
                parts: vec![Part {
                    text: format!("line {n}"),
                    name: false,
                }],
            },
        }
    }

    /// The view drawn at 80 × 24, as one string.
    fn screen(view: &View) -> String {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| view.draw(frame, &Palette::of(Colours::True), 0))
            .unwrap();
        let buffer = terminal.backend().buffer();
        buffer
            .area
            .positions()
            .map(|at| buffer[at].symbol())
            .collect()
    }

    #[test]
    fn ctrl_c_sends_stop_in_process_and_only_leaves_when_attached() {
        // The brief's rule, Review M4.
        let (to_server, mut sent) = mpsc::channel(8);
        let mut last_id = 0;
        let mut view = View::new("F".into(), vec![], Mode::InProcess);
        assert_eq!(
            act(
                &mut view,
                ctrl_c(),
                Mode::InProcess,
                &to_server,
                &mut last_id
            ),
            None
        );
        assert_eq!(
            sent.try_recv(),
            Ok(ConsoleMsg::Run {
                id: 1,
                line: "stop".into()
            })
        );
        // A line run in-process is the next command, `exit` included.
        assert_eq!(
            run(&mut view, "exit", Mode::InProcess, &to_server, &mut last_id),
            None
        );
        assert_eq!(
            sent.try_recv(),
            Ok(ConsoleMsg::Run {
                id: 2,
                line: "exit".into()
            })
        );
        // Attached, Ctrl-C and `exit` leave, and send nothing.
        let mut view = View::new("F".into(), vec![], Mode::Attached);
        assert_eq!(
            act(
                &mut view,
                ctrl_c(),
                Mode::Attached,
                &to_server,
                &mut last_id
            ),
            Some(Ending::Detached)
        );
        assert_eq!(
            run(&mut view, "exit", Mode::Attached, &to_server, &mut last_id),
            Some(Ending::Detached)
        );
        assert_eq!(
            run(
                &mut view,
                "online",
                Mode::Attached,
                &to_server,
                &mut last_id
            ),
            None
        );
        assert_eq!(
            sent.try_recv(),
            Ok(ConsoleMsg::Run {
                id: 3,
                line: "online".into()
            })
        );
        assert!(sent.try_recv().is_err());
    }

    #[test]
    fn a_busy_server_is_noted_and_a_gone_one_ends_the_console() {
        let (to_server, sent) = mpsc::channel(1);
        to_server
            .try_send(ConsoleMsg::Run {
                id: 0,
                line: "backup".into(),
            })
            .unwrap();
        let mut last_id = 0;
        let mut view = View::new("F".into(), vec![], Mode::InProcess);
        // A full queue: the command isn't sent, and the log says so; the console carries on.
        assert_eq!(
            run(
                &mut view,
                "online",
                Mode::InProcess,
                &to_server,
                &mut last_id
            ),
            None
        );
        assert!(
            screen(&view).contains("the server is busy"),
            "{}",
            screen(&view)
        );
        drop(sent);
        assert_eq!(
            run(
                &mut view,
                "online",
                Mode::InProcess,
                &to_server,
                &mut last_id
            ),
            Some(Ending::Lost)
        );
        assert_eq!(
            act(
                &mut view,
                ctrl_c(),
                Mode::InProcess,
                &to_server,
                &mut last_id
            ),
            Some(Ending::Lost)
        );
    }

    #[test]
    fn the_servers_goodbye_or_its_going_ends_the_console() {
        let mut view = View::new("F".into(), vec![], Mode::InProcess);
        let (to_console, mut from_server) = mpsc::channel(8);
        to_console.try_send(log(1)).unwrap();
        to_console
            .try_send(ServerMsg::Bye {
                reason: "the server is stopping".into(),
            })
            .unwrap();
        to_console.try_send(log(2)).unwrap();
        assert_eq!(
            take_messages(&mut view, Some(log(0)), &mut from_server),
            Some(Ending::ServerStopped("the server is stopping".into()))
        );
        // The lines before the goodbye are shown, and none after it is taken.
        let shown = screen(&view);
        assert!(
            shown.contains("line 0") && shown.contains("line 1"),
            "{shown}"
        );
        assert_eq!(from_server.len(), 1);
        // A server gone, at once or after the messages waiting, is lost.
        assert_eq!(
            take_messages(&mut view, None, &mut from_server),
            Some(Ending::Lost)
        );
        drop(to_console);
        let next = from_server.try_recv().ok();
        assert_eq!(
            take_messages(&mut view, next, &mut from_server),
            Some(Ending::Lost)
        );
        assert!(screen(&view).contains("line 2"));
    }

    #[test]
    fn a_flood_of_messages_is_taken_a_batch_at_a_time() {
        let mut view = View::new("F".into(), vec![], Mode::InProcess);
        let (to_console, mut from_server) = mpsc::channel(BATCH + 10);
        for n in 0..BATCH + 10 {
            to_console.try_send(log(n)).unwrap();
        }
        let first = from_server.try_recv().ok();
        assert_eq!(take_messages(&mut view, first, &mut from_server), None);
        assert_eq!(
            from_server.len(),
            10,
            "the rest wait their turn, after the keys and a draw"
        );
        // A batch ends early when nothing more is waiting.
        let next = from_server.try_recv().ok();
        assert_eq!(take_messages(&mut view, next, &mut from_server), None);
        assert!(from_server.is_empty());
        assert!(screen(&view).contains(&format!("line {}", BATCH + 9)));
    }
}
