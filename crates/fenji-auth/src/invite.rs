//! Invite codes (M3 spec D13): 80 random bits in Crockford's base32, 16 symbols shown in four
//! groups (`ABCD-EFGH-JKMN-PQRS`), stored only as the SHA-256 of the 16 normalised symbols.
use sha2::{Digest, Sha256};

use crate::{Error, Result};

/// Crockford's base32 alphabet: no I, L, O or U.
pub const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

#[derive(Clone, PartialEq, Eq)]
pub struct InviteCode([u8; 16]);

impl InviteCode {
    pub fn generate() -> Result<Self> {
        let mut raw = [0u8; 10];
        getrandom::fill(&mut raw).map_err(|_| Error::Random)?;
        let bits = raw.iter().fold(0u128, |acc, b| (acc << 8) | u128::from(*b));
        let mut out = [0u8; 16];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = ALPHABET[((bits >> (75 - 5 * i)) & 31) as usize];
        }
        Ok(Self(out))
    }

    /// Accepts either case, and dashes and spaces anywhere; reads O as 0, and I or L as 1.
    pub fn parse(input: &str) -> Option<Self> {
        let mut out = [0u8; 16];
        let mut n = 0;
        for c in input.bytes() {
            let c = match c.to_ascii_uppercase() {
                b'-' | b' ' => continue,
                b'O' => b'0',
                b'I' | b'L' => b'1',
                c if ALPHABET.contains(&c) => c,
                _ => return None,
            };
            *out.get_mut(n)? = c;
            n += 1;
        }
        (n == 16).then_some(Self(out))
    }

    pub fn display(&self) -> String {
        let s = std::str::from_utf8(&self.0).expect("the alphabet is ASCII");
        format!("{}-{}-{}-{}", &s[..4], &s[4..8], &s[8..12], &s[12..])
    }

    /// What the store keeps.
    pub fn hash(&self) -> [u8; 32] {
        Sha256::digest(self.0).into()
    }
}

impl std::fmt::Debug for InviteCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InviteCode(…)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_displays_in_four_groups_and_parses_back() {
        let c = InviteCode::generate().unwrap();
        let d = c.display();
        assert_eq!(d.len(), 19);
        assert_eq!(d.matches('-').count(), 3);
        assert!(d.bytes().all(|ch| ch == b'-' || ALPHABET.contains(&ch)));
        assert_eq!(InviteCode::parse(&d).unwrap(), c);
    }

    #[test]
    fn codes_differ() {
        assert_ne!(
            InviteCode::generate().unwrap(),
            InviteCode::generate().unwrap()
        );
    }

    #[test]
    fn parse_normalises_what_people_type() {
        let c = InviteCode::parse("0123-4567-89AB-CDEF").unwrap();
        assert_eq!(InviteCode::parse("0123456789abcdef").unwrap(), c);
        assert_eq!(
            InviteCode::parse(" o123 4567 89ab cdef ").unwrap(),
            c,
            "O reads as 0"
        );
        let one = InviteCode::parse("1111-1111-1111-1111").unwrap();
        assert_eq!(
            InviteCode::parse("iIlL-1111-1111-1111").unwrap(),
            one,
            "I and L read as 1"
        );
        assert!(
            InviteCode::parse("UUUU-1111-1111-1111").is_none(),
            "U isn't in the alphabet"
        );
        assert!(InviteCode::parse("0123-4567-89AB-CDE").is_none());
        assert!(InviteCode::parse("0123-4567-89AB-CDEF0").is_none());
        assert!(InviteCode::parse("0123-4567-89AB-CDÉF").is_none());
    }

    #[test]
    fn the_hash_is_of_the_normalised_code_and_debug_hides_it() {
        let a = InviteCode::parse("0123-4567-89AB-CDEF").unwrap();
        let b = InviteCode::parse("0123456789abcdef").unwrap();
        assert_eq!(a.hash(), b.hash());
        assert!(!format!("{a:?}").contains("0123"));
    }
}
