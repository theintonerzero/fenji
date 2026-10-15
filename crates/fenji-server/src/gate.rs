//! The gate listener (M3 spec D3, D17; docs/protocol/transport.md): the hello, the auth with its
//! deadline and size cap, the ticket, the online registry, then the lobby loop, which hands the
//! connection to the link on 10 (world.md §1). Close reasons use the client's own keys where it has
//! one (docs/protocol/packets/lobby.md §9, world.md §10). The ticket is never logged, and the auth
//! payload's machine bytes and fingerprint are dropped here, never stored.
use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::bail;
use fenji_proto::codec::StoredCodec;
use fenji_proto::handshake::{AuthMessage, Event, Server};
use fenji_proto::hello::{HELLO_LEN, Variants};
use fenji_proto::lobby::NEVER_SEND;
use fenji_proto::opcode::Stc;
use fenji_proto::packet::Up;
use fenji_proto::pickle::{self, Value};
use fenji_proto::variant::AES128;
use fenji_store::{Account, Character};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, watch};
use tokio::task::{JoinError, JoinSet};

use crate::accounts::Accounts;
use crate::clock::Clock;
use crate::config::Config;
use crate::link;
use crate::lobby::{self, LobbyLimits};
use crate::online::{Closing, Join, Online};
use crate::world::{Location, MAX_PLAYER_ID, WorldHandle};

/// The client's close-reason key for a refused login (net.luac's close-reason table).
pub const AUTH_FAILED: &str = "error auth-failed";
/// The client's key for a session ended by a newer login to the same account.
pub const DUPLICATE_LOGIN: &str = "error duplicate-login";
/// The client's key for a server that is stopping: its text says the server is unavailable for now
/// (lobby.md 9.3; world.md 10.3).
pub const SERVER_STOPPING: &str = "error server-not-running";
/// The client's key for an admin's kick: its text says the player is offline and may log in again,
/// accusing them of nothing, unlike `error kick-by-gm` (world.md 10.2–10.3).
pub const KICKED: &str = "error kick-by-user";
/// The client's key for a banned account's connection, in the lobby or the world (M4 hosting spec
/// §8, HA5): an admin's kick's, which the world's ban gives too (`KickReason::Kicked`). The
/// launcher names the ban's reason when the player next signs in.
pub const BANNED: &str = KICKED;
/// The client's close-reason table has no key for a full server, so this shows as "Internal
/// server error: server full" (run m2-r3b showed that form).
pub const SERVER_FULL: &str = "server full";
/// The client's key for the server's own failure, such as a store error while checking a ticket.
pub const SERVER_ERROR: &str = "error server-inner-error";
/// The most bytes taken before the auth message is complete: its 26-byte head (with the inner
/// header) and a payload of at most 0x4000 bytes (transport.md 1.1, §6), plus frame lengths and
/// a few time syncs.
pub const PRE_AUTH_CAP: usize = 26 + 4 + 0x4000 + 512;
/// No frame from the client for this long ends the connection (it sends a time sync every 2 s).
pub const IDLE: Duration = Duration::from_secs(60);
/// How long a refused or replaced connection stays open, by default, for the client to read why.
pub const LINGER: Duration = Duration::from_secs(2);
/// How often, by default, a connection in the world checks that its account still exists.
pub const ACCOUNT_CHECK: Duration = Duration::from_secs(30);

/// The gate's limits (M3 spec D17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GateLimits {
    /// Accounts in the lobby (and later the world) at once.
    pub max_players: usize,
    /// From the hello to a complete auth message.
    pub auth_deadline: Duration,
    /// Connections between their hello and their auth; past this, new ones are dropped unanswered.
    pub max_unauthenticated: usize,
    /// Of those, from one network (an IPv4 address or an IPv6 /64), so one host can't take them all.
    pub max_unauthenticated_per_network: usize,
    /// No frame from a client in the lobby for this long ends the connection (it sends a time
    /// sync every 2 s).
    pub idle: Duration,
    /// A write to a client that takes longer than this ends the connection: a client that stops
    /// reading can't hold its task, its buffers and its place.
    pub write_deadline: Duration,
    /// How long a refused or replaced connection stays open for its client to read why.
    pub linger: Duration,
    /// How often a connection in the world checks that its account still exists (Ruling R5): the
    /// lobby checks at every script message, but a player who only moves sends none.
    pub account_check: Duration,
}

