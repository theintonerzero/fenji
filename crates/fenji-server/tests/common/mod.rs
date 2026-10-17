//! The bot (M3 spec §6): a client on `fenji_proto::client::Client` over a real socket, which sends
//! what the real client sends on the launcher path, and helpers to set up a server for it. In the
//! world it is the protocol bot (M4 world plan, Amendment D): it enters, sees the others, walks and
//! chats as the plan scripts it, and checks every reply through the codecs that build it. And
//! [`caught`], the log as a test catches it.
#![allow(dead_code)]
pub mod caught;

use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fenji_auth::invite::InviteCode;
use fenji_proto::chat;
use fenji_proto::client::{Client, ClientEvent};
use fenji_proto::handshake::{AuthMessage, Message};
use fenji_proto::lobby;
use fenji_proto::opcode::{Cts, Stc};
use fenji_proto::packet::script_body;
use fenji_proto::pickle::{self, Table, Value};
use fenji_proto::world::{self, MoveDetail};
use fenji_server::clock::TestClock;
use fenji_server::config::Config;
use fenji_server::server::{GateLimits, RunningServer, start};
use fenji_store::Place;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

pub const NOW: i64 = 1_800_000_000_000;
pub const LOCAL: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

pub fn config(dir: &Path, extra: &str) -> Config {
    let text = format!(
        "[server]\nname = \"Fenji\"\nlogin_bind = \"127.0.0.1:0\"\napi_bind = \"127.0.0.1:0\"\n{extra}"
    );
    Config::parse(&text, dir).unwrap()
}

pub async fn server(dir: &Path, extra: &str, limits: GateLimits) -> (RunningServer, TestClock) {
    let clock = TestClock::at(NOW);
    let s = start(&config(dir, extra), Arc::new(clock.clone()), limits)
        .await
        .unwrap();
    (s, clock)
}

/// A new account with a password, through an invite.
pub async fn account(s: &RunningServer, name: &str) {
    let code = InviteCode::generate().unwrap();
    s.accounts()
        .store()
        .create_invite(&code.hash(), None, NOW, None)
        .await
        .unwrap();
    s.accounts()
        .register(&code.display(), name, "a long password", LOCAL)
        .await
        .unwrap();
}

/// A fresh game token for `name`.
pub async fn ticket(s: &RunningServer, name: &str) -> String {
    let session = s
        .accounts()
        .sign_in(name, "a long password", LOCAL)
        .await
        .unwrap();
    s.accounts()
        .issue_game_token(&session.token)
        .await
        .unwrap()
        .ticket
}

/// The launcher path's auth payload (payloads.md, the auth payload): ticket, 0, account, user id,
/// server id, version, "", 0, the fingerprint (zeros here), language. The real client compresses
/// it from 128 bytes up.
pub fn payload(ticket: &str, account: &str, compressed: bool) -> Vec<u8> {
    let v = vec![
        Value::Str(ticket.as_bytes().to_vec()),
        Value::Int(0),
        Value::Str(account.as_bytes().to_vec()),
        Value::Int(1),
        Value::Int(1),
        Value::Str(b"2.1.19.42851".to_vec()),
        Value::Str(Vec::new()),
        Value::Int(0),
        Value::Str(vec![0; 20]),
        Value::Str(b"en-GB".to_vec()),
    ];
    if compressed {
        let p = pickle::encode_compressed(&v).unwrap();
        assert_eq!(p[0], 0xFF, "the payload should compress");
        p
    } else {
        pickle::encode(&v).unwrap()
    }
}

fn s(x: &str) -> Value {
    Value::Str(x.as_bytes().to_vec())
}

/// The create request as `CreatePanel.luac` builds it (lobby.md §5); the face string is index 5.
pub fn request(name: &str, vocation: i64, skeleton: i64) -> Vec<Value> {
    vec![Value::Table(Table {
        array: Vec::new(),
        hash: vec![
            (s("prop_base_name"), s(name)),
            (s("prop_base_vocation"), Value::Int(vocation)),
            (s("prop_base_skeleton"), Value::Int(skeleton)),
            (s("prop_sys_player_voice"), Value::Int(4)),
            (s("prop_sys_avatar_feature"), Value::Int(2)),
            (
                s("prop_sys_avatar_feature_face"),
                s("v:5;2:1,2,3;3:1,2,3,4,5;4:7,2,3,4"),
            ),
            (s("prop_sys_player_tattoo_pos"), Value::Int(0)),
            (s("prop_sys_player_birthday"), Value::Int(0)),
            (s("prop_base_equips_id"), Value::Int(2)),
            (s("prop_sys_portrait"), Value::Int(24 << 16)),
        ],
    })]
}

