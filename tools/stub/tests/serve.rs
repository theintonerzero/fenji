use std::path::Path;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};

/// Starts the stub on an ephemeral localhost port, logging into `dir`.
async fn start(dir: &Path) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(fenji_stub::server::serve(listener, dir.to_path_buf()));
    addr
}

/// Waits (up to 5 s) until `count` session logs in `dir` have their closing line.
async fn closed_logs(dir: &Path, count: usize) -> Vec<String> {
    for _ in 0..100 {
        let logs: Vec<String> = std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(|e| std::fs::read_to_string(e.ok()?.path()).ok())
                    .filter(|text| text.contains("# closed after"))
                    .collect()
            })
            .unwrap_or_default();
        if logs.len() >= count {
            return logs;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("expected {count} closed session logs in {}", dir.display());
}

#[tokio::test]
async fn logs_bytes_a_client_sends() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = start(tmp.path()).await;

    let mut client = TcpStream::connect(addr).await.unwrap();
    client.write_all(b"\x01\x02hello").await.unwrap();
    client.shutdown().await.unwrap();

    let logs = closed_logs(tmp.path(), 1).await;
    assert!(logs[0].contains("01 02 68 65 6c 6c 6f"), "{}", logs[0]);
    assert!(
        logs[0].contains("# closed after 7 bytes: peer closed"),
        "{}",
        logs[0]
    );
}

#[tokio::test]
async fn connection_that_sends_nothing_is_still_logged() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = start(tmp.path()).await;

    drop(TcpStream::connect(addr).await.unwrap());

    let logs = closed_logs(tmp.path(), 1).await;
    assert!(
        logs[0].contains("# closed after 0 bytes: peer closed"),
        "{}",
        logs[0]
    );
}

#[tokio::test]
async fn concurrent_connections_get_separate_logs() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = start(tmp.path()).await;

    let mut a = TcpStream::connect(addr).await.unwrap();
    let mut b = TcpStream::connect(addr).await.unwrap();
    a.write_all(b"AAAA").await.unwrap();
    b.write_all(b"BB").await.unwrap();
    a.shutdown().await.unwrap();
    b.shutdown().await.unwrap();

    let logs = closed_logs(tmp.path(), 2).await;
    assert!(logs.iter().any(|l| l.contains("# closed after 4 bytes")));
    assert!(logs.iter().any(|l| l.contains("# closed after 2 bytes")));
}

#[tokio::test]
async fn large_burst_is_logged_completely() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = start(tmp.path()).await;

    let payload = vec![0xabu8; 200_000];
    let mut client = TcpStream::connect(addr).await.unwrap();
    client.write_all(&payload).await.unwrap();
    client.shutdown().await.unwrap();

    let logs = closed_logs(tmp.path(), 1).await;
    assert!(
        logs[0].contains("# closed after 200000 bytes"),
        "tail: {}",
        &logs[0][logs[0].len().saturating_sub(200)..]
    );
}
