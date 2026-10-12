//! Invite links (M3 spec D13): `https://<host>/#invite=<code>`, the code in the fragment so it
//! never reaches a server log, or a bare code.
use fenji_auth::invite::InviteCode;

/// The server's address (when the input is a link) and the code.
pub fn parse(input: &str) -> Option<(Option<String>, InviteCode)> {
    let input = input.trim();
    match input.split_once('#') {
        Some((server, fragment)) => {
            let code = InviteCode::parse(fragment.strip_prefix("invite=")?)?;
            let server = server.trim_end_matches('/');
            let rest = server
                .strip_prefix("https://")
                .or_else(|| server.strip_prefix("http://"))?;
            if rest.is_empty() {
                return None;
            }
            Some((Some(server.to_string()), code))
        }
        None => Some((None, InviteCode::parse(input)?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_and_bare_codes() {
        let (server, code) = parse("https://play.example.net/#invite=ABCD-EFGH-JKMN-PQRS").unwrap();
        assert_eq!(server.as_deref(), Some("https://play.example.net"));
        assert_eq!(code.display(), "ABCD-EFGH-JKMN-PQRS");
        let (server, code) = parse(" abcd efgh jkmn pqrs ").unwrap();
        assert!(server.is_none());
        assert_eq!(code.display(), "ABCD-EFGH-JKMN-PQRS");
        let (server, _) = parse("http://127.0.0.1:7801#invite=ABCDEFGHJKMNPQRS").unwrap();
        assert_eq!(server.as_deref(), Some("http://127.0.0.1:7801"));
        assert!(parse("https://play.example.net/#nothing").is_none());
        assert!(parse("https://play.example.net/#invite=").is_none());
        assert!(parse("ftp://play.example.net/#invite=ABCD-EFGH-JKMN-PQRS").is_none());
        assert!(parse("not a code").is_none());
    }
}