/// A table's value under a string key.
pub fn field<'a>(t: &'a Table, k: &str) -> Option<&'a Value> {
    t.hash.iter().find(|(kk, _)| *kk == s(k)).map(|(_, v)| v)
}

pub struct Bot {
    sock: TcpStream,
    client: Client,
    events: VecDeque<ClientEvent>,
    /// The character the bot made (304), once it has.
    made: Option<Made>,
    /// Where that character stands, as the world last put it (772, a walk, a 20), once selected.
    place: Option<Place>,
}

impl Bot {
    pub async fn connect(addr: SocketAddr, payload: Vec<u8>) -> Bot {
        let sock = TcpStream::connect(addr).await.unwrap();
        let client = Client::new(AuthMessage {
            dict_id: [0; 16],
            machine: [2, 0, 0, 0, 0, 1],
            payload,
        });
        let mut bot = Bot {
            sock,
            client,
            events: VecDeque::new(),
            made: None,
            place: None,
        };
        bot.pump().await;
        bot
    }

    /// Reads once (at most 5 s) and feeds the client; sends whatever it answers. False when the
    /// connection has ended.
    async fn pump(&mut self) -> bool {
        let mut buf = vec![0u8; 64 * 1024];
        let n = match tokio::time::timeout(Duration::from_secs(5), self.sock.read(&mut buf)).await {
            Ok(Ok(n)) => n,
            _ => return false,
        };
        if n == 0 {
            return false;
        }
        let Ok((events, out)) = self.client.receive(&buf[..n]) else {
            return false;
        };
        if !out.is_empty() && self.sock.write_all(&out).await.is_err() {
            return false;
        }
        self.events.extend(events);
        true
    }

    /// The next event other than a time sync; `None` once the connection has ended.
    pub async fn next(&mut self) -> Option<ClientEvent> {
        loop {
            if let Some(e) = self.events.pop_front() {
                if matches!(e, ClientEvent::TimeSync(_)) {
                    continue;
                }
                return Some(e);
            }
            if !self.pump().await {
                return None;
            }
        }
    }

    /// Through `ok` and `SET_NET_CONNECTED` (414).
    pub async fn expect_work(&mut self) {
        let e = self.next().await;
        assert!(matches!(e, Some(ClientEvent::Accepted)), "{e:?}");
        let e = self.next().await.expect("414");
        let ClientEvent::Message(m) = e else {
            panic!("{e:?}")
        };
        assert_eq!(pickle::decode(&m.body).unwrap()[0], Value::Int(414));
    }

    pub async fn expect_rejected(&mut self) -> Vec<u8> {
        match self.next().await {
            Some(ClientEvent::Rejected(r)) => r,
            other => panic!("expected a rejection, got {other:?}"),
        }
    }

    pub async fn expect_closed(&mut self) -> Vec<u8> {
        match self.next().await {
            Some(ClientEvent::Closed(r)) => r,
            other => panic!("expected a close, got {other:?}"),
        }
    }

    /// True when the socket ends or the client is told to close.
    pub async fn next_is_end(&mut self) -> bool {
        matches!(
            self.next().await,
            None | Some(ClientEvent::Rejected(_) | ClientEvent::Closed(_))
        )
    }

    /// A gateway script message (type 0xFF), as `send_gateway` sends it.
    pub async fn send(&mut self, id: i64, args: &[Value]) {
        let out = self
            .client
            .send(0xFF, &script_body(id, args).unwrap())
            .unwrap();
        self.sock.write_all(&out).await.unwrap();
    }

    /// A message body exactly as given, as a gateway script message (type 0xFF): for a recorded
    /// body such as a fixture.
    pub async fn send_raw(&mut self, body: &[u8]) {
        let out = self.client.send(0xFF, body).unwrap();
        self.sock.write_all(&out).await.unwrap();
    }

    /// The next event, which must be a transport-level time sync from the server (transport.md
    /// 4.4): its clock value. `None` when another event comes first or the connection ends.
    pub async fn next_transport_time_sync(&mut self) -> Option<u64> {
        loop {
            if let Some(e) = self.events.pop_front() {
                return match e {
                    ClientEvent::TimeSync(ms) => Some(ms),
                    _ => None,
                };
            }
            if !self.pump().await {
                return None;
            }
        }
    }

