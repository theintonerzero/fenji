//! The account API over TLS with the server's own key (M4 hosting spec §3.10, H1). A host without
//! a domain sets `[server] tls = "self"`: the server makes an ECDSA P-256 key and a self-signed
//! certificate on its first start, keeps both in `data/tls/`, and serves the API with them. Invite
//! links carry the key's pin, the base64url SHA-256 of the certificate's SubjectPublicKeyInfo
//! (Ruling HP2), which launchers check in place of a certificate authority.
//!
//! Each handshake runs on a task of its own, within a time limit, and only so many at once (Review
//! Focus 3): connections that never finish theirs hold a place for the limit at most, and keep no
//! one else from the account API. The rest queue in the system's backlog until a place frees.
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, bail};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use sha2::{Digest, Sha256};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;

use crate::clock::{Clock, SystemClock, Utc};

/// The key's file in its folder: PKCS #8 in PEM, owner-only (0600) on Unix.
pub const KEY_FILE: &str = "key.pem";

/// The certificate's file in the key's folder: PEM.
pub const CERT_FILE: &str = "cert.pem";

/// How long the certificate is valid: ten years. The pin is the key's, so a new certificate for
/// the same key would keep it.
const VALID_YEARS: i64 = 10;

/// Where the server keeps its key: `tls/` in the data folder, the directory of `[store] path`.
pub fn folder(store_path: &Path) -> PathBuf {
    store_path.parent().unwrap_or(Path::new("")).join("tls")
}

/// The server's own key, its self-signed certificate, and the certificate's pin.
pub struct ServerKey {
    pub cert: CertificateDer<'static>,
    pub key: PrivateKeyDer<'static>,
    pub pin: String,
}

impl std::fmt::Debug for ServerKey {
    /// The pin alone: never the private key.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerKey")
            .field("pin", &self.pin)
            .finish_non_exhaustive()
    }
}

impl ServerKey {
    /// The key in `dir`, made there with a certificate naming `public_host` if neither file is
    /// there. The certificate's names don't matter to launchers, which pin the key. One file
    /// without the other is an error, and nothing is made: a new key would change the pin every
    /// player holds.
    pub fn load_or_make(dir: &Path, public_host: &str) -> anyhow::Result<ServerKey> {
        match key_files(dir)? {
            Some((key, cert)) => load(&key, &cert),
            None => make(dir, public_host),
        }
    }
}

/// The pin of the key in `dir`, read from its certificate alone: `invite create`'s, which may run
/// as a user who can't read the key. `None` when the server hasn't made its key yet; nothing is
/// made here. One file without the other is an error, as for [`ServerKey::load_or_make`].
pub fn pin_in(dir: &Path) -> anyhow::Result<Option<String>> {
    match key_files(dir)? {
        Some((_, cert)) => Ok(Some(pin_of(&read_cert(&cert)?)?)),
        None => Ok(None),
    }
}

/// The pin of `cert`: base64url without padding of the SHA-256 of its SubjectPublicKeyInfo, as
/// rustls parses it (Ruling HP2). 43 characters.
pub fn pin_of(cert: &CertificateDer<'_>) -> anyhow::Result<String> {
    let parsed = rustls::server::ParsedCertificate::try_from(cert)
        .map_err(|e| anyhow::anyhow!("the certificate doesn't parse: {e}"))?;
    let spki = parsed.subject_public_key_info();
    Ok(base64url(&Sha256::digest(spki.as_ref())))
}

/// The key's and the certificate's files in `dir` when both are there; `None` when neither is.
fn key_files(dir: &Path) -> anyhow::Result<Option<(PathBuf, PathBuf)>> {
    let (key, cert) = (dir.join(KEY_FILE), dir.join(CERT_FILE));
    let there = |p: &Path| {
        p.try_exists()
            .with_context(|| format!("can't tell whether {} is there", p.display()))
    };
    let (have, missing) = match (there(&key)?, there(&cert)?) {
        (true, true) => return Ok(Some((key, cert))),
        (false, false) => return Ok(None),
        (true, false) => (KEY_FILE, CERT_FILE),
        (false, true) => (CERT_FILE, KEY_FILE),
    };
    bail!(
        "{} has the server's {have} but not its {missing}: restore both from a backup. Deleting \
         {have} instead makes a new key at the next start, and every player then needs a new link",
        dir.display()
    )
}

fn read_cert(path: &Path) -> anyhow::Result<CertificateDer<'static>> {
    let pem = std::fs::read(path).with_context(|| format!("can't read {}", path.display()))?;
    CertificateDer::from_pem_slice(&pem)
        .map_err(|e| anyhow::anyhow!("{} isn't a PEM certificate: {e}", path.display()))
}

