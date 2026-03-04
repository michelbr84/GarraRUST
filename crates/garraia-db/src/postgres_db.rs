//! Postgres implementation of [`GarraDb`](crate::db_trait::GarraDb) (GAR-302).
//!
//! Only compiled when the `postgres` feature is enabled.
//! Connects via [`sqlx::PgPool`] and maps to the same schema used by
//! the SQLite backend.
//!
//! # Schema (applied by migration before first use)
//!
//! ```sql
//! CREATE TABLE IF NOT EXISTS sessions (
//!     id TEXT PRIMARY KEY,
//!     channel_id TEXT NOT NULL DEFAULT '',
//!     user_id TEXT NOT NULL DEFAULT '',
//!     metadata JSONB NOT NULL DEFAULT '{}',
//!     created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
//!     updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
//! );
//!
//! CREATE TABLE IF NOT EXISTS messages (
//!     id BIGSERIAL PRIMARY KEY,
//!     session_id TEXT NOT NULL REFERENCES sessions(id),
//!     direction TEXT NOT NULL,
//!     content TEXT NOT NULL,
//!     timestamp TIMESTAMPTZ NOT NULL DEFAULT now(),
//!     metadata JSONB NOT NULL DEFAULT '{}',
//!     source TEXT,
//!     provider TEXT,
//!     model TEXT,
//!     tokens_in INTEGER,
//!     tokens_out INTEGER
//! );
//! ```

use async_trait::async_trait;
use garraia_common::{Error, Result};
use sqlx::PgPool;

use crate::db_trait::GarraDb;
use crate::session_store::StoredMessage;

/// Async Postgres backend for GarraIA sessions.
///
/// Obtain a pool with [`PostgresDb::connect`] and wire it into the
/// dependency graph instead of the default [`SqliteDb`].
pub struct PostgresDb {
    pool: PgPool,
}

impl PostgresDb {
    /// Connect to Postgres and return a ready-to-use [`PostgresDb`].
    ///
    /// Runs the two-table schema migration on first boot.
    pub async fn connect(url: &str) -> Result<Self> {
        let pool = PgPool::connect(url)
            .await
            .map_err(|e| Error::Database(format!("postgres connect failed: {e}")))?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS sessions (
                 id         TEXT        PRIMARY KEY,
                 channel_id TEXT        NOT NULL DEFAULT '',
                 user_id    TEXT        NOT NULL DEFAULT '',
                 metadata   TEXT        NOT NULL DEFAULT '{}',
                 created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
                 updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
             )",
        )
        .execute(&pool)
        .await
        .map_err(|e| Error::Database(format!("sessions migration failed: {e}")))?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS messages (
                 id         BIGSERIAL   PRIMARY KEY,
                 session_id TEXT        NOT NULL REFERENCES sessions(id),
                 direction  TEXT        NOT NULL,
                 content    TEXT        NOT NULL,
                 timestamp  TIMESTAMPTZ NOT NULL DEFAULT now(),
                 metadata   TEXT        NOT NULL DEFAULT '{}',
                 source     TEXT,
                 provider   TEXT,
                 model      TEXT,
                 tokens_in  INTEGER,
                 tokens_out INTEGER
             )",
        )
        .execute(&pool)
        .await
        .map_err(|e| Error::Database(format!("messages migration failed: {e}")))?;

        Ok(Self { pool })
    }
}

#[async_trait]
impl GarraDb for PostgresDb {
    async fn create_session(&self, id: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO sessions (id) VALUES ($1)
             ON CONFLICT (id) DO UPDATE SET updated_at = now()",
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(format!("create_session: {e}")))?;
        Ok(())
    }

    async fn append_message(
        &self,
        session_id: &str,
        direction: &str,
        content: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO messages (session_id, direction, content) VALUES ($1, $2, $3)",
        )
        .bind(session_id)
        .bind(direction)
        .bind(content)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(format!("append_message: {e}")))?;
        Ok(())
    }

    async fn list_messages(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<StoredMessage>> {
        use sqlx::Row as _;

        let rows = sqlx::query(
            "SELECT direction, content,
                    timestamp::TEXT AS timestamp,
                    metadata, source, provider, model, tokens_in, tokens_out
             FROM messages
             WHERE session_id = $1
             ORDER BY id DESC
             LIMIT $2",
        )
        .bind(session_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Database(format!("list_messages: {e}")))?;

        let mut msgs: Vec<StoredMessage> = rows
            .iter()
            .map(|r| {
                let ts_raw: Option<String> = r.try_get("timestamp").ok();
                let meta_raw: Option<String> = r.try_get("metadata").ok();
                StoredMessage {
                    direction: r.try_get("direction").unwrap_or_default(),
                    content: r.try_get("content").unwrap_or_default(),
                    timestamp: ts_raw
                        .as_deref()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or_else(chrono::Utc::now),
                    metadata: meta_raw
                        .as_deref()
                        .and_then(|s| serde_json::from_str(s).ok())
                        .unwrap_or(serde_json::Value::Null),
                    source: r.try_get("source").ok(),
                    provider: r.try_get("provider").ok(),
                    model: r.try_get("model").ok(),
                    tokens_in: r.try_get("tokens_in").ok(),
                    tokens_out: r.try_get("tokens_out").ok(),
                }
            })
            .collect();

        // Query is DESC (efficient tail fetch); return chronological order.
        msgs.reverse();
        Ok(msgs)
    }
}