    /// A transport-level time sync with `millis` as the client's clock (transport.md 4.6).
    pub async fn send_transport_time_sync(&mut self, millis: u64) {
        let bytes = self.client.time_sync(millis);
        self.sock.write_all(&bytes).await.unwrap();
    }

    /// The next script message: its id and arguments.
    pub async fn recv(&mut self) -> (i64, Vec<Value>) {
        let e = self.next().await.expect("a message");
        let ClientEvent::Message(m) = e else {
            panic!("{e:?}")
        };
        let mut v = pickle::decode(&m.body).unwrap();
        let Value::Int(id) = v.remove(0) else {
            panic!("id")
        };
        (id, v)
    }

    /// Nothing arrives within `ms`, and the connection stays open.
    pub async fn quiet(&mut self, ms: u64) {
        let got = tokio::time::timeout(Duration::from_millis(ms), self.next()).await;
        assert!(got.is_err(), "expected silence, got {got:?}");
    }

    /// A message of type `ty` exactly as given, as `send_time_sync`, `send_enter`, `send_leave` or
    /// `send` sends one (world.md §1).
    pub async fn send_native(&mut self, ty: u8, body: &[u8]) {
        let out = self.client.send(ty, body).unwrap();
        self.sock.write_all(&out).await.unwrap();
    }

    /// The next message, whatever its type.
    pub async fn recv_message(&mut self) -> Message {
        match self.next().await {
            Some(ClientEvent::Message(m)) => m,
            other => panic!("expected a message, got {other:?}"),
        }
    }

    /// `n` copies of one message, written as the client writes its stream: false when the write
    /// didn't finish within `wait` (the server stopped reading) or failed.
    pub async fn send_many(&mut self, ty: u8, body: &[u8], n: usize, wait: Duration) -> bool {
        for _ in 0..n {
            if self.client.queue(ty, body).is_err() {
                return false;
            }
        }
        let Ok(out) = self.client.flush() else {
            return false;
        };
        matches!(
            tokio::time::timeout(wait, self.sock.write_all(&out)).await,
            Ok(Ok(()))
        )
    }

    /// Several messages, each a type and a body, written as one write, as the client writes its
    /// stream: the server reads them in one go.
    pub async fn send_together(&mut self, messages: &[(u8, Vec<u8>)]) {
        for (ty, body) in messages {
            self.client.queue(*ty, body).unwrap();
        }
        let out = self.client.flush().unwrap();
        self.sock.write_all(&out).await.unwrap();
    }

    /// Reads and drops whatever is left until the server closes the connection: true when it
    /// does within `wait`.
    pub async fn ends_within(&mut self, wait: Duration) -> bool {
        let mut buf = vec![0u8; 64 * 1024];
        let drain = async { while matches!(self.sock.read(&mut buf).await, Ok(n) if n > 0) {} };
        tokio::time::timeout(wait, drain).await.is_ok()
    }
}

/// A bot signed in as a new account `name` (Amendment D, step 1): the account through an invite,
/// then the launcher path's auth with a fresh ticket, through the transport's `ok` and 414.
pub async fn signed_in(s: &RunningServer, name: &str) -> Bot {
    account(s, name).await;
    let t = ticket(s, name).await;
    let mut bot = Bot::connect(s.login_addr, payload(&t, name, false)).await;
    bot.expect_work().await;
    bot
}

/// The protocol bot in the world (M4 world plan, Amendment D, steps 2 to 7): it plays the character
/// it made, and checks what comes back through the codecs that build it.
impl Bot {
    /// 304 for a character called `name` (step 2; lobby.md §5), with `request`'s look: 628 comes
    /// back with its actor, whose pid the bot keeps with the name, vocation and skeleton.
    pub async fn create(&mut self, name: &str, vocation: i64, skeleton: i64) -> Made {
        self.send(lobby::CREATE_ACTOR, &request(name, vocation, skeleton))
            .await;
        let (id, args) = self.recv().await;
        assert_eq!(id, lobby::ROLE_CREATE, "{name}: {args:?}");
        let Some(Value::Table(actor)) = args.first() else {
            panic!("628's actor: {args:?}")
        };
        assert_eq!(field(actor, "prop_base_name"), Some(&s(name)), "{args:?}");
        let made = Made {
            pid: pid_of(actor),
            name: name.into(),
            vocation,
            skeleton,
        };
        self.made = Some(made.clone());
        made
    }

