//! The full-screen console in the host's terminal (M4 hosting spec §3.1, §3.2). It takes the
//! terminal over and gives it back on every way out, a panic included (Review Focus 2).
use std::cell::Cell;
use std::io;
#[cfg(unix)]
use std::os::fd::AsFd;
use std::panic::{self, PanicHookInfo};
use std::sync::Arc;
#[cfg(unix)]
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crossterm::cursor::{Hide, Show};
use crossterm::event::KeyEvent;
#[cfg(not(unix))]
use crossterm::event::{Event, EventStream};
use crossterm::execute;
use crossterm::terminal::{Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
#[cfg(not(unix))]
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
#[cfg(not(unix))]
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

/// Whether the console is drawing a frame. A panic on another thread waits for it before it gives
/// the terminal back: the rest of the frame would land on the main screen, under the panic's
/// message.
static DRAWING: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// Whether this thread is the one drawing: a panic in the drawing doesn't wait for itself.
    static DRAWS_HERE: Cell<bool> = const { Cell::new(false) };
    /// Whether this thread is the console's, running [`run_tui`]: its panics end the console, and
    /// give the terminal back for their message. Other threads' go to the log, when there's one.
    static CONSOLE_HERE: Cell<bool> = const { Cell::new(false) };
}

/// How long a panic on another thread waits for a frame being drawn.
const FRAME_WAIT: Duration = Duration::from_secs(1);

/// The full-screen console, until the host leaves it, the server goes or the terminal does. It
/// draws on every message and key, and every second for the uptime. Ctrl-C sends `stop`
/// in-process, and leaves when attached. Once `leave` is done, the console leaves as an attached
/// one's Ctrl-C does, with [`Ending::Detached`] and the terminal given back: an attached console's
/// signals. A terminal that hangs up ends it with [`Ending::HungUp`], whether a read of its keys or
/// a frame's write finds it gone (#359).
///
/// A panic on the thread that runs it ends it, the terminal given back first for the panic's
/// message (Review Focus 2). One on another thread goes to `panics`, and the console runs on;
/// without `panics`, or if it turns the panic away, that one ends the console too.
pub async fn run_tui(
    session: Session,
    mode: Mode,
    colours: Colours,
    leave: impl Future<Output = ()>,
    panics: Option<PanicLog>,
) -> io::Result<Ending> {
    let Session {
        hello,
        mut from_server,
        to_server,
    } = session;
    let mut view = View::new(hello.server, hello.commands, mode);
    let palette = Palette::of(colours);
    // Before the hook is in place: a panic here is the console's.
    let _here = ConsoleHere::mark();
    // The screen comes cleared ([`enter_to`]), and the terminal's buffers start empty, so the first
    // frame draws it all. Never `Terminal::clear`: it asks the terminal where its cursor is, and
    // waits up to 2 s for an answer that some never give (#359).
    let _screen = Screen::take(panics)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut input = Input::open()?;
    let mut second = tokio::time::interval(Duration::from_secs(1));
    second.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut last_id = 0;
    let mut leave = std::pin::pin!(leave);
    loop {
        let Some(drawing) = Drawing::begin() else {
            // The panic hook gave the terminal back: drawing now would scribble on the shell.
            return Err(io::Error::other(
                "a panic ended the console; its message is above",
            ));
        };
        let drawn = terminal.draw(|frame| view.draw(frame, &palette, now_ms()));
        drop(drawing);
        if let Err(e) = drawn {
            return if hung_up(&e) || input.goes_within(GONE_WAIT).await {
                Ok(Ending::HungUp)
            } else {
                Err(e)
            };
        }
        tokio::select! {
            msg = from_server.recv() => {
                if let Some(ending) = take_messages(&mut view, msg, &mut from_server) {
                    return Ok(ending);
                }
            }
            said = input.next() => match said? {
                Said::Keys(keys) => {
                    for key in keys {
                        if let Some(ending) = act(&mut view, key, mode, &to_server, &mut last_id) {
                            return Ok(ending);
                        }
                    }
                }
                Said::Redraw => {}
                #[cfg(unix)]
                Said::Gone => return Ok(Ending::HungUp),
                Said::Ended => return Ok(Ending::InputClosed),
            },
            _ = second.tick() => {}
            () = &mut leave => return Ok(Ending::Detached),
        }
    }
}

