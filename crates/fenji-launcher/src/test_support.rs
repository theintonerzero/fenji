//! What the crate's tests share.
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

/// Held by every test that writes an executable or starts a process. A child forked while another
/// test still has its fresh script open for writing would make that script "Text file busy"
/// (ETXTBSY) to exec, so the two never overlap.
static SPAWN: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) fn spawning() -> std::sync::MutexGuard<'static, ()> {
    SPAWN.lock().unwrap_or_else(|e| e.into_inner())
}

/// A TLS server on loopback presenting `cert` and proving it with `key`. It reads each request
/// and answers it as `answer(its first line)` says: after how long, and with what JSON body (as
/// `200 OK`), on a connection it then closes. Returns its port.
pub(crate) async fn tls_server(
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
    answer: impl Fn(&str) -> (Duration, &'static str) + Send + Sync + 'static,
) -> u16 {
    #[derive(Debug)]
    struct Fixed(Arc<rustls::sign::CertifiedKey>);
    impl rustls::server::ResolvesServerCert for Fixed {
        fn resolve(
            &self,
            _: rustls::server::ClientHello<'_>,
        ) -> Option<Arc<rustls::sign::CertifiedKey>> {
            Some(self.0.clone())
        }
    }
    let signing = rustls::crypto::ring::sign::any_supported_type(&key).unwrap();
    let certified = Arc::new(rustls::sign::CertifiedKey::new(vec![cert], signing));
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_cert_resolver(Arc::new(Fixed(certified)));
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let answer = Arc::new(answer);
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let (acceptor, answer) = (acceptor.clone(), answer.clone());
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(tcp).await else {
                    return;
                };
                let Some(line) = request_line(&mut tls).await else {
                    return;
                };
                let (wait, body) = answer(&line);
                tokio::time::sleep(wait).await;
                let reply = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = tls.write_all(reply.as_bytes()).await;
                let _ = tls.shutdown().await;
            });
        }
    });
    port
}

/// Reads a request's head and then its body (by its `content-length`), and gives back its first
/// line.
async fn request_line(s: &mut (impl AsyncRead + Unpin)) -> Option<String> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if s.read(&mut byte).await.ok()? == 0 || head.len() > 16 * 1024 {
            return None;
        }
        head.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    let length = head
        .lines()
        .filter_map(|l| l.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, n)| n.trim().parse().ok())
        .unwrap_or(0);
    let mut body = vec![0; length];
    s.read_exact(&mut body).await.ok()?;
    head.lines().next().map(str::to_string)
}
