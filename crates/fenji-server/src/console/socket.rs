//! The console socket (M4 hosting spec §3.5), on Linux and macOS: `console.sock` in the data
//! folder, owner-only, which `fenji-server console` attaches to.
//! - [`claim`], first thing in `fenji-server run`, keeps one server to a data folder. It takes
//!   the folder's lock ([`crate::lock`]) and refuses if another server holds it or answers on the
//!   socket. It replaces a stale socket with its own, never more open than 0600. A folder too deep
//!   for a socket's path keeps its server, which runs without one and says why.
//! - [`serve`] gives each console, four at once, the hello and then [`service::connect`]'s feed as
//!   JSON lines, each within the wire's limit ([`wire::encode_server`]), and runs its numbered
//!   commands. A console that stops reading holds up nothing: its socket and its queue fill, and
//!   it loses log lines, as the log hub has it, and is told how many once it reads again. Stalled,
//!   it keeps its place, one of the four, as any attached console does until it goes: four
//!   stalled consoles, such as over frozen SSH links, turn the next away until one of them goes
//!   (the hosting plan's Task 11; #359).
use std::fs::{DirBuilder, File, Permissions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, bail};
use fenji_console::text::clean;
use fenji_console::wire::{self, ConsoleMsg, LINE_MAX, ServerMsg};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};
use tokio::task::JoinSet;

use crate::console::log::LogHub;
use crate::console::service::{self, ConsoleService};
use crate::lock::{self, Lock};

/// The console socket's name in the data folder.
pub const NAME: &str = "console.sock";

/// Where a socket is made before it's moved into place: a folder only this user may enter. With
/// the socket's name, [`IN_NEST`], in it, its path is shorter than [`NAME`]'s, so it fits wherever
/// that does.
const NEST: &str = ".sock.new";

/// The socket's name in [`NEST`].
const IN_NEST: &str = "s";

/// The most consoles attached at once.
pub const CONSOLES: usize = 4;

/// Why a console beyond [`CONSOLES`] is closed (M4 hosting plan, Task 11's value).
const TOO_MANY: &str = "too many consoles (4)";

/// Why a console that sends a line over the wire's limit is closed (Task 11's value).
const LINE_TOO_LONG: &str = "line too long";

/// The refusals of a console's lines that may wait for its writer, beside its feed.
const REFUSALS: usize = 16;

/// How long the server's end waits for its consoles to take their goodbye: one that has stopped
/// reading holds it up no longer.
const GOODBYE_WAIT: Duration = Duration::from_secs(5);

/// How long a console turned away has to take its goodbye.
const TURN_AWAY_WAIT: Duration = Duration::from_secs(1);

/// How long the socket rests after it failed to take a console, so that a lasting failure, such
/// as no file descriptors left, doesn't spin.
const ACCEPT_REST: Duration = Duration::from_millis(100);

/// The longest path a socket can have, in bytes: `sun_path`'s 108 on Linux, 104 on macOS and the
/// BSDs, less its closing NUL.
#[cfg(target_os = "linux")]
const PATH_MAX: usize = 107;
#[cfg(not(target_os = "linux"))]
const PATH_MAX: usize = 103;

/// The console socket of the data folder `data`.
pub fn path(data: &Path) -> PathBuf {
    data.join(NAME)
}

/// A server's claim on its data folder: the folder's lock, and the console socket unless it
/// couldn't be made. Dropped, it removes the socket, then lets go of the lock.
#[derive(Debug)]
pub struct Claim {
    /// Declared before the lock, so dropped before it: no next server sees this one's socket.
    socket: Option<Bound>,
    unattachable: Option<String>,
    _lock: Lock,
}

/// The console socket, bound at `path`; dropped, it's removed.
#[derive(Debug)]
struct Bound {
    listener: UnixListener,
    path: PathBuf,
}

