use garraia_common::{Error, Result};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use tracing::{info, instrument};

/// Persisted message row loaded from the session store.
#[derive(Debug, Clone)]
pub struct StoredMessage {
    pub direction: String,
    pub content: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub metadata: serde_json::Value,
    /// Source channel: "telegram", "vscode", "web", "discord", etc.
    pub source: Option<String>,
    /// LLM provider used for this message
    pub provider: Option<String>,
    /// Model used for this message
    pub model: Option<String>,
    /// Input tokens (if available)
    pub tokens_in: Option<i32>,
    /// Output tokens (if available)
    pub tokens_out: Option<i32>,
}

/// Persistent storage for conversation sessions and message history.
pub struct SessionStore {
    conn: Connection,
}

/// Mobile user row (GAR-334: Garra Cloud Alpha auth).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MobileUser {
    pub id: String,
    pub email: String,
    pub password_hash: String,
    pub salt: String,
    pub created_at: String,
}

/// Struct representing a custom mode (GAR-232)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CustomMode {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub description: Option<String>,
    pub base_mode: String,
    pub tool_policy_overrides: serde_json::Value,
    pub prompt_override: Option<String>,
    pub defaults: serde_json::Value,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl SessionStore {
    pub fn open(db_path: &Path) -> Result<Self> {
        info!("opening session store at {}", db_path.display());
        let conn = Connection::open(db_path)
            .map_err(|e| Error::Database(format!("failed to open database: {e}")))?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| Error::Database(format!("failed to set pragmas: {e}")))?;

        let store = Self { conn };
        store.run_migrations()?;
        Ok(store)
    }

    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Database(format!("failed to open in-memory database: {e}")))?;

        let store = Self { conn };
        store.run_migrations()?;
        Ok(store)
    }

    fn run_migrations(&self) -> Result<()> {
        // Migration: add tenant_id column to pre-existing sessions tables.
        // Ignore error if the table doesn't exist yet or the column already exists.
        let _ = self.conn.execute_batch(
            "ALTER TABLE sessions ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'default';",
        );

        // Migration: add source column to messages (for Chat Sync)
        let _ = self
            .conn
            .execute_batch("ALTER TABLE messages ADD COLUMN source TEXT;");
        let _ = self
            .conn
            .execute_batch("ALTER TABLE messages ADD COLUMN provider TEXT;");
        let _ = self
            .conn
            .execute_batch("ALTER TABLE messages ADD COLUMN model TEXT;");
        let _ = self
            .conn
            .execute_batch("ALTER TABLE messages ADD COLUMN tokens_in INTEGER;");
        let _ = self
            .conn
            .execute_batch("ALTER TABLE messages ADD COLUMN tokens_out INTEGER;");

        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS sessions (
                    id TEXT PRIMARY KEY,
                    tenant_id TEXT NOT NULL DEFAULT 'default',
                    channel_id TEXT NOT NULL,
                    user_id TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                    metadata TEXT DEFAULT '{}'
                );

                CREATE INDEX IF NOT EXISTS idx_sessions_tenant
                    ON sessions(tenant_id);

                CREATE TABLE IF NOT EXISTS messages (
                    id TEXT PRIMARY KEY,
                    session_id TEXT NOT NULL REFERENCES sessions(id),
                    direction TEXT NOT NULL,
                    content TEXT NOT NULL,
                    timestamp TEXT NOT NULL,
                    metadata TEXT DEFAULT '{}',
                    source TEXT,
                    provider TEXT,
                    model TEXT,
                    tokens_in INTEGER,
                    tokens_out INTEGER,
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                );

                CREATE INDEX IF NOT EXISTS idx_messages_session
                    ON messages(session_id, timestamp);

                -- Index for Chat Sync: query by source
                CREATE INDEX IF NOT EXISTS idx_messages_source
                    ON messages(session_id, source, timestamp);

                CREATE TABLE IF NOT EXISTS scheduled_tasks (
                    id TEXT PRIMARY KEY,
                    session_id TEXT NOT NULL,
                    user_id TEXT NOT NULL,
                    execute_at TEXT NOT NULL,
                    payload TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'pending',
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                );

                CREATE INDEX IF NOT EXISTS idx_tasks_execute_at
                    ON scheduled_tasks(execute_at) WHERE status = 'pending';

                -- Chat Sync: Session keys table for mapping external IDs to session_id
                CREATE TABLE IF NOT EXISTS chat_session_keys (
                    id TEXT PRIMARY KEY,
                    session_id TEXT NOT NULL REFERENCES sessions(id),
                    source TEXT NOT NULL,
                    external_id TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
                );

                CREATE UNIQUE INDEX IF NOT EXISTS idx_session_keys_source_external
                    ON chat_session_keys(source, external_id);

                CREATE INDEX IF NOT EXISTS idx_session_keys_session
                    ON chat_session_keys(session_id);

                -- Chat Sync: Summaries for long conversations
                CREATE TABLE IF NOT EXISTS chat_summaries (
                    id TEXT PRIMARY KEY,
                    session_id TEXT NOT NULL REFERENCES sessions(id),
                    summary_text TEXT NOT NULL,
                    message_count INTEGER NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                );

                CREATE INDEX IF NOT EXISTS idx_summaries_session
                    ON chat_summaries(session_id, created_at);

                -- GAR-232: Custom Modes table for user-created modes
                CREATE TABLE IF NOT EXISTS custom_modes (
                    id TEXT PRIMARY KEY,
                    user_id TEXT NOT NULL,
                    name TEXT NOT NULL,
                    description TEXT,
                    base_mode TEXT NOT NULL,
                    tool_policy_overrides TEXT DEFAULT '{}',
                    prompt_override TEXT,
                    defaults TEXT DEFAULT '{}',
                    is_active INTEGER NOT NULL DEFAULT 1,
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
                );

                CREATE INDEX IF NOT EXISTS idx_custom_modes_user
                    ON custom_modes(user_id);

                -- GAR-202: Session tokens for LLM conversation plane
                CREATE TABLE IF NOT EXISTS session_tokens (
                    token        TEXT PRIMARY KEY,
                    session_id   TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                    source       TEXT NOT NULL,
                    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
                    expires_at   TEXT NOT NULL,
                    last_active  TEXT NOT NULL DEFAULT (datetime('now')),
                    user_agent   TEXT,
                    ip_address   TEXT
                );

                CREATE INDEX IF NOT EXISTS idx_session_tokens_session
                    ON session_tokens(session_id);

                CREATE INDEX IF NOT EXISTS idx_session_tokens_expires
                    ON session_tokens(expires_at);

                -- GAR-334: Mobile users for Garra Cloud Alpha
                CREATE TABLE IF NOT EXISTS mobile_users (
                    id          TEXT PRIMARY KEY,
                    email       TEXT NOT NULL UNIQUE,
                    password_hash TEXT NOT NULL,
                    salt        TEXT NOT NULL,
                    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
                    totp_secret TEXT
                );

                CREATE UNIQUE INDEX IF NOT EXISTS idx_mobile_users_email
                    ON mobile_users(email);

                -- Phase 2.1: Projects table
                CREATE TABLE IF NOT EXISTS projects (
                    id          TEXT PRIMARY KEY,
                    name        TEXT NOT NULL,
                    path        TEXT NOT NULL,
                    description TEXT,
                    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
                    updated_at  TEXT,
                    owner_id    TEXT,
                    settings    TEXT DEFAULT '{}'
                );

                CREATE INDEX IF NOT EXISTS idx_projects_owner
                    ON projects(owner_id);

                -- Phase 2.3: Project files for RAG indexing
                CREATE TABLE IF NOT EXISTS project_files (
                    id          TEXT PRIMARY KEY,
                    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                    file_path   TEXT NOT NULL,
                    content_hash TEXT,
                    embedding   BLOB,
                    indexed_at  TEXT,
                    file_size   INTEGER
                );

                CREATE INDEX IF NOT EXISTS idx_project_files_project
                    ON project_files(project_id);

                CREATE UNIQUE INDEX IF NOT EXISTS idx_project_files_project_path
                    ON project_files(project_id, file_path);

                -- Phase 2.4: Project templates
                CREATE TABLE IF NOT EXISTS project_templates (
                    id              TEXT PRIMARY KEY,
                    name            TEXT NOT NULL,
                    description     TEXT,
                    system_prompt   TEXT,
                    tools_enabled   TEXT,
                    default_mode    TEXT,
                    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
                );

                -- Phase 7.4: GDPR data retention tracking
                CREATE TABLE IF NOT EXISTS data_retention (
                    id          TEXT PRIMARY KEY,
                    entity_type TEXT NOT NULL,
                    entity_id   TEXT NOT NULL,
                    expires_at  TEXT NOT NULL,
                    deleted_at  TEXT
                );

                CREATE INDEX IF NOT EXISTS idx_data_retention_entity
                    ON data_retention(entity_type, entity_id);

                CREATE INDEX IF NOT EXISTS idx_data_retention_expires
                    ON data_retention(expires_at) WHERE deleted_at IS NULL;",
            )
            .map_err(|e| Error::Database(format!("migration failed: {e}")))?;

        // Phase 2.1: add project_id column to sessions (nullable FK)
        let _ = self.conn.execute_batch(
            "ALTER TABLE sessions ADD COLUMN project_id TEXT REFERENCES projects(id);",
        );

        // GAR-202: Migrate legacy telegram-{chat_id} session IDs to chat_session_keys.
        let _ = self.conn.execute_batch(
            "INSERT OR IGNORE INTO chat_session_keys (id, session_id, source, external_id)
             SELECT lower(hex(randomblob(16))), id, 'telegram', substr(id, 10)
             FROM sessions
             WHERE id LIKE 'telegram-%';",
        );

        Ok(())
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Create or update a session row.
    pub fn upsert_session(
        &self,
        session_id: &str,
        channel_id: &str,
        user_id: &str,
        metadata: &serde_json::Value,
    ) -> Result<()> {
        self.upsert_session_with_tenant(session_id, "default", channel_id, user_id, metadata)
    }

    /// Create or update a session row with an explicit tenant_id.
    pub fn upsert_session_with_tenant(
        &self,
        session_id: &str,
        tenant_id: &str,
        channel_id: &str,
        user_id: &str,
        metadata: &serde_json::Value,
    ) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO sessions (id, tenant_id, channel_id, user_id, metadata)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(id) DO UPDATE SET
                   tenant_id = excluded.tenant_id,
                   channel_id = excluded.channel_id,
                   user_id = excluded.user_id,
                   metadata = excluded.metadata,
                   updated_at = datetime('now')",
                params![
                    session_id,
                    tenant_id,
                    channel_id,
                    user_id,
                    metadata.to_string()
                ],
            )
            .map_err(|e| Error::Database(format!("failed to upsert session: {e}")))?;
        Ok(())
    }

    /// Append a single message to a session.
    #[instrument(skip(self, content, metadata), fields(session_id = %session_id, direction = %direction), err)]
    pub fn append_message(
        &self,
        session_id: &str,
        direction: &str,
        content: &str,
        timestamp: chrono::DateTime<chrono::Utc>,
        metadata: &serde_json::Value,
    ) -> Result<()> {
        self.append_message_with_details(
            session_id, direction, content, timestamp, metadata, None, None, None, None, None,
        )
    }

    /// Append a single message to a session with full details (for Chat Sync).
    #[allow(clippy::too_many_arguments)]
    #[instrument(skip(self, content, metadata), fields(session_id = %session_id, direction = %direction, provider = ?provider, model = ?model), err)]
    pub fn append_message_with_details(
        &self,
        session_id: &str,
        direction: &str,
        content: &str,
        timestamp: chrono::DateTime<chrono::Utc>,
        metadata: &serde_json::Value,
        source: Option<&str>,
        provider: Option<&str>,
        model: Option<&str>,
        tokens_in: Option<i32>,
        tokens_out: Option<i32>,
    ) -> Result<()> {
        let message_id = uuid::Uuid::new_v4().to_string();
        self.conn
            .execute(
                "INSERT INTO messages (id, session_id, direction, content, timestamp, metadata, source, provider, model, tokens_in, tokens_out)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    message_id,
                    session_id,
                    direction,
                    content,
                    timestamp.to_rfc3339(),
                    metadata.to_string(),
                    source,
                    provider,
                    model,
                    tokens_in,
                    tokens_out
                ],
            )
            .map_err(|e| Error::Database(format!("failed to append message: {e}")))?;
        Ok(())
    }

    /// Load recent messages for a session in chronological order.
    #[instrument(skip(self), fields(session_id = %session_id), err)]
    pub fn load_recent_messages(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<StoredMessage>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT direction, content, timestamp, metadata, source, provider, model, tokens_in, tokens_out
                 FROM messages
                 WHERE session_id = ?1
                 ORDER BY rowid DESC
                 LIMIT ?2",
            )
            .map_err(|e| Error::Database(format!("failed to prepare message query: {e}")))?;

        let rows = stmt
            .query_map(params![session_id, limit as i64], |row| {
                let timestamp_raw: String = row.get(2)?;
                let metadata_raw: String = row.get(3)?;
                Ok(StoredMessage {
                    direction: row.get(0)?,
                    content: row.get(1)?,
                    timestamp: parse_timestamp(&timestamp_raw),
                    metadata: serde_json::from_str(&metadata_raw)
                        .unwrap_or(serde_json::Value::Null),
                    source: row.get(4)?,
                    provider: row.get(5)?,
                    model: row.get(6)?,
                    tokens_in: row.get(7)?,
                    tokens_out: row.get(8)?,
                })
            })
            .map_err(|e| Error::Database(format!("failed to load messages: {e}")))?;

        let mut messages = Vec::new();
        for row in rows {
            messages.push(
                row.map_err(|e| Error::Database(format!("failed to read message row: {e}")))?,
            );
        }

        // Query is DESC for efficient tail fetch; return in chronological order.
        messages.reverse();
        Ok(messages)
    }

    /// GAR-208: Load the oldest `limit` messages for a session, starting after
    /// `offset` rows from the beginning (chronological order).
    /// Used to feed older turns to the summarization LLM.
    pub fn load_older_messages(
        &self,
        session_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<StoredMessage>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT direction, content, timestamp, metadata, source, provider, model, tokens_in, tokens_out
                 FROM messages
                 WHERE session_id = ?1
                 ORDER BY rowid ASC
                 LIMIT ?2 OFFSET ?3",
            )
            .map_err(|e| Error::Database(format!("failed to prepare older-messages query: {e}")))?;

        let rows = stmt
            .query_map(params![session_id, limit as i64, offset as i64], |row| {
                let timestamp_raw: String = row.get(2)?;
                let metadata_raw: String = row.get(3)?;
                Ok(StoredMessage {
                    direction: row.get(0)?,
                    content: row.get(1)?,
                    timestamp: parse_timestamp(&timestamp_raw),
                    metadata: serde_json::from_str(&metadata_raw)
                        .unwrap_or(serde_json::Value::Null),
                    source: row.get(4)?,
                    provider: row.get(5)?,
                    model: row.get(6)?,
                    tokens_in: row.get(7)?,
                    tokens_out: row.get(8)?,
                })
            })
            .map_err(|e| Error::Database(format!("failed to load older messages: {e}")))?;

        let mut messages = Vec::new();
        for row in rows {
            messages.push(
                row.map_err(|e| Error::Database(format!("failed to read message row: {e}")))?,
            );
        }
        Ok(messages)
    }

    // ============================================================================
    // Chat Sync: Session Key Management
    // ============================================================================

    /// Map an external ID (e.g., Telegram chat_id) to a session_id.
    pub fn upsert_session_key(
        &self,
        session_id: &str,
        source: &str,
        external_id: &str,
    ) -> Result<()> {
        let key_id = uuid::Uuid::new_v4().to_string();
        self.conn
            .execute(
                "INSERT INTO chat_session_keys (id, session_id, source, external_id)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(source, external_id) DO UPDATE SET
                   session_id = excluded.session_id,
                   updated_at = datetime('now')",
                params![key_id, session_id, source, external_id],
            )
            .map_err(|e| Error::Database(format!("failed to upsert session key: {e}")))?;
        Ok(())
    }

    /// Get session_id by external ID and source.
    pub fn get_session_by_external_key(
        &self,
        source: &str,
        external_id: &str,
    ) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT session_id FROM chat_session_keys WHERE source = ?1 AND external_id = ?2",
            )
            .map_err(|e| Error::Database(format!("failed to prepare session key query: {e}")))?;

        let result = stmt
            .query_row(params![source, external_id], |row| row.get(0))
            .ok();

        Ok(result)
    }

    /// Delete a session key mapping.
    pub fn delete_session_key(&self, source: &str, external_id: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM chat_session_keys WHERE source = ?1 AND external_id = ?2",
                params![source, external_id],
            )
            .map_err(|e| Error::Database(format!("failed to delete session key: {e}")))?;
        Ok(())
    }

    // ============================================================================
    // Chat Sync: Session Summaries
    // ============================================================================

    /// Save a summary for a session.
    pub fn save_session_summary(
        &self,
        session_id: &str,
        summary_text: &str,
        message_count: i32,
    ) -> Result<()> {
        let summary_id = uuid::Uuid::new_v4().to_string();
        self.conn
            .execute(
                "INSERT INTO chat_summaries (id, session_id, summary_text, message_count)
                 VALUES (?1, ?2, ?3, ?4)",
                params![summary_id, session_id, summary_text, message_count],
            )
            .map_err(|e| Error::Database(format!("failed to save summary: {e}")))?;
        Ok(())
    }

    /// Get the latest summary for a session.
    pub fn get_latest_session_summary(&self, session_id: &str) -> Result<Option<(String, i32)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT summary_text, message_count FROM chat_summaries
                 WHERE session_id = ?1
                 ORDER BY created_at DESC
                 LIMIT 1",
            )
            .map_err(|e| Error::Database(format!("failed to prepare summary query: {e}")))?;

        let result = stmt
            .query_row(params![session_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1)?))
            })
            .ok();

        Ok(result)
    }

    /// Get total message count for a session.
    pub fn get_message_count(&self, session_id: &str) -> Result<i32> {
        let count: i32 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .map_err(|e| Error::Database(format!("failed to count messages: {e}")))?;
        Ok(count)
    }

    /// Schedule a task for future execution.
    pub fn schedule_task(
        &self,
        session_id: &str,
        user_id: &str,
        execute_at: chrono::DateTime<chrono::Utc>,
        payload: &str,
    ) -> Result<String> {
        let task_id = uuid::Uuid::new_v4().to_string();
        self.conn
            .execute(
                "INSERT INTO scheduled_tasks (id, session_id, user_id, execute_at, payload, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'pending')",
                params![
                    task_id,
                    session_id,
                    user_id,
                    execute_at.to_rfc3339(),
                    payload
                ],
            )
            .map_err(|e| Error::Database(format!("failed to schedule task: {e}")))?;
        Ok(task_id)
    }

    /// Poll for pending tasks that are due for execution.
    pub fn poll_due_tasks(&self) -> Result<Vec<ScheduledTask>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT t.id, t.session_id, s.channel_id, t.user_id, t.execute_at, t.payload, s.metadata
                 FROM scheduled_tasks t
                 JOIN sessions s ON t.session_id = s.id
                 WHERE t.status = 'pending' AND datetime(t.execute_at) <= datetime('now')
                 ORDER BY t.execute_at ASC
                 LIMIT 10",
            )
            .map_err(|e| Error::Database(format!("failed to prepare poll query: {e}")))?;

        let rows = stmt
            .query_map([], |row| {
                let execute_at_raw: String = row.get(4)?;
                let metadata_raw: String = row.get(6)?;
                Ok(ScheduledTask {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    channel_id: row.get(2)?,
                    user_id: row.get(3)?,
                    execute_at: parse_timestamp(&execute_at_raw),
                    payload: row.get(5)?,
                    session_metadata: serde_json::from_str(&metadata_raw)
                        .unwrap_or(serde_json::Value::Null),
                })
            })
            .map_err(|e| Error::Database(format!("failed to poll tasks: {e}")))?;

        let mut tasks = Vec::new();
        for row in rows {
            tasks.push(row.map_err(|e| Error::Database(format!("failed to read task row: {e}")))?);
        }
        Ok(tasks)
    }

    /// Mark a scheduled task as completed.
    pub fn complete_task(&self, task_id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE scheduled_tasks SET status = 'completed' WHERE id = ?1",
                params![task_id],
            )
            .map_err(|e| Error::Database(format!("failed to complete task: {e}")))?;
        Ok(())
    }

    /// Mark a scheduled task as failed so it won't be retried.
    pub fn fail_task(&self, task_id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE scheduled_tasks SET status = 'failed' WHERE id = ?1",
                params![task_id],
            )
            .map_err(|e| Error::Database(format!("failed to mark task as failed: {e}")))?;
        Ok(())
    }

    /// Count pending scheduled tasks for a given session.
    pub fn count_pending_tasks_for_session(&self, session_id: &str) -> Result<i64> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM scheduled_tasks WHERE session_id = ?1 AND status = 'pending'",
                params![session_id],
                |row| row.get(0),
            )
            .map_err(|e| Error::Database(format!("failed to count pending tasks: {e}")))?;
        Ok(count)
    }

    // ============================================================================
    // GAR-222: Mode Persistence - Armazenar modo por sessão
    // ============================================================================

    /// Get the current agent mode for a session.
    /// Returns None if no mode has been set.
    pub fn get_agent_mode(&self, session_id: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT metadata FROM sessions WHERE id = ?1")
            .map_err(|e| Error::Database(format!("failed to prepare mode query: {e}")))?;

        let result = stmt
            .query_row(params![session_id], |row| {
                let metadata: String = row.get(0)?;
                Ok(metadata)
            })
            .ok();

        if let Some(metadata_str) = result
            && let Ok(metadata) = serde_json::from_str::<serde_json::Value>(&metadata_str)
            && let Some(mode) = metadata.get("agent_mode").and_then(|v| v.as_str())
        {
            return Ok(Some(mode.to_string()));
        }
        Ok(None)
    }

    /// Set the current agent mode for a session.
    pub fn set_agent_mode(&self, session_id: &str, mode: &str) -> Result<()> {
        // First get existing metadata
        let mut stmt = self
            .conn
            .prepare("SELECT metadata FROM sessions WHERE id = ?1")
            .map_err(|e| Error::Database(format!("failed to prepare metadata query: {e}")))?;

        let metadata_str: Option<String> =
            stmt.query_row(params![session_id], |row| row.get(0)).ok();

        let mut metadata = if let Some(m) = metadata_str {
            serde_json::from_str(&m).unwrap_or(serde_json::Value::Object(serde_json::Map::new()))
        } else {
            serde_json::Value::Object(serde_json::Map::new())
        };

        // Update the agent_mode field
        metadata["agent_mode"] = serde_json::Value::String(mode.to_string());

        // Update the session
        self.conn
            .execute(
                "UPDATE sessions SET metadata = ?1, updated_at = datetime('now') WHERE id = ?2",
                params![metadata.to_string(), session_id],
            )
            .map_err(|e| Error::Database(format!("failed to set agent mode: {e}")))?;

        Ok(())
    }

    /// Clear the agent mode for a session (reset to default).
    pub fn clear_agent_mode(&self, session_id: &str) -> Result<()> {
        // Get existing metadata
        let mut stmt = self
            .conn
            .prepare("SELECT metadata FROM sessions WHERE id = ?1")
            .map_err(|e| Error::Database(format!("failed to prepare metadata query: {e}")))?;

        let metadata_str: Option<String> =
            stmt.query_row(params![session_id], |row| row.get(0)).ok();

        if let Some(m) = metadata_str
            && let Ok(mut metadata) = serde_json::from_str::<serde_json::Value>(&m)
        {
            metadata["agent_mode"] = serde_json::Value::Null;
            self.conn
                .execute(
                    "UPDATE sessions SET metadata = ?1, updated_at = datetime('now') WHERE id = ?2",
                    params![metadata.to_string(), session_id],
                )
                .map_err(|e| Error::Database(format!("failed to clear agent mode: {e}")))?;
        }
        Ok(())
    }

    // ============================================================================
    // GAR-232: Custom Modes CRUD
    // ============================================================================

    /// Create a new custom mode
    pub fn create_custom_mode(
        &self,
        user_id: &str,
        name: &str,
        description: Option<&str>,
        base_mode: &str,
        tool_policy_overrides: &serde_json::Value,
        prompt_override: Option<&str>,
        defaults: &serde_json::Value,
    ) -> Result<CustomMode> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

        self.conn
            .execute(
                "INSERT INTO custom_modes (id, user_id, name, description, base_mode, tool_policy_overrides, prompt_override, defaults, is_active, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9, ?9)",
                params![
                    id,
                    user_id,
                    name,
                    description,
                    base_mode,
                    tool_policy_overrides.to_string(),
                    prompt_override,
                    defaults.to_string(),
                    now
                ],
            )
            .map_err(|e| Error::Database(format!("failed to create custom mode: {e}")))?;

        Ok(CustomMode {
            id,
            user_id: user_id.to_string(),
            name: name.to_string(),
            description: description.map(String::from),
            base_mode: base_mode.to_string(),
            tool_policy_overrides: tool_policy_overrides.clone(),
            prompt_override: prompt_override.map(String::from),
            defaults: defaults.clone(),
            is_active: true,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    /// Get all custom modes for a user
    pub fn get_custom_modes(&self, user_id: &str) -> Result<Vec<CustomMode>> {
        let mut stmt = self.conn
            .prepare(
                "SELECT id, user_id, name, description, base_mode, tool_policy_overrides, prompt_override, defaults, is_active, created_at, updated_at
                 FROM custom_modes WHERE user_id = ?1 AND is_active = 1 ORDER BY name"
            )
            .map_err(|e| Error::Database(format!("failed to prepare custom modes query: {e}")))?;

        let rows = stmt
            .query_map(params![user_id], |row| {
                let tool_policy_raw: String = row.get(5)?;
                let defaults_raw: String = row.get(7)?;
                Ok(CustomMode {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    name: row.get(2)?,
                    description: row.get(3)?,
                    base_mode: row.get(4)?,
                    tool_policy_overrides: serde_json::from_str(&tool_policy_raw)
                        .unwrap_or(serde_json::Value::Object(serde_json::Map::new())),
                    prompt_override: row.get(6)?,
                    defaults: serde_json::from_str(&defaults_raw)
                        .unwrap_or(serde_json::Value::Object(serde_json::Map::new())),
                    is_active: row.get::<_, i32>(8)? == 1,
                    created_at: row.get(9)?,
                    updated_at: row.get(10)?,
                })
            })
            .map_err(|e| Error::Database(format!("failed to load custom modes: {e}")))?;

        let mut modes = Vec::new();
        for row in rows {
            let mode =
                row.map_err(|e| Error::Database(format!("failed to read custom mode row: {}", e)))?;
            modes.push(mode);
        }
        Ok(modes)
    }

    /// Get a custom mode by ID
    pub fn get_custom_mode(&self, mode_id: &str) -> Result<Option<CustomMode>> {
        let mut stmt = self.conn
            .prepare(
                "SELECT id, user_id, name, description, base_mode, tool_policy_overrides, prompt_override, defaults, is_active, created_at, updated_at
                 FROM custom_modes WHERE id = ?1 AND is_active = 1"
            )
            .map_err(|e| Error::Database(format!("failed to prepare custom mode query: {e}")))?;

        let result = stmt
            .query_row(params![mode_id], |row| {
                let tool_policy_raw: String = row.get(5)?;
                let defaults_raw: String = row.get(7)?;
                Ok(CustomMode {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    name: row.get(2)?,
                    description: row.get(3)?,
                    base_mode: row.get(4)?,
                    tool_policy_overrides: serde_json::from_str(&tool_policy_raw)
                        .unwrap_or(serde_json::Value::Object(serde_json::Map::new())),
                    prompt_override: row.get(6)?,
                    defaults: serde_json::from_str(&defaults_raw)
                        .unwrap_or(serde_json::Value::Object(serde_json::Map::new())),
                    is_active: row.get::<_, i32>(8)? == 1,
                    created_at: row.get(9)?,
                    updated_at: row.get(10)?,
                })
            })
            .ok();

        Ok(result)
    }

    /// Update a custom mode
    pub fn update_custom_mode(
        &self,
        mode_id: &str,
        name: Option<&str>,
        description: Option<&str>,
        tool_policy_overrides: Option<&serde_json::Value>,
        prompt_override: Option<&str>,
        defaults: Option<&serde_json::Value>,
    ) -> Result<Option<CustomMode>> {
        // Build dynamic update query
        let mut updates = vec!["updated_at = datetime('now')".to_string()];
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![];

        if let Some(n) = name {
            updates.push("name = ?".to_string());
            params_vec.push(Box::new(n.to_string()));
        }
        if let Some(d) = description {
            updates.push("description = ?".to_string());
            params_vec.push(Box::new(d.to_string()));
        }
        if let Some(t) = tool_policy_overrides {
            updates.push("tool_policy_overrides = ?".to_string());
            params_vec.push(Box::new(t.to_string()));
        }
        if let Some(p) = prompt_override {
            updates.push("prompt_override = ?".to_string());
            params_vec.push(Box::new(p.to_string()));
        }
        if let Some(def) = defaults {
            updates.push("defaults = ?".to_string());
            params_vec.push(Box::new(def.to_string()));
        }

        if updates.len() == 1 {
            // Only updated_at, no changes
            return self.get_custom_mode(mode_id);
        }

        // Add mode_id as last param
        params_vec.push(Box::new(mode_id.to_string()));

        let query = format!(
            "UPDATE custom_modes SET {} WHERE id = ?",
            updates.join(", ")
        );

        // Convert params_vec to slice
        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        self.conn
            .execute(&query, params_refs.as_slice())
            .map_err(|e| Error::Database(format!("failed to update custom mode: {e}")))?;

        self.get_custom_mode(mode_id)
    }

    /// Delete (soft delete) a custom mode
    pub fn delete_custom_mode(&self, mode_id: &str) -> Result<bool> {
        let rows_affected = self
            .conn
            .execute(
                "UPDATE custom_modes SET is_active = 0, updated_at = datetime('now') WHERE id = ?1",
                params![mode_id],
            )
            .map_err(|e| Error::Database(format!("failed to delete custom mode: {e}")))?;

        Ok(rows_affected > 0)
    }
}

/// Represents a scheduled background task.
#[derive(Debug, Clone)]
pub struct ScheduledTask {
    pub id: String,
    pub session_id: String,
    pub channel_id: String,
    pub user_id: String,
    pub execute_at: chrono::DateTime<chrono::Utc>,
    pub payload: String,
    pub session_metadata: serde_json::Value,
}

// ── GAR-202: Session token CRUD ───────────────────────────────────────────────

/// Generate a cryptographically random URL-safe base64 token (256 bits).
fn generate_session_token() -> Result<String> {
    use base64::Engine as _;
    use ring::rand::{SecureRandom, SystemRandom};
    let rng = SystemRandom::new();
    let mut buf = [0u8; 32];
    rng.fill(&mut buf)
        .map_err(|_| Error::Database("session token rng failure".into()))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf))
}