    /// The character the bot made.
    pub fn made(&self) -> &Made {
        self.made.as_ref().expect("the bot has made its character")
    }

    pub fn pid(&self) -> i64 {
        self.made().pid
    }

    /// Where its character stands, as the world last put it.
    pub fn place(&self) -> Place {
        self.place.expect("the bot has selected its character")
    }

    /// 10 for the character it made (step 3; Amendment A): a type-0 script message comes back, 772
    /// for that pid, as `world::load_scene` builds it. Where 772 puts the character, which the bot
    /// keeps.
    pub async fn select(&mut self, pid: i64) -> Place {
        assert_eq!(pid, self.pid(), "a bot plays the character it made");
        self.send(lobby::SELECT_ACTOR, &[Value::Int(pid)]).await;
        let m = self.recv_message().await;
        assert_eq!(m.ty, Stc::Script.to_u8(), "772 is a script message");
        let at = loads_at(pid, &m.body);
        self.place = Some(at);
        at
    }

    /// Type 3 (step 3; Amendment A): its own 7, which [`assert_own_seven`] checks against where 772
    /// put it, then its 8, with no 23 between them. The body of 7.
    pub async fn load(&mut self) -> Vec<u8> {
        let (made, at) = (self.made().clone(), self.place());
        self.send_native(Cts::ClientEnter.to_u8(), &CLIENT_ENTER)
            .await;
        let seven = self.recv_message().await;
        assert_eq!(seven.ty, Stc::SyncEnter.to_u8(), "its own 7");
        assert_own_seven(&made, at, &seven.body);
        self.expect_finish(made.pid).await;
        seven.body
    }

    /// The next message is `pid`'s 8, `world::sync_enter_finish`.
    async fn expect_finish(&mut self, pid: i64) {
        let eight = self.recv_message().await;
        assert_eq!(
            (eight.ty, eight.body),
            (
                Stc::SyncEnterFinish.to_u8(),
                world::sync_enter_finish(pid).to_vec()
            ),
            "{pid}'s 8"
        );
    }

    /// The next message is a 6 telling of `pid` at `at` (step 4; Amendment B), as
    /// `world::sync_id_enter` builds it.
    pub async fn expect_announced(&mut self, pid: i64, at: Place) {
        let six = self.recv_message().await;
        let want = world::sync_id_enter(pid, [at.x, at.y, at.z]);
        assert_eq!(
            (six.ty, six.body),
            (Stc::SyncIdEnter.to_u8(), want.to_vec()),
            "a 6 for {pid} at {at:?}"
        );
    }

    /// Type 5 pulling `pid` (step 4; Amendment B), with the 18 bytes [`pull`] builds: that
    /// player's 7, then its 8. The body of 7, the same bytes as that player's own client got
    /// (world.md 14.9).
    pub async fn pull(&mut self, pid: i64) -> Vec<u8> {
        self.send_native(Cts::SyncFilterSet.to_u8(), &pull(pid))
            .await;
        self.expect_entered(pid).await
    }

    /// [`Bot::pull`] for each of `pids` at once, as a client pulls whoever it was told of in one
    /// tick: each one's 7 and 8, in the order pulled. The bodies of the 7s.
    pub async fn pull_all(&mut self, pids: &[i64]) -> Vec<Vec<u8>> {
        for &pid in pids {
            self.send_native(Cts::SyncFilterSet.to_u8(), &pull(pid))
                .await;
        }
        let mut sevens = Vec::with_capacity(pids.len());
        for &pid in pids {
            sevens.push(self.expect_entered(pid).await);
        }
        sevens
    }

    /// The next messages are `pid`'s 7 and 8. The body of 7.
    async fn expect_entered(&mut self, pid: i64) -> Vec<u8> {
        let seven = self.recv_message().await;
        assert_eq!(seven.ty, Stc::SyncEnter.to_u8(), "{pid}'s 7");
        assert_eq!(seven.body[..8], pid.to_le_bytes(), "{pid}'s 7");
        self.expect_finish(pid).await;
        seven.body
    }

    /// Type 8 to `at` (step 5; Amendment C), as [`walk`] builds it: at most [`STEP`] units from
    /// where it stands. The world takes it if it fits the speed rule, so the bot then stands there.
    pub async fn walk_to(&mut self, at: [i32; 3]) {
        let (body, to) = walk(self.place(), at);
        self.send_native(Cts::MoveDir.to_u8(), &body).await;
        self.place = Some(to);
    }

