//! The bot (M3 spec §6): a client on `fenji_proto::client::Client` over a real socket, which sends
//! what the real client sends on the launcher path, and helpers to set up a server for it.
#![allow(dead_code)]
use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use fenji_auth::invite::InviteCode;
use fenji_proto::client::{Client, ClientEvent};
use fenji_proto::handshake::AuthMessage;
use fenji_proto::packet::script_body;
use fenji_proto::pickle::{self, Table, Value};
use fenji_server::clock::TestClock;
use fenji_server::config::Config;
use fenji_server::server::{GateLimits, RunningServer, start};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

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
                s("v:5;2:1,2,3;3:1,2,3,4,5;4:1,2,3,4"),
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
}
