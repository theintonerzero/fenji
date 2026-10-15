//! The account API over TLS with the server's own key (M4 hosting spec §3.10, H1). A host without
//! a domain sets `[server] tls = "self"`: the server makes an ECDSA P-256 key and a self-signed
//! certificate on its first start, keeps both in `data/tls/`, and serves the API with them. Invite
//! links carry the key's pin, the base64url SHA-256 of the certificate's SubjectPublicKeyInfo
//! (Ruling HP2), which launchers check in place of a certificate authority.
//!
//! Every connection is accepted at once, then given a place among the open connections and one
//! among the handshakes, both counted by the peer's network (Ruling HP9), or closed at once. Each
//! handshake runs on a task of its own within a time limit (Review Focus 3), so connections that
//! never finish theirs hold their network's places for the limit at most, and keep no other
//! network from the account API. A finished one is handed to axum in a [`Conn`], which keeps its
//! open place and ends it once it has sent nothing for the idle time (`crate::listen`).
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, bail};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use sha2::{Digest, Sha256};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;

use crate::clock::{Clock, SystemClock, Utc};
use crate::listen::{ApiPlaces, Conn, ConnLimits, local_of};

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
    /// player holds. So is a folder that other users can open (Unix), or a key and certificate
    /// that don't go together.
    pub fn load_or_make(dir: &Path, public_host: &str) -> anyhow::Result<ServerKey> {
        match key_files(dir)? {
            Some((key, cert)) => {
                owner_only(dir)?;
                load(&key, &cert)
            }
            None => make(dir, public_host),
        }
    }
}

/// Refuses a key folder that other users can open (on Unix): the server's key is its own alone.
fn owner_only(dir: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir)
            .with_context(|| format!("can't read {}'s permissions", dir.display()))?
            .permissions()
            .mode()
            & 0o777;
        if mode & 0o077 != 0 {
            bail!(
                "{0} holds the server's key but other users can open it (mode {mode:03o}): run \
                 `chmod 700 {0}`",
                dir.display()
            );
        }
    }
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
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

/// The key and certificate at these paths, which must go together: the certificate's public key
/// the key's.
fn load(key_path: &Path, cert_path: &Path) -> anyhow::Result<ServerKey> {
    let cert = read_cert(cert_path)?;
    let pem =
        std::fs::read(key_path).with_context(|| format!("can't read {}", key_path.display()))?;
    let key = PrivateKeyDer::from_pem_slice(&pem)
        .map_err(|e| anyhow::anyhow!("{} isn't a PEM private key: {e}", key_path.display()))?;
    let provider = rustls::crypto::ring::default_provider();
    rustls::sign::CertifiedKey::from_der(vec![cert.clone()], key.clone_key(), &provider).map_err(
        |e| {
            anyhow::anyhow!(
                "{} and {} don't go together ({e}): restore both from one backup",
                key_path.display(),
                cert_path.display()
            )
        },
    )?;
    let pin = pin_of(&cert).with_context(|| format!("in {}", cert_path.display()))?;
    Ok(ServerKey { cert, key, pin })
}

/// Files removed when dropped, whatever happens: a start's own temporary ones.
struct Temporary([PathBuf; 2]);

