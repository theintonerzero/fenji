//! Real-client fixtures from sandboxed runs (docs/protocol/transport.md §9), scrubbed: the machine
//! bytes and the auth payload are synthetic.
use fenji_proto::frame::{Frame, UpReader};
use fenji_proto::handshake::AuthMessage;
use fenji_proto::header::split_message;
use fenji_proto::hello::{CLIENT_MAGIC, check};
use fenji_proto::pickle::{Value, decode};
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
    // payloads.md: value 9 is the machine fingerprint, zeroed; the rest are the run's own
    // synthetic gj_uinfo/gj_ticket values and client facts.
    let values = decode(&auth.payload).unwrap();
    assert_eq!(values.len(), 10);
    assert_eq!(
        values[8],
        Value::Str(vec![0; 20]),
        "fingerprint must be zeroed"
    );
}

#[test]
fn r2_payload_decodes_to_the_login_fields() {
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
    let values = decode(&AuthMessage::parse(body).unwrap().payload).unwrap();
    let s = |x: &str| Value::Str(x.as_bytes().to_vec());
    assert_eq!(values[0], s("fenji-dev"), "the ticket (gj_ticket)");
    assert_eq!(values[2], s("tester"), "the account (gj_uinfo)");
    assert_eq!(values[5], s("2.1.19.42851"), "the client version");
    assert_eq!(values[9], s("en-GB"), "the language");
}
