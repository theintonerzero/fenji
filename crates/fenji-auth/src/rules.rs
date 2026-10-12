//! The rules Fenji applies to new usernames and passwords (M3 spec §4).
use std::ops::RangeInclusive;

/// Usernames: 3 to 16 characters from `A–Z a–z 0–9 _ -`. The client splits `gj_uinfo` on `:`
/// (docs/protocol/login.md 2.8), so a colon must never pass.
pub const USERNAME_LEN: RangeInclusive<usize> = 3..=16;
/// Passwords: 10 to 128 characters (Unicode scalar values), nothing else required.
pub const PASSWORD_LEN: RangeInclusive<usize> = 10..=128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleError {
    UsernameLength,
    UsernameChars,
    PasswordLength,
}

impl std::fmt::Display for RuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::UsernameLength => "a username has 3 to 16 characters",
            Self::UsernameChars => "a username uses only A-Z, a-z, 0-9, _ and -",
            Self::PasswordLength => "a password has 10 to 128 characters",
        })
    }
}

/// The characters are checked first, so for a name that passes, its byte length is its length in
/// characters.
pub fn check_username(name: &str) -> Result<(), RuleError> {
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(RuleError::UsernameChars);
    }
    if !USERNAME_LEN.contains(&name.len()) {
        return Err(RuleError::UsernameLength);
    }
    Ok(())
}

pub fn check_password(password: &str) -> Result<(), RuleError> {
    if PASSWORD_LEN.contains(&password.chars().count()) {
        Ok(())
    } else {
        Err(RuleError::PasswordLength)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usernames() {
        assert_eq!(check_username("abc"), Ok(()));
        assert_eq!(check_username("Ab_c-9"), Ok(()));
        assert_eq!(check_username(&"a".repeat(16)), Ok(()));
        assert_eq!(check_username("ab"), Err(RuleError::UsernameLength));
        assert_eq!(
            check_username(&"a".repeat(17)),
            Err(RuleError::UsernameLength)
        );
        assert_eq!(check_username("a:bc"), Err(RuleError::UsernameChars));
        assert_eq!(check_username("ab c"), Err(RuleError::UsernameChars));
        assert_eq!(check_username("abé"), Err(RuleError::UsernameChars));
    }

    #[test]
    fn passwords_count_characters_not_bytes() {
        assert_eq!(check_password(&"é".repeat(10)), Ok(()));
        assert_eq!(
            check_password(&"a".repeat(9)),
            Err(RuleError::PasswordLength)
        );
        assert_eq!(check_password(&"a".repeat(128)), Ok(()));
        assert_eq!(
            check_password(&"a".repeat(129)),
            Err(RuleError::PasswordLength)
        );
    }
}
