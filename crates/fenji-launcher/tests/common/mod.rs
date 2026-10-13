//! What the launcher's integration tests share: a canned account API on 127.0.0.1.
#![allow(dead_code)] // each test crate uses its own part

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

pub fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// What the canned API answers: a status line ("200 OK") and a JSON body, by method and path.
pub type Answer = Box<dyn Fn(&str, &str) -> (&'static str, String) + Send>;

/// A canned account API on 127.0.0.1 that records each request's method and path.
pub fn canned(answer: Answer) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    std::thread::spawn(move || {
        for sock in listener.incoming() {
            let Ok(mut sock) = sock else { return };
            let mut reader = BufReader::new(sock.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
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
            let req: Vec<&str> = line.split(' ').take(2).collect();
            if req.len() < 2 {
                continue;
            }
            log.lock().unwrap().push(req.join(" "));
            let (status, json) = answer(req[0], req[1]);
            let reply = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{json}",
                json.len()
            );
            let _ = sock.write_all(reply.as_bytes());
        }
    });
    (format!("http://{addr}"), seen)
}

pub fn server_info(gate_host: &str) -> String {
    format!(
        r#"{{"name":"Fenji","version":"0.4.0","pinned_build":"10015727","gate_host":"{gate_host}","gate_port":7800,"server_id":1,"api_version":1}}"#
    )
}

pub fn grant(server_name: &str) -> String {
    format!(
        r#"{{"ticket":"fenji1-{}","expires_in_secs":60,"gate_host":"127.0.0.1","gate_port":7800,"server_id":1,"server_name":"{server_name}","account":"tester","user_id":5}}"#,
        "ab".repeat(32)
    )
}

/// The usual answers: server info with a local gate, a session, a game token, a sign-out.
pub fn usual(method: &str, path: &str) -> (&'static str, String) {
    match (method, path) {
        ("GET", "/api/v1/server") => ("200 OK", server_info("127.0.0.1")),
        ("POST", "/api/v1/accounts") => ("201 Created", r#"{"username":"tester"}"#.into()),
        ("POST", "/api/v1/sessions") => (
            "200 OK",
            r#"{"token":"fenji1-session","expires_at":1}"#.into(),
        ),
        ("POST", "/api/v1/game-tokens") => ("200 OK", grant("Fenji")),
        ("DELETE", "/api/v1/sessions") => ("204 No Content", String::new()),
        _ => ("404 Not Found", r#"{"error":"not_found"}"#.into()),
    }
}