    /// The next message is a 13 relaying `pid`'s walk to `at` (step 5; Amendment C), as
    /// `world::move_dir_relay` builds it from the place and [`walking`]'s detail.
    pub async fn expect_moved(&mut self, pid: i64, at: Place) {
        let m = self.recv_message().await;
        let want = world::move_dir_relay(pid, [at.x, at.y, at.z, at.yaw], &walking());
        assert_eq!(
            (m.ty, m.body),
            (Stc::MoveDir.to_u8(), want.to_vec()),
            "a 13 for {pid} at {at:?}"
        );
    }

    /// The next message is a 20 standing the bot's own character at `at` (Amendment C; world.md
    /// 16.12), as a refused move or an admin's `!tp` sends it, with T from the server's clock: the
    /// bot then stands there.
    pub async fn expect_stand(&mut self, at: Place) {
        let m = self.recv_message().await;
        let t = world::sync_time(NOW_MS);
        let want = world::move_sync_svr(self.pid(), t, [at.x, at.y, at.z, at.yaw]);
        assert_eq!(
            (m.ty, m.body),
            (Stc::MoveSyncSvr.to_u8(), want.to_vec()),
            "a 20 at {at:?}"
        );
        self.place = Some(at);
    }

    /// The next message is a 9 taking `pids` away (step 8; world.md 14.11), as `world::sync_leave`
    /// builds it.
    pub async fn expect_left(&mut self, pids: &[i64]) {
        let nine = self.recv_message().await;
        assert_eq!(
            (nine.ty, nine.body),
            (Stc::SyncLeave.to_u8(), world::sync_leave(pids)),
            "a 9 for {pids:?}"
        );
    }

    /// A line on `channel` (step 6): 115 with a nil `extra`, as a type-0 script message, as the
    /// real client sends it.
    pub async fn say(&mut self, channel: i64, text: &str) {
        let body = transmit(channel, text, Value::Nil);
        self.send_native(Cts::Script.to_u8(), &body).await;
    }

    /// A whisper to `to` (step 6): 115 on channel 1 with `to`'s `extra`.
    pub async fn whisper(&mut self, to: Whom<'_>, text: &str) {
        let body = transmit(chat::channel::WHISPER, text, to.extra());
        self.send_native(Cts::Script.to_u8(), &body).await;
    }

    /// The next message, which must be a 378 (step 6): what it says, through [`Heard::read`].
    pub async fn heard(&mut self) -> Heard {
        let m = self.recv_message().await;
        assert_eq!(m.ty, Stc::Script.to_u8(), "378 is a script message");
        Heard::read(&m.body)
    }

    /// The next message is a 270 with `code` (step 6; chat.md §5), as `lobby::error_code` builds
    /// it.
    pub async fn expect_refused(&mut self, code: i64) {
        let m = self.recv_message().await;
        assert_eq!(
            (m.ty, m.body),
            (Stc::Script.to_u8(), lobby::error_code(code)),
            "a 270 with {code}"
        );
    }

    /// The bot's two halves, for a harness that reads and writes at once (Amendment D's load
    /// harness): each takes its own end of the socket, and they share the client, whose two
    /// directions keep their own state. The bot must be in the world.
    pub fn split(self) -> (Reader, Writer) {
        let place = self.place();
        let (read, write) = self.sock.into_split();
        let client = Arc::new(Mutex::new(self.client));
        let reader = Reader {
            sock: read,
            client: Arc::clone(&client),
            events: self.events,
            buf: vec![0; 64 * 1024],
        };
        let writer = Writer {
            sock: write,
            client,
            place,
        };
        (reader, writer)
    }
}

/// A bot's reading half ([`Bot::split`]).
pub struct Reader {
    sock: OwnedReadHalf,
    client: Arc<Mutex<Client>>,
    events: VecDeque<ClientEvent>,
    buf: Vec<u8>,
}

impl Reader {
    /// The next event other than a transport-level time sync, as [`Bot::next`] reads it but
    /// without its deadline: a harness reads until the server ends the connection. `None` once it
    /// has ended.
    pub async fn next(&mut self) -> Option<ClientEvent> {
        loop {
            if let Some(e) = self.events.pop_front() {
                if matches!(e, ClientEvent::TimeSync(_)) {
                    continue;
                }
                return Some(e);
            }
            let n = match self.sock.read(&mut self.buf).await {
                Ok(n) if n > 0 => n,
                _ => return None,
            };
            let Ok((events, out)) = self.client.lock().unwrap().receive(&self.buf[..n]) else {
                return None;
            };
            // A client answers by itself only in its handshake, long over in the world.
            assert!(out.is_empty(), "the client answered by itself in the world");
            self.events.extend(events);
        }
    }
}