fn load(key_path: &Path, cert_path: &Path) -> anyhow::Result<ServerKey> {
    let cert = read_cert(cert_path)?;
    let pem =
        std::fs::read(key_path).with_context(|| format!("can't read {}", key_path.display()))?;
    let key = PrivateKeyDer::from_pem_slice(&pem)
        .map_err(|e| anyhow::anyhow!("{} isn't a PEM private key: {e}", key_path.display()))?;
    let pin = pin_of(&cert).with_context(|| format!("in {}", cert_path.display()))?;
    Ok(ServerKey { cert, key, pin })
}

/// Makes the key and its certificate in `dir`, owner-only (0700) on Unix. Each file is written
/// whole beside its place, then moved in, the key first: a write that fails leaves neither.
fn make(dir: &Path, public_host: &str) -> anyhow::Result<ServerKey> {
    crate::config::private_folder(dir)
        .with_context(|| format!("can't make the key's folder {}", dir.display()))?;
    let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;
    let mut params = rcgen::CertificateParams::new(vec![public_host.to_owned()])
        .with_context(|| format!("can't name {public_host:?} in a certificate"))?;
    let mut name = rcgen::DistinguishedName::new();
    name.push(rcgen::DnType::CommonName, "Fenji");
    params.distinguished_name = name;
    // From yesterday, for clocks a little behind, for ten years; the 28th stands in for a 29
    // February that the year ten years on may lack.
    let now = SystemClock.now_ms();
    let date = |t: Utc, years_on: i64, last_day: i64| -> anyhow::Result<_> {
        let year = i32::try_from(t.year + years_on)?;
        let (month, day) = (u8::try_from(t.month)?, u8::try_from(t.day.min(last_day))?);
        Ok(rcgen::date_time_ymd(year, month, day))
    };
    params.not_before = date(Utc::from_ms(now - 86_400_000), 0, 31)?;
    params.not_after = date(Utc::from_ms(now), VALID_YEARS, 28)?;
    let cert = params.self_signed(&key)?;
    let (key_path, cert_path) = (dir.join(KEY_FILE), dir.join(CERT_FILE));
    let (key_new, cert_new) = (dir.join("key.pem.new"), dir.join("cert.pem.new"));
    write_file(&key_new, key.serialize_pem().as_bytes(), true)?;
    write_file(&cert_new, cert.pem().as_bytes(), false)?;
    for (from, to) in [(&key_new, &key_path), (&cert_new, &cert_path)] {
        std::fs::rename(from, to)
            .with_context(|| format!("can't move {} to {}", from.display(), to.display()))?;
    }
    let made = load(&key_path, &cert_path)?;
    tracing::info!(
        pin = %made.pin,
        "made the server's own key in {}: invite links carry its pin, so keep the folder with \
         your backups",
        dir.display()
    );
    Ok(made)
}

/// Writes `bytes` to `path` whole, replacing what is there; owner-only (0600) on Unix when
/// `private`, from the moment the file exists.
fn write_file(path: &Path, bytes: &[u8], private: bool) -> anyhow::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if private {
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    }
    #[cfg(not(unix))]
    let _ = private;
    let written = options
        .open(path)
        .and_then(|mut file| file.write_all(bytes).and_then(|()| file.sync_all()));
    if written.is_err() {
        let _ = std::fs::remove_file(path);
    }
    written.with_context(|| format!("can't write {}", path.display()))
}

/// `bytes` in base64url without padding (RFC 4648 §5).
fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        // A chunk of k bytes makes k + 1 characters.
        for i in 0..=chunk.len() {
            out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]));
        }
    }
    out
}

/// How the TLS listener bounds handshakes (M4 hosting spec §3, exact values): each must finish
/// within `handshake`, and at most `in_flight` run at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TlsLimits {
    pub handshake: Duration,
    pub in_flight: usize,
}

impl Default for TlsLimits {
    fn default() -> Self {
        Self {
            handshake: Duration::from_secs(10),
            in_flight: 64,
        }
    }
}

/// A connection whose handshake has finished, with its peer's address.
type Finished = (TlsStream<TcpStream>, SocketAddr);

/// The account API's listener under `tls = "self"`: TCP connections in, finished TLS streams out,
/// for `axum::serve`. A task of its own accepts, and runs each handshake on a task of its own;
/// dropping the listener ends them all, and closes the port.
pub struct TlsListener {
    local: SocketAddr,
    finished: mpsc::Receiver<Finished>,
}

