//! Session and game tokens (M3 spec D3): 32 bytes from the OS's random source, written as
//! `fenji1-` and 64 hex digits, and stored only as the SHA-256 of the 32 bytes. The client's
//! `net_manager_login` hex-decodes a ticket that matches `^%x+$` only for platform types this build
//! never reports; the prefix makes sure it never could (docs/protocol/packets/lobby.md 12.7).
use sha2::{Digest, Sha256};

use crate::{Error, Result};

/// Every token's prefix.
pub const PREFIX: &str = "fenji1-";

#[derive(Clone, PartialEq, Eq)]
pub struct Token([u8; 32]);

impl Token {
    pub fn generate() -> Result<Self> {
        let mut b = [0u8; 32];
        getrandom::fill(&mut b).map_err(|_| Error::Random)?;
        Ok(Self(b))
    }

    /// `fenji1-` and 64 hex digits, either case; anything else is `None`.
    pub fn parse(s: &str) -> Option<Self> {
        let hex = s.strip_prefix(PREFIX)?;
        if hex.len() != 64 {
            return None;
        }
        let mut b = [0u8; 32];
        for (slot, pair) in b.iter_mut().zip(hex.as_bytes().chunks(2)) {
            *slot = (nibble(pair[0])? << 4) | nibble(pair[1])?;
        }
        Some(Self(b))
    }

    pub fn as_string(&self) -> String {
        let mut s = String::with_capacity(PREFIX.len() + 64);
        s.push_str(PREFIX);
        for byte in self.0 {
            s.push(char::from(HEX[usize::from(byte >> 4)]));
            s.push(char::from(HEX[usize::from(byte & 15)]));
        }
        s
    }

    /// What the store keeps.
    pub fn hash(&self) -> [u8; 32] {
        Sha256::digest(self.0).into()
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(…)")
    }
}

const HEX: &[u8; 16] = b"0123456789abcdef";

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_round_trips_through_its_string() {
        let t = Token::generate().unwrap();
        let s = t.as_string();
        assert!(s.starts_with(PREFIX));
        assert_eq!(s.len(), PREFIX.len() + 64);
        assert_eq!(Token::parse(&s).unwrap(), t);
        let upper = format!("{PREFIX}{}", s[PREFIX.len()..].to_uppercase());
        assert_eq!(
            Token::parse(&upper).unwrap(),
            t,
            "hex digits in either case"
        );
    }

    #[test]
    fn parse_refuses_anything_else() {
        let s = Token::generate().unwrap().as_string();
        assert!(Token::parse(&s[PREFIX.len()..]).is_none(), "no prefix");
        assert!(Token::parse(&s[..s.len() - 1]).is_none(), "short");
        assert!(Token::parse(&format!("{s}0")).is_none(), "long");
        assert!(
            Token::parse(&format!("{PREFIX}{}", "g".repeat(64))).is_none(),
            "not hex"
        );
        assert!(
            Token::parse(&format!("{PREFIX}{}", "é".repeat(32))).is_none(),
            "not ASCII"
        );
        assert!(Token::parse("").is_none());
    }

    #[test]
    fn tokens_differ_and_hash_stably() {
        let a = Token::generate().unwrap();
        let b = Token::generate().unwrap();
        assert_ne!(a, b);
        assert_ne!(a.hash(), b.hash());
        assert_eq!(a.hash(), Token::parse(&a.as_string()).unwrap().hash());
    }

    #[test]
    fn debug_never_shows_the_token() {
        let t = Token::generate().unwrap();
        let s = t.as_string();
        assert!(!format!("{t:?}").contains(&s[PREFIX.len()..PREFIX.len() + 8]));
    }
}
