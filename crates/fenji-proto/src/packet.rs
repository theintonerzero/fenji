//! Messages above the inner header (docs/protocol/opcodes.md, payloads.md). The client's script
//! opcodes carry a pickle whose first value is the script message id; every other opcode is kept
//! raw for now.
use crate::opcode::Cts;
use crate::pickle::{self, Table, Value};
use crate::{Error, Result};

/// `PROTOG2C_SET_NET_CONNECTED`: moves the client from `QUEUE` to `WORK` (transport.md 10.3).
pub const SET_NET_CONNECTED: i64 = 414;

/// A message from the client.
#[derive(Debug, Clone, PartialEq)]
pub enum Up {
    /// `CTS_PROTOCOL_GATE_SCRIPT` (0xFF, `gateway`) or `CTS_PROTOCOL_SCRIPT` (0): a script message
    /// id (a number or a string, as `send_gateway` and `send` require) and its arguments.
    Script {
        gateway: bool,
        id: Value,
        args: Vec<Value>,
    },
    /// Any other opcode, undecoded.
    Unknown { opcode: u8, bytes: Vec<u8> },
}

impl Up {
    pub fn decode(ty: u8, body: &[u8]) -> Result<Self> {
        match Cts::from_u8(ty) {
            Cts::GateScript | Cts::Script => {
                let mut values = pickle::decode(body)?.into_iter();
                let id = values
                    .next()
                    .ok_or_else(|| Error::Pickle("a script message with no id".into()))?;
                if !matches!(id, Value::Int(_) | Value::Float(_) | Value::Str(_)) {
                    return Err(Error::Pickle(format!(
                        "a script message id must be a number or string, not {}",
                        id.kind()
                    )));
                }
                Ok(Self::Script {
                    gateway: ty == Cts::GateScript.to_u8(),
                    id,
                    args: values.collect(),
                })
            }
            _ => Ok(Self::Unknown {
                opcode: ty,
                bytes: body.to_vec(),
            }),
        }
    }
}

/// Script messages the server never sends (ground rule 6; lobby.md §2). 556 runs Lua bytecode,
/// 243 and 216 replace scripts and layouts, 627 patches tables, 729 writes a file to the player's
/// disk, and 720 and 598 open URLs.
pub const NEVER_SEND: [i64; 7] = [556, 243, 216, 627, 729, 720, 598];

/// The body of an `STC_PROTOCOL_SCRIPT` message: the pickle of `id`, then `args`. Every script
/// message the server sends is built here, so this is where [`NEVER_SEND`] is refused.
pub fn script_body(id: i64, args: &[Value]) -> Result<Vec<u8>> {
    if NEVER_SEND.contains(&id) {
        return Err(Error::NeverSend(id));
    }
    let mut values = Vec::with_capacity(1 + args.len());
    values.push(Value::Int(id));
    values.extend_from_slice(args);
    pickle::encode(&values)
}

/// The billing fields `SET_NET_CONNECTED`'s handler stores; a UI listener formats them as numbers,
/// so they must be present (run m2-r4, transport.md §9).
pub const BILLING_FIELDS: [&str; 4] = [
    "times_remain",
    "coins_remain",
    "bind_coins_remain",
    "cards_endtime",
];

/// `SET_NET_CONNECTED` with no time-zone offset, every billing field 0 and no user id: the handler
/// then keeps the client's own user id (transport.md 10.3–10.4).
pub fn set_net_connected() -> Vec<u8> {
    let billing = Table {
        array: Vec::new(),
        hash: BILLING_FIELDS
            .iter()
            .map(|k| (Value::Str(k.as_bytes().to_vec()), Value::Int(0)))
            .collect(),
    };
    script_body(
        SET_NET_CONNECTED,
        &[Value::Nil, Value::Table(billing), Value::Nil],
    )
    .expect("a fixed, small message")
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::pickle::Table;

    #[test]
    fn set_net_connected_carries_zero_billing_fields() {
        // transport.md 10.3: (414, nil, billing table, nil). Run m2-r4 showed a UI listener
        // formats a billing number, so the four fields are sent as 0 (i8), in this order.
        let mut want = vec![0x04, 0x04, 0x9e, 0x01, 0x00, 0x0b, 0x00, 0x00, 0x00, 0x00];
        for key in [
            &b"times_remain"[..],
            b"coins_remain",
            b"bind_coins_remain",
            b"cards_endtime",
        ] {
            want.extend([0x08, key.len() as u8]);
            want.extend_from_slice(key);
            want.extend([0x03, 0x00]);
        }
        want.extend([0x00, 0x00]); // end of the table, then the nil user id
        assert_eq!(set_net_connected(), want);
    }

    #[test]
    fn script_ids_may_be_strings_or_floats() {
        // The client refuses an id that isn't a number or a string (transport.md 10.5).
        for id in [Value::Str(b"login".to_vec()), Value::Float(402.0)] {
            let body = pickle::encode(&[id.clone(), Value::Bool(true)]).unwrap();
            assert_eq!(
                Up::decode(0, &body).unwrap(),
                Up::Script {
                    gateway: false,
                    id,
                    args: vec![Value::Bool(true)]
                }
            );
        }
    }

    #[test]
    fn gateway_402_decodes() {
        // transport.md 10.4: the client's expected answer, type 0xFF, body pickle(402).
        let got = Up::decode(0xff, &[0x01, 0x04, 0x92, 0x01]).unwrap();
        assert_eq!(
            got,
            Up::Script {
                gateway: true,
                id: Value::Int(402),
                args: vec![]
            }
        );
    }

    #[test]
    fn script_with_arguments() {
        let body = script_body(278, &[Value::Str(b"md5".to_vec()), Value::Nil]).unwrap();
        let got = Up::decode(0, &body).unwrap();
        assert_eq!(
            got,
            Up::Script {
                gateway: false,
                id: Value::Int(278),
                args: vec![Value::Str(b"md5".to_vec()), Value::Nil]
            }
        );
    }

    #[test]
    fn other_opcodes_stay_raw() {
        assert_eq!(
            Up::decode(4, &[]).unwrap(),
            Up::Unknown {
                opcode: 4,
                bytes: vec![]
            }
        );
        assert_eq!(
            Up::decode(2, &[1, 2, 3]).unwrap(),
            Up::Unknown {
                opcode: 2,
                bytes: vec![1, 2, 3]
            }
        );
    }

    #[test]
    fn script_ids_must_be_numbers_or_strings() {
        // send_gateway / send refuse other ids ("script PT type error"); so do we.
        for bad in [
            vec![Value::Nil],
            vec![Value::Table(Table::default())],
            vec![Value::Bool(true)],
        ] {
            let body = crate::pickle::encode(&bad).unwrap();
            assert!(Up::decode(0xff, &body).is_err(), "{bad:?}");
        }
        assert!(Up::decode(0xff, &[0x00]).is_err(), "no values at all");
        assert!(Up::decode(0, &[0x01, 0x11]).is_err(), "bad pickle");
        // A refused id is named by its kind, not printed.
        let big = Value::Table(Table {
            array: vec![Value::Nil; 1000],
            hash: vec![],
        });
        let body = pickle::encode(&[big]).unwrap();
        assert_eq!(
            Up::decode(0xff, &body).unwrap_err().to_string(),
            "pickle: a script message id must be a number or string, not a table"
        );
    }
}
