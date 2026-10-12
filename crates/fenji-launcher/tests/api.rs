//! The account API client against canned servers (M3 plan Task 7, review of #134).
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;

use fenji_launcher::api::{ApiClient, ApiError};

/// Answers every request on a new loopback port with `reply` (a whole HTTP response).
fn answering(reply: String) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for sock in listener.incoming() {
            let Ok(mut sock) = sock else { return };
            let mut reader = BufReader::new(sock.try_clone().unwrap());
            let mut len = 0;
            loop {
                let mut h = String::new();
                if reader.read_line(&mut h).is_err() || h == "\r\n" || h.is_empty() {
                    break;
                }
                if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0; len];
            let _ = reader.read_exact(&mut body);
            let _ = sock.write_all(reply.as_bytes());
        }
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn a_redirect_is_never_followed() {
    let trap = TcpListener::bind("127.0.0.1:0").unwrap();
    let to = format!("http://{}/api/v1/sessions", trap.local_addr().unwrap());
    let server = answering(format!(
        "HTTP/1.1 307 Temporary Redirect\r\nlocation: {to}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
    ));
    let api = ApiClient::new(&server).unwrap();
    let e = api.sign_in("tester", "a long password").await.unwrap_err();
    assert!(matches!(e, ApiError::Server { status: 307, .. }), "{e:?}");
    trap.set_nonblocking(true).unwrap();
    assert!(trap.accept().is_err(), "the redirect was followed");
}

#[tokio::test]
async fn network_errors_say_what_went_wrong() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let api = ApiClient::new(&format!("http://127.0.0.1:{port}")).unwrap();
    let e = api.server_info().await.unwrap_err().to_string();
    assert!(e.to_ascii_lowercase().contains("connect"), "{e}");
}

#[tokio::test]
async fn an_oversized_answer_is_refused() {
    let body = "x".repeat(100_000);
    let server = answering(format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    ));
    let api = ApiClient::new(&server).unwrap();
    let e = api.server_info().await.unwrap_err().to_string();
    assert!(e.contains("too large"), "{e}");
}

#[tokio::test]
async fn a_hostile_error_code_is_not_shown() {
    let body = "{\"error\":\"\\u001b[2Jgotcha\"}";
    let server = answering(format!(
        "HTTP/1.1 400 Bad Request\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    ));
    let api = ApiClient::new(&server).unwrap();
    let e = api.sign_in("tester", "a long password").await.unwrap_err();
    assert!(
        matches!(e, ApiError::Server { status: 400, ref code, .. } if code == "unknown"),
        "{e:?}"
    );
}

#[test]
fn a_bad_address_is_not_echoed() {
    let e = ApiClient::new("htps://play.example.net/#invite=ABCD-EFGH-JKMN-PQRS")
        .err()
        .unwrap()
        .to_string();
    assert!(!e.contains("ABCD"), "{e}");
}
