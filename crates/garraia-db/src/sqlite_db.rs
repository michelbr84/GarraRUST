//! SQLite implementation of [`GarraDb`](crate::db_trait::GarraDb) (GAR-302).
//!
//! Wraps the existing [`SessionStore`] behind `Arc<Mutex<…>>` so the sync
//! rusqlite API fits into the async trait surface.

use std::sync::Arc;

use async_trait::async_trait;
use garraia_common::Result;
use tokio::sync::Mutex;

use crate::db_trait::GarraDb;
use crate::session_store::{SessionStore, StoredMessage};

/// Async-trait adapter over the synchronous [`SessionStore`].
///
/// ```rust,no_run
/// use std::sync::Arc;
/// use tokio::sync::Mutex;
/// use garraia_db::{SessionStore, sqlite_db::SqliteDb};
///
/// # async fn run() {
/// let store = SessionStore::in_memory().unwrap();
/// let db = SqliteDb::new(Arc::new(Mutex::new(store)));
/// # }
/// ```
#[derive(Clone)]
pub struct SqliteDb(pub Arc<Mutex<SessionStore>>);

impl SqliteDb {
    /// Wrap an existing `SessionStore` handle.
    pub fn new(store: Arc<Mutex<SessionStore>>) -> Self {
        Self(store)
    }
}

#[async_trait]
impl GarraDb for SqliteDb {
    async fn create_session(&self, id: &str) -> Result<()> {
        let guard = self.0.lock().await;
        // Upsert with sensible defaults for channel_id / user_id / metadata.
        guard.upsert_session(id, "api", id, &serde_json::Value::Null)
    }

    async fn append_message(&self, session_id: &str, direction: &str, content: &str) -> Result<()> {
        let guard = self.0.lock().await;
        guard.append_message(
            session_id,
            direction,
            content,
            chrono::Utc::now(),
            &serde_json::Value::Null,
        )
    }

    async fn list_messages(&self, session_id: &str, limit: usize) -> Result<Vec<StoredMessage>> {
        let guard = self.0.lock().await;
        guard.load_recent_messages(session_id, limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn make_db() -> SqliteDb {
        let store = SessionStore::in_memory().expect("in-memory store");
        SqliteDb::new(Arc::new(Mutex::new(store)))
    }

    #[tokio::test]
    async fn test_create_session() {
        let db = make_db().await;
        db.create_session("s1").await.expect("create_session");
    }

    #[tokio::test]
    async fn test_append_and_list() {
        let db = make_db().await;
        db.create_session("s2").await.expect("create");
        db.append_message("s2", "user", "hello")
            .await
            .expect("append user");
        db.append_message("s2", "assistant", "world")
            .await
            .expect("append assistant");

        let msgs = db.list_messages("s2", 10).await.expect("list");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].direction, "user");
        assert_eq!(msgs[0].content, "hello");
        assert_eq!(msgs[1].direction, "assistant");
        assert_eq!(msgs[1].content, "world");
    }

    #[tokio::test]
    async fn test_list_respects_limit() {
        let db = make_db().await;
        db.create_session("s3").await.expect("create");
        for i in 0..5u32 {
            db.append_message("s3", "user", &format!("msg {i}"))
                .await
                .expect("append");
        }
        let msgs = db.list_messages("s3", 3).await.expect("list");
        assert_eq!(msgs.len(), 3, "should respect limit");
    }
}
