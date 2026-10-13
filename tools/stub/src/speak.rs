//! `--speak`: the stub's side of the gate handshake, driven by `fenji_proto::handshake::Server`
//! (M2 spec §5). Raw bytes in both directions go to the session log; decoded events go to
//! `<stem>.events`; the auth body goes to `<stem>.auth.bin` for analysis (all under re/). With
//! `--no-auth-capture` no file keeps the auth, which can carry a real platform ticket (#102).
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::net::SocketAddr;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use fenji_proto::codec::StoredCodec;
use fenji_proto::handshake::{Event, Server};
use fenji_proto::hello::{HELLO_LEN, Variants};
use fenji_proto::packet::{Up, set_net_connected};
use fenji_proto::pickle::{self, Value};
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
    /// frame closes with `CLOSED_AUTH` (transport.md 6.6).
    Reject,
}

/// The close reason `AuthReply::Reject` sends (never empty and never `ok`).
pub const REJECT_REASON: &[u8] = b"fenji-stub: rejected (control run)";

#[derive(Debug, Clone, Copy)]
pub struct SpeakOpts {
    pub variants: Variants,
    pub auth_reply: AuthReply,
    /// Bytes of padding the work message carries as a fifth argument, which its handler ignores.
    /// 0 sends the plain message. Enough padding takes its stored packet past 251 bytes, to the
    /// `FC` prefix (transport.md 5.2, #88).
    pub work_pad: usize,
    /// Whether the auth reaches `<stem>.auth.bin`, the events log's identifiers and the session
    /// log. When false, the session log skips the frames that carried it (#102).
    pub capture_auth: bool,
}

/// `PROTOG2C_SET_NET_CONNECTED` with a fifth argument of `pad` bytes.
fn padded_work(pad: usize) -> Vec<u8> {
    let mut values = pickle::decode(&set_net_connected()).expect("our own body");
    values.push(Value::Str(vec![b'.'; pad]));
    pickle::encode(&values).expect("five values")
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

/// A script message id as the events log shows it.
fn show(id: &Value) -> String {
    match id {
        Value::Int(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        // Escaped, so an id can't add a tab or a line to the events log.
        Value::Str(s) => String::from_utf8_lossy(s).escape_debug().to_string(),
        other => format!("{other:?}"),
    }
}

fn other(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

/// The answer to the auth, as `opts` says: `ok` then the work message, a rejection, or nothing.
async fn reply(
    stream: &mut TcpStream,
    server: &mut Server,
    log: &mut SessionLog,
    events: &mut EventLog,
    opts: SpeakOpts,
) -> io::Result<()> {
    let answer = match opts.auth_reply {
        AuthReply::Ok => Some((server.accept(&StoredCodec), "ok")),
        AuthReply::Reject => Some((server.reject(REJECT_REASON, &StoredCodec), "reject")),
        AuthReply::None => None,
    };
    if let Some((out, what)) = answer {
        let out = out.map_err(other)?;
        stream.write_all(&out).await?;
        log.record_sent(&out)?;
        events.line("sent", &format!("{what} (stored frame)"))?;
    }
    if opts.auth_reply == AuthReply::Ok {
        // QUEUE -> WORK (transport.md 10.3-10.4).
        let (out, what) = if opts.work_pad == 0 {
            let out = server.begin_work(&StoredCodec);
            (out, "work (SET_NET_CONNECTED)".to_string())
        } else {
            let body = padded_work(opts.work_pad);
            let what = format!("work (SET_NET_CONNECTED, {}-byte body)", body.len());
            (server.begin_work_with(&body, &StoredCodec), what)
        };
        let out = out.map_err(other)?;
        stream.write_all(&out).await?;
        log.record_sent(&out)?;
        events.line("sent", &format!("{what}; state {:?}", server.state()))?;
    }
    Ok(())
}

pub async fn handle(
    mut stream: TcpStream,
    local: SocketAddr,
    peer: SocketAddr,
    out_dir: &Path,
    opts: SpeakOpts,
) -> io::Result<()> {
    let mut log = SessionLog::create(out_dir, local, peer)?;
    if !opts.capture_auth {
        log.hold();
    }
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
        if log.holding()
            && let Some(through) = server.auth_through()
        {
            log.release(usize::try_from(through).unwrap_or(usize::MAX))?;
        }
        for event in got {
            match event {
                Event::Auth(a) if !opts.capture_auth => {
                    events.line(
                        "auth",
                        &format!("(not captured) payload_len={}", a.payload.len()),
                    )?;
                    reply(&mut stream, &mut server, &mut log, &mut events, opts).await?;
                }
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
                    reply(&mut stream, &mut server, &mut log, &mut events, opts).await?;
                }
                Event::TimeSync(ms) => events.line("time_sync", &format!("client_ms={ms}"))?,
                Event::Message(m) => match Up::decode(m.ty, &m.body) {
                    Ok(Up::Script { gateway, id, args }) => {
                        let kind = if gateway { "gateway" } else { "script" };
                        events.line(
                            "call",
                            &format!("{kind} id={} args={}", show(&id), args.len()),
                        )?;
                    }
                    Ok(Up::Unknown { .. }) => {
                        let head = &m.body[..m.body.len().min(32)];
                        events.line(
                            "message",
                            &format!("ty={} len={} head={}", m.ty, m.body.len(), hex(head)),
                        )?;
                    }
                    // A script message the decoder refuses (an unknown tag, say): logged with
                    // the reason, and the stream goes on.
                    Err(error) => {
                        let kind = if m.ty == 0xff { "gateway" } else { "script" };
                        let head = &m.body[..m.body.len().min(32)];
                        events.line(
                            "bad_call",
                            &format!(
                                "{kind} len={} head={} error={error}",
                                m.body.len(),
                                hex(head)
                            ),
                        )?;
                    }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_cannot_break_the_events_log() {
        // The events log is one tab-separated line per event (events.rs).
        let shown = show(&Value::Str(b"a\nb\tc".to_vec()));
        assert!(!shown.contains(['\n', '\t']), "{shown}");
    }
}
