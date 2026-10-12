//! The gate listener (M3 spec D3, D17; docs/protocol/transport.md): the hello, the auth with its
//! deadline and size cap, the ticket, the online registry, then the lobby loop. Close reasons use
//! the client's own keys where it has one (docs/protocol/packets/lobby.md §9). The ticket is never
//! logged, and the auth payload's machine bytes and fingerprint are dropped here, never stored.
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
use fenji_store::Account;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::task::JoinSet;

use crate::accounts::Accounts;
use crate::clock::Clock;
use crate::config::Config;
use crate::lobby::{self, LobbyLimits};
use crate::online::{Join, Online};

/// The client's close-reason key for a refused login (net.luac's close-reason table).
pub const AUTH_FAILED: &str = "error auth-failed";
/// The client's key for a session ended by a newer login to the same account.
pub const DUPLICATE_LOGIN: &str = "error duplicate-login";
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
/// How long a refused connection stays open for the client to read why.
const LINGER: Duration = Duration::from_secs(2);

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
}

impl Default for GateLimits {
    fn default() -> Self {
        Self {
            max_players: 20,
            auth_deadline: Duration::from_secs(30),
            max_unauthenticated: 64,
            max_unauthenticated_per_network: 8,
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
}

/// What every connection shares.
pub(crate) struct Gate {
    pub accounts: Arc<Accounts>,
    pub online: Online,
    pub lobby: LobbyLimits,
    pub limits: GateLimits,
    pub waiting: Places,
    pub clock: Arc<dyn Clock>,
}

/// The places for connections waiting for their auth (M3 spec D17): at most `total`, and at most
/// `per_network` from one network.
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
    net: IpAddr,
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
        let net = network(addr);
        let mut taken = self.0.taken.lock().unwrap_or_else(|e| e.into_inner());
        let (all, nets) = &mut *taken;
        if *all >= self.0.total || nets.get(&net).is_some_and(|&n| n >= self.0.per_network) {
            return None;
        }
        *all += 1;
        *nets.entry(net).or_insert(0) += 1;
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
        if let Some(n) = nets.get_mut(&self.net) {
            *n -= 1;
            if *n == 0 {
                nets.remove(&self.net);
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

/// Accepts connections until aborted. Connections live in a `JoinSet` this task owns, so aborting
/// it ends them all; finished ones are reaped as they end.
pub(crate) async fn accept_loop(listener: TcpListener, gate: Arc<Gate>) {
    let mut conns = JoinSet::new();
    loop {
        tokio::select! {
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
            Some(_) = conns.join_next(), if !conns.is_empty() => {}
        }
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
    let auth = match tokio::time::timeout(deadline, read_auth(sock, &mut server)).await {
        Ok(auth) => auth?,
        Err(_) => bail!("no auth within {deadline:?}"),
    };
    drop(place);
    let Some(auth) = auth else {
        return Ok(());
    };
    let account = match login(&auth, &gate.accounts).await {
        Ok(a) => a,
        Err(reason) => {
            tracing::info!(%peer, reason, "login refused");
            return refuse(sock, &mut server, reason).await;
        }
    };
    let (conn, mut replaced) = match gate.online.join(account.id) {
        Join::Joined(id, rx) => (id, rx),
        Join::Full => {
            tracing::info!(%peer, account = %account.username, "login refused: the server is full");
            return refuse(sock, &mut server, SERVER_FULL).await;
        }
    };
    let worked = lobby_loop(sock, peer, gate, &mut server, &account, &mut replaced).await;
    gate.online.leave(account.id, conn);
    worked
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
async fn refuse(sock: &mut TcpStream, server: &mut Server, reason: &str) -> anyhow::Result<()> {
    let out = server.reject(reason.as_bytes(), &StoredCodec)?;
    sock.write_all(&out).await?;
    sock.shutdown().await?;
    let mut buf = [0u8; 1024];
    let _ = tokio::time::timeout(LINGER, async {
        while matches!(sock.read(&mut buf).await, Ok(n) if n > 0) {}
    })
    .await;
    Ok(())
}

async fn lobby_loop(
    sock: &mut TcpStream,
    peer: SocketAddr,
    gate: &Gate,
    server: &mut Server,
    account: &Account,
    replaced: &mut oneshot::Receiver<()>,
) -> anyhow::Result<()> {
    let mut out = server.accept(&StoredCodec)?;
    out.extend(server.begin_work(&StoredCodec)?);
    sock.write_all(&out).await?;
    tracing::info!(%peer, account = %account.username, "in the lobby");
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        tokio::select! {
            read = tokio::time::timeout(IDLE, sock.read(&mut buf)) => {
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
                    if let Event::Message(m) = event {
                        for reply in on_message(gate, account, m.ty, &m.body).await {
                            let bytes = script_out(server, &reply)?;
                            sock.write_all(&bytes).await?;
                        }
                    }
                }
            }
            Ok(()) = &mut *replaced => {
                tracing::info!(%peer, account = %account.username, "replaced by a newer login");
                let bytes = server.send(Stc::Close.to_u8(), DUPLICATE_LOGIN.as_bytes(), &StoredCodec)?;
                sock.write_all(&bytes).await?;
                return Ok(());
            }
        }
    }
}

/// The replies to one message from a client in the lobby.
async fn on_message(gate: &Gate, account: &Account, ty: u8, body: &[u8]) -> Vec<Vec<u8>> {
    match Up::decode(ty, body) {
        Ok(Up::Script {
            id: Value::Int(id),
            args,
            gateway,
        }) => {
            tracing::trace!(id, gateway, body = %hex(body), "script message");
            let now = gate.clock.now_ms();
            lobby::handle(gate.accounts.store(), &gate.lobby, account, now, id, &args).await
        }
        Ok(Up::Script { id, .. }) => {
            tracing::debug!(
                id = id.kind(),
                "script message without an integer id ignored"
            );
            Vec::new()
        }
        Ok(Up::Unknown { opcode, bytes }) => {
            tracing::debug!(opcode, len = bytes.len(), "message ignored");
            Vec::new()
        }
        Err(e) => {
            tracing::debug!(ty, len = body.len(), error = %e, "undecodable message skipped");
            Vec::new()
        }
    }
}

/// Every script message to a client goes out here. Bodies come from `script_body`, which refuses
/// `NEVER_SEND`; this checks the id again, so no other builder can slip one through (ground
/// rule 6; lobby.md §2).
fn script_out(server: &mut Server, body: &[u8]) -> anyhow::Result<Vec<u8>> {
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
        let cfg = Config::parse(
            "[server]\nname = \"F\"\nmax_players = 5\n",
            std::path::Path::new("/"),
        )
        .unwrap();
        assert_eq!(GateLimits::from_config(&cfg).max_players, 5);
    }
}