impl Default for GateLimits {
    fn default() -> Self {
        Self {
            max_players: 20,
            auth_deadline: Duration::from_secs(30),
            max_unauthenticated: 64,
            max_unauthenticated_per_network: 8,
            idle: IDLE,
            write_deadline: Duration::from_secs(10),
            linger: LINGER,
            account_check: ACCOUNT_CHECK,
        }
    }
}

impl GateLimits {
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            max_players: usize::try_from(cfg.server.max_players).unwrap_or(usize::MAX),
            ..Self::default()
        }
    }

    /// How long a stopping server gives its connections to end (#219): a player in the world
    /// writes its close within the write deadline, then lingers for its client to read it, and a
    /// second more covers the world's last tick (`tick_hz` is at least 1). Connections left after
    /// it are ended.
    pub fn grace(&self) -> Duration {
        self.write_deadline + self.linger + Duration::from_secs(1)
    }
}

/// What every connection shares.
pub(crate) struct Gate {
    pub accounts: Arc<Accounts>,
    /// Shared with the account API, whose server facts count the players online.
    pub online: Arc<Online>,
    pub lobby: LobbyLimits,
    pub limits: GateLimits,
    pub waiting: Places,
    pub clock: Arc<dyn Clock>,
    /// The world, which the link joins on the gate's behalf.
    pub world: WorldHandle,
    /// Where a character that has never entered starts (`[world]` start_scene, start_position).
    pub start: Location,
    /// `[access].admins`, lower-cased: names are compared without regard to ASCII case. They and
    /// the store's ops are the admins ([`crate::accounts::is_admin`]).
    pub admins: Vec<String>,
    /// True once the server is stopping: the listener stops accepting, and connections before
    /// their auth or in the lobby end at once. Players in the world end when the world lets go of
    /// them (#219).
    pub stopping: watch::Sender<bool>,
}

impl Gate {
    /// Resolves once the server is stopping.
    pub async fn stopped(&self) {
        let mut stopping = self.stopping.subscribe();
        // The sender lives as long as the gate, so this ends only when the value turns true.
        let _ = stopping.wait_for(|s| *s).await;
    }
}

/// The places for connections waiting for their auth (M3 spec D17): at most `total`, and at most
/// `per_network` from one network. The account API's listeners count theirs so too (Ruling HP9).
#[derive(Clone)]
pub(crate) struct Places(Arc<PlacesInner>);

struct PlacesInner {
    total: usize,
    per_network: usize,
    /// Places taken, in all and per network.
    taken: Mutex<(usize, HashMap<IpAddr, usize>)>,
}

/// A taken place, given back when dropped.
pub(crate) struct Place {
    places: Places,
    /// The network it counts against; none for a trusted proxy's.
    net: Option<IpAddr>,
}

impl Places {
    pub fn new(total: usize, per_network: usize) -> Self {
        Self(Arc::new(PlacesInner {
            total,
            per_network,
            taken: Mutex::new((0, HashMap::new())),
        }))
    }

    pub fn take(&self, addr: IpAddr) -> Option<Place> {
        self.take_counted(Some(network(addr)))
    }

    /// A place that counts toward the total alone: a trusted proxy's, whose connections carry
    /// many players' requests (M3 spec D12).
    pub fn take_for_proxy(&self) -> Option<Place> {
        self.take_counted(None)
    }

    fn take_counted(&self, net: Option<IpAddr>) -> Option<Place> {
        let mut taken = self.0.taken.lock().unwrap_or_else(|e| e.into_inner());
        let (all, nets) = &mut *taken;
        let full = |net: &IpAddr| nets.get(net).is_some_and(|&n| n >= self.0.per_network);
        if *all >= self.0.total || net.as_ref().is_some_and(full) {
            return None;
        }
        *all += 1;
        if let Some(net) = net {
            *nets.entry(net).or_insert(0) += 1;
        }
        Some(Place {
            places: self.clone(),
            net,
        })
    }
}

impl Drop for Place {
    fn drop(&mut self) {
        let mut taken = self
            .places
            .0
            .taken
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (all, nets) = &mut *taken;
        *all -= 1;
        if let Some(net) = &self.net
            && let Some(n) = nets.get_mut(net)
        {
            *n -= 1;
            if *n == 0 {
                nets.remove(net);
            }
        }
    }
}

/// An IPv4 address (IPv4-mapped ones included) as itself; an IPv6 address as its /64.
fn network(addr: IpAddr) -> IpAddr {
    match addr.to_canonical() {
        IpAddr::V6(v6) => {
            let s = v6.segments();
            IpAddr::V6(Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0))
        }
        v4 => v4,
    }
}

