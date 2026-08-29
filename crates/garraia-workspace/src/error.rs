//! Errors surfaced by the garraia-workspace crate.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("workspace config invalid: {0}")]
    Config(String),

    #[error("database connection failed: {0}")]
    Connect(#[source] sqlx::Error),

    #[error("migration failed: {0}")]
    Migrate(#[source] sqlx::migrate::MigrateError),

    #[error("query failed: {0}")]
    Query(#[source] sqlx::Error),

    /// An RRULE or timezone that cannot be interpreted. Surfaced at write
    /// time so a bad recurrence is rejected instead of silently never firing.
    #[error("invalid recurrence: {0}")]
    Recurrence(String),
}

pub type Result<T> = std::result::Result<T, WorkspaceError>;
