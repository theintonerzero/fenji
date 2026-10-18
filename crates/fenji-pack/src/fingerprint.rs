use std::fmt;
use std::str::FromStr;

use sha2::{Digest, Sha256};

use crate::PackError;

/// A line's fingerprint: the first 16 bytes of the SHA-256 of its exact source bytes, as the
/// client's locale map holds them (retranslation spec §6.2). Nothing normalizes the bytes first,
/// so lines that differ by a byte are different lines, as they are to the client.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fingerprint(pub [u8; 16]);

/// `source`'s fingerprint.
pub fn fingerprint(source: &[u8]) -> Fingerprint {
    let digest = Sha256::digest(source);
    let mut fp = [0; 16];
    fp.copy_from_slice(&digest[..16]);
    Fingerprint(fp)
}

/// The fingerprint of `source` in the locale context `env`. The client's locale map has a main
/// table, context `""`, and a few small tables for contexts such as `noun`, where one source
/// line has another translation. A line in the main table is keyed by [`fingerprint`] of its
/// source; a line in a context by the fingerprint of `<env>\0<source>`, which no source holds.
pub fn fingerprint_in(env: &str, source: &[u8]) -> Fingerprint {
    if env.is_empty() {
        return fingerprint(source);
    }
    fingerprint(&[env.as_bytes(), b"\0", source].concat())
}

impl fmt::Display for Fingerprint {
    /// 32 lowercase hex digits.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({self})")
    }
}

impl FromStr for Fingerprint {
    type Err = PackError;

    /// Exactly 32 hex digits, of either case.
    fn from_str(s: &str) -> Result<Self, PackError> {
        let refused = || PackError::Fingerprint(s.to_string());
        let digits = s.as_bytes();
        if digits.len() != 32 {
            return Err(refused());
        }
        let mut fp = [0; 16];
        for (byte, pair) in fp.iter_mut().zip(digits.chunks(2)) {
            let nibble = |d: u8| char::from(d).to_digit(16).ok_or_else(refused);
            *byte =
                u8::try_from(nibble(pair[0])? * 16 + nibble(pair[1])?).map_err(|_| refused())?;
        }
        Ok(Fingerprint(fp))
    }
}

/// As its 32 hex digits, as the tools' JSON files hold it.
impl serde::Serialize for Fingerprint {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for Fingerprint {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = <std::borrow::Cow<'de, str>>::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_are_the_first_16_bytes_of_sha256() {
        let fp = fingerprint(b"abc");
        assert_eq!(fp.to_string(), "ba7816bf8f01cfea414140de5dae2223");
        assert_eq!(
            "BA7816BF8F01CFEA414140DE5DAE2223"
                .parse::<Fingerprint>()
                .unwrap(),
            fp
        );
        for bad in [
            "ba7816bf8f01cfea414140de5dae222",
            "ba7816bf8f01cfea414140de5dae2223a",
            "za7816bf8f01cfea414140de5dae2223",
            "",
        ] {
            assert!(bad.parse::<Fingerprint>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_line_in_a_context_has_a_fingerprint_of_its_own() {
        assert_eq!(
            fingerprint_in("", "剑".as_bytes()),
            fingerprint("剑".as_bytes())
        );
        assert_eq!(
            fingerprint_in("noun", "剑".as_bytes()),
            fingerprint("noun\0剑".as_bytes())
        );
        assert_ne!(
            fingerprint_in("noun", "剑".as_bytes()),
            fingerprint("剑".as_bytes())
        );
    }

    #[test]
    fn fingerprints_serialize_as_their_hex() {
        let fp = fingerprint(b"abc");
        let json = serde_json::to_string(&fp).unwrap();
        assert_eq!(json, "\"ba7816bf8f01cfea414140de5dae2223\"");
        assert_eq!(serde_json::from_str::<Fingerprint>(&json).unwrap(), fp);
        assert!(serde_json::from_str::<Fingerprint>("\"abc\"").is_err());
        assert!(serde_json::from_str::<Fingerprint>("7").is_err());
    }

    #[test]
    fn nothing_normalizes_a_source() {
        assert_ne!(
            fingerprint("你好".as_bytes()),
            fingerprint("你好 ".as_bytes())
        );
        assert_ne!(fingerprint(b"a\nb"), fingerprint(b"a\\nb"));
    }
}