/// Accepts connections until the server is stopping, then closes the listener and returns the
/// connections still open, for [`finish`]. Connections live in a `JoinSet`, so dropping it ends
/// them all; finished ones are reaped as they end.
pub(crate) async fn accept_loop(listener: TcpListener, gate: Arc<Gate>) -> JoinSet<()> {
    let mut conns = JoinSet::new();
    loop {
        tokio::select! {
            () = gate.stopped() => return conns,
            accepted = listener.accept() => match accepted {
                Ok((sock, peer)) => {
                    let _ = sock.set_nodelay(true);
                    conns.spawn(connection(sock, peer, Arc::clone(&gate)));
                }
                Err(e) => {
                    // Such as running out of file descriptors: wait rather than spin.
                    tracing::warn!(error = %e, "gate accept");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            },
            Some(done) = conns.join_next(), if !conns.is_empty() => reap(done),
        }
    }
}

/// Waits up to `grace` for the connections a stopping server still has to end, then ends the rest
/// (#219).
pub(crate) async fn finish(mut conns: JoinSet<()>, grace: Duration) {
    let all = async {
        while let Some(done) = conns.join_next().await {
            reap(done);
        }
    };
    if tokio::time::timeout(grace, all).await.is_err() {
        tracing::warn!(
            open = conns.len(),
            "connections still open after the shutdown's grace: ending them"
        );
        conns.shutdown().await;
    }
}

fn reap(done: Result<(), JoinError>) {
    if let Err(e) = done
        && e.is_panic()
    {
        tracing::error!("a connection task panicked");
    }
}

async fn connection(mut sock: TcpStream, peer: SocketAddr, gate: Arc<Gate>) {
    let Some(place) = gate.waiting.take(peer.ip()) else {
        tracing::debug!(%peer, "dropped: too many connections are waiting for their auth");
        return;
    };
    if let Err(e) = serve(&mut sock, peer, &gate, place).await {
        tracing::debug!(%peer, error = %e, "connection ended");
    }
}

async fn serve(
    sock: &mut TcpStream,
    peer: SocketAddr,
    gate: &Gate,
    place: Place,
) -> anyhow::Result<()> {
    let mut fill = [0u8; HELLO_LEN];
    getrandom::fill(&mut fill).map_err(|e| anyhow::anyhow!("no random bytes: {e}"))?;
    let variants = Variants {
        down: AES128,
        up: AES128,
    };
    let (mut server, hello) = Server::start(fill, variants)?;
    sock.write_all(&hello).await?;
    let deadline = gate.limits.auth_deadline;
    let auth = tokio::select! {
        auth = tokio::time::timeout(deadline, read_auth(sock, &mut server)) => match auth {
            Ok(auth) => auth?,
            Err(_) => bail!("no auth within {deadline:?}"),
        },
        () = gate.stopped() => return Ok(()),
    };
    let Some(auth) = auth else {
        return Ok(());
    };
    // The place is kept through the ticket check and any refusal (with its linger), so connections
    // being checked or refused count against the waiting caps too.
    let account = match login(&auth, &gate.accounts).await {
        Ok(a) => a,
        Err(reason) => {
            tracing::info!(%peer, reason, "login refused");
            return refuse(sock, &mut server, reason, gate.limits.linger).await;
        }
    };
    let (conn, mut replaced) = match gate.online.join(account.id) {
        Join::Joined(id, rx) => (id, rx),
        Join::Full => {
            tracing::info!(%peer, account = %account.username, "login refused: the server is full");
            return refuse(sock, &mut server, SERVER_FULL, gate.limits.linger).await;
        }
    };
    drop(place);
    // Gives the online place back however the lobby loop ends, a panic included.
    let _online = Leave {
        online: &gate.online,
        account: account.id,
        conn,
    };
    lobby_loop(sock, peer, gate, &mut server, &account, conn, &mut replaced).await
}

struct Leave<'a> {
    online: &'a Online,
    account: i64,
    conn: u64,
}

impl Drop for Leave<'_> {
    fn drop(&mut self) {
        self.online.leave(self.account, self.conn);
    }
}