impl TlsListener {
    /// Starts accepting on `tcp`, with `key`, under `limits`: rustls with the `ring` provider, TLS
    /// 1.2 and 1.3, and ALPN `http/1.1`.
    pub fn new(
        tcp: TcpListener,
        key: &ServerKey,
        limits: TlsLimits,
    ) -> anyhow::Result<TlsListener> {
        let local = tcp.local_addr()?;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut config = rustls::ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])?
            .with_no_client_auth()
            .with_single_cert(vec![key.cert.clone()], key.key.clone_key())
            .context("the server's key and certificate don't go together")?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        let in_flight = limits.in_flight.max(1);
        let (done, finished) = mpsc::channel(in_flight);
        tokio::spawn(accept(
            tcp,
            TlsAcceptor::from(Arc::new(config)),
            limits,
            done,
        ));
        Ok(TlsListener { local, finished })
    }
}

/// Accepts on `tcp` while the listener holds `done`'s other end. Before each connection it takes
/// one of `in_flight` places, which the connection's handshake holds until it ends or runs out of
/// time, and until its stream is handed over.
async fn accept(
    mut tcp: TcpListener,
    tls: TlsAcceptor,
    limits: TlsLimits,
    done: mpsc::Sender<Finished>,
) {
    let places = Arc::new(Semaphore::new(limits.in_flight.max(1)));
    let mut handshakes = JoinSet::new();
    loop {
        while handshakes.try_join_next().is_some() {}
        let place = tokio::select! {
            () = done.closed() => return,
            place = Arc::clone(&places).acquire_owned() => match place {
                Ok(place) => place,
                Err(_) => return,
            },
        };
        let (stream, peer) = tokio::select! {
            () = done.closed() => return,
            // axum's own accept: it waits out and logs the errors a TCP listener can meet.
            accepted = axum::serve::Listener::accept(&mut tcp) => accepted,
        };
        let (tls, done) = (tls.clone(), done.clone());
        handshakes.spawn(async move {
            let _place = place;
            match tokio::time::timeout(limits.handshake, tls.accept(stream)).await {
                Ok(Ok(stream)) => {
                    let _ = done.send((stream, peer)).await;
                }
                Ok(Err(e)) => tracing::debug!(%peer, error = %e, "a TLS handshake failed"),
                Err(_) => tracing::debug!(%peer, "a TLS handshake ran out of time"),
            }
        });
    }
}

impl axum::serve::Listener for TlsListener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.finished.recv().await {
            Some(finished) => finished,
            // The accepting task is gone, which only a panic does: no connection comes again.
            None => {
                tracing::error!("the account API's TLS listener stopped accepting");
                std::future::pending().await
            }
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(self.local)
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::time::{Duration, Instant};

    use axum::Router;
    use axum::extract::ConnectInfo;
    use axum::routing::get;
    use sha2::{Digest, Sha256};
    use tokio::net::{TcpListener, TcpStream};

    use super::*;
    use crate::api::Addrs;