impl Drop for Temporary {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// A name beside `path` for this start's own temporary copy of it, which no other start shares.
fn temporary_beside(path: &Path) -> anyhow::Result<PathBuf> {
    let mut nonce = [0u8; 8];
    getrandom::fill(&mut nonce).map_err(|e| anyhow::anyhow!("no random bytes: {e}"))?;
    let nonce: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("tls");
    let name = format!(".{name}.{}-{nonce}.new", std::process::id());
    Ok(path.with_file_name(name))
}

/// Makes the key and its certificate in `dir`, an owner-only folder (0700) on Unix: made if it's
/// missing, refused if other users can open it. Each file is written whole under a name of this
/// start's own, then linked into place, the key first. A link never replaces a file, so a start
/// that finds the files there, another first start's made meanwhile, loads those instead, and two
/// starts never serve two keys. A crash between the two links leaves the key without its
/// certificate, which the next start refuses, as it refuses any key without its certificate.
fn make(dir: &Path, public_host: &str) -> anyhow::Result<ServerKey> {
    crate::config::private_folder(dir)
        .with_context(|| format!("can't make the key's folder {}", dir.display()))?;
    owner_only(dir)?;
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
    let own = Temporary([temporary_beside(&key_path)?, temporary_beside(&cert_path)?]);
    let [key_new, cert_new] = &own.0;
    for stale in [key_new, cert_new] {
        let _ = std::fs::remove_file(stale);
    }
    write_file(key_new, key.serialize_pem().as_bytes(), true)?;
    write_file(cert_new, cert.pem().as_bytes(), false)?;
    for (from, to) in [(key_new, &key_path), (cert_new, &cert_path)] {
        match std::fs::hard_link(from, to) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                tracing::info!(
                    "found the server's key made meanwhile in {}: serving that one",
                    dir.display()
                );
                return load(&key_path, &cert_path);
            }
            Err(e) => {
                let e = anyhow::Error::new(e);
                return Err(e.context(format!(
                    "can't link {} to {}",
                    from.display(),
                    to.display()
                )));
            }
        }
    }
    drop(own);
    let made = load(&key_path, &cert_path)?;
    tracing::info!(
        pin = %made.pin,
        "made the server's own key in {}: invite links carry its pin, so keep the folder with \
         your backups",
        dir.display()
    );
    Ok(made)
}

/// Writes `bytes` to `path` as a new file, owner-only (0600) on Unix when `private`, from the
/// moment it exists: never into a file or through a link already there. A file this call made
/// and couldn't finish is removed.
fn write_file(path: &Path, bytes: &[u8], private: bool) -> anyhow::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if private {
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    }
    #[cfg(not(unix))]
    let _ = private;
    let mut file = options
        .open(path)
        .with_context(|| format!("can't make {}", path.display()))?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    if written.is_err() {
        drop(file);
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

/// How the TLS listener bounds handshakes (M4 hosting spec §3, exact values; Ruling HP9): each
/// must finish within `handshake`, at most `in_flight` run at once, and at most
/// `in_flight_per_network` of those from one network (an IPv4 address or an IPv6 /64), so that
/// one host can't take them all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TlsLimits {
    pub handshake: Duration,
    pub in_flight: usize,
    pub in_flight_per_network: usize,
}

impl Default for TlsLimits {
    fn default() -> Self {
        Self {
            handshake: Duration::from_secs(10),
            in_flight: 64,
            in_flight_per_network: 8,
        }
    }
}

/// A connection whose handshake has finished, with its peer's address.
type Finished = (Conn<TlsStream<TcpStream>>, SocketAddr);

/// The account API's listener under `tls = "self"`: TCP connections in, finished TLS streams out,
/// for `axum::serve`. A task of its own accepts, and runs each handshake on a task of its own;
/// dropping the listener ends them all, and closes the port.
pub struct TlsListener {
    local: SocketAddr,
    finished: mpsc::Receiver<Finished>,
}

impl TlsListener {
    /// Starts accepting on `tcp`, with `key`, under `limits` and `conns`, `proxies` being
    /// `[server] trusted_proxies`: rustls with the `ring` provider, TLS 1.2 and 1.3, and ALPN
    /// `http/1.1`.
    pub fn new(
        tcp: TcpListener,
        key: &ServerKey,
        limits: TlsLimits,
        conns: ConnLimits,
        proxies: &[IpAddr],
    ) -> anyhow::Result<TlsListener> {
        let local = tcp.local_addr()?;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut config = rustls::ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])?
            .with_no_client_auth()
            .with_single_cert(vec![key.cert.clone()], key.key.clone_key())
            .context("the server's key and certificate don't go together")?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        let places = Places {
            open: ApiPlaces::new(conns.open, conns.open_per_network, proxies),
            handshakes: ApiPlaces::new(limits.in_flight, limits.in_flight_per_network, proxies),
        };
        let (done, finished) = mpsc::channel(limits.in_flight.max(1));
        let tls = TlsAcceptor::from(Arc::new(config));
        tokio::spawn(accept(tcp, tls, limits, conns, places, done));
        Ok(TlsListener { local, finished })
    }
}

/// The places a connection takes as it's accepted: one among the open connections, which it keeps
/// until it closes, and one among the handshakes, which it gives back once its handshake ends.
struct Places {
    open: ApiPlaces,
    handshakes: ApiPlaces,
}