/// Writes `bytes`, or fails once `deadline` passes: a client that stops reading ends its
/// connection instead of holding it.
pub(crate) async fn send<W: AsyncWrite + Unpin>(
    sock: &mut W,
    bytes: &[u8],
    deadline: Duration,
) -> anyhow::Result<()> {
    match tokio::time::timeout(deadline, sock.write_all(bytes)).await {
        Ok(written) => Ok(written?),
        Err(_) => bail!("the client stopped reading: a write took over {deadline:?}"),
    }
}

/// Sends a reply within `deadline`, unless the online registry tells the connection to close
/// first, for a newer login or a ban: then it gives up at once and returns false. The connection
/// has lost its place, and a client that stopped reading shouldn't keep this socket until the
/// deadline, outside the caps.
pub(crate) async fn send_or_replaced<W: AsyncWrite + Unpin>(
    sock: &mut W,
    bytes: &[u8],
    deadline: Duration,
    replaced: &mut oneshot::Receiver<Closing>,
) -> anyhow::Result<bool> {
    tokio::select! {
        sent = send(sock, bytes, deadline) => sent.map(|()| true),
        Ok(_) = &mut *replaced => Ok(false),
    }
}

/// Half-closes, then reads until the client closes or `time` passes, so what was just sent isn't
/// lost to a reset. Callers hold a waiting place while they linger, so lingering sockets count
/// against the caps.
pub(crate) async fn linger<S: AsyncRead + AsyncWrite + Unpin>(sock: &mut S, time: Duration) {
    let _ = sock.shutdown().await;
    let mut buf = [0u8; 1024];
    let _ = tokio::time::timeout(time, async {
        while matches!(sock.read(&mut buf).await, Ok(n) if n > 0) {}
    })
    .await;
}

/// Reads until the auth message is complete. `None` when the client hangs up first.
async fn read_auth(
    sock: &mut TcpStream,
    server: &mut Server,
) -> anyhow::Result<Option<AuthMessage>> {
    let mut buf = vec![0u8; 4096];
    let mut total = 0;
    loop {
        let n = sock.read(&mut buf).await?;
        if n == 0 {
            return Ok(None);
        }
        total += n;
        if total > PRE_AUTH_CAP {
            bail!("over {PRE_AUTH_CAP} bytes before the auth");
        }
        for event in server.receive(&buf[..n])? {
            if let Event::Auth(auth) = event {
                return Ok(Some(auth));
            }
        }
    }
}

/// The account the auth's ticket names, or the close reason to refuse it with. The ticket is
/// spent even when the account name doesn't match.
async fn login(auth: &AuthMessage, accounts: &Accounts) -> Result<Account, &'static str> {
    let Some((ticket, name)) = login_fields(&auth.payload) else {
        return Err(AUTH_FAILED);
    };
    match accounts.consume_game_token(&ticket).await {
        Ok(Some(a)) if a.username.eq_ignore_ascii_case(&name) => Ok(a),
        Ok(_) => Err(AUTH_FAILED),
        Err(e) => {
            tracing::error!(error = %e, "checking a ticket");
            Err(SERVER_ERROR)
        }
    }
}

/// The ticket (value 0) and the account name (value 2) of the launcher path's auth payload
/// (payloads.md, the auth payload). The other values are dropped unread.
fn login_fields(payload: &[u8]) -> Option<(String, String)> {
    let values = pickle::decode(payload).ok()?;
    match (values.first()?, values.get(2)?) {
        (Value::Str(ticket), Value::Str(name)) => Some((
            String::from_utf8(ticket.clone()).ok()?,
            String::from_utf8(name.clone()).ok()?,
        )),
        _ => None,
    }
}

/// Refuses the auth with `reason`, then waits briefly for the client to close, so the reason
/// isn't lost to a reset.
async fn refuse(
    sock: &mut TcpStream,
    server: &mut Server,
    reason: &str,
    time: Duration,
) -> anyhow::Result<()> {
    let out = server.reject(reason.as_bytes(), &StoredCodec)?;
    send(sock, &out, time).await?;
    linger(sock, time).await;
    Ok(())
}