    #[test]
    fn the_key_is_made_once_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let first = ServerKey::load_or_make(dir.path(), "203.0.113.5").unwrap();
        assert_eq!(first.pin.len(), 43);
        let again = ServerKey::load_or_make(dir.path(), "198.51.100.7").unwrap();
        assert_eq!(
            again.pin, first.pin,
            "the same key, whatever public_host says now"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join("key.pem"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::remove_file(dir.path().join("cert.pem")).unwrap();
        assert!(
            ServerKey::load_or_make(dir.path(), "203.0.113.5").is_err(),
            "never a silent new key"
        );
    }

    #[test]
    fn the_key_goes_in_an_owner_only_folder_beside_the_database() {
        let dir = tempfile::tempdir().unwrap();
        let tls = folder(&dir.path().join("data/fenji.db"));
        assert_eq!(tls, dir.path().join("data/tls"));
        ServerKey::load_or_make(&tls, "127.0.0.1").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&tls).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }
        let mut names: Vec<String> = std::fs::read_dir(&tls)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["cert.pem", "key.pem"], "nothing else is left");
    }

    #[test]
    fn a_key_without_its_certificate_is_an_error_and_never_replaced() {
        let dir = tempfile::tempdir().unwrap();
        ServerKey::load_or_make(dir.path(), "127.0.0.1").unwrap();
        let key = std::fs::read(dir.path().join("key.pem")).unwrap();
        std::fs::remove_file(dir.path().join("cert.pem")).unwrap();
        let e = format!(
            "{:#}",
            ServerKey::load_or_make(dir.path(), "127.0.0.1").unwrap_err()
        );
        assert!(e.contains("cert.pem"), "names what is missing: {e}");
        assert_eq!(
            std::fs::read(dir.path().join("key.pem")).unwrap(),
            key,
            "the key stays as it was"
        );
        assert!(!dir.path().join("cert.pem").exists());
        // And the other way round.
        let dir = tempfile::tempdir().unwrap();
        ServerKey::load_or_make(dir.path(), "127.0.0.1").unwrap();
        std::fs::remove_file(dir.path().join("key.pem")).unwrap();
        let e = format!(
            "{:#}",
            ServerKey::load_or_make(dir.path(), "127.0.0.1").unwrap_err()
        );
        assert!(e.contains("key.pem"), "names what is missing: {e}");
    }

    #[test]
    fn the_pin_is_the_hash_of_the_keys_subject_public_key_info() {
        // Ruling HP2: computed from the certificate with rustls, it must equal the hash of the key's
        // own SubjectPublicKeyInfo, as rcgen encodes it from the private key.
        let dir = tempfile::tempdir().unwrap();
        let made = ServerKey::load_or_make(dir.path(), "127.0.0.1").unwrap();
        let pem = std::fs::read_to_string(dir.path().join("key.pem")).unwrap();
        let pair = rcgen::KeyPair::from_pem(&pem).unwrap();
        let spki = rcgen::PublicKeyData::subject_public_key_info(&pair);
        assert_eq!(made.pin, base64url(&Sha256::digest(&spki)));
        assert_eq!(pin_of(&made.cert).unwrap(), made.pin);
        assert!(
            made.pin
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "{}",
            made.pin
        );
    }

    #[test]
    fn the_pin_can_be_read_from_the_certificate_alone() {
        // `invite create` reads the pin without the key, which may be the server's user's alone.
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(pin_in(dir.path()).unwrap(), None, "no key made yet");
        let made = ServerKey::load_or_make(dir.path(), "127.0.0.1").unwrap();
        assert_eq!(pin_in(dir.path()).unwrap(), Some(made.pin));
        std::fs::remove_file(dir.path().join("key.pem")).unwrap();
        assert!(pin_in(dir.path()).is_err(), "a certificate without its key");
        let none = tempfile::tempdir().unwrap();
        let missing = none.path().join("tls");
        assert_eq!(pin_in(&missing).unwrap(), None, "no folder, no pin");
        assert!(!missing.exists(), "and reading makes none");
    }

    #[test]
    fn base64url_is_rfc_4648s_url_alphabet_without_padding() {
        for (bytes, text) in [
            (&b""[..], ""),
            (b"f", "Zg"),
            (b"fo", "Zm8"),
            (b"foo", "Zm9v"),
            (b"foob", "Zm9vYg"),
            (b"fooba", "Zm9vYmE"),
            (b"foobar", "Zm9vYmFy"),
            (&[0xfb, 0xff, 0xbf], "-_-_"),
        ] {
            assert_eq!(base64url(bytes), text, "{bytes:?}");
        }
        assert_eq!(base64url(&[0; 32]).len(), 43);
    }

    /// A TLS listener on a free loopback port with `limits`, serving a router whose `/` says `ok`
    /// when the connection's own address reached it: its address.
    async fn serving(dir: &std::path::Path, limits: TlsLimits) -> SocketAddr {
        let key = ServerKey::load_or_make(dir, "127.0.0.1").unwrap();
        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = tcp.local_addr().unwrap();
        let listener = TlsListener::new(tcp, &key, limits).unwrap();
        assert_eq!(axum::serve::Listener::local_addr(&listener).unwrap(), addr);
        let app = Router::new().route(
            "/",
            get(move |ConnectInfo(a): ConnectInfo<Addrs>| async move {
                if a.local == addr && a.peer != addr {
                    "ok"
                } else {
                    "not this connection's addresses"
                }
            }),
        );
        let service = app.into_make_service_with_connect_info::<Addrs>();
        tokio::spawn(axum::serve(listener, service).into_future());
        addr
    }

    /// A client that takes any certificate, as Task 5 brings pinning to the launcher.
    fn client() -> reqwest::Client {
        let _ = rustls::crypto::ring::default_provider().install_default();
        reqwest::Client::builder()
            .tls_danger_accept_invalid_certs(true)
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap()
    }

    async fn get_ok(client: &reqwest::Client, addr: SocketAddr) {
        let body = client
            .get(format!("https://{addr}/"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert_eq!(body, "ok");
    }

    #[tokio::test]
    async fn handshakes_that_never_finish_dont_keep_others_out() {
        // Review Focus 3, with short limits so the test is quick.
        let dir = tempfile::tempdir().unwrap();
        let limits = TlsLimits {
            handshake: Duration::from_millis(300),
            in_flight: 4,
        };
        let addr = serving(dir.path(), limits).await;
        let client = client();
        let started = Instant::now();
        let idle = idle(addr, 4).await;
        get_ok(&client, addr).await;
        let took = started.elapsed();
        assert!(took < Duration::from_secs(3), "{took:?}");
        // The four held every place, the request queued behind them, and its handshake began
        // only once one of theirs had run out of time: at most in_flight run at once.
        assert!(took >= limits.handshake, "{took:?}");
        drop(idle);
    }

    /// `n` connections to `addr` that never begin a handshake.
    async fn idle(addr: SocketAddr, n: usize) -> Vec<TcpStream> {
        futures_util::future::join_all((0..n).map(|_| TcpStream::connect(addr)))
            .await
            .into_iter()
            .map(Result::unwrap)
            .collect()
    }

    /// A TLS record header for a 512-byte handshake record, then a ClientHello's first bytes.
    const PARTIAL_CLIENT_HELLO: [u8; 11] = [
        0x16, 0x03, 0x01, 0x02, 0x00, 0x01, 0x00, 0x01, 0xfc, 0x03, 0x03,
    ];

    #[tokio::test]
    async fn a_handshake_that_stalls_is_closed_when_its_time_runs_out() {
        // Silent, or stopped partway through a ClientHello: the server lets go of either once the
        // limit has passed, which frees its place.
        let dir = tempfile::tempdir().unwrap();
        let limits = TlsLimits {
            handshake: Duration::from_millis(300),
            in_flight: 4,
        };
        let addr = serving(dir.path(), limits).await;
        let started = Instant::now();
        let mut silent = TcpStream::connect(addr).await.unwrap();
        let mut partial = TcpStream::connect(addr).await.unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut partial, &PARTIAL_CLIENT_HELLO)
            .await
            .unwrap();
        for stream in [&mut silent, &mut partial] {
            let mut rest = Vec::new();
            // The server's end closes: the read ends, empty or reset.
            let read = tokio::io::AsyncReadExt::read_to_end(stream, &mut rest);
            let _ = tokio::time::timeout(Duration::from_secs(3), read)
                .await
                .expect("closed by the server within its limit");
            assert!(rest.is_empty(), "no answer: {rest:?}");
        }
        let took = started.elapsed();
        assert!(took >= limits.handshake, "not before the limit: {took:?}");
    }

    #[tokio::test]
    async fn handshakes_run_side_by_side() {
        // With places to spare, idle connections hold up no one: the request isn't queued behind
        // their time limits.
        let dir = tempfile::tempdir().unwrap();
        let limits = TlsLimits {
            handshake: Duration::from_secs(20),
            in_flight: 64,
        };
        let addr = serving(dir.path(), limits).await;
        let client = client();
        let idle = idle(addr, 8).await;
        let started = Instant::now();
        get_ok(&client, addr).await;
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        drop(idle);
    }

    #[tokio::test]
    async fn a_connection_that_isnt_tls_is_dropped_and_the_listener_goes_on() {
        let dir = tempfile::tempdir().unwrap();
        let addr = serving(dir.path(), TlsLimits::default()).await;
        let mut plain = TcpStream::connect(addr).await.unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut plain, b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .unwrap();
        let mut answer = Vec::new();
        let read = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::io::AsyncReadExt::read_to_end(&mut plain, &mut answer),
        )
        .await
        .expect("closed, not left open");
        assert!(
            read.is_err() || !answer.starts_with(b"HTTP"),
            "no HTTP over the TLS port"
        );
        get_ok(&client(), addr).await;
    }

    #[test]
    fn the_limits_default_to_ten_seconds_and_sixty_four_handshakes() {
        let d = TlsLimits::default();
        assert_eq!((d.handshake, d.in_flight), (Duration::from_secs(10), 64));
    }

    #[tokio::test]
    async fn dropping_the_listener_closes_its_port() {
        let dir = tempfile::tempdir().unwrap();
        let key = ServerKey::load_or_make(dir.path(), "127.0.0.1").unwrap();
        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = tcp.local_addr().unwrap();
        let listener = TlsListener::new(tcp, &key, TlsLimits::default()).unwrap();
        drop(listener);
        let deadline = Instant::now() + Duration::from_secs(5);
        while TcpStream::connect(addr).await.is_ok() {
            assert!(Instant::now() < deadline, "the port is still open");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}
