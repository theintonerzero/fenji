//! The launcher's saved servers (launcher spec §3.3, L7, L18) and what the Add dialog takes.

use fenji_auth::invite::InviteCode;
use serde::{Deserialize, Serialize};

use crate::api::ApiError;
use crate::invite_link;
use crate::pin::ServerAddr;

/// A server in the player's list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedServer {
    pub id: u32,
    /// `https://host[:port]`, or `http://` on this machine.
    pub origin: String,
    /// The server's own key, from its link (M4 hosting spec §3.10): 43 base64url characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin: Option<String>,
    /// The name the server last gave.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// When it last answered, in Unix milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_ms: Option<i64>,
}

impl SavedServer {
    /// Its address, with its pin.
    pub fn addr(&self) -> Result<ServerAddr, ApiError> {
        match &self.pin {
            Some(pin) => ServerAddr::parse(&format!("{}/#pin={pin}", self.origin)),
            None => ServerAddr::parse(&self.origin),
        }
    }

    /// The origin without its scheme, for screens: `moonward.example.net`, `203.0.113.5:7801`.
    pub fn shown_address(&self) -> String {
        self.origin
            .split_once("://")
            .map_or(self.origin.as_str(), |(_, rest)| rest)
            .to_string()
    }
}

/// What the Add dialog was given.
#[derive(Debug, Clone)]
pub enum Added {
    /// An address: `host`, `host:port`, `https://…` (with or without `#pin=`), or `http://` on this
    /// machine.
    Address(ServerAddr),
    /// An invite link: its server and its code.
    Invite(ServerAddr, InviteCode),
    /// An invite code alone: it redeems on the active server (plan LP7).
    Code(InviteCode),
}

/// Why the Add dialog's text wasn't taken, in the launcher's words.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AddError {
    #[error("Paste an address or the link your host sent.")]
    Empty,
    #[error("That isn't a server address or an invite link. Paste the link your host sent.")]
    Unreadable,
    #[error("Plain http:// works only for a server on this computer.")]
    Insecure,
}

/// Reads the Add dialog's text: a link (with `#`), then a bare invite code (no `.`, `:` or `/`),
/// then an address, where a bare host gets `https://` in front (launcher spec L18).
pub fn parse_added(text: &str) -> Result<Added, AddError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(AddError::Empty);
    }
    if text.contains('#') {
        if let Some((Some(server), code)) = invite_link::parse(text) {
            return Ok(Added::Invite(server, code));
        }
        // A link without an invite, such as `…/#pin=…`, is an address.
        return address(text);
    }
    if !text.contains(['.', ':', '/'])
        && let Some(code) = InviteCode::parse(text)
    {
        return Ok(Added::Code(code));
    }
    address(text)
}

fn address(text: &str) -> Result<Added, AddError> {
    let full = if text.starts_with("https://") || text.starts_with("http://") {
        text.to_string()
    } else {
        format!("https://{text}")
    };
    match ServerAddr::parse(&full) {
        Ok(server) => Ok(Added::Address(server)),
        Err(ApiError::Insecure) => Err(AddError::Insecure),
        Err(_) => Err(AddError::Unreadable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const PIN: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    #[test]
    fn the_add_dialog_reads_links_codes_and_addresses() {
        match parse_added(&format!(
            "https://203.0.113.5:7801/#invite=ABCD-EFGH-JKMN-PQRS&pin={PIN}"
        ))
        .unwrap()
        {
            Added::Invite(addr, code) => {
                assert_eq!(addr.origin, "https://203.0.113.5:7801");
                assert_eq!(addr.pin.unwrap().to_string(), PIN);
                assert_eq!(code.display(), "ABCD-EFGH-JKMN-PQRS");
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            parse_added(" abcd efgh jkmn pqrs ").unwrap(),
            Added::Code(_)
        ));
        let Added::Address(a) = parse_added("play.example.net").unwrap() else {
            panic!()
        };
        assert_eq!(
            (a.origin.as_str(), a.pin.is_none()),
            ("https://play.example.net", true)
        );
        let Added::Address(a) = parse_added("play.example.net:8443").unwrap() else {
            panic!()
        };
        assert_eq!(a.origin, "https://play.example.net:8443");
        let Added::Address(a) =
            parse_added(&format!("https://203.0.113.5:7801/#pin={PIN}")).unwrap()
        else {
            panic!()
        };
        assert!(a.pin.is_some());
        let Added::Address(a) = parse_added("http://127.0.0.1:7801").unwrap() else {
            panic!()
        };
        assert_eq!(a.origin, "http://127.0.0.1:7801");
        assert!(matches!(parse_added("   "), Err(AddError::Empty)));
        assert!(matches!(
            parse_added("http://203.0.113.5:7801"),
            Err(AddError::Insecure)
        ));
        assert!(matches!(
            parse_added("ftp://x/#invite=ABCD-EFGH-JKMN-PQRS"),
            Err(AddError::Unreadable)
        ));
        assert!(matches!(
            parse_added("not an address at all"),
            Err(AddError::Unreadable)
        ));
    }

    #[test]
    fn a_saved_server_gives_back_its_pinned_address_and_a_short_name_for_screens() {
        let s = SavedServer {
            id: 1,
            origin: "https://203.0.113.5:7801".into(),
            pin: Some(PIN.into()),
            name: None,
            username: None,
            last_seen_ms: None,
        };
        assert_eq!(s.addr().unwrap().pin.unwrap().to_string(), PIN);
        assert_eq!(s.shown_address(), "203.0.113.5:7801");
    }
}
