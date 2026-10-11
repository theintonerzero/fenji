use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};

use crate::session::SessionLog;

/// Accepts connections forever, logging each one to its own file in `out_dir`.
/// Never writes to the client. Transient accept errors (e.g. too many open files) are logged
/// and retried rather than stopping the server.
pub async fn serve(listener: TcpListener, out_dir: PathBuf) -> io::Result<()> {
    let local = listener.local_addr()?;
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(conn) => conn,
            Err(error) => {
                tracing::warn!(%local, %error, "accept failed; retrying");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let out_dir = out_dir.clone();
        tokio::spawn(async move {
            if let Err(error) = handle(stream, local, peer, &out_dir).await {
                tracing::warn!(%peer, %error, "session ended with error");
            }
        });
    }
}

async fn handle(
    mut stream: TcpStream,
    local: SocketAddr,
    peer: SocketAddr,
    out_dir: &Path,
) -> io::Result<()> {
    let mut log = SessionLog::create(out_dir, local, peer)?;
    tracing::info!(%peer, %local, log = %log.path().display(), "connection");
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match stream.read(&mut buf).await {
            Ok(0) => {
                tracing::info!(%peer, bytes = log.received(), "peer closed");
                return log.close("peer closed");
            }
            Ok(n) => {
                log.record(&buf[..n])?;
                tracing::info!(%peer, bytes = n, "received");
            }
            Err(error) => {
                log.close(&format!("read error: {error}"))?;
                return Err(error);
            }
        }
    }
}