/// How long a frame that failed waits for the terminal's keys to say it has gone. A frame drawn as
/// the terminal hangs up may fail before the keys say so, and not always with a write's error: for
/// the window's size, crossterm runs `tput` once the terminal no longer answers.
const GONE_WAIT: Duration = Duration::from_millis(200);

/// What the terminal says to the console.
enum Said {
    /// Keys, in the order they were pressed.
    Keys(Vec<KeyEvent>),
    /// Nothing for the view, but a reason to draw again: a new size, say.
    Redraw,
    /// The terminal has gone: it hung up.
    #[cfg(unix)]
    Gone,
    /// Its keys have ended for another reason.
    Ended,
}

/// The terminal's keys and changes of size. On Linux and macOS the console reads them itself
/// ([`crate::keys`]), since crossterm's reading spins on a terminal that has hung up (#359), and
/// SIGWINCH says the window's size has changed.
#[cfg(unix)]
struct Input {
    reads: mpsc::Receiver<crate::keys::Read>,
    parser: crate::keys::Parser,
    resized: Option<tokio::signal::unix::Signal>,
}

#[cfg(unix)]
impl Input {
    fn open() -> io::Result<Input> {
        use tokio::signal::unix::{SignalKind, signal};
        Ok(Input {
            reads: crate::keys::read(rustix::stdio::stdin())?,
            parser: crate::keys::Parser::default(),
            // Without it, a new size shows at the next second's frame.
            resized: signal(SignalKind::window_change()).ok(),
        })
    }

    /// What the terminal says next. Cancel-safe: cancelled, it has taken nothing.
    async fn next(&mut self) -> io::Result<Said> {
        let Input {
            reads,
            parser,
            resized,
        } = self;
        let resized = async {
            match resized {
                Some(resized) => resized.recv().await,
                None => std::future::pending().await,
            }
        };
        Ok(tokio::select! {
            read = reads.recv() => match read {
                Some(crate::keys::Read::Bytes(bytes)) => Said::Keys(parser.keys(&bytes)),
                Some(crate::keys::Read::Gone) => Said::Gone,
                // Its thread has gone without a word.
                None => Said::Ended,
            },
            Some(()) = resized => Said::Redraw,
        })
    }

    /// Whether the terminal says, within `wait`, that it has gone. Keys that come meanwhile are let
    /// go: the console is ending.
    async fn goes_within(&mut self, wait: Duration) -> bool {
        let gone = async {
            loop {
                match self.reads.recv().await {
                    Some(crate::keys::Read::Gone) => return true,
                    Some(crate::keys::Read::Bytes(_)) => {}
                    None => return false,
                }
            }
        };
        tokio::time::timeout(wait, gone).await.unwrap_or(false)
    }
}

/// The terminal's keys and changes of size, as crossterm's event stream gives them: Windows' console
/// has no hang-up of the kind it spins on.
#[cfg(not(unix))]
struct Input(EventStream);

#[cfg(not(unix))]
impl Input {
    fn open() -> io::Result<Input> {
        Ok(Input(EventStream::new()))
    }

    /// What the terminal says next. Cancel-safe: cancelled, it has taken nothing.
    async fn next(&mut self) -> io::Result<Said> {
        Ok(match self.0.next().await {
            None => Said::Ended,
            Some(Err(e)) => return Err(e),
            Some(Ok(Event::Key(key))) => Said::Keys(vec![key]),
            // A resize and the rest: drawn again.
            Some(Ok(_)) => Said::Redraw,
        })
    }

    /// Never: a console here has no hang-up to wait for.
    async fn goes_within(&mut self, _wait: Duration) -> bool {
        false
    }
}