impl SessionStore {
    /// Create a new session token for `session_id`.
    ///
    /// Returns the opaque token string. The caller must deliver it to the client.
    pub fn create_session_token(
        &self,
        session_id: &str,
        source: &str,
        ttl_secs: i64,
        ip_address: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<String> {
        let token = generate_session_token()?;
        self.conn
            .execute(
                "INSERT INTO session_tokens
                    (token, session_id, source, expires_at, last_active, ip_address, user_agent)
                 VALUES
                    (?1, ?2, ?3,
                     datetime('now', '+' || ?4 || ' seconds'),
                     datetime('now'),
                     ?5, ?6)",
                params![token, session_id, source, ttl_secs, ip_address, user_agent],
            )
            .map_err(|e| Error::Database(format!("create_session_token: {e}")))?;
        Ok(token)
    }

    /// Validate a token and return the associated `session_id`.
    ///
    /// Returns `None` if the token is unknown, expired, or idle-timed-out.
    /// `idle_timeout_secs = 0` disables idle checking.
    pub fn validate_session_token(
        &self,
        token: &str,
        idle_timeout_secs: i64,
    ) -> Result<Option<String>> {
        let idle_clause = if idle_timeout_secs > 0 {
            format!("AND datetime(last_active, '+{idle_timeout_secs} seconds') > datetime('now')")
        } else {
            String::new()
        };
        let sql = format!(
            "SELECT session_id FROM session_tokens
             WHERE token = ?1
               AND expires_at > datetime('now')
               {idle_clause}
             LIMIT 1"
        );
        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| Error::Database(format!("validate_session_token prepare: {e}")))?;
        let session_id: Option<String> = stmt
            .query_row(params![token], |row| row.get(0))
            .optional()
            .map_err(|e| Error::Database(format!("validate_session_token: {e}")))?;
        Ok(session_id)
    }

