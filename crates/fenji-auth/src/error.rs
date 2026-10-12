/// What can go wrong here. None of the messages carries a secret.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("the OS random source failed")]
    Random,
    #[error("password hashing failed: {0}")]
    Hash(String),
    #[error("a stored password hash can't be read")]
    BadStoredHash,
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