impl Drop for Bound {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Claim {
    /// Why consoles can't attach to this server, when it has no socket.
    pub fn unattachable(&self) -> Option<&str> {
        self.unattachable.as_deref()
    }
}

/// Claims the data folder `data` for this process, before the server opens its store or listens
/// (Review Focus 5), making the folder owner-only if it's missing:
/// - another server holds its lock, or answers on its socket: `a server is already using this
///   data folder (<path>)`;
/// - a socket nobody answers on is stale, and is replaced;
/// - the new socket is bound 0600 from the start.
///
/// A socket that can't be made, such as one whose path is too long, leaves the server
/// unattachable ([`Claim::unattachable`]), still one to its data folder. `given` is the `--config`
/// this run was given, which the command a refusal suggests repeats.
///
/// # Errors
/// If another server uses the folder, or the folder or its lock can't be made or taken.
pub async fn claim(data: &Path, given: Option<&Path>) -> anyhow::Result<Claim> {
    let lock = lock::take(data, given)?;
    let path = path(data);
    let unattachable = |why: String| {
        Some(format!(
            "`fenji-server console` can't attach to this server: {why}"
        ))
    };
    if let Some(why) = too_long(&path) {
        return Ok(Claim {
            socket: None,
            unattachable: unattachable(why),
            _lock: lock,
        });
    }
    // A server without the lock: one whose lock file was deleted from under it.
    if answers(&path).await? {
        bail!(lock::in_use(data, given));
    }
    let (socket, unattachable) = match bind(data, &path) {
        Ok(listener) => (Some(Bound { listener, path }), None),
        Err(e) => (
            None,
            unattachable(format!("can't make its socket {}: {e}", path.display())),
        ),
    };
    Ok(Claim {
        socket,
        unattachable,
        _lock: lock,
    })
}

/// Why a socket can't be at `path`, if the path is too long for one.
pub(crate) fn too_long(path: &Path) -> Option<String> {
    let len = path.as_os_str().len();
    (len > PATH_MAX).then(|| {
        format!(
            "the path of its console socket, {}, is too long for a socket here ({len} bytes, of \
             {PATH_MAX} at most); a data folder with a shorter path lets consoles attach",
            path.display()
        )
    })
}

/// Whether a server answers on the socket at `path`. Nobody does where there's no socket, a stale
/// one, or something else in its place.
async fn answers(path: &Path) -> anyhow::Result<bool> {
    match UnixStream::connect(path).await {
        Ok(_) => Ok(true),
        // A server whose queue of consoles is full.
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => Err(e)
            .with_context(|| format!("can't tell whether a server answers on {}", path.display())),
        Err(_) => Ok(false),
    }
}

/// A listener at `path`, never more open than 0600: it's bound in [`NEST`], a folder only this
/// user may enter, set to 0600 there, then moved over whatever was at `path`.
fn bind(data: &Path, path: &Path) -> io::Result<UnixListener> {
    bind_with(data, path, |_| {})
}

/// [`bind`], with `made` called on the nest once it's made, before anything goes through it: a
/// test swaps a link in for it there. The nest is held open from then on, and its mode, the
/// socket's and the move into place go through it, so that none of them follows a link swapped in
/// for it (the hosting plan's final review, Minor 1).
fn bind_with(data: &Path, path: &Path, made: impl FnOnce(&Path)) -> io::Result<UnixListener> {
    use rustix::fs::{AtFlags, CWD, Mode};
    let nest = data.join(NEST);
    // Left by a server that ended as it bound: the lock says no other is here. A link in its place
    // is removed, never followed.
    match std::fs::remove_dir_all(&nest) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    DirBuilder::new().mode(0o700).create(&nest)?;
    made(&nest);
    let bound = (|| -> io::Result<UnixListener> {
        let folder = own_folder(&nest)?;
        // Whatever the umask left of it.
        folder.set_permissions(Permissions::from_mode(0o700))?;
        let listener = UnixListener::bind(nest.join(IN_NEST))?;
        rustix::fs::chmodat(&folder, IN_NEST, Mode::RUSR | Mode::WUSR, AtFlags::empty())?;
        rustix::fs::renameat(&folder, IN_NEST, CWD, path)?;
        Ok(listener)
    })();
    let _ = std::fs::remove_dir_all(&nest);
    bound
}

/// The folder at `dir`, held open: never through a link in its place, and only as this user's.
fn own_folder(dir: &Path) -> io::Result<File> {
    use std::os::unix::fs::MetadataExt;

    use rustix::fs::{Mode, OFlags};
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let folder = File::from(rustix::fs::open(dir, flags, Mode::empty())?);
    let meta = folder.metadata()?;
    if !meta.is_dir() || meta.uid() != rustix::process::geteuid().as_raw() {
        return Err(io::Error::other(format!(
            "{} isn't the folder this server made",
            dir.display()
        )));
    }
    Ok(folder)
}

/// Serves the claim's console socket until the server has stopped, then gives its consoles
/// [`GOODBYE_WAIT`] at most to take their goodbye, and lets go of the claim. Without a socket, it
/// holds the claim, the data folder's lock, until the server has stopped.
pub async fn serve(claim: Claim, service: Arc<ConsoleService>, hub: LogHub) {
    let mut stopped = service.stopped();
    let Some(socket) = &claim.socket else {
        let _ = stopped.wait_for(|stopped| *stopped).await;
        return;
    };
    let slots = Arc::new(Semaphore::new(CONSOLES));
    let mut consoles = JoinSet::new();
    loop {
        let failed = tokio::select! {
            accepted = socket.listener.accept() => match accepted {
                Ok((stream, _)) => {
                    match Arc::clone(&slots).try_acquire_owned() {
                        Ok(slot) => {
                            let service = Arc::clone(&service);
                            consoles.spawn(attached(stream, service, hub.clone(), slot));
                        }
                        Err(_) => {
                            tracing::warn!("a console was turned away: {TOO_MANY}");
                            consoles.spawn(turn_away(stream));
                        }
                    }
                    false
                }
                Err(e) => {
                    tracing::warn!(error = %e, "the console socket couldn't take a console");
                    true
                }
            },
            _ = stopped.wait_for(|stopped| *stopped) => break,
            Some(_) = consoles.join_next(), if !consoles.is_empty() => false,
        };
        if failed {
            tokio::time::sleep(ACCEPT_REST).await;
        }
    }
    let goodbyes = async { while consoles.join_next().await.is_some() {} };
    let _ = tokio::time::timeout(GOODBYE_WAIT, goodbyes).await;
}

/// Tells a console beyond [`CONSOLES`] why, then closes it.
async fn turn_away(mut stream: UnixStream) {
    let bye = ServerMsg::Bye {
        reason: TOO_MANY.into(),
    };
    let line = format!("{}\n", wire::encode(&bye));
    let _ = tokio::time::timeout(TURN_AWAY_WAIT, stream.write_all(line.as_bytes())).await;
}

/// One console, holding its `slot`: the hello, then its feed, while its commands go to the
/// service. It ends when the console goes, when the server says goodbye, or when it's closed for a
/// line too long.
async fn attached(
    stream: UnixStream,
    service: Arc<ConsoleService>,
    hub: LogHub,
    _slot: OwnedSemaphorePermit,
) {
    let hello = service.hello();
    let (feed, commands) = service::connect(&service, &hub);
    let (refuse, refusals) = mpsc::channel(REFUSALS);
    let (read, write) = stream.into_split();
    let mut reads = pin!(read_commands(read, commands, refuse));
    let mut writing = pin!(write_feed(write, hello, feed, refusals));
    let mut listening = true;
    loop {
        tokio::select! {
            reading = &mut reads, if listening => match reading {
                // Its place is free at once, though nothing is written that would find it gone.
                Reading::Gone => return,
                // It still gets its answers and the feed, or its goodbye.
                Reading::Shut | Reading::Refused => listening = false,
            },
            () = &mut writing => return,
        }
    }
}

/// How a console's reading ended.
enum Reading {
    /// Its socket is closed both ways, as when its process ends: it has gone.
    Gone,
    /// It sends no more, its side of the socket shut, but still reads.
    Shut,
    /// It sent a line too long, and is told goodbye.
    Refused,
}

/// The console's lines, its commands for the service, until it goes, shuts its side, or sends a
/// line too long. A line that isn't a message is refused if it has an id, and let go otherwise.
async fn read_commands(
    read: OwnedReadHalf,
    commands: mpsc::Sender<ConsoleMsg>,
    refuse: mpsc::Sender<ServerMsg>,
) -> Reading {
    let mut reader = BufReader::new(read);
    let mut buf = Vec::new();
    loop {
        let line = match read_line(&mut reader, &mut buf).await {
            Ok(Some(Line::Text(line))) => line,
            Ok(Some(Line::TooLong { .. })) => {
                tracing::warn!("a console was closed: it sent a line over {LINE_MAX} bytes");
                let bye = ServerMsg::Bye {
                    reason: LINE_TOO_LONG.into(),
                };
                let _ = refuse.send(bye).await;
                return Reading::Refused;
            }
            Ok(None) if gone(reader.get_ref()) => return Reading::Gone,
            Ok(None) => return Reading::Shut,
            Err(_) => return Reading::Gone,
        };
        match wire::decode_console(&line) {
            Ok(command) => {
                // The server has stopped: its goodbye is on its way.
                if commands.send(command).await.is_err() {
                    return Reading::Shut;
                }
            }
            Err(e) => {
                let Some(id) = id_of(&line) else { continue };
                let refused = ServerMsg::Answer {
                    id,
                    ok: false,
                    lines: vec![clean(&e.to_string()).into_owned()],
                    more: false,
                };
                if refuse.send(refused).await.is_err() {
                    return Reading::Gone;
                }
            }
        }
    }
}

/// Whether the console at the other end of `read`, whose side is shut, has closed the socket both
/// ways. A write of nothing finds out without writing: it fails once the other end is closed.
fn gone(read: &OwnedReadHalf) -> bool {
    let stream: &UnixStream = read.as_ref();
    matches!(stream.try_write(&[]), Err(e) if e.kind() != io::ErrorKind::WouldBlock)
}

/// The number a line that isn't a message gives as its `id`, if any.
fn id_of(line: &str) -> Option<u64> {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()?
        .get("id")?
        .as_u64()
}

/// Writes the hello, then the feed and the refusals of the console's lines as they come, until a
/// goodbye, the feed's end, or a console that takes no more.
async fn write_feed(
    mut write: OwnedWriteHalf,
    hello: ServerMsg,
    mut feed: mpsc::Receiver<ServerMsg>,
    mut refusals: mpsc::Receiver<ServerMsg>,
) {
    if send(&mut write, hello).await.is_err() {
        return;
    }
    let mut refusing = true;
    loop {
        let msg = tokio::select! {
            msg = refusals.recv(), if refusing => match msg {
                Some(msg) => msg,
                None => {
                    refusing = false;
                    continue;
                }
            },
            msg = feed.recv() => match msg {
                Some(msg) => msg,
                None => return,
            },
        };
        let bye = matches!(msg, ServerMsg::Bye { .. });
        if send(&mut write, msg).await.is_err() || bye {
            return;
        }
    }
}

/// `msg` on the socket, as the lines [`wire::encode_server`] makes of it.
async fn send(write: &mut OwnedWriteHalf, msg: ServerMsg) -> io::Result<()> {
    let mut bytes = Vec::new();
    for line in wire::encode_server(msg) {
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\n');
    }
    write.write_all(&bytes).await
}

/// A line read from a console socket.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Line {
    /// Without its line ending.
    Text(String),
    /// Over [`LINE_MAX`] bytes, and let go: `ended` says whether its end was read too.
    TooLong { ended: bool },
}