async fn lobby_loop(
    sock: &mut TcpStream,
    peer: SocketAddr,
    gate: &Gate,
    server: &mut Server,
    account: &Account,
    conn: u64,
    replaced: &mut oneshot::Receiver<Closing>,
) -> anyhow::Result<()> {
    let deadline = gate.limits.write_deadline;
    let mut out = server.accept(&StoredCodec)?;
    out.extend(server.begin_work(&StoredCodec)?);
    send(sock, &out, deadline).await?;
    tracing::info!(%peer, account = %account.username, "in the lobby");
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        tokio::select! {
            read = tokio::time::timeout(gate.limits.idle, sock.read(&mut buf)) => {
                let n = match read {
                    Ok(n) => n?,
                    Err(_) => {
                        tracing::info!(%peer, account = %account.username, "idle: closing");
                        return Ok(());
                    }
                };
                if n == 0 {
                    tracing::info!(%peer, account = %account.username, "left");
                    return Ok(());
                }
                for event in server.receive(&buf[..n])? {
                    let m = match event {
                        Event::Message(m) => m,
                        // The client's ping display reads only what a server time sync sets: its
                        // clock less the one echoed (transport.md 4.4, #269).
                        Event::TimeSync(ms) => {
                            if !send_or_replaced(sock, &server.time_sync(ms), deadline, replaced).await? {
                                tracing::info!(%peer, account = %account.username, "replaced by a newer login, or banned, while a reply was blocked: closing");
                                return Ok(());
                            }
                            continue;
                        }
                        _ => continue,
                    };
                    match on_message(gate, account, m.ty, &m.body).await {
                        Step::Replies(replies) => {
                            for reply in replies {
                                let bytes = script_out(server, &reply)?;
                                if !send_or_replaced(sock, &bytes, deadline, replaced).await? {
                                    tracing::info!(%peer, account = %account.username, "replaced by a newer login, or banned, while a reply was blocked: closing");
                                    return Ok(());
                                }
                            }
                        }
                        Step::Enter(pid) => match entry(gate, account, pid).await {
                            Entry::Play(c) => {
                                return link::play(sock, peer, gate, server, account, conn, c, replaced).await;
                            }
                            Entry::Banned => {
                                tracing::info!(%peer, account = %account.username, "banned: not entering the world");
                                return close_with(sock, peer, gate, server, BANNED).await;
                            }
                            Entry::Refused(code) => {
                                let bytes = script_out(server, &fenji_proto::lobby::error_code(code))?;
                                if !send_or_replaced(sock, &bytes, deadline, replaced).await? {
                                    tracing::info!(%peer, account = %account.username, "replaced by a newer login, or banned, while a reply was blocked: closing");
                                    return Ok(());
                                }
                            }
                        },
                        Step::Leave => {
                            tracing::info!(%peer, account = %account.username, "left (the client is quitting)");
                            return Ok(());
                        }
                        Step::AccountGone => {
                            tracing::info!(%peer, account = %account.username, "the account was deleted: closing");
                            return close_with(sock, peer, gate, server, AUTH_FAILED).await;
                        }
                    }
                }
            }
            Ok(closing) = &mut *replaced => {
                return close_told(sock, peer, gate, server, account, closing).await;
            }
            // Closed at once and without a reason, as the shutdown always closed the lobby:
            // lobby.md 9.5 gives none for a stopping server.
            () = gate.stopped() => {
                tracing::info!(%peer, account = %account.username, "the server is stopping: closing");
                return Ok(());
            }
        }
    }
}

/// Closes a connection that still holds its online place with `reason`, which its client shows:
/// sent within the write deadline, then a linger so it isn't lost to a reset, only within a
/// waiting place. For a deleted account (in the lobby, and in the world by Ruling R5), a stopping
/// server, an admin's kick, and a banned account's character at the world's door (HA5).
pub(crate) async fn close_with<S: AsyncRead + AsyncWrite + Unpin>(
    sock: &mut S,
    peer: SocketAddr,
    gate: &Gate,
    server: &mut Server,
    reason: &str,
) -> anyhow::Result<()> {
    let bytes = server.send(Stc::Close.to_u8(), reason.as_bytes(), &StoredCodec)?;
    send(sock, &bytes, gate.limits.write_deadline).await?;
    if let Some(_place) = gate.waiting.take(peer.ip()) {
        linger(sock, gate.limits.linger).await;
    }
    Ok(())
}

/// Closes a connection the online registry told to close, with the reason its client shows: M3's
/// for a login that a newer one replaced, and a ban's for an account the host banned
/// (`Online::ban`). Either has lost its online place, so it closes as [`close_replaced`] does.
pub(crate) async fn close_told<S: AsyncRead + AsyncWrite + Unpin>(
    sock: &mut S,
    peer: SocketAddr,
    gate: &Gate,
    server: &mut Server,
    account: &Account,
    closing: Closing,
) -> anyhow::Result<()> {
    let reason = match closing {
        Closing::Replaced => {
            tracing::info!(%peer, account = %account.username, "replaced by a newer login");
            DUPLICATE_LOGIN
        }
        Closing::Banned => {
            tracing::info!(%peer, account = %account.username, "banned: closing");
            BANNED
        }
    };
    let bytes = server.send(Stc::Close.to_u8(), reason.as_bytes(), &StoredCodec)?;
    let place = gate.waiting.take(peer.ip());
    close_replaced(sock, &bytes, place, gate.limits.linger).await;
    Ok(())
}