/// Whether `e`, from writing to the terminal, says it has hung up: on Linux and macOS, a write to a
/// terminal that has fails with EIO, or ENXIO.
fn hung_up(e: &io::Error) -> bool {
    #[cfg(unix)]
    {
        use rustix::io::Errno;
        [Errno::IO, Errno::NXIO]
            .iter()
            .any(|gone| e.raw_os_error() == Some(gone.raw_os_error()))
    }
    #[cfg(not(unix))]
    {
        let _ = e;
        false
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

/// A frame being drawn, from the check that no panic has given the terminal back to the frame's
/// end. The flags are sequentially consistent on both sides: the drawing marks itself, then looks
/// for a panic; a panic marks itself, then looks for a drawing. One sees the other.
struct Drawing;

impl Drawing {
    /// `None` once a panic has given the terminal back: nothing more may be drawn.
    fn begin() -> Option<Drawing> {
        DRAWS_HERE.set(true);
        DRAWING.store(true, Ordering::SeqCst);
        let drawing = Drawing;
        (!PANICKED.load(Ordering::SeqCst)).then_some(drawing)
    }
}

impl Drop for Drawing {
    fn drop(&mut self) {
        DRAWING.store(false, Ordering::SeqCst);
        DRAWS_HERE.set(false);
    }
}

/// Waits, [`FRAME_WAIT`] at most, for the frame another thread is drawing, so that none of it lands
/// on the main screen once the terminal is given back. A panic in the drawing itself goes on.
fn wait_for_the_frame() {
    if DRAWS_HERE.try_with(Cell::get).unwrap_or(false) {
        return;
    }
    let until = Instant::now() + FRAME_WAIT;
    while DRAWING.load(Ordering::SeqCst) && Instant::now() < until {
        std::thread::yield_now();
    }
}

/// The terminal, taken over: raw mode, the alternate screen, a hidden cursor and stderr pointed
/// away from it. Dropping it gives the terminal back, and so does a panic while it's held: on the
/// console's own thread, or on any other that `panics` doesn't take.
struct Screen {
    _hook: PanicHook,
}

impl Screen {
    fn take(panics: Option<PanicLog>) -> io::Result<Screen> {
        PANICKED.store(false, Ordering::SeqCst);
        // In place before the first change, so a failure part-way gives back what changed.
        let screen = Screen {
            _hook: PanicHook::install(give_back, panics),
        };
        raw_mode()?;
        // Marked first: the switch may be written even if the call then fails.
        ALTERNATE.store(true, Ordering::SeqCst);
        enter_to(&mut io::stdout())?;
        quiet_stderr();
        Ok(screen)
    }
}

/// The terminal's mode as the console found it, while the console has it raw: what
/// [`restore_mode`] gives back.
#[cfg(unix)]
static FOUND: Mutex<Option<rustix::termios::Termios>> = Mutex::new(None);

/// Puts the terminal in raw mode, as crossterm does (`cfmakeraw`), keeping the mode it found. On
/// Linux and macOS the console does it itself, so that [`restore_mode`] can give the mode back from
/// any thread, with no lock of crossterm's.
#[cfg(unix)]
fn raw_mode() -> io::Result<()> {
    raw_mode_of(rustix::stdio::stdin())
}

/// [`raw_mode`] for `terminal`.
#[cfg(unix)]
fn raw_mode_of(terminal: impl AsFd) -> io::Result<()> {
    use rustix::termios::{OptionalActions, tcgetattr, tcsetattr};
    let mut found = FOUND.lock().unwrap_or_else(|e| e.into_inner());
    if found.is_some() {
        return Ok(());
    }
    let mode = tcgetattr(&terminal)?;
    let mut raw = mode.clone();
    raw.make_raw();
    // Kept before the change: one that fails part-way is given back too.
    *found = Some(mode);
    tcsetattr(&terminal, OptionalActions::Now, &raw)?;
    Ok(())
}

/// Puts the terminal in raw mode, crossterm's way, which Windows' console takes.
#[cfg(not(unix))]
fn raw_mode() -> io::Result<()> {
    enable_raw_mode()?;
    RAW.store(true, Ordering::SeqCst);
    Ok(())
}

/// Gives the terminal back the mode the console found it in, if the console has it raw, and nothing
/// else: on Linux and macOS with `tcsetattr` alone, which writes nothing to the terminal and reads
/// nothing from it, so that it never waits on a terminal that has stopped reading. From any thread,
/// and once: done again, it does nothing.
///
/// For a console that is to be left behind, its thread caught in a write to such a terminal, as
/// `fenji-server run` leaves one once the server has stopped (#359). The screen stays the
/// console's, as giving it back takes a write, but the shell after it reads lines and echoes them.
pub fn restore_mode() {
    #[cfg(unix)]
    restore_mode_of(rustix::stdio::stdin());
    #[cfg(not(unix))]
    if RAW.swap(false, Ordering::SeqCst) {
        let _ = disable_raw_mode();
    }
}

/// [`restore_mode`] for `terminal`. The mode is held only to look at it or change it, never across
/// a write, so this never waits for another thread's: while one gives it back, this has nothing to
/// do.
#[cfg(unix)]
fn restore_mode_of(terminal: impl AsFd) {
    let found = match FOUND.try_lock() {
        Ok(mut found) => found.take(),
        Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner().take(),
        Err(std::sync::TryLockError::WouldBlock) => None,
    };
    if let Some(found) = found {
        let now = rustix::termios::OptionalActions::Now;
        let _ = rustix::termios::tcsetattr(&terminal, now, &found);
    }
}

/// Takes `terminal`'s screen: the alternate screen, cleared, with the cursor hidden and the wheel's
/// ↑ and ↓ turned off ([`AlternateScroll`]). Nothing here asks the terminal anything.
fn enter_to(terminal: &mut impl io::Write) -> io::Result<()> {
    execute!(
        terminal,
        EnterAlternateScreen,
        Clear(ClearType::All),
        Hide,
        AlternateScroll(false)
    )
}

/// The terminal's alternate-scroll mode (xterm's 1007, which VTE, macOS's Terminal, iTerm2 and
/// Windows Terminal follow): on the alternate screen of a program that doesn't take the mouse, as
/// the console doesn't so that a host can select a link, it turns the wheel into ↑ and ↓. Those
/// walk the console's command history, and what a scrolled-back host typed next was added to a
/// recalled command (the hosting plan's final review B, Minor 5). Off while the console holds the
/// terminal (DECRST 1007), and on again once it's given back (DECSET 1007).
struct AlternateScroll(bool);

impl crossterm::Command for AlternateScroll {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str(if self.0 {
            "\u{1b}[?1007h"
        } else {
            "\u{1b}[?1007l"
        })
    }

    /// The legacy Windows console, without escape sequences, has no such mode.
    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        give_back();
    }
}