/// The next line of `reader`, read into `buf`, of which no more than [`LINE_MAX`] bytes and a line
/// ending are read; `None` at the end. A last line without its ending counts.
pub(crate) async fn read_line(
    reader: &mut (impl AsyncBufRead + Unpin),
    buf: &mut Vec<u8>,
) -> io::Result<Option<Line>> {
    const LIMIT: u64 = LINE_MAX as u64 + 2;
    buf.clear();
    let read = (&mut *reader).take(LIMIT).read_until(b'\n', buf).await?;
    if read == 0 {
        return Ok(None);
    }
    let ended = buf.last() == Some(&b'\n');
    if ended {
        buf.pop();
        if buf.last() == Some(&b'\r') {
            buf.pop();
        }
    } else if read as u64 == LIMIT {
        return Ok(Some(Line::TooLong { ended }));
    }
    if buf.len() > LINE_MAX {
        return Ok(Some(Line::TooLong { ended }));
    }
    Ok(Some(Line::Text(String::from_utf8_lossy(buf).into_owned())))
}

/// Reads to the end of the line [`read_line`] let go of, keeping none of it.
pub(crate) async fn rest_of_line(reader: &mut (impl AsyncBufRead + Unpin)) -> io::Result<()> {
    let mut scrap = Vec::new();
    loop {
        scrap.clear();
        let read = (&mut *reader)
            .take(LINE_MAX as u64)
            .read_until(b'\n', &mut scrap)
            .await?;
        if read == 0 || scrap.last() == Some(&b'\n') {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn lines_of(input: &[u8]) -> Vec<Line> {
        let mut reader = BufReader::new(input);
        let mut buf = Vec::new();
        let mut lines = Vec::new();
        while let Some(line) = read_line(&mut reader, &mut buf).await.unwrap() {
            let rest_to_come = matches!(line, Line::TooLong { ended: false });
            lines.push(line);
            if rest_to_come {
                rest_of_line(&mut reader).await.unwrap();
            }
        }
        lines
    }

    #[tokio::test]
    async fn a_link_swapped_in_for_the_nest_is_never_followed() {
        // The hosting plan's final review, Minor 1: the nest's mode, and the socket's, were set
        // through their paths, so a link swapped in for the nest once it was made had its target
        // made 0700. Nothing goes through the link now: the bind fails, and the folder it points
        // to keeps its mode and gets no socket.
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        DirBuilder::new().mode(0o700).create(&data).unwrap();
        let elsewhere = dir.path().join("elsewhere");
        DirBuilder::new().mode(0o755).create(&elsewhere).unwrap();
        let swap = |nest: &Path| {
            std::fs::remove_dir(nest).unwrap();
            std::os::unix::fs::symlink(&elsewhere, nest).unwrap();
        };
        let socket = path(&data);
        assert!(bind_with(&data, &socket, swap).is_err());
        let mode = std::fs::metadata(&elsewhere).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755, "the link's target kept its mode");
        assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
        assert!(!socket.exists());
        assert!(!data.join(NEST).exists(), "the link is gone");
    }

    #[tokio::test]
    async fn a_socket_is_bound_owner_only_whatever_the_umask_left() {
        use std::os::unix::fs::{FileTypeExt, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let socket = path(dir.path());
        let _listener = bind(dir.path(), &socket).unwrap();
        let meta = std::fs::symlink_metadata(&socket).unwrap();
        assert!(meta.file_type().is_socket());
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        assert!(!dir.path().join(NEST).exists(), "the nest is gone");
    }

    #[tokio::test]
    async fn lines_are_read_up_to_the_limit_and_one_over_it_is_let_go() {
        let at_limit = "x".repeat(LINE_MAX);
        let input = format!(
            "a\r\n{at_limit}\r\n{at_limit}y\n{}\nb",
            "z".repeat(3 * LINE_MAX)
        );
        assert_eq!(
            lines_of(input.as_bytes()).await,
            [
                Line::Text("a".into()),
                Line::Text(at_limit),
                Line::TooLong { ended: true },
                Line::TooLong { ended: false },
                // The over-long line's rest is let go, and the last line counts without its end.
                Line::Text("b".into()),
            ]
        );
    }
}
