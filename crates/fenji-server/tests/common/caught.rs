//! The log a test catches: through the process's own subscriber, set once, which asks at every
//! event and takes the events of the threads catching them, writing nothing for any other. A
//! thread's own default subscriber can miss a line: while one dispatcher alone is registered,
//! tracing keeps whether a callsite is wanted as the first thread to log there found it, and
//! another test's thread, with no subscriber, can make it unwanted everywhere (the hosting plan's
//! final review A, Minor 8; #359). As `fenji_server`'s own tests catch it.
//!
//! A test on tokio's current-thread runtime, as `#[tokio::test]` gives it, catches what its server
//! and its launcher log on its thread; not their other threads', such as the world's.
use std::cell::{Cell, RefCell};
use std::io;
use std::marker::PhantomData;
use std::sync::Once;

use fenji_server::console::log::{LogHub, LogLayer};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::filter;
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

thread_local! {
    /// What this thread's events are written to, as text, while it catches them so.
    static TEXT: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
    /// The level this thread's events are caught at as text.
    static TEXT_LEVEL: Cell<Option<Level>> = const { Cell::new(None) };
    /// The hub this thread's events at info and above go to, while it gives them to one.
    static HUB: RefCell<Option<LogLayer>> = const { RefCell::new(None) };
}

/// This thread's events at `level` and above, as the server's log shows them, until the guard is
/// dropped.
pub fn as_text(level: Level) -> Caught {
    subscribed();
    TEXT.with_borrow_mut(|text| *text = Some(Vec::new()));
    TEXT_LEVEL.set(Some(level));
    Caught {
        what: What::Text,
        _here: PhantomData,
    }
}

/// This thread's events at info and above, given to `hub` as the server's log gives it each line,
/// until the guard is dropped.
pub fn to_hub(hub: &LogHub) -> Caught {
    subscribed();
    HUB.with_borrow_mut(|layer| *layer = Some(hub.layer()));
    Caught {
        what: What::Hub,
        _here: PhantomData,
    }
}

/// This thread catching its events, until dropped, on this thread.
pub struct Caught {
    what: What,
    /// Neither `Send` nor `Sync`: what it catches is its thread's.
    _here: PhantomData<*const ()>,
}

/// How a thread catches its events.
enum What {
    Text,
    Hub,
}

impl Caught {
    /// What this thread has logged as text so far.
    pub fn text(&self) -> String {
        TEXT.with_borrow(|text| String::from_utf8_lossy(text.as_deref().unwrap_or_default()).into())
    }
}

impl Drop for Caught {
    fn drop(&mut self) {
        match self.what {
            What::Text => {
                TEXT_LEVEL.set(None);
                TEXT.with_borrow_mut(|text| *text = None);
            }
            What::Hub => HUB.with_borrow_mut(|layer| *layer = None),
        }
    }
}

/// Sets the process's subscriber, the first time.
fn subscribed() {
    static SET: Once = Once::new();
    SET.call_once(|| {
        // More verbose levels are greater: TRACE > DEBUG > INFO.
        let as_text = filter::dynamic_filter_fn(|meta, _| {
            TEXT_LEVEL.get().is_some_and(|level| *meta.level() <= level)
        });
        let text = tracing_subscriber::fmt::layer()
            .with_writer(|| ThisThread)
            .with_ansi(false)
            .with_filter(as_text);
        let to_hub = filter::dynamic_filter_fn(|meta, _| {
            *meta.level() <= Level::INFO && HUB.with_borrow(Option::is_some)
        });
        let hub = ToHub.with_filter(to_hub);
        let subscriber = tracing_subscriber::registry().with(text).with(hub);
        tracing::subscriber::set_global_default(subscriber)
            .expect("no other test sets the process's subscriber");
    });
    // A callsite first seen between the subscriber's registration and its setting is wanted.
    tracing::callsite::rebuild_interest_cache();
}

/// Where this thread's events are written as text.
struct ThisThread;

impl io::Write for ThisThread {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        TEXT.with_borrow_mut(|text| {
            if let Some(text) = text {
                text.extend_from_slice(buf);
            }
        });
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Gives an event to the hub of the thread that logged it.
struct ToHub;

impl<S: Subscriber> Layer<S> for ToHub {
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        HUB.with_borrow(|hub| {
            if let Some(layer) = hub {
                layer.on_event(event, ctx);
            }
        });
    }
}