/// Whether the console has the terminal in raw mode, on Windows.
#[cfg(not(unix))]
static RAW: AtomicBool = AtomicBool::new(false);

/// Whether the console has the terminal on its alternate screen, with the cursor hidden.
static ALTERNATE: AtomicBool = AtomicBool::new(false);

/// Gives the terminal back: stderr first, so that what is printed next, a panic's message among
/// it, reaches the terminal; then cooked mode ([`restore_mode`]), which takes no write, so that a
/// terminal that has stopped reading gets it even if what follows waits; then the main screen, a
/// cursor and the wheel's alternate scroll ([`AlternateScroll`]). Each is given back once, so
/// running it again, as a panic's hook and then the console's end do, changes nothing: a second
/// switch to the main screen would move the cursor back to where it was when the console began,
/// and what's printed next would overwrite the panic's message.
fn give_back() {
    give_back_to(&mut io::stdout());
}

/// [`give_back`], writing the switch to the main screen to `terminal`.
fn give_back_to(terminal: &mut impl io::Write) {
    loud_stderr();
    restore_mode();
    if ALTERNATE.swap(false, Ordering::SeqCst) {
        let _ = execute!(terminal, LeaveAlternateScreen, Show, AlternateScroll(true));
    }
}

/// Where stderr pointed before the console took the terminal, while it holds it.
#[cfg(unix)]
static STDERR: std::sync::Mutex<Option<std::os::fd::OwnedFd>> = std::sync::Mutex::new(None);

/// Points stderr at `/dev/null` while the console holds the terminal, if stderr is that terminal:
/// a line a library prints there, such as the log file's writer's when a day's file can't be made,
/// would draw over the console. Whatever the console's process logs reaches its pane through the
/// log instead. A stderr the host sent elsewhere (`2>file`) is left there.
#[cfg(unix)]
fn quiet_stderr() {
    use std::io::IsTerminal;
    quiet_stderr_when(io::stderr().is_terminal());
}

/// [`quiet_stderr`], `terminal` saying whether stderr is a terminal. Where stderr can't be pointed
/// away, it stays as it is.
#[cfg(unix)]
fn quiet_stderr_when(terminal: bool) {
    use std::os::fd::AsFd;
    if !terminal {
        return;
    }
    let Ok(null) = std::fs::OpenOptions::new().write(true).open("/dev/null") else {
        return;
    };
    let mut saved = STDERR.lock().unwrap_or_else(|e| e.into_inner());
    if saved.is_some() {
        return;
    }
    let Ok(stderr) = rustix::io::fcntl_dupfd_cloexec(io::stderr().as_fd(), 0) else {
        return;
    };
    if rustix::stdio::dup2_stderr(&null).is_ok() {
        *saved = Some(stderr);
    }
}