/// A bot's writing half ([`Bot::split`]): it walks and says as the bot does, without reading what
/// comes back.
pub struct Writer {
    sock: OwnedWriteHalf,
    client: Arc<Mutex<Client>>,
    place: Place,
}

impl Writer {
    /// A message of type `ty`: false once the connection can't take it.
    async fn send(&mut self, ty: u8, body: &[u8]) -> bool {
        let Ok(bytes) = self.client.lock().unwrap().send(ty, body) else {
            return false;
        };
        self.sock.write_all(&bytes).await.is_ok()
    }

    /// [`Bot::walk_to`]: false once the connection can't take it.
    pub async fn walk_to(&mut self, at: [i32; 3]) -> bool {
        let (body, to) = walk(self.place, at);
        let sent = self.send(Cts::MoveDir.to_u8(), &body).await;
        if sent {
            self.place = to;
        }
        sent
    }

    /// [`Bot::say`]: false once the connection can't take it.
    pub async fn say(&mut self, channel: i64, text: &str) -> bool {
        let body = transmit(channel, text, Value::Nil);
        self.send(Cts::Script.to_u8(), &body).await
    }

    /// Where its character stands, as the bot last walked it.
    pub fn place(&self) -> Place {
        self.place
    }
}

/// A time-sync body as the client sends it (world.md 7.2): its clock's low 32 bits, a byte 1, and
/// two counters, zero here.
pub fn time_sync(clock: u32) -> Vec<u8> {
    let mut b = clock.to_le_bytes().to_vec();
    b.push(1);
    b.extend([0; 8]);
    b
}

/// Type 3's body as the client builds it (Amendment A's V3).
pub const CLIENT_ENTER: [u8; 21] = [
    1, 0, 0, 0, 0, 0, 0, 0, 0, 0x65, 0x4a, 0x00, 0x0a, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// The client's pull of `pid` (type 5 with 0; world.md 14.3, 14.4): the actor, the value, a byte 1,
/// a zero `u32` and the machine byte, 0 here.
pub fn pull(pid: i64) -> Vec<u8> {
    let mut b = pid.to_le_bytes().to_vec();
    b.extend(world::PULL.to_le_bytes());
    b.push(1);
    b.extend([0; 5]);
    b
}

/// The test server's clock, which no test moves, in the move codecs' terms: a multiple of 16.
pub const NOW_MS: u64 = NOW as u64;

/// A 200-byte type 8 as the client builds it (world.md §16, the plan's V11 layout): the time, x,
/// y and z, the yaw, then the direction part with the direction, the yaw again, the yaw offset 0,
/// the rate and the pitches 0. The state blocks and the 24 bytes after the part are 0.
pub fn move_body(time: u64, at: [i32; 3], yaw: i16, dir: u8, rate: u16) -> Vec<u8> {
    let mut b = time.to_le_bytes().to_vec();
    for c in at {
        b.extend(c.to_le_bytes());
    }
    b.extend([0; 5]);
    b.extend(yaw.to_le_bytes());
    b.extend([0; 138]);
    b.push(dir);
    b.extend(yaw.to_le_bytes());
    b.extend([0, 0]);
    b.extend(rate.to_le_bytes());
    b.extend([0; 28]);
    assert_eq!(b.len(), world::MOVE_DIR_LEN);
    b
}

// --- The protocol bot's script (M4 world plan, Amendment D): what it sends and how it reads.

/// The speed rate every player runs at in M4 (world spec §3.4): 100 is 500 units a second.
pub const RATE: u16 = 100;

/// The farthest a bot walks in one step (Amendment D, step 5). Nothing paces the steps: the
/// callers keep inside the speed rule, which at rate 100 and the default tolerance allows 1,275
/// units a second and saves up at most a second of that. `world_bot.rs` waits for each step's
/// relay before the next, and the load harness steps 40 units every 100 ms, 400 a second.
pub const STEP: f64 = 50.0;

/// Type 7's header, before its blob (world.md 4.1): the pid (8 bytes), the actor type (1), the
/// difficulty (1) and the template (4).
pub const SEVEN_HEADER: usize = 8 + 1 + 1 + 4;

/// The team channel, which the client's team line goes on (chat.md §7) and M4 doesn't carry:
/// the world refuses a line on it with `CHAT_INVALID_CHANNEL`.
pub const TEAM: i64 = 2;

/// A character as its bot made it (Amendment D, step 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Made {
    pub pid: i64,
    pub name: String,
    pub vocation: i64,
    pub skeleton: i64,
}

