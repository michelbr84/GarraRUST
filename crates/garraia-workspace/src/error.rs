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
}

pub type Result<T> = std::result::Result<T, WorkspaceError>;