/// Points stderr back where it was before [`quiet_stderr`].
#[cfg(unix)]
fn loud_stderr() {
    let mut saved = match STDERR.try_lock() {
        Ok(saved) => saved,
        Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner(),
        // Being pointed away on another thread, which a panic interrupted: nothing to give back.
        Err(std::sync::TryLockError::WouldBlock) => return,
    };
    if let Some(stderr) = saved.take() {
        let _ = rustix::stdio::dup2_stderr(&stderr);
    }
}

#[cfg(not(unix))]
fn quiet_stderr() {}

#[cfg(not(unix))]
fn loud_stderr() {}

type Hook = Box<dyn Fn(&PanicHookInfo<'_>) + Sync + Send + 'static>;

/// A panic hook that runs `give_back` before the hook it replaced prints the panic, so the message
/// lands on the main screen, readable, once a frame another thread is drawing is done: for the
/// console's own thread's panics, which end it. A panic on another thread goes to the log it's
/// given ([`PanicLog`]) instead, and the console runs on, unless the log turns it away. Each panic
/// goes on to the hook it replaced, which comes back when this is dropped.
struct PanicHook {
    previous: Option<Arc<Hook>>,
}

/// Where a panic on a thread other than the console's goes, given the panic's text: true once it's
/// logged, and false when it can't be, as when the panic is the log's own code's, whose locks it
/// may hold. The server passes its log, which cleans what it logs, as every line (the hosting
/// plan's final review B, Minor 6).
pub type PanicLog = fn(&str) -> bool;

/// This thread marked as the console's ([`CONSOLE_HERE`]) until dropped.
struct ConsoleHere {
    was: bool,
}

impl ConsoleHere {
    fn mark() -> ConsoleHere {
        ConsoleHere {
            was: CONSOLE_HERE.replace(true),
        }
    }
}

impl Drop for ConsoleHere {
    fn drop(&mut self) {
        CONSOLE_HERE.set(self.was);
    }
}

/// A panic's text, as one line: where, and what it said. Not the thread's name: a hook that
/// panicked would abort the process, and `std::thread::current` can, as a thread ends.
fn panic_text(info: &PanicHookInfo<'_>) -> String {
    let said = info.payload_as_str().unwrap_or("Box<dyn Any>");
    match info.location() {
        Some(at) => format!("a thread panicked at {at}: {said}"),
        None => format!("a thread panicked: {said}"),
    }
}

impl PanicHook {
    /// The hook, `elsewhere` taking the panics of threads other than the console's, if it's given.
    fn install(give_back: fn(), elsewhere: Option<PanicLog>) -> PanicHook {
        let previous = Arc::new(panic::take_hook());
        let chained = Arc::clone(&previous);
        panic::set_hook(Box::new(move |info| {
            // A thread that can no longer say is taken for the console's: its ending is the safe one.
            let here = CONSOLE_HERE.try_with(Cell::get).unwrap_or(true);
            let logged = !here && elsewhere.is_some_and(|log| log(&panic_text(info)));
            if !logged {
                PANICKED.store(true, Ordering::SeqCst);
                wait_for_the_frame();
                give_back();
            }
            (*chained)(info);
        }));
        PanicHook {
            previous: Some(previous),
        }
    }
}

impl Drop for PanicHook {
    fn drop(&mut self) {
        // A panicking thread can't change the hook, so ours stays when the console unwinds. The
        // process may run on after that, as the server catches its console's panic: harmless, as
        // `give_back` does nothing once the terminal is given back, and the hook hands every
        // panic on to the one it replaced.
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

    /// Held by each test that changes what the process shares: its panic hook, its stderr, and the
    /// flags of what the console holds of the terminal. One test at a time.
    static HOOKS: Mutex<()> = Mutex::new(());

    #[test]
    fn the_terminal_is_given_back_once() {
        // Found by the M4 hosting plan's Task 10's by-hand check: after a panic, the hook gives
        // the terminal back, then the console's end does. A second switch to the main screen moves
        // the cursor back to where the console began (xterm, VTE and tmux restore the cursor saved
        // as it began at every switch), and the console's note then overwrote the panic's message.
        let _hooks = HOOKS.lock().unwrap_or_else(|e| e.into_inner());
        ALTERNATE.store(true, Ordering::SeqCst);
        let mut first = Vec::new();
        give_back_to(&mut first);
        let first = String::from_utf8(first).unwrap();
        assert!(
            first.contains("\u{1b}[?1049l") && first.contains("\u{1b}[?25h"),
            "{first:?}"
        );
        let mut again = Vec::new();
        give_back_to(&mut again);
        assert!(again.is_empty(), "{again:?}");
    }

    #[test]
    fn the_wheel_scrolls_no_commands_while_the_console_holds_the_terminal() {
        // The hosting plan's final review B, Minor 5: on the alternate screen, terminals turn the
        // wheel into ↑ and ↓ (alternate scroll, 1007), which walked a scrolled-back host's command
        // history, and what they typed next was added to a recalled command. It's off while the
        // console holds the terminal, and on again once it's given back, the panic hook's way
        // included, as that gives it back here too.
        let _hooks = HOOKS.lock().unwrap_or_else(|e| e.into_inner());
        let mut entered = Vec::new();
        enter_to(&mut entered).unwrap();
        let entered = String::from_utf8(entered).unwrap();
        let (screen, wheel) = (entered.find("\u{1b}[?1049h"), entered.find("\u{1b}[?1007l"));
        assert!(screen.is_some() && wheel > screen, "{entered:?}");
        ALTERNATE.store(true, Ordering::SeqCst);
        let mut given = Vec::new();
        give_back_to(&mut given);
        let given = String::from_utf8(given).unwrap();
        let (screen, wheel) = (given.find("\u{1b}[?1049l"), given.find("\u{1b}[?1007h"));
        assert!(screen.is_some() && wheel > screen, "{given:?}");
    }

    /// A pseudo-terminal: its master end, and the end a program has, never the test's controlling
    /// terminal.
    #[cfg(unix)]
    fn pty() -> (std::os::fd::OwnedFd, std::fs::File) {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::OpenOptionsExt;

        use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
        let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
        grantpt(&master).unwrap();
        unlockpt(&master).unwrap();
        let name = ptsname(&master, Vec::new()).unwrap();
        let terminal = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(rustix::fs::OFlags::NOCTTY.bits() as i32)
            .open(std::ffi::OsStr::from_bytes(name.as_bytes()))
            .unwrap();
        (master, terminal)
    }

    /// Whether `terminal` is in its usual, cooked mode: lines, echo and signals from keys.
    #[cfg(unix)]
    fn cooked(terminal: &std::fs::File) -> bool {
        use rustix::termios::LocalModes;
        let modes = rustix::termios::tcgetattr(terminal).unwrap().local_modes;
        modes.contains(LocalModes::ICANON | LocalModes::ECHO | LocalModes::ISIG)
    }

    #[cfg(unix)]
    #[test]
    fn the_mode_comes_back_from_any_thread_without_a_write_to_the_terminal() {
        // #359: a console `run` leaves behind, its thread caught in a write to a terminal that
        // has stopped reading, left that terminal raw. Its mode comes back with tcsetattr alone,
        // which neither writes to the terminal nor reads it, so it never waits on one like that.
        let _hooks = HOOKS.lock().unwrap_or_else(|e| e.into_inner());
        let (_master, terminal) = pty();
        raw_mode_of(&terminal).unwrap();
        assert!(!cooked(&terminal), "raw");
        // A terminal that takes no more output, as over a stalled SSH link: a write would wait.
        rustix::termios::tcflow(&terminal, rustix::termios::Action::OOff).unwrap();
        let (done, given_back) = std::sync::mpsc::channel();
        let restoring = std::thread::spawn(move || {
            restore_mode_of(&terminal);
            done.send(()).unwrap();
            terminal
        });
        given_back
            .recv_timeout(Duration::from_secs(5))
            .expect("it waited on the terminal");
        let terminal = restoring.join().unwrap();
        assert!(cooked(&terminal), "the mode wasn't given back");
        // Once: done again, it does nothing.
        raw_mode_of(&terminal).unwrap();
        restore_mode_of(&terminal);
        let mut raw = rustix::termios::tcgetattr(&terminal).unwrap();
        raw.make_raw();
        let now = rustix::termios::OptionalActions::Now;
        rustix::termios::tcsetattr(&terminal, now, &raw).unwrap();
        restore_mode_of(&terminal);
        assert!(!cooked(&terminal), "given back twice");
    }

    #[test]
    fn the_screen_is_taken_cleared_and_the_terminal_is_asked_nothing() {
        // #359: ratatui's Terminal::clear asked where the cursor was, and waited up to 2 s for an
        // answer some terminals never give. The screen is cleared as it's taken instead.
        let mut entered = Vec::new();
        enter_to(&mut entered).unwrap();
        let entered = String::from_utf8(entered).unwrap();
        let (screen, cleared) = (entered.find("\u{1b}[?1049h"), entered.find("\u{1b}[2J"));
        assert!(screen.is_some() && cleared > screen, "{entered:?}");
        assert!(!entered.contains("\u{1b}[6n"), "{entered:?}");
    }

    /// The hook the tests had, back in place of `before`'s replacement.
    fn restore(before: Arc<Hook>) {
        drop(panic::take_hook());
        match Arc::try_unwrap(before) {
            Ok(hook) => panic::set_hook(hook),
            Err(shared) => panic::set_hook(Box::new(move |info| (*shared)(info))),
        }
    }

    #[test]
    fn a_panic_elsewhere_that_ends_the_console_waits_for_the_frame_being_drawn() {
        // Review Focus 2, as the by-hand check of the M4 hosting plan's Task 10 found it: a panic
        // on another thread while the console draws a frame, here with no log to take it, as an
        // attached console has none. Given back at once, the terminal would get the rest of the
        // frame on its main screen, under the panic's message.
        let _hooks = HOOKS.lock().unwrap_or_else(|e| e.into_inner());
        PANICKED.store(false, Ordering::SeqCst);
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
            let give_back = || {
                if on_hook_test_thread() {
                    SEEN.lock().unwrap().push("the terminal given back");
                }
            };
            let _hook = PanicHook::install(give_back, None);
            let drawing = Drawing::begin().expect("no panic yet");
            let panicking = std::thread::Builder::new()
                .name(HOOK_TEST.into())
                .spawn(|| panic!("while a frame is drawn"))
                .unwrap();
            std::thread::sleep(Duration::from_millis(200));
            SEEN.lock().unwrap().push("the frame drawn");
            drop(drawing);
            assert!(panicking.join().is_err());
            assert!(Drawing::begin().is_none(), "no frame after the panic");
        }
        PANICKED.store(false, Ordering::SeqCst);
        restore(before);
        assert_eq!(
            *SEEN.lock().unwrap(),
            [
                "the frame drawn",
                "the terminal given back",
                "the previous hook"
            ]
        );
    }

    /// What the hook tests saw, in order, of their own threads' panics.
    static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

    fn saw(what: impl Into<String>) {
        SEEN.lock().unwrap().push(what.into());
    }

    /// Runs `test` with a hook of its own as the one the console's replaces, which notes "the
    /// previous hook" for the hook test's threads, and hands every other thread's panic on to the
    /// hook that was there: what the test saw, in order.
    fn with_previous_hook(test: impl FnOnce()) -> Vec<String> {
        SEEN.lock().unwrap().clear();
        let before: Arc<Hook> = Arc::new(panic::take_hook());
        let others = Arc::clone(&before);
        panic::set_hook(Box::new(move |info| {
            if on_hook_test_thread() {
                saw("the previous hook");
            } else {
                (*others)(info);
            }
        }));
        test();
        PANICKED.store(false, Ordering::SeqCst);
        restore(before);
        std::mem::take(&mut *SEEN.lock().unwrap())
    }

    /// The console's `give_back` for the hook tests: notes it for their threads.
    fn noted_give_back() {
        if on_hook_test_thread() {
            saw("the terminal given back");
        }
    }

    /// A log that takes every panic, and notes the hook test's.
    fn noted_log(text: &str) -> bool {
        if on_hook_test_thread() {
            saw(format!("logged: {text}"));
        }
        true
    }

    #[test]
    fn a_panic_on_another_thread_goes_to_the_log_and_the_console_runs_on() {
        // The hosting plan's final review B, Minor 6: a panic on any thread ended the console. One
        // on another of the server's threads goes to the log now, which cleans it, the terminal
        // stays the console's, and the hook it replaced still hears of it.
        let _hooks = HOOKS.lock().unwrap_or_else(|e| e.into_inner());
        PANICKED.store(false, Ordering::SeqCst);
        let seen = with_previous_hook(|| {
            let _hook = PanicHook::install(noted_give_back, Some(noted_log));
            assert!(panics_on_hook_test_thread("in a task of the server's"));
            assert!(Drawing::begin().is_some(), "the console draws on");
        });
        assert_eq!(seen.len(), 2, "{seen:?}");
        let logged = &seen[0];
        assert!(
            logged.starts_with("logged: a thread panicked at crates/fenji-console/src/tui.rs:")
                && logged.ends_with(": in a task of the server's"),
            "{logged}"
        );
        assert_eq!(seen[1], "the previous hook");
    }

    #[test]
    fn a_panic_on_the_consoles_own_thread_gives_the_terminal_back() {
        // The console's own panics end it as before: the terminal back first, then the message.
        let _hooks = HOOKS.lock().unwrap_or_else(|e| e.into_inner());
        let seen = with_previous_hook(|| {
            let _hook = PanicHook::install(noted_give_back, Some(noted_log));
            let panicked = std::thread::Builder::new()
                .name(HOOK_TEST.into())
                .spawn(|| {
                    let _here = ConsoleHere::mark();
                    panic!("in the console's own code")
                })
                .unwrap()
                .join();
            assert!(panicked.is_err());
            assert!(Drawing::begin().is_none(), "no frame after the panic");
        });
        assert_eq!(seen, ["the terminal given back", "the previous hook"]);
    }

    #[test]
    fn a_panic_the_log_turns_away_ends_the_console_as_before() {
        // A panic in the log's own code may hold its locks: the log turns it away, and the console
        // ends as it did, the terminal given back for the message.
        let _hooks = HOOKS.lock().unwrap_or_else(|e| e.into_inner());
        fn turned_away(_: &str) -> bool {
            if on_hook_test_thread() {
                saw("turned away");
            }
            false
        }
        let seen = with_previous_hook(|| {
            let _hook = PanicHook::install(noted_give_back, Some(turned_away));
            assert!(panics_on_hook_test_thread("in the log"));
            assert!(Drawing::begin().is_none(), "no frame after the panic");
        });
        assert_eq!(
            seen,
            [
                "turned away",
                "the terminal given back",
                "the previous hook"
            ]
        );
    }

    #[test]
    fn a_panic_gives_the_terminal_back_before_the_previous_hook_prints() {
        // Review Focus 2.
        let _hooks = HOOKS.lock().unwrap_or_else(|e| e.into_inner());
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
            let give_back = || {
                if on_hook_test_thread() {
                    SEEN.lock().unwrap().push("the terminal given back");
                }
            };
            let _hook = PanicHook::install(give_back, None);
            assert!(panics_on_hook_test_thread("in the console"));
        }
        // Gone with its guard: the previous hook alone.
        assert!(panics_on_hook_test_thread("after the console"));
        PANICKED.store(false, Ordering::SeqCst);
        restore(before);
        assert_eq!(
            *SEEN.lock().unwrap(),
            [
                "the terminal given back",
                "the previous hook",
                "the previous hook"
            ]
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn stderr_points_away_while_the_console_holds_the_terminal_and_back_after() {
        // Carry-forward 4 of the M4 hosting plan's Task 10: tracing-appender prints to stderr when
        // a day's log file can't be made, which would draw over the console.
        let _hooks = HOOKS.lock().unwrap_or_else(|e| e.into_inner());
        let stderr = || std::fs::read_link("/proc/self/fd/2").unwrap();
        let before = stderr();
        quiet_stderr_when(true);
        let during = stderr();
        // Twice is harmless, either way.
        quiet_stderr_when(true);
        loud_stderr();
        let after = stderr();
        loud_stderr();
        assert_eq!(during, std::path::Path::new("/dev/null"));
        assert_eq!(after, before);
        assert_eq!(stderr(), before);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_stderr_the_host_sent_elsewhere_is_left_there() {
        // Review 1, Minor 6: a host's own `2>file`, or a pipe, keeps getting what's written to
        // stderr; only a terminal's is pointed away.
        let _hooks = HOOKS.lock().unwrap_or_else(|e| e.into_inner());
        let stderr = || std::fs::read_link("/proc/self/fd/2").unwrap();
        let before = stderr();
        quiet_stderr_when(false);
        assert_eq!(stderr(), before);
        loud_stderr();
        assert_eq!(stderr(), before);
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