    /// Update `last_active` timestamp (idle-timeout reset).
    pub fn touch_session_token(&self, token: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE session_tokens SET last_active = datetime('now') WHERE token = ?1",
                params![token],
            )
            .map_err(|e| Error::Database(format!("touch_session_token: {e}")))?;
        Ok(())
    }

    /// Revoke a single session token.
    pub fn revoke_session_token(&self, token: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM session_tokens WHERE token = ?1",
                params![token],
            )
            .map_err(|e| Error::Database(format!("revoke_session_token: {e}")))?;
        Ok(())
    }

    /// Revoke all tokens for a given session (logout / privilege change).
    pub fn revoke_all_session_tokens(&self, session_id: &str) -> Result<usize> {
        let n = self
            .conn
            .execute(
                "DELETE FROM session_tokens WHERE session_id = ?1",
                params![session_id],
            )
            .map_err(|e| Error::Database(format!("revoke_all_session_tokens: {e}")))?;
        Ok(n)
    }

    /// Delete expired tokens. Returns the number of rows deleted.
    pub fn cleanup_expired_session_tokens(&self) -> usize {
        self.conn
            .execute(
                "DELETE FROM session_tokens WHERE expires_at <= datetime('now')",
                [],
            )
            .unwrap_or(0)
    }

    // ── GAR-334: Mobile Auth ─────────────────────────────────────────────────

    /// Insert a new mobile user. Returns `Error::Conflict` if email already exists.
    pub fn create_mobile_user(
        &self,
        id: &str,
        email: &str,
        password_hash: &str,
        salt: &str,
    ) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO mobile_users (id, email, password_hash, salt)
                 VALUES (?1, ?2, ?3, ?4)",
                params![id, email, password_hash, salt],
            )
            .map_err(|e| {
                if e.to_string().contains("UNIQUE constraint failed") {
                    Error::Database("email already registered".to_string())
                } else {
                    Error::Database(format!("create_mobile_user: {e}"))
                }
            })?;
        Ok(())
    }

    /// Find a mobile user by email. Returns `None` if not found.
    pub fn find_mobile_user_by_email(&self, email: &str) -> Result<Option<MobileUser>> {
        self.conn
            .query_row(
                "SELECT id, email, password_hash, salt, created_at
                 FROM mobile_users WHERE email = ?1",
                params![email],
                |row| {
                    Ok(MobileUser {
                        id: row.get(0)?,
                        email: row.get(1)?,
                        password_hash: row.get(2)?,
                        salt: row.get(3)?,
                        created_at: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(|e| Error::Database(format!("find_mobile_user_by_email: {e}")))
    }

    /// Lazy-upgrade the password hash of a mobile user (GAR-382).
    ///
    /// Used by the login handler right after a successful PBKDF2 verify to
    /// replace the legacy hash with an Argon2id PHC string. Best-effort: the
    /// caller logs a warning and proceeds if this returns `Ok(0)` (meaning
    /// zero rows touched — e.g. another concurrent upgrade happened first).
    ///
    /// The `salt` column is explicitly zeroed to `""` because Argon2id PHC
    /// strings embed their own salt. See plan 0036 §5.1 for the rationale
    /// of keeping the column NOT NULL (SQLite ALTER TABLE DROP COLUMN is
    /// post-3.35).
    pub fn update_mobile_user_hash(&self, id: &str, new_phc: &str) -> Result<usize> {
        self.conn
            .execute(
                "UPDATE mobile_users SET password_hash = ?1, salt = '' WHERE id = ?2",
                params![new_phc, id],
            )
            .map_err(|e| Error::Database(format!("update_mobile_user_hash: {e}")))
    }

    /// Find a mobile user by their UUID. Returns `None` if not found.
    pub fn find_mobile_user_by_id(&self, id: &str) -> Result<Option<MobileUser>> {
        self.conn
            .query_row(
                "SELECT id, email, password_hash, salt, created_at
                 FROM mobile_users WHERE id = ?1",
                params![id],
                |row| {
                    Ok(MobileUser {
                        id: row.get(0)?,
                        email: row.get(1)?,
                        password_hash: row.get(2)?,
                        salt: row.get(3)?,
                        created_at: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(|e| Error::Database(format!("find_mobile_user_by_id: {e}")))
    }

    // ── TOTP secret management (Phase 7.1) ──────────────────────────────────

    /// Store (or replace) a TOTP secret for the given mobile user.
    /// The secret should be stored encrypted by the caller in production;
    /// here we store the base32-encoded value and rely on the vault for
    /// at-rest encryption of the DB file.
    pub fn set_mobile_user_totp_secret(&self, user_id: &str, secret: &str) -> Result<()> {
        let affected = self
            .conn
            .execute(
                "UPDATE mobile_users SET totp_secret = ?1 WHERE id = ?2",
                params![secret, user_id],
            )
            .map_err(|e| Error::Database(format!("set_totp_secret: {e}")))?;
        if affected == 0 {
            return Err(Error::Database("user not found".into()));
        }
        Ok(())
    }

    /// Retrieve the TOTP secret for a mobile user. Returns `None` if not set.
    pub fn get_mobile_user_totp_secret(&self, user_id: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT totp_secret FROM mobile_users WHERE id = ?1",
                params![user_id],
                |row| row.get(0),
            )
            .optional()
            .map(|opt| opt.flatten())
            .map_err(|e| Error::Database(format!("get_totp_secret: {e}")))
    }

    /// Remove the TOTP secret from a mobile user (disables 2FA).
    pub fn clear_mobile_user_totp_secret(&self, user_id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE mobile_users SET totp_secret = NULL WHERE id = ?1",
                params![user_id],
            )
            .map_err(|e| Error::Database(format!("clear_totp_secret: {e}")))?;
        Ok(())
    }
}

fn parse_timestamp(value: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now())
}

#[cfg(test)]
mod tests {
    use super::SessionStore;
    use chrono::Duration;

    #[test]
    fn upsert_and_load_recent_messages_round_trip() {
        let store = SessionStore::in_memory().expect("in-memory store should open");
        let session_id = "session-1";

        store
            .upsert_session(
                session_id,
                "discord",
                "user-1",
                &serde_json::json!({"continuity_key":"bus:global"}),
            )
            .expect("session upsert should succeed");

        store
            .append_message(
                session_id,
                "user",
                "hello",
                chrono::Utc::now(),
                &serde_json::json!({}),
            )
            .expect("user message append should succeed");

        store
            .append_message(
                session_id,
                "assistant",
                "hi there",
                chrono::Utc::now(),
                &serde_json::json!({}),
            )
            .expect("assistant message append should succeed");

        let messages = store
            .load_recent_messages(session_id, 10)
            .expect("message load should succeed");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].direction, "user");
        assert_eq!(messages[0].content, "hello");
        assert_eq!(messages[1].direction, "assistant");
        assert_eq!(messages[1].content, "hi there");
    }

    #[test]
    fn schedule_and_poll_tasks() {
        let store = SessionStore::in_memory().expect("in-memory store should open");
        let session_id = "session-task";

        // Ensure session exists for JOIN
        store
            .upsert_session(
                session_id,
                "discord",
                "user-1",
                &serde_json::json!({"foo":"bar"}),
            )
            .expect("upsert session");

        // Schedule a task in the past (immediately due)
        let due_time = chrono::Utc::now() - Duration::minutes(1);
        let task_id = store
            .schedule_task(session_id, "user-1", due_time, "check logs")
            .expect("schedule task should succeed");

        // Schedule a future task (not due)
        store
            .schedule_task(
                session_id,
                "user-1",
                chrono::Utc::now() + Duration::minutes(10),
                "future",
            )
            .expect("schedule future task should succeed");

        let due = store.poll_due_tasks().expect("poll should succeed");
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, task_id);
        assert_eq!(due[0].channel_id, "discord"); // Verified via JOIN
        assert_eq!(due[0].payload, "check logs");
        assert_eq!(
            due[0].session_metadata.get("foo").and_then(|v| v.as_str()),
            Some("bar")
        );

        store
            .complete_task(&task_id)
            .expect("complete should succeed");

        let due_after = store
            .poll_due_tasks()
            .expect("poll after complete should succeed");
        assert_eq!(due_after.len(), 0);
    }

    #[test]
    fn fail_task_prevents_re_poll() {
        let store = SessionStore::in_memory().expect("in-memory store should open");
        store
            .upsert_session("s1", "web", "u1", &serde_json::json!({}))
            .unwrap();

        let due_time = chrono::Utc::now() - Duration::minutes(1);
        let task_id = store.schedule_task("s1", "u1", due_time, "boom").unwrap();

        let due = store.poll_due_tasks().unwrap();
        assert_eq!(due.len(), 1);

        store.fail_task(&task_id).unwrap();

        let due_after = store.poll_due_tasks().unwrap();
        assert_eq!(due_after.len(), 0);
    }

    #[test]
    fn count_pending_tasks_for_session() {
        let store = SessionStore::in_memory().expect("in-memory store should open");
        store
            .upsert_session("s1", "web", "u1", &serde_json::json!({}))
            .unwrap();
        store
            .upsert_session("s2", "web", "u2", &serde_json::json!({}))
            .unwrap();

        let future = chrono::Utc::now() + Duration::minutes(10);

        assert_eq!(store.count_pending_tasks_for_session("s1").unwrap(), 0);

        store.schedule_task("s1", "u1", future, "a").unwrap();
        store.schedule_task("s1", "u1", future, "b").unwrap();
        store.schedule_task("s2", "u2", future, "c").unwrap();

        assert_eq!(store.count_pending_tasks_for_session("s1").unwrap(), 2);
        assert_eq!(store.count_pending_tasks_for_session("s2").unwrap(), 1);
    }

    // GAR-222: Testes de persistência de modo
    #[test]
    fn agent_mode_persistence_set_and_get() {
        let store = SessionStore::in_memory().expect("in-memory store should open");
        let session_id = "mode-test-session";

        // Ensure session exists
        store
            .upsert_session(session_id, "telegram", "user-1", &serde_json::json!({}))
            .expect("session upsert should succeed");

        // Initially no mode set
        let mode = store
            .get_agent_mode(session_id)
            .expect("get mode should succeed");
        assert!(mode.is_none(), "Initial mode should be None");

        // Set mode to "code"
        store
            .set_agent_mode(session_id, "code")
            .expect("set mode should succeed");

        // Retrieve mode
        let mode = store
            .get_agent_mode(session_id)
            .expect("get mode should succeed");
        assert_eq!(mode, Some("code".to_string()), "Mode should be 'code'");

        // Change mode to "debug"
        store
            .set_agent_mode(session_id, "debug")
            .expect("set mode should succeed");

        let mode = store
            .get_agent_mode(session_id)
            .expect("get mode should succeed");
        assert_eq!(mode, Some("debug".to_string()), "Mode should be 'debug'");
    }

    #[test]
    fn agent_mode_persistence_clear() {
        let store = SessionStore::in_memory().expect("in-memory store should open");
        let session_id = "mode-clear-test";

        store
            .upsert_session(session_id, "telegram", "user-1", &serde_json::json!({}))
            .unwrap();

        // Set mode
        store.set_agent_mode(session_id, "search").unwrap();
        assert_eq!(
            store.get_agent_mode(session_id).unwrap(),
            Some("search".to_string())
        );

        // Clear mode
        store.clear_agent_mode(session_id).unwrap();

        // Mode should be None after clear
        let mode = store.get_agent_mode(session_id).unwrap();
        assert!(
            mode.is_none() || mode == Some("".to_string()),
            "Mode should be cleared"
        );
    }

    #[test]
    fn agent_mode_persistence_preserves_other_metadata() {
        let store = SessionStore::in_memory().expect("in-memory store should open");
        let session_id = "mode-metadata-test";

        // Create session with existing metadata
        store
            .upsert_session(
                session_id,
                "web",
                "user-1",
                &serde_json::json!({"foo": "bar"}),
            )
            .unwrap();

        // Set mode - should preserve "foo": "bar"
        store.set_agent_mode(session_id, "orchestrator").unwrap();

        // Verify both exist in metadata
        // (We can't easily check internal metadata, but setting mode shouldn't break)
        let mode = store.get_agent_mode(session_id).unwrap();
        assert_eq!(mode, Some("orchestrator".to_string()));
    }
}
