//! Password hashes (ground rule 7): argon2id with the `argon2` crate's defaults, 19 MiB, 2 passes
//! and 1 lane (OWASP's recommendation), stored as PHC strings such as
//! `$argon2id$v=19$m=19456,t=2,p=1$<salt>$<hash>`.
use std::sync::OnceLock;

use argon2::password_hash::Error as PhError;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};

use crate::{Error, Result};

/// Hashes `password` with a fresh salt.
pub fn hash(password: &str) -> Result<String> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| Error::Hash(e.to_string()))
}

/// Whether `password` matches the stored PHC string. A stored hash that doesn't parse is an error,
/// not a mismatch, so that a damaged row gets noticed.
pub fn verify(password: &str, phc: &str) -> Result<bool> {
    let parsed = PasswordHash::new(phc).map_err(|_| Error::BadStoredHash)?;
    match Argon2::default().verify_password(password.as_bytes(), &parsed) {
        Ok(()) => Ok(true),
        Err(PhError::PasswordInvalid) => Ok(false),
        Err(_) => Err(Error::BadStoredHash),
    }
}

static DUMMY: OnceLock<String> = OnceLock::new();

fn dummy() -> &'static str {
    DUMMY.get_or_init(|| hash("fenji: no such account").expect("hashing a constant"))
}

/// Spends the time a real check takes, against a fixed hash, for a username that doesn't exist:
/// so the time taken doesn't tell an unknown username from a wrong password.
pub fn verify_dummy(password: &str) {
    let _ = verify(password, dummy());
}

/// Makes the fixed hash now. Made at the first unknown username instead, that sign-in would take
/// about twice as long as any other, which would tell it apart.
pub fn warm_dummy() {
    dummy();
}

/// Whether the fixed hash is made.
pub fn dummy_ready() -> bool {
    DUMMY.get().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_argon2id_with_the_default_parameters_and_verifies() {
        let phc = hash("correct horse battery").unwrap();
        assert!(phc.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"), "{phc}");
        assert!(verify("correct horse battery", &phc).unwrap());
        assert!(!verify("correct horse batterY", &phc).unwrap());
    }

    #[test]
    fn two_hashes_of_one_password_differ_by_salt() {
        assert_ne!(
            hash("same password!").unwrap(),
            hash("same password!").unwrap()
        );
    }

    #[test]
    fn a_damaged_stored_hash_is_an_error_not_a_mismatch() {
        assert_eq!(
            verify("x", "not a phc string"),
            Err(crate::Error::BadStoredHash)
        );
    }

    #[test]
    fn verify_dummy_runs() {
        verify_dummy("anything at all");
    }
}