impl Made {
    /// The player link every line it says names it by (chat.md 2.2).
    pub fn link(&self) -> String {
        chat::player_link(&chat::Player {
            pid: self.pid,
            name: &self.name,
            vocation: self.vocation,
        })
    }
}

/// `[world]`'s start in `cfg`: where a character that never entered stands.
pub fn start_of(cfg: &Config) -> Place {
    let [x, y, z, yaw] = cfg.world.start_position;
    Place {
        scene: cfg.world.start_scene,
        x,
        y,
        z,
        yaw,
    }
}

/// Where a 772 for `pid` puts the player (Amendment A; world.md §2): its scene and its four
/// floats, checked against the bytes `world::load_scene` builds for them, which hold the pid and
/// `PROPS_VERSION` too.
pub fn loads_at(pid: i64, body: &[u8]) -> Place {
    let v = pickle::decode(body).unwrap();
    assert_eq!(v.first(), Some(&Value::Int(world::LOAD_SCENE)), "{v:?}");
    let (Some(Value::Table(scene)), Some(Value::Table(at))) = (v.get(2), v.get(3)) else {
        panic!("772's scene and place: {v:?}")
    };
    let Some(Value::Int(scene)) = field(scene, "scene_tid") else {
        panic!("772's scene: {v:?}")
    };
    let [
        Value::Float(x),
        Value::Float(y),
        Value::Float(z),
        Value::Float(yaw),
    ] = at.array[..]
    else {
        panic!("772's place: {v:?}")
    };
    assert_eq!(
        body,
        world::load_scene(pid, *scene, [x, y, z, yaw]),
        "772 for {pid} as the codec builds it"
    );
    Place {
        scene: *scene,
        x,
        y,
        z,
        yaw,
    }
}

/// Checks a player's own 7 (Amendment A; world.md §5) through `world::decode_blob`: it is
/// `made`'s, at level 37, where 772 put it (`at`), at speed 100 and full health.
pub fn assert_own_seven(made: &Made, at: Place, seven: &[u8]) {
    use world::{BlobValue, prop};
    assert_eq!(seven[..8], made.pid.to_le_bytes(), "the pid");
    let blob = world::decode_blob(&seven[SEVEN_HEADER..]).unwrap();
    let get = |i: u16| {
        blob.iter()
            .find(|(index, _)| *index == i)
            .map(|(_, v)| v.clone())
    };
    let name = BlobValue::Str(made.name.as_bytes().to_vec());
    assert_eq!(get(prop::NAME), Some(name), "the name");
    assert_eq!(get(prop::VOCATION), Some(BlobValue::Int(made.vocation)));
    assert_eq!(get(prop::SKELETON), Some(BlobValue::Int(made.skeleton)));
    assert_eq!(get(prop::LEVEL), Some(BlobValue::Int(37)), "the level");
    // Positions are f32 in the blob (world.md 4.12).
    let place = BlobValue::Vec([at.x, at.y, at.z, at.yaw].map(|c| c as f32));
    assert_eq!(get(prop::POSITION), Some(place), "where 772 put it");
    let speed = BlobValue::Int(RATE.into());
    assert_eq!(get(prop::MOVE_SPEED), Some(speed), "the speed");
    assert_eq!(
        get(prop::HP_RATIO),
        Some(BlobValue::Num(1.0)),
        "full health"
    );
    let full = Some(BlobValue::Int(world::FULL_HEALTH));
    assert_eq!(get(prop::HP_MAX), full, "full health");
    assert_eq!(get(prop::HP), full, "full health");
}

/// A walk from `from` to `at` (Amendment D, step 5): the type 8 a bot sends, `move_body`'s with the
/// server's clock, direction 1 and rate 100, facing as it was; and where it then stands.
pub fn walk(from: Place, at: [i32; 3]) -> (Vec<u8>, Place) {
    let to = Place {
        x: at[0].into(),
        y: at[1].into(),
        z: at[2].into(),
        yaw: from.yaw.round(),
        ..from
    };
    let far = ((to.x - from.x).powi(2) + (to.y - from.y).powi(2) + (to.z - from.z).powi(2)).sqrt();
    assert!(
        far <= STEP,
        "a bot walks at most {STEP} units a step, not {far}"
    );
    (move_body(NOW_MS, at, to.yaw as i16, 1, RATE), to)
}

