/// What can go wrong in the store. The domain variants are the refusals callers act on.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database: {0}")]
    Db(#[from] sqlx::Error),
    #[error("migration: {0}")]
    Migrate(String),
    #[error("the invite is unknown, used, revoked or expired")]
    InviteInvalid,
    #[error("that username is taken")]
    UsernameTaken,
    #[error("that character name is taken")]
    NameTaken,
    #[error("the account has as many characters as it may")]
    TooMany,
}

impl From<sqlx::migrate::MigrateError> for Error {
    fn from(e: sqlx::migrate::MigrateError) -> Self {
        Self::Migrate(e.to_string())
    }
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Whether `e` is SQLite's UNIQUE constraint failure.
pub(crate) fn is_unique(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(d) if d.is_unique_violation())
}
