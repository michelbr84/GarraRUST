//! Storage port: `GarraDb` trait that decouples domain logic from the
//! concrete database backend (GAR-302).
//!
//! The minimal surface required by the domain:
//! - `create_session` — upsert a session record
//! - `append_message` — write a turn to the history
//! - `list_messages`  — read the N most recent turns
//!
//! Both [`SqliteDb`](crate::sqlite_db::SqliteDb) (default) and
//! [`PostgresDb`](crate::postgres_db::PostgresDb) (`feature = "postgres"`)
//! implement this trait.

use async_trait::async_trait;
use garraia_common::Result;

use crate::session_store::StoredMessage;

/// Minimal async storage port for GarraIA sessions.
#[async_trait]
pub trait GarraDb: Send + Sync {
    /// Create or update a session record identified by `id`.
    async fn create_session(&self, id: &str) -> Result<()>;

    /// Append a message turn to `session_id`.
    ///
    /// `direction` is `"user"`, `"assistant"`, or `"system"`.
    async fn append_message(
        &self,
        session_id: &str,
        direction: &str,
        content: &str,
    ) -> Result<()>;

    /// Return the `limit` most recent messages for `session_id`,
    /// in chronological order (oldest first).
    async fn list_messages(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<StoredMessage>>;
}