/// Closes a connection that a newer login replaced, or that the world let go of with a reason. A
/// replaced one no longer holds an online place, so it says why and lingers only within a waiting
/// place, and for `time` at most: a client that stopped reading can't keep its socket outside the
/// caps (#174). With no waiting place free it closes at once, without a reason.
pub(crate) async fn close_replaced<S: AsyncRead + AsyncWrite + Unpin>(
    sock: &mut S,
    close: &[u8],
    place: Option<Place>,
    time: Duration,
) {
    let Some(_place) = place else {
        return;
    };
    if send(sock, close, time).await.is_ok() {
        linger(sock, time).await;
    }
}

/// What the lobby loop does after one message from a client.
enum Step {
    /// Send these replies, in order, and go on.
    Replies(Vec<Vec<u8>>),
    /// Enter the world as this pid (10, `SELECT_ACTOR`), if it is the account's.
    Enter(i64),
    /// The client is quitting (392, `CLIENT_EXIT`): close the connection.
    Leave,
    /// The account was deleted since the login (`fenji-server account delete`): close with the
    /// client's own auth-failed reason.
    AccountGone,
}

/// The step for one message from a client in the lobby. Every script message first checks that
/// the account still exists, since an admin may delete it while its player is in the lobby.
async fn on_message(gate: &Gate, account: &Account, ty: u8, body: &[u8]) -> Step {
    match Up::decode(ty, body) {
        Ok(Up::Script {
            id: Value::Int(id),
            args,
            gateway,
        }) => {
            tracing::trace!(id, gateway, body = %hex(body), "script message");
            if id == fenji_proto::lobby::CLIENT_EXIT {
                return Step::Leave;
            }
            match gate.accounts.store().account_by_id(account.id).await {
                Ok(None) => return Step::AccountGone,
                Ok(Some(_)) => {}
                // The handler meets the same store and answers the client's screen.
                Err(e) => tracing::error!(error = %e, "account check failed"),
            }
            if id == fenji_proto::lobby::SELECT_ACTOR {
                return match lobby::pid(args.first()) {
                    Some(pid) => Step::Enter(pid),
                    None => Step::Replies(vec![fenji_proto::lobby::error_code(
                        fenji_proto::lobby::code::ROLE_NOT_FOUND,
                    )]),
                };
            }
            let now = gate.clock.now_ms();
            Step::Replies(
                lobby::handle(gate.accounts.store(), &gate.lobby, account, now, id, &args).await,
            )
        }
        Ok(Up::Script { id, .. }) => {
            tracing::debug!(
                id = id.kind(),
                "script message without an integer id ignored"
            );
            Step::Replies(Vec::new())
        }
        Ok(Up::Unknown { opcode, bytes }) => {
            tracing::debug!(opcode, len = bytes.len(), "message ignored");
            Step::Replies(Vec::new())
        }
        Err(e) => {
            tracing::debug!(ty, len = body.len(), error = %e, "undecodable message skipped");
            Step::Replies(Vec::new())
        }
    }
}

/// What a client's 10 comes to.
enum Entry {
    /// Enter the world as this character.
    Play(Character),
    /// Refuse with this 270 code: the player stays in the lobby, and may try again.
    Refused(i64),
    /// The account is banned: close the connection.
    Banned,
}

/// Whether the account may enter the world as its character `pid`. A ban that came after its
/// ticket keeps it out, and its connection closes as a ban closes one in the world (M4 hosting
/// spec §8, HA5). A store that can't say whether the account is banned refuses it with
/// `SERVER_NOT_RESP`, as one that can't read the character does ([`character`]): the player stays
/// in the lobby, and may try again.
async fn entry(gate: &Gate, account: &Account, pid: i64) -> Entry {
    match gate.accounts.store().ban_of(account.id).await {
        Ok(None) => {}
        Ok(Some(_)) => return Entry::Banned,
        Err(e) => {
            tracing::error!(error = %e, account = %account.username, "checking for a ban to enter the world");
            return Entry::Refused(fenji_proto::lobby::code::SERVER_NOT_RESP);
        }
    }
    match character(gate, account, pid).await {
        Ok(c) => Entry::Play(c),
        Err(code) => Entry::Refused(code),
    }
}