/// The detail a viewer's 13 carries for any bot's walk (Amendment C; world.md 16.15): what `walk`
/// sends, bounded by the link against the server's clock.
pub fn walking() -> MoveDetail {
    let sent = MoveDetail {
        time: NOW_MS,
        dir: 1,
        yaw_offset: 0,
        rate: RATE,
        pitch: 0,
        pitch2: 0,
    };
    MoveDetail::from_bytes(&sent.to_bytes(), NOW_MS).unwrap()
}

/// 115's body as the client's `net_manager_send` sends it (chat.md 1.1, 1.2): the channel, the text
/// and `extra`, for a type-0 script message.
pub fn transmit(channel: i64, text: &str, extra: Value) -> Vec<u8> {
    let args = [Value::Int(channel), s(text), extra];
    script_body(chat::CHAT_TRANSMIT, &args).unwrap()
}

/// Whom a whisper is for, as the client's input box sets its target (chat.md 1.4).
#[derive(Debug, Clone, Copy)]
pub enum Whom<'a> {
    /// Typed with `/w <name>`: the name alone.
    Typed(&'a str),
    /// Set by clicking the player's name in a line: its pid, then its name.
    Clicked { pid: i64, name: &'a str },
}

impl Whom<'_> {
    /// The whisper's `extra`: the name with the client's `@Unknown`, since Fenji sends no server
    /// names (chat.md 1.4), after the pid when the name was clicked.
    pub fn extra(self) -> Value {
        let (pid, name) = match self {
            Self::Typed(name) => (None, name),
            Self::Clicked { pid, name } => (Some(pid), name),
        };
        let mut hash = Vec::new();
        if let Some(pid) = pid {
            hash.push((s("pid"), Value::Int(pid)));
        }
        hash.push((s("name"), s(&format!("{name}@Unknown"))));
        Value::Table(Table {
            array: Vec::new(),
            hash,
        })
    }
}

/// A 378 as a bot hears it (chat.md §2): the channel, the sender's link, the content and the
/// receiver's link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heard {
    pub channel: i64,
    pub sender: String,
    pub content: String,
    pub receiver: String,
}

impl Heard {
    /// A player's line (Amendment D, step 6): `from`'s link, and for a whisper `to`'s.
    pub fn said(channel: i64, from: &Made, text: &str, to: Option<&Made>) -> Self {
        Self {
            channel,
            sender: from.link(),
            content: text.into(),
            receiver: to.map(Made::link).unwrap_or_default(),
        }
    }

    /// An admin's announcement: the marquee (66), for everyone, with no sender.
    pub fn marquee(text: &str) -> Self {
        Self::server(chat::channel::MARQUEE, text)
    }

    /// A line from the server to one player (64), such as a command's answer, with no sender.
    pub fn system(text: &str) -> Self {
        Self::server(chat::channel::SYSTEM, text)
    }

    fn server(channel: i64, text: &str) -> Self {
        Self {
            channel,
            sender: String::new(),
            content: text.into(),
            receiver: String::new(),
        }
    }

    /// The 378 in a script message's `body`, through `pickle::decode`: all six arguments, the last
    /// two nil (chat.md 2.1).
    pub fn read(body: &[u8]) -> Self {
        let v = pickle::decode(body).unwrap();
        assert_eq!(
            v.first(),
            Some(&Value::Int(chat::SYS_CHAT_MESSAGE)),
            "{v:?}"
        );
        assert_eq!(v.len(), 7, "378 and its six arguments: {v:?}");
        assert_eq!(v[5..], [Value::Nil, Value::Nil], "no options or template");
        let Value::Int(channel) = v[1] else {
            panic!("378's channel: {v:?}")
        };
        let text = |i: usize| match &v[i] {
            Value::Str(b) => String::from_utf8(b.clone()).unwrap(),
            other => panic!("{other:?} in {v:?}"),
        };
        Self {
            channel,
            sender: text(2),
            content: text(3),
            receiver: text(4),
        }
    }
}

/// The pid of the actor in 628's or a list's table.
pub fn pid_of(actor: &Table) -> i64 {
    match field(actor, "pid") {
        Some(Value::Int(pid)) => *pid,
        other => panic!("no pid: {other:?}"),
    }
}