/// Accepts on `tcp` while the listener holds `done`'s other end. Every connection is accepted at
/// once, never kept waiting for a place, and closed at once without both of its places; with them,
/// its handshake runs on a task of its own, within the time limit, and its stream is handed over
/// in a [`Conn`] under `conns`. Each place is a guard, given back when dropped: when a handshake
/// fails, runs out of time or panics, when its stream can't be handed over, when the connection
/// ends, and when the listener goes, which ends this task and with it the handshakes' tasks. The
/// handshakes that ended are reaped at every turn, so the set holds no more than those in flight
/// and those that ended since; the channel to axum holds as many as there are handshake places.
async fn accept(
    mut tcp: TcpListener,
    tls: TlsAcceptor,
    limits: TlsLimits,
    conns: ConnLimits,
    places: Places,
    done: mpsc::Sender<Finished>,
) {
    let mut handshakes = JoinSet::new();
    loop {
        while handshakes.try_join_next().is_some() {}
        let (stream, peer) = tokio::select! {
            () = done.closed() => return,
            // axum's own accept: it waits out and logs the errors a TCP listener can meet.
            accepted = axum::serve::Listener::accept(&mut tcp) => accepted,
        };
        let Some(open) = places.open.take(peer.ip()) else {
            tracing::debug!(%peer, "closed at once: no open connection's place free for it");
            continue;
        };
        let Some(handshake) = places.handshakes.take(peer.ip()) else {
            tracing::debug!(%peer, "closed at once: no handshake's place free for it");
            continue;
        };
        let local = local_of(&stream);
        let (tls, done) = (tls.clone(), done.clone());
        handshakes.spawn(async move {
            let _handshake = handshake;
            match tokio::time::timeout(limits.handshake, tls.accept(stream)).await {
                Ok(Ok(stream)) => {
                    let _ = done
                        .send((Conn::new(stream, local, open, conns), peer))
                        .await;
                }
                Ok(Err(e)) => tracing::debug!(%peer, error = %e, "a TLS handshake failed"),
                Err(_) => tracing::debug!(%peer, "a TLS handshake ran out of time"),
            }
        });
    }
}

impl axum::serve::Listener for TlsListener {
    type Io = Conn<TlsStream<TcpStream>>;
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
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    use super::*;
    use crate::api::Addrs;

