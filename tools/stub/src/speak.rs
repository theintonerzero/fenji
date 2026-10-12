//! `--speak`: the stub's side of the gate handshake, driven by `fenji_proto::handshake::Server`
//! (M2 spec §5). Raw bytes in both directions go to the session log; decoded events go to
//! `<stem>.events`; the auth body goes to `<stem>.auth.bin` for analysis (all under re/).
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::net::SocketAddr;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use fenji_proto::codec::StoredCodec;
use fenji_proto::handshake::{Event, Server};
use fenji_proto::hello::{HELLO_LEN, Variants};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::events::{EventLog, hex};
use crate::session::SessionLog;

/// What to answer the client's auth message with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthReply {
    /// `ok` in a stored frame (rung R3).
    Ok,
    /// Nothing: stop after reading the auth (rungs R1–R2).
    None,
    /// `REJECT_REASON` in a stored frame: R3's control. A client that decodes and reads the
    /// frame closes with `CLOSED_AUTH` (transport.md 6.7).
    Reject,
}

/// The close reason `AuthReply::Reject` sends (never empty and never `ok`).
pub const REJECT_REASON: &[u8] = b"fenji-stub: rejected (control run)";

#[derive(Debug, Clone, Copy)]
pub struct SpeakOpts {
    pub variants: Variants,
    pub auth_reply: AuthReply,
}

/// Fresh bytes for the hello. The hello goes out in clear and the keys are folded from it, so it
/// needs to differ per connection, not to be secret: std's per-process random hasher keys plus the
/// clock are enough.
fn fill() -> [u8; HELLO_LEN] {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut out = [0u8; HELLO_LEN];
    for (i, chunk) in out.chunks_mut(8).enumerate() {
        let mut h = RandomState::new().build_hasher();
        h.write_usize(i);
        h.write_u128(nanos);
        chunk.copy_from_slice(&h.finish().to_le_bytes());
    }
    out
}

fn other(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

pub async fn handle(
    mut stream: TcpStream,
    local: SocketAddr,
    peer: SocketAddr,
    out_dir: &Path,
    opts: SpeakOpts,
) -> io::Result<()> {
    let mut log = SessionLog::create(out_dir, local, peer)?;
    let mut events = EventLog::create(&log.path().with_extension("events"))?;
    tracing::info!(%peer, %local, log = %log.path().display(), "connection (speak)");
    let (mut server, hello) = Server::start(fill(), opts.variants).map_err(other)?;
    stream.write_all(&hello).await?;
    log.record_sent(&hello)?;
    events.line(
        "hello",
        &format!("down={} up={}", opts.variants.down, opts.variants.up),
    )?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut decoding = true;
    loop {
        let n = match stream.read(&mut buf).await {
            Ok(0) => {
                events.line("closed", "peer closed")?;
                return log.close("peer closed");
            }
            Ok(n) => n,
            Err(error) => {
                events.line("closed", &format!("read error: {error}"))?;
                log.close(&format!("read error: {error}"))?;
                return Err(error);
            }
        };
        log.record(&buf[..n])?;
        if !decoding {
            continue;
        }
        let got = match server.receive(&buf[..n]) {
            Ok(got) => got,
            Err(error) => {
                // Keep logging raw bytes; stop decoding a stream we no longer follow.
                events.line("error", &error.to_string())?;
                decoding = false;
                continue;
            }
        };
        for event in got {
            match event {
                Event::Auth(a) => {
                    events.line(
                        "auth",
                        &format!(
                            "dict_id={} machine={} payload_len={}",
                            hex(&a.dict_id),
                            hex(&a.machine),
                            a.payload.len()
                        ),
                    )?;
                    std::fs::write(log.path().with_extension("auth.bin"), a.to_body())?;
                    let reply = match opts.auth_reply {
                        AuthReply::Ok => Some((server.accept(&StoredCodec), "ok")),
                        AuthReply::Reject => {
                            Some((server.reject(REJECT_REASON, &StoredCodec), "reject"))
                        }
                        AuthReply::None => None,
                    };
                    if let Some((out, what)) = reply {
                        let out = out.map_err(other)?;
                        stream.write_all(&out).await?;
                        log.record_sent(&out)?;
                        events.line("sent", &format!("{what} (stored frame)"))?;
                    }
                }
                Event::TimeSync(ms) => events.line("time_sync", &format!("client_ms={ms}"))?,
                Event::Message(m) => {
                    let head = &m.body[..m.body.len().min(32)];
                    events.line(
                        "message",
                        &format!("ty={} len={} head={}", m.ty, m.body.len(), hex(head)),
                    )?;
                }
            }
        }
    }
}
