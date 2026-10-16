//! Invite links (M3 spec D13; M4 hosting spec §3.10): `https://<host>/#invite=<code>`, with
//! `&pin=<pin>` for a server with its own key, in either order, the fragment never reaching a server
//! log; or a bare code.
use fenji_auth::invite::InviteCode;

use crate::pin::{ServerAddr, unwrapped};

/// The server's address (when the input is a link) and the code. A link whose pin, or anything
/// else in its fragment, doesn't read is no link: it never falls back to an unpinned address. A
/// link wrapped over several lines reads as one ([`unwrapped`]).
pub fn parse(input: &str) -> Option<(Option<ServerAddr>, InviteCode)> {
    let input = unwrapped(input);
    match input.split_once('#') {
        Some((server, fragment)) => {
            let (mut code, mut pin) = (None, None);
            for part in fragment.split('&') {
                if let Some(c) = part.strip_prefix("invite=") {
                    if code.replace(InviteCode::parse(c)?).is_some() {
                        return None;
                    }
                    continue;
                }
                // Anything but an invite or a pin spoils the link.
                if pin.replace(part.strip_prefix("pin=")?).is_some() {
                    return None;
                }
            }
            let code = code?;
            let address = match pin {
                Some(p) => format!("{}#pin={p}", server.trim_end_matches('/')),
                None => server.to_string(),
            };
            Some((Some(ServerAddr::parse(&address).ok()?), code))
        }
        None => Some((None, InviteCode::parse(&input)?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(server: Option<ServerAddr>) -> Option<String> {
        server.map(|s| s.origin)
    }

    #[test]
    fn links_and_bare_codes() {
        let (server, code) = parse("https://play.example.net/#invite=ABCD-EFGH-JKMN-PQRS").unwrap();
        assert_eq!(origin(server).as_deref(), Some("https://play.example.net"));
        assert_eq!(code.display(), "ABCD-EFGH-JKMN-PQRS");
        let (server, code) = parse(" abcd efgh jkmn pqrs ").unwrap();
        assert!(server.is_none());
        assert_eq!(code.display(), "ABCD-EFGH-JKMN-PQRS");
        let (server, _) = parse("http://127.0.0.1:7801#invite=ABCDEFGHJKMNPQRS").unwrap();
        assert_eq!(origin(server).as_deref(), Some("http://127.0.0.1:7801"));
        assert!(parse("https://play.example.net/#nothing").is_none());
        assert!(parse("https://play.example.net/#invite=").is_none());
        assert!(parse("ftp://play.example.net/#invite=ABCD-EFGH-JKMN-PQRS").is_none());
        assert!(parse("not a code").is_none());
    }

    #[test]
    fn links_carry_a_pin_in_either_order_and_a_bad_pin_spoils_the_link() {
        let pin = "A".repeat(43);
        let (server, code) = parse(&format!(
            "https://203.0.113.5:7801/#invite=ABCD-EFGH-JKMN-PQRS&pin={pin}"
        ))
        .unwrap();
        let server = server.unwrap();
        assert_eq!(server.origin, "https://203.0.113.5:7801");
        assert_eq!(server.pin.unwrap().to_string(), pin);
        assert_eq!(code.display(), "ABCD-EFGH-JKMN-PQRS");
        assert!(parse(&format!("https://h/#pin={pin}&invite=ABCDEFGHJKMNPQRS")).is_some());
        assert!(parse("https://h/#invite=ABCDEFGHJKMNPQRS&pin=short").is_none());
        assert!(
            parse(&format!(
                "http://203.0.113.5:7801/#invite=ABCDEFGHJKMNPQRS&pin={pin}"
            ))
            .is_none()
        );
    }

    #[test]
    fn a_link_copied_from_a_wrapped_line_still_reads() {
        // The host console wraps a long link onto rows of its own (#357), and chat apps and mail
        // wrap it too: whitespace inside a link is never part of it.
        let pin = "A".repeat(43);
        let (head, tail) = pin.split_at(20);
        for wrapped in [
            format!("https://192.0.2.10:7801/#invite=ABCD-EFGH-JKMN-PQRS&pin={head}\n   {tail}"),
            format!("https://192.0.2.10:7801/#invite=ABCD-EFGH-\r\nJKMN-PQRS&pin={head} {tail}\n"),
            format!("https://192.0.2.10:\n   7801/#invite=ABCD-EFGH-JKMN-PQRS&pin={pin}"),
        ] {
            let (server, code) = parse(&wrapped).unwrap_or_else(|| panic!("{wrapped:?}"));
            let server = server.unwrap();
            assert_eq!(server.origin, "https://192.0.2.10:7801");
            assert_eq!(server.pin.unwrap().to_string(), pin);
            assert_eq!(code.display(), "ABCD-EFGH-JKMN-PQRS");
        }
        let (server, code) = parse("ABCD-EFGH-\n   JKMN-PQRS").unwrap();
        assert!(server.is_none());
        assert_eq!(code.display(), "ABCD-EFGH-JKMN-PQRS");
    }
}