/// The account's character `pid`, if it may enter the world; otherwise the 270 code to refuse it
/// with: `ROLE_NOT_FOUND` for another account's, an unknown pid or one no player may have (world.md
/// 8.2), and `SERVER_NOT_RESP` when the store fails.
async fn character(gate: &Gate, account: &Account, pid: i64) -> Result<Character, i64> {
    use fenji_proto::lobby::code;
    match gate.accounts.store().character(account.id, pid).await {
        Ok(Some(c)) if (1..=MAX_PLAYER_ID).contains(&c.pid) => Ok(c),
        Ok(Some(c)) => {
            tracing::warn!(pid = c.pid, "a character whose pid no player may have");
            Err(code::ROLE_NOT_FOUND)
        }
        Ok(None) => Err(code::ROLE_NOT_FOUND),
        Err(e) => {
            tracing::error!(error = %e, pid, "reading a character to enter the world");
            Err(code::SERVER_NOT_RESP)
        }
    }
}

/// Every script message to a client goes out here. Bodies come from `script_body`, which refuses
/// `NEVER_SEND`; this checks the id again, so no other builder can slip one through (ground
/// rule 6; lobby.md §2).
pub(crate) fn script_out(server: &mut Server, body: &[u8]) -> anyhow::Result<Vec<u8>> {
    sendable(body)?;
    Ok(server.send(Stc::Script.to_u8(), body, &StoredCodec)?)
}