    /// A temporary folder, and the key's folder in it, `tls/`: made owner-only by the first
    /// `load_or_make`, as the server's is.
    fn key_dir() -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("tls");
        (root, dir)
    }

    #[test]
    fn the_key_is_made_once_and_kept() {
        let (_root, dir) = key_dir();
        let first = ServerKey::load_or_make(dir.as_path(), "203.0.113.5").unwrap();
        assert_eq!(first.pin.len(), 43);
        let again = ServerKey::load_or_make(dir.as_path(), "198.51.100.7").unwrap();
        assert_eq!(
            again.pin, first.pin,
            "the same key, whatever public_host says now"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.as_path().join("key.pem"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::remove_file(dir.as_path().join("cert.pem")).unwrap();
        assert!(
            ServerKey::load_or_make(dir.as_path(), "203.0.113.5").is_err(),
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
        let (_root, dir) = key_dir();
        ServerKey::load_or_make(dir.as_path(), "127.0.0.1").unwrap();
        let key = std::fs::read(dir.as_path().join("key.pem")).unwrap();
        std::fs::remove_file(dir.as_path().join("cert.pem")).unwrap();
        let e = format!(
            "{:#}",
            ServerKey::load_or_make(dir.as_path(), "127.0.0.1").unwrap_err()
        );
        assert!(e.contains("cert.pem"), "names what is missing: {e}");
        assert_eq!(
            std::fs::read(dir.as_path().join("key.pem")).unwrap(),
            key,
            "the key stays as it was"
        );
        assert!(!dir.as_path().join("cert.pem").exists());
        // And the other way round.
        let (_root, dir) = key_dir();
        ServerKey::load_or_make(dir.as_path(), "127.0.0.1").unwrap();
        std::fs::remove_file(dir.as_path().join("key.pem")).unwrap();
        let e = format!(
            "{:#}",
            ServerKey::load_or_make(dir.as_path(), "127.0.0.1").unwrap_err()
        );
        assert!(e.contains("key.pem"), "names what is missing: {e}");
    }

    #[test]
    fn the_pin_is_the_hash_of_the_keys_subject_public_key_info() {
        // Ruling HP2: computed from the certificate with rustls, it must equal the hash of the key's
        // own SubjectPublicKeyInfo, as rcgen encodes it from the private key.
        let (_root, dir) = key_dir();
        let made = ServerKey::load_or_make(dir.as_path(), "127.0.0.1").unwrap();
        let pem = std::fs::read_to_string(dir.as_path().join("key.pem")).unwrap();
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
        let (_root, dir) = key_dir();
        assert_eq!(pin_in(dir.as_path()).unwrap(), None, "no key made yet");
        let made = ServerKey::load_or_make(dir.as_path(), "127.0.0.1").unwrap();
        assert_eq!(pin_in(dir.as_path()).unwrap(), Some(made.pin));
        std::fs::remove_file(dir.as_path().join("key.pem")).unwrap();
        assert!(
            pin_in(dir.as_path()).is_err(),
            "a certificate without its key"
        );
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

    #[test]
    fn the_pin_is_the_one_openssl_computes() {
        // A known answer, so that the launcher's pin (hosting Task 5) can't drift from the
        // server's: tests/fixtures/pin-known-answer.pem, an openssl certificate whose pin is
        //   openssl x509 -in pin-known-answer.pem -pubkey -noout | openssl pkey -pubin -outform der \
        //     | openssl dgst -sha256 -binary | basenc --base64url
        // without its padding.
        let pem = include_bytes!("../tests/fixtures/pin-known-answer.pem");
        let cert = CertificateDer::from_pem_slice(pem).unwrap();
        assert_eq!(
            pin_of(&cert).unwrap(),
            "6QFQfpzx9v37ULM885PDlbUAQse-jH9cH7nfRVOL9Lo"
        );
    }

    /// The files in `dir`, sorted.
    fn names(dir: &std::path::Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_key_another_start_made_meanwhile_is_loaded_and_never_replaced() {
        // Two first starts on one folder: the one that finds the files there when it moves its
        // own in loads them, so both serve the same key.
        let (_root, dir) = key_dir();
        let first = ServerKey::load_or_make(dir.as_path(), "127.0.0.1").unwrap();
        let key = std::fs::read(dir.as_path().join(KEY_FILE)).unwrap();
        let second = make(dir.as_path(), "127.0.0.1").unwrap();
        assert_eq!(second.pin, first.pin);
        assert_eq!(std::fs::read(dir.as_path().join(KEY_FILE)).unwrap(), key);
        assert_eq!(
            names(dir.as_path()),
            ["cert.pem", "key.pem"],
            "no file left over"
        );
    }

    #[test]
    fn a_key_and_certificate_that_dont_go_together_are_refused() {
        let ((_a, a), (_b, b)) = (key_dir(), key_dir());
        ServerKey::load_or_make(a.as_path(), "127.0.0.1").unwrap();
        ServerKey::load_or_make(b.as_path(), "127.0.0.1").unwrap();
        std::fs::copy(b.as_path().join(KEY_FILE), a.as_path().join(KEY_FILE)).unwrap();
        let e = format!(
            "{:#}",
            ServerKey::load_or_make(a.as_path(), "127.0.0.1").unwrap_err()
        );
        assert!(e.contains("don't go together"), "{e}");
        assert!(
            pin_in(a.as_path()).is_ok(),
            "the certificate alone still has its pin"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_new_file_never_follows_a_link_planted_in_its_place() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::write(&target, b"the host's").unwrap();
        let planted = dir.path().join("key.pem.new");
        std::os::unix::fs::symlink(&target, &planted).unwrap();
        assert!(write_file(&planted, b"a key", true).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"the host's");
    }

    #[cfg(unix)]
    #[test]
    fn a_key_folder_other_users_can_open_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let tls = dir.path().join("tls");
        ServerKey::load_or_make(&tls, "127.0.0.1").unwrap();
        let open = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&tls, open.clone()).unwrap();
        let e = format!(
            "{:#}",
            ServerKey::load_or_make(&tls, "127.0.0.1").unwrap_err()
        );
        assert!(e.contains("chmod 700") && e.contains("tls"), "{e}");
        // One made by hand, empty: refused before a key goes in it.
        let made = dir.path().join("made-by-hand");
        std::fs::create_dir(&made).unwrap();
        std::fs::set_permissions(&made, open).unwrap();
        assert!(ServerKey::load_or_make(&made, "127.0.0.1").is_err());
        assert!(names(&made).is_empty(), "no key made: {:?}", names(&made));
    }

    /// A TLS listener on a free loopback port under `limits` and `conns`, serving a router whose
    /// `/` says `ok` when the connection's own address reached it: its address.
    async fn serving(dir: &std::path::Path, limits: TlsLimits, conns: ConnLimits) -> SocketAddr {
        let key = ServerKey::load_or_make(&dir.join("tls"), "127.0.0.1").unwrap();
        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = tcp.local_addr().unwrap();
        let listener = TlsListener::new(tcp, &key, limits, conns, &[]).unwrap();
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

    /// Handshakes limited to `ms` milliseconds, with the default places.
    fn within_ms(ms: u64) -> TlsLimits {
        TlsLimits {
            handshake: Duration::from_millis(ms),
            ..TlsLimits::default()
        }
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

    /// `n` connections to `addr` that never begin a handshake.
    async fn idle(addr: SocketAddr, n: usize) -> Vec<TcpStream> {
        futures_util::future::join_all((0..n).map(|_| TcpStream::connect(addr)))
            .await
            .into_iter()
            .map(Result::unwrap)
            .collect()
    }

    /// Whether the server has closed `conn` within `within`: reading on comes to its end, empty,
    /// reset or cut short, past whatever came first (such as an alert for a failed handshake),
    /// rather than still waiting.
    async fn closed_within<S: AsyncRead + Unpin>(conn: &mut S, within: Duration) -> bool {
        let mut rest = Vec::new();
        tokio::time::timeout(within, conn.read_to_end(&mut rest))
            .await
            .is_ok()
    }

    /// A certificate verifier that takes any certificate, but checks the handshake's signatures:
    /// these tests are about places and times, not trust.
    #[derive(Debug)]
    struct AnyCertificate(Arc<rustls::crypto::CryptoProvider>);

    impl rustls::client::danger::ServerCertVerifier for AnyCertificate {
        fn verify_server_cert(
            &self,
            _: &CertificateDer<'_>,
            _: &[CertificateDer<'_>],
            _: &rustls::pki_types::ServerName<'_>,
            _: &[u8],
            _: rustls::pki_types::UnixTime,
        ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            let algorithms = &self.0.signature_verification_algorithms;
            rustls::crypto::verify_tls12_signature(message, cert, dss, algorithms)
        }

        fn verify_tls13_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            let algorithms = &self.0.signature_verification_algorithms;
            rustls::crypto::verify_tls13_signature(message, cert, dss, algorithms)
        }

        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            self.0.signature_verification_algorithms.supported_schemes()
        }
    }

    /// `tcp` after a finished TLS handshake, with nothing sent over it.
    async fn handshaken(tcp: TcpStream) -> tokio_rustls::client::TlsStream<TcpStream> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
            .with_safe_default_protocol_versions()
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AnyCertificate(provider)))
            .with_no_client_auth();
        let name = rustls::pki_types::ServerName::try_from("127.0.0.1").unwrap();
        tokio_rustls::TlsConnector::from(Arc::new(config))
            .connect(name, tcp)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn handshakes_that_never_finish_dont_keep_others_out() {
        // Review Focus 3, with short limits so the test is quick: four idle connections, then a
        // request, from one network with places to spare (one past them is closed at once, below).
        let dir = tempfile::tempdir().unwrap();
        let addr = serving(dir.path(), within_ms(300), ConnLimits::default()).await;
        let client = client();
        let idle = idle(addr, 4).await;
        let started = Instant::now();
        get_ok(&client, addr).await;
        let took = started.elapsed();
        assert!(took < Duration::from_secs(3), "{took:?}");
        drop(idle);
    }

    #[tokio::test]
    async fn handshakes_run_side_by_side() {
        // Idle connections hold up no one: under a limit far beyond the client's patience, the
        // request isn't queued behind theirs.
        let dir = tempfile::tempdir().unwrap();
        let addr = serving(dir.path(), within_ms(20_000), ConnLimits::default()).await;
        let client = client();
        let idle = idle(addr, 6).await;
        let started = Instant::now();
        get_ok(&client, addr).await;
        let took = started.elapsed();
        assert!(took < Duration::from_secs(5), "{took:?}");
        drop(idle);
    }

    #[tokio::test]
    async fn a_connection_past_the_handshake_places_is_closed_at_once_not_queued() {
        // Ruling HP9: every connection is accepted, and one with no place left is closed then,
        // never kept waiting for a place.
        let dir = tempfile::tempdir().unwrap();
        let limits = TlsLimits {
            handshake: Duration::from_secs(10),
            in_flight: 2,
            in_flight_per_network: 2,
        };
        let addr = serving(dir.path(), limits, ConnLimits::default()).await;
        let idle = idle(addr, 2).await;
        let mut third = TcpStream::connect(addr).await.unwrap();
        let started = Instant::now();
        assert!(closed_within(&mut third, Duration::from_secs(2)).await);
        let took = started.elapsed();
        assert!(
            took < limits.handshake / 4,
            "not after a time limit: {took:?}"
        );
        drop(idle);
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
        let limits = within_ms(300);
        let addr = serving(dir.path(), limits, ConnLimits::default()).await;
        let started = Instant::now();
        let mut silent = TcpStream::connect(addr).await.unwrap();
        let mut partial = TcpStream::connect(addr).await.unwrap();
        partial.write_all(&PARTIAL_CLIENT_HELLO).await.unwrap();
        for stream in [&mut silent, &mut partial] {
            let mut rest = Vec::new();
            // The server's end closes: the read ends, empty or reset.
            let read = stream.read_to_end(&mut rest);
            let _ = tokio::time::timeout(Duration::from_secs(3), read)
                .await
                .expect("closed by the server within its limit");
            assert!(rest.is_empty(), "no answer: {rest:?}");
        }
        let took = started.elapsed();
        assert!(took >= limits.handshake, "not before the limit: {took:?}");
    }

    #[tokio::test]
    async fn a_connection_idle_after_its_handshake_is_closed_after_the_idle_time() {
        // Ruling HP9, part 2: the handshake done, a connection that sends nothing more is let go,
        // which hyper would never do without a timer.
        let dir = tempfile::tempdir().unwrap();
        let conns = ConnLimits {
            idle: Duration::from_millis(300),
            ..ConnLimits::default()
        };
        let addr = serving(dir.path(), TlsLimits::default(), conns).await;
        let mut tls = handshaken(TcpStream::connect(addr).await.unwrap()).await;
        let started = Instant::now();
        assert!(closed_within(&mut tls, Duration::from_secs(3)).await);
        let took = started.elapsed();
        assert!(took >= conns.idle, "not before the idle time: {took:?}");
    }

    #[tokio::test]
    async fn a_connection_that_isnt_tls_is_dropped_and_the_listener_goes_on() {
        let dir = tempfile::tempdir().unwrap();
        let addr = serving(dir.path(), TlsLimits::default(), ConnLimits::default()).await;
        let mut plain = TcpStream::connect(addr).await.unwrap();
        plain
            .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .unwrap();
        let mut answer = Vec::new();
        let read = tokio::time::timeout(Duration::from_secs(5), plain.read_to_end(&mut answer))
            .await
            .expect("closed, not left open");
        assert!(
            read.is_err() || !answer.starts_with(b"HTTP"),
            "no HTTP over the TLS port"
        );
        get_ok(&client(), addr).await;
    }

    /// Whether a fresh connection to `addr` is served within 3 s, by a client that closes it after
    /// its one request; then a moment for the close to reach the server.
    async fn served_again(addr: SocketAddr) -> bool {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .tls_danger_accept_invalid_certs(true)
            .pool_max_idle_per_host(0)
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if let Ok(answer) = client.get(format!("https://{addr}/")).send().await
                && answer.text().await.is_ok_and(|body| body == "ok")
            {
                tokio::time::sleep(Duration::from_millis(200)).await;
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }

    #[tokio::test]
    async fn every_place_comes_back_however_its_connection_ends() {
        // One place of each kind, so each step needs the one before to have given both back.
        let dir = tempfile::tempdir().unwrap();
        let limits = TlsLimits {
            handshake: Duration::from_millis(300),
            in_flight: 1,
            in_flight_per_network: 1,
        };
        let conns = ConnLimits {
            open: 1,
            open_per_network: 1,
            idle: Duration::from_millis(300),
            ..ConnLimits::default()
        };
        let addr = serving(dir.path(), limits, conns).await;
        assert!(
            served_again(addr).await,
            "served, then closed by its client"
        );
        let mut failed = TcpStream::connect(addr).await.unwrap();
        failed
            .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .unwrap();
        assert!(closed_within(&mut failed, Duration::from_secs(2)).await);
        assert!(served_again(addr).await, "after a handshake that failed");
        let mut silent = TcpStream::connect(addr).await.unwrap();
        assert!(closed_within(&mut silent, Duration::from_secs(3)).await);
        assert!(served_again(addr).await, "after a handshake out of time");
        let mut idle = handshaken(TcpStream::connect(addr).await.unwrap()).await;
        assert!(closed_within(&mut idle, Duration::from_secs(3)).await);
        assert!(
            served_again(addr).await,
            "after a connection ended for idling"
        );
        drop(TcpStream::connect(addr).await.unwrap());
        assert!(
            served_again(addr).await,
            "after a client that left mid-handshake"
        );
    }

    #[test]
    fn the_limits_default_to_ten_seconds_and_64_handshakes_8_from_a_network() {
        let d = TlsLimits::default();
        assert_eq!(
            (d.handshake, d.in_flight, d.in_flight_per_network),
            (Duration::from_secs(10), 64, 8)
        );
    }

    #[tokio::test]
    async fn dropping_the_listener_closes_its_port() {
        let dir = tempfile::tempdir().unwrap();
        let key = ServerKey::load_or_make(&dir.path().join("tls"), "127.0.0.1").unwrap();
        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = tcp.local_addr().unwrap();
        let listener =
            TlsListener::new(tcp, &key, TlsLimits::default(), ConnLimits::default(), &[]).unwrap();
        drop(listener);
        let deadline = Instant::now() + Duration::from_secs(5);
        while TcpStream::connect(addr).await.is_ok() {
            assert!(Instant::now() < deadline, "the port is still open");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Places by network (Ruling HP9), from a second loopback address: Linux routes all of
    /// 127.0.0.0/8 to loopback, where macOS has 127.0.0.1 alone unless told otherwise.
    #[cfg(target_os = "linux")]
    mod other_networks {
        use tokio::net::TcpSocket;

        use super::*;

        /// A connection to `addr` from `source`.
        async fn from(source: &str, addr: SocketAddr) -> TcpStream {
            let sock = TcpSocket::new_v4().unwrap();
            sock.bind(format!("{source}:0").parse().unwrap()).unwrap();
            sock.connect(addr).await.unwrap()
        }

        #[tokio::test]
        async fn one_networks_stalled_handshakes_keep_no_other_network_out() {
            // Part 1: past its network's places, a silent connection is closed at once, and a
            // player on another network is answered at once.
            let dir = tempfile::tempdir().unwrap();
            let limits = TlsLimits {
                handshake: Duration::from_secs(10),
                in_flight: 64,
                in_flight_per_network: 2,
            };
            let addr = serving(dir.path(), limits, ConnLimits::default()).await;
            let mut flood = Vec::new();
            for _ in 0..6 {
                flood.push(from("127.0.0.2", addr).await);
            }
            let client = client();
            let started = Instant::now();
            get_ok(&client, addr).await;
            let took = started.elapsed();
            assert!(took < limits.handshake / 4, "{took:?}");
            let closes = flood
                .iter_mut()
                .map(|conn| closed_within(conn, Duration::from_secs(2)));
            let closed = futures_util::future::join_all(closes)
                .await
                .into_iter()
                .filter(|&c| c)
                .count();
            assert_eq!(closed, 6 - 2, "all but the network's places");
        }

        #[tokio::test]
        async fn one_networks_open_connections_keep_no_other_network_out() {
            // Part 2: three finished handshakes hold 127.0.0.2's places, its fourth connection is
            // closed at once, and 127.0.0.1 still gets in.
            let dir = tempfile::tempdir().unwrap();
            let conns = ConnLimits {
                open_per_network: 3,
                ..ConnLimits::default()
            };
            let addr = serving(dir.path(), TlsLimits::default(), conns).await;
            let mut held = Vec::new();
            for _ in 0..3 {
                held.push(handshaken(from("127.0.0.2", addr).await).await);
            }
            let mut fourth = from("127.0.0.2", addr).await;
            assert!(closed_within(&mut fourth, Duration::from_secs(2)).await);
            for conn in &mut held {
                assert!(!closed_within(conn, Duration::from_millis(50)).await);
            }
            get_ok(&client(), addr).await;
        }
    }
}
