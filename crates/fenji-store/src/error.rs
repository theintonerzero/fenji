/// What can go wrong in the store. The domain variants are the refusals callers act on.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database: {0}")]
    Db(#[from] sqlx::Error),
    #[error("migration: {0}")]
    Migrate(String),
    /// Creating the database file, or taking group and other access away from it; or creating a
    /// backup's file, which the message names.
    #[error("the database file: {0}")]
    File(#[from] std::io::Error),
    #[error("the invite is unknown, used, revoked or expired")]
    InviteInvalid,
    #[error("that username is taken")]
    UsernameTaken,
    #[error("that character name is taken")]
    NameTaken,
    #[error("the account has as many characters as it may")]
    TooMany,
    #[error("a place must be finite")]
    NotFinite,
    /// The database's schema isn't this build's ([`crate::Store::open_as_is`]).
    #[error("{0}")]
    Schema(SchemaDiffers),
}

/// How a database's schema differs from this build's: the latest migration each has. Another
/// version of Fenji has migrated it further, or this build would migrate it further.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaDiffers {
    /// The latest migration the database has, none for a database without a schema.
    pub database: Option<i64>,
    /// The latest this build has.
    pub build: i64,
}

impl SchemaDiffers {
    /// Whether a newer build than this one has migrated the database.
    pub fn newer(&self) -> bool {
        self.database.is_some_and(|database| database > self.build)
    }
}

impl std::fmt::Display for SchemaDiffers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.database {
            Some(database) => write!(
                f,
                "the database's schema is at migration {database}, and this build's at {}",
                self.build
            ),
            None => write!(
                f,
                "the database has no schema yet, and this build's is at migration {}",
                self.build
            ),
        }
    }
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
