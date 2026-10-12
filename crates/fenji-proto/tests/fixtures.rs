//! Real-client fixtures from sandboxed runs (docs/protocol/transport.md §9), scrubbed: the machine
//! bytes and the auth payload are synthetic.
use fenji_proto::frame::{Frame, UpReader};
use fenji_proto::handshake::AuthMessage;
use fenji_proto::header::split_message;
use fenji_proto::hello::{CLIENT_MAGIC, check};
use fenji_proto::variant::Cipher;

fn unhex(s: &str) -> Vec<u8> {
    let s: String = s.split_whitespace().collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn r2_client_auth_decodes() {
    let hello: [u8; 64] = unhex(include_str!("fixtures/r2-hello.hex"))
        .try_into()
        .unwrap();
    let up = unhex(include_str!("fixtures/r2-up.hex"));
    let v = check(&hello, CLIENT_MAGIC).unwrap();
    let cipher = Cipher::new(v.up, &hello).unwrap();
    let mut reader = UpReader::new();
    let mut stream = Vec::new();
    let mut at = 0;
    while let Some((frame, used)) = reader.read(&up[at..]) {
        at += used;
        if let Frame::Data(mut body) = frame {
            cipher.decrypt(&mut body);
            stream.extend(body);
        }
    }
    let (h, body, _) = split_message(&stream).expect("one whole message");
    assert_eq!(h.ty, 0);
    let auth = AuthMessage::parse(body).unwrap();
    assert!(!auth.payload.is_empty());
}

#[test]
fn fixture_is_scrubbed() {
    let hello: [u8; 64] = unhex(include_str!("fixtures/r2-hello.hex"))
        .try_into()
        .unwrap();
    let up = unhex(include_str!("fixtures/r2-up.hex"));
    let v = check(&hello, CLIENT_MAGIC).unwrap();
    let cipher = Cipher::new(v.up, &hello).unwrap();
    let mut reader = UpReader::new();
    let mut stream = Vec::new();
    let mut at = 0;
    while let Some((frame, used)) = reader.read(&up[at..]) {
        at += used;
        if let Frame::Data(mut body) = frame {
            cipher.decrypt(&mut body);
            stream.extend(body);
        }
    }
    let (_, body, _) = split_message(&stream).unwrap();
    let auth = AuthMessage::parse(body).unwrap();
    assert_eq!(auth.machine, [0x02, 0, 0, 0, 0, 0x01]);
    assert!(
        auth.payload.iter().all(|&b| b == 0),
        "payload must be zeroed until Task 10"
    );
}