fn sendable(body: &[u8]) -> anyhow::Result<()> {
    match pickle::decode(body)?.first() {
        Some(Value::Int(id)) if NEVER_SEND.contains(id) => {
            bail!("refusing to send script message {id}")
        }
        Some(Value::Int(_)) => Ok(()),
        _ => bail!("a script body without an integer id"),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use fenji_proto::lobby;
    use fenji_proto::pickle::{self, Value};

    use super::*;

    #[tokio::test]
    async fn a_blocked_reply_gives_way_when_the_connection_is_replaced() {
        // A pipe nobody reads: a write beyond its 64 bytes blocks, as one to a client that stopped
        // reading does.
        let (mut w, _unread) = tokio::io::duplex(64);
        let (newer_login, mut replaced) = oneshot::channel();
        let bytes = vec![0u8; 4096];
        let send = send_or_replaced(&mut w, &bytes, Duration::from_secs(30), &mut replaced);
        tokio::pin!(send);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut send)
                .await
                .is_err(),
            "the write blocks"
        );
        newer_login.send(Closing::Replaced).unwrap();
        let sent = tokio::time::timeout(Duration::from_secs(1), send)
            .await
            .expect("gave way at once, not at the 30 s deadline")
            .unwrap();
        assert!(!sent);
    }

    #[tokio::test]
    async fn a_stopping_server_waits_out_its_grace_then_ends_the_connections_left() {
        // #219: connections get the grace to write their closes, and no more.
        let mut conns = JoinSet::new();
        let (held, never) = oneshot::channel::<()>();
        conns.spawn(async move {
            let _ = never.await;
        });
        let (quick, done) = oneshot::channel::<()>();
        conns.spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let _ = quick.send(());
        });
        let began = std::time::Instant::now();
        finish(conns, Duration::from_millis(300)).await;
        let took = began.elapsed();
        assert!(took >= Duration::from_millis(300), "{took:?}");
        assert!(took < Duration::from_secs(2), "{took:?}");
        assert_eq!(done.await, Ok(()), "a connection that ends in time ends");
        assert!(held.is_closed(), "the one left was ended");
        // With nothing left open, it doesn't wait for the grace.
        let mut conns = JoinSet::new();
        conns.spawn(async {});
        let began = std::time::Instant::now();
        finish(conns, Duration::from_secs(30)).await;
        assert!(began.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn the_shutdowns_grace_is_a_write_deadline_a_linger_and_a_tick() {
        let limits = GateLimits {
            write_deadline: Duration::from_secs(3),
            linger: Duration::from_millis(300),
            ..GateLimits::default()
        };
        assert_eq!(limits.grace(), Duration::from_millis(4_300));
        assert_eq!(GateLimits::default().grace(), Duration::from_secs(13));
    }

    #[tokio::test]
    async fn a_reply_that_fits_is_sent() {
        let (mut w, mut r) = tokio::io::duplex(8192);
        let (_newer_login, mut replaced) = oneshot::channel::<Closing>();
        assert!(
            send_or_replaced(&mut w, b"hello", Duration::from_secs(1), &mut replaced)
                .await
                .unwrap()
        );
        let mut buf = [0u8; 5];
        r.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hello");
    }

    #[tokio::test]
    async fn a_replaced_client_that_stopped_reading_is_let_go_within_the_linger() {
        // A pipe nobody reads: the close reason doesn't fit, as with a client that stopped reading.
        let (mut sock, _unread) = tokio::io::duplex(8);
        let places = Places::new(1, 1);
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        let began = std::time::Instant::now();
        close_replaced(
            &mut sock,
            &[0u8; 64],
            places.take(ip),
            Duration::from_millis(100),
        )
        .await;
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "not the 10 s write deadline"
        );
        assert!(places.take(ip).is_some(), "the waiting place is given back");
    }

    #[tokio::test]
    async fn with_no_waiting_place_free_a_replaced_connection_closes_without_a_reason() {
        let (mut sock, mut client) = tokio::io::duplex(1024);
        close_replaced(&mut sock, b"why", None, Duration::from_secs(30)).await;
        drop(sock);
        let mut got = Vec::new();
        client.read_to_end(&mut got).await.unwrap();
        assert!(got.is_empty());
    }

    #[test]
    fn waiting_places_are_counted_per_network() {
        let places = Places::new(3, 2);
        let a: IpAddr = "10.0.0.1".parse().unwrap();
        let b: IpAddr = "::ffff:10.0.0.1".parse().unwrap();
        let first = places.take(a).unwrap();
        let _second = places.take(b).unwrap(); // the same IPv4 address, mapped
        assert!(places.take(a).is_none(), "a third from one address");
        let v6: IpAddr = "2001:db8::1".parse().unwrap();
        let same64: IpAddr = "2001:db8::ffff".parse().unwrap();
        let _third = places.take(v6).unwrap();
        assert!(places.take(same64).is_none(), "the total is 3");
        drop(first);
        let _fourth = places.take(same64).unwrap();
        assert!(places.take(v6).is_none(), "one /64 counts as one network");
    }

    #[test]
    fn the_never_sent_ids_cannot_go_out() {
        for id in NEVER_SEND {
            let body = pickle::encode(&[Value::Int(id), Value::Nil]).unwrap();
            assert!(sendable(&body).is_err(), "{id}");
        }
        assert!(sendable(&pickle::encode(&[Value::Str(b"x".to_vec())]).unwrap()).is_err());
        assert!(sendable(&lobby::delete_result(1)).is_ok());
        assert!(sendable(&lobby::error_code(lobby::code::ROLE_NOT_FOUND)).is_ok());
    }

    #[test]
    fn login_fields_take_the_ticket_and_the_account_only() {
        let v = [
            Value::Str(b"fenji1-ab".to_vec()),
            Value::Int(0),
            Value::Str(b"tester".to_vec()),
            Value::Str(vec![9; 20]),
        ];
        let got = login_fields(&pickle::encode(&v).unwrap());
        assert_eq!(got, Some(("fenji1-ab".into(), "tester".into())));
        for bad in [
            vec![Value::Int(1), Value::Int(0), Value::Str(b"tester".to_vec())],
            vec![Value::Str(b"t".to_vec()), Value::Int(0)],
            vec![
                Value::Str(vec![0xFF]),
                Value::Int(0),
                Value::Str(b"x".to_vec()),
            ],
        ] {
            assert_eq!(login_fields(&pickle::encode(&bad).unwrap()), None);
        }
        assert_eq!(login_fields(&[0xFF, 0, 0]), None);
    }

    #[test]
    fn the_limits_default_to_the_spec_and_take_max_players_from_the_config() {
        let d = GateLimits::default();
        assert_eq!(
            (
                d.max_players,
                d.auth_deadline,
                d.max_unauthenticated,
                d.max_unauthenticated_per_network
            ),
            (20, Duration::from_secs(30), 64, 8)
        );
        assert_eq!(d.account_check, Duration::from_secs(30), "Ruling R5");
        let cfg = Config::parse(
            "[server]\nname = \"F\"\nmax_players = 5\n",
            std::path::Path::new("/"),
        )
        .unwrap();
        assert_eq!(GateLimits::from_config(&cfg).max_players, 5);
    }
}
