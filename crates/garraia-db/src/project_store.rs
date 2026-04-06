//! Project, template, and RAG file management (Phases 2.1, 2.3, 2.4).
//!
//! All operations run on the same [`SessionStore`] connection so that
//! sessions can reference projects via `project_id`.

use garraia_common::{Error, Result};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::session_store::SessionStore;

// ============================================================================
// Data types
// ============================================================================

/// A project row (Phase 2.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: Option<String>,
    pub owner_id: Option<String>,
    /// JSON blob for template config / arbitrary settings.
    pub settings: serde_json::Value,
}

/// An indexed file inside a project (Phase 2.3 — RAG).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFile {
    pub id: String,
    pub project_id: String,
    pub file_path: String,
    pub content_hash: Option<String>,
    pub embedding: Option<Vec<u8>>,
    pub indexed_at: Option<String>,
    pub file_size: Option<i64>,
}

/// A reusable project template (Phase 2.4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectTemplate {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub system_prompt: Option<String>,
    pub tools_enabled: Option<String>,
    pub default_mode: Option<String>,
    pub created_at: String,
}

/// A data-retention record (Phase 7.4 — GDPR).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataRetentionRecord {
    pub id: String,
    pub entity_type: String,
    pub entity_id: String,
    pub expires_at: String,
    pub deleted_at: Option<String>,
}

// ============================================================================
// Phase 2.1 — Project CRUD
// ============================================================================

impl SessionStore {
    /// Create a new project.
    pub fn create_project(
        &self,
        name: &str,
        path: &str,
        description: Option<&str>,
        owner_id: Option<&str>,
        settings: Option<&serde_json::Value>,
    ) -> Result<Project> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let settings_json = settings
            .map(|s| s.to_string())
            .unwrap_or_else(|| "{}".to_string());

        self.connection()
            .execute(
                "INSERT INTO projects (id, name, path, description, created_at, updated_at, owner_id, settings)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7)",
                params![id, name, path, description, now, owner_id, settings_json],
            )
            .map_err(|e| Error::Database(format!("failed to create project: {e}")))?;

        Ok(Project {
            id,
            name: name.to_string(),
            path: path.to_string(),
            description: description.map(String::from),
            created_at: now.clone(),
            updated_at: Some(now),
            owner_id: owner_id.map(String::from),
            settings: settings.cloned().unwrap_or(serde_json::json!({})),
        })
    }

    /// Get a single project by ID.
    pub fn get_project(&self, project_id: &str) -> Result<Option<Project>> {
        self.connection()
            .query_row(
                "SELECT id, name, path, description, created_at, updated_at, owner_id, settings
                 FROM projects WHERE id = ?1",
                params![project_id],
                row_to_project,
            )
            .optional()
            .map_err(|e| Error::Database(format!("failed to get project: {e}")))
    }

    /// List all projects, optionally filtered by owner.
    pub fn list_projects(&self, owner_id: Option<&str>) -> Result<Vec<Project>> {
        let conn = self.connection();
        let mut stmt = conn
            .prepare(
                "SELECT id, name, path, description, created_at, updated_at, owner_id, settings
                 FROM projects
                 WHERE (?1 IS NULL OR owner_id = ?1)
                 ORDER BY created_at DESC",
            )
            .map_err(|e| Error::Database(format!("failed to prepare list projects: {e}")))?;

        let rows = stmt
            .query_map(params![owner_id], row_to_project)
            .map_err(|e| Error::Database(format!("failed to list projects: {e}")))?;

        let mut projects = Vec::new();
        for row in rows {
            projects.push(
                row.map_err(|e| Error::Database(format!("failed to read project row: {e}")))?,
            );
        }
        Ok(projects)
    }

    /// Update a project. Only non-`None` fields are changed.
    pub fn update_project(
        &self,
        project_id: &str,
        name: Option<&str>,
        path: Option<&str>,
        description: Option<&str>,
        settings: Option<&serde_json::Value>,
    ) -> Result<Option<Project>> {
        let mut updates = vec!["updated_at = datetime('now')".to_string()];
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![];

        if let Some(n) = name {
            updates.push("name = ?".to_string());
            params_vec.push(Box::new(n.to_string()));
        }
        if let Some(p) = path {
            updates.push("path = ?".to_string());
            params_vec.push(Box::new(p.to_string()));
        }
        if let Some(d) = description {
            updates.push("description = ?".to_string());
            params_vec.push(Box::new(d.to_string()));
        }
        if let Some(s) = settings {
            updates.push("settings = ?".to_string());
            params_vec.push(Box::new(s.to_string()));
        }

        if updates.len() == 1 {
            return self.get_project(project_id);
        }

        params_vec.push(Box::new(project_id.to_string()));

        let query = format!(
            "UPDATE projects SET {} WHERE id = ?",
            updates.join(", ")
        );
        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        self.connection()
            .execute(&query, params_refs.as_slice())
            .map_err(|e| Error::Database(format!("failed to update project: {e}")))?;

        self.get_project(project_id)
    }

    /// Delete a project (cascades to project_files via FK).
    pub fn delete_project(&self, project_id: &str) -> Result<bool> {
        let rows = self
            .connection()
            .execute("DELETE FROM projects WHERE id = ?1", params![project_id])
            .map_err(|e| Error::Database(format!("failed to delete project: {e}")))?;
        Ok(rows > 0)
    }

    /// Get all sessions associated with a project.
    pub fn get_project_sessions(
        &self,
        project_id: &str,
    ) -> Result<Vec<String>> {
        let conn = self.connection();
        let mut stmt = conn
            .prepare("SELECT id FROM sessions WHERE project_id = ?1 ORDER BY updated_at DESC")
            .map_err(|e| Error::Database(format!("failed to prepare project sessions query: {e}")))?;

        let rows = stmt
            .query_map(params![project_id], |row| row.get::<_, String>(0))
            .map_err(|e| Error::Database(format!("failed to get project sessions: {e}")))?;

        let mut ids = Vec::new();
        for row in rows {
            ids.push(
                row.map_err(|e| Error::Database(format!("failed to read session id: {e}")))?,
            );
        }
        Ok(ids)
    }

    /// Associate an existing session with a project.
    pub fn associate_session_to_project(
        &self,
        session_id: &str,
        project_id: &str,
    ) -> Result<()> {
        self.connection()
            .execute(
                "UPDATE sessions SET project_id = ?1, updated_at = datetime('now') WHERE id = ?2",
                params![project_id, session_id],
            )
            .map_err(|e| Error::Database(format!("failed to associate session to project: {e}")))?;
        Ok(())
    }

    // ========================================================================
    // Phase 2.3 — RAG Indexing
    // ========================================================================

    /// Index (upsert) a file for a project.
    pub fn index_project_file(
        &self,
        project_id: &str,
        file_path: &str,
        content_hash: Option<&str>,
        embedding: Option<&[u8]>,
        file_size: Option<i64>,
    ) -> Result<ProjectFile> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

        self.connection()
            .execute(
                "INSERT INTO project_files (id, project_id, file_path, content_hash, embedding, indexed_at, file_size)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(project_id, file_path) DO UPDATE SET
                   content_hash = excluded.content_hash,
                   embedding    = excluded.embedding,
                   indexed_at   = excluded.indexed_at,
                   file_size    = excluded.file_size",
                params![id, project_id, file_path, content_hash, embedding, now, file_size],
            )
            .map_err(|e| Error::Database(format!("failed to index project file: {e}")))?;

        Ok(ProjectFile {
            id,
            project_id: project_id.to_string(),
            file_path: file_path.to_string(),
            content_hash: content_hash.map(String::from),
            embedding: embedding.map(Vec::from),
            indexed_at: Some(now),
            file_size,
        })
    }

    /// List all indexed files for a project.
    pub fn get_project_files(&self, project_id: &str) -> Result<Vec<ProjectFile>> {
        let conn = self.connection();
        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, file_path, content_hash, embedding, indexed_at, file_size
                 FROM project_files WHERE project_id = ?1 ORDER BY file_path",
            )
            .map_err(|e| Error::Database(format!("failed to prepare project files query: {e}")))?;

        let rows = stmt
            .query_map(params![project_id], row_to_project_file)
            .map_err(|e| Error::Database(format!("failed to get project files: {e}")))?;

        let mut files = Vec::new();
        for row in rows {
            files.push(
                row.map_err(|e| Error::Database(format!("failed to read project file row: {e}")))?,
            );
        }
        Ok(files)
    }

    /// Simple vector-similarity search over project file embeddings.
    ///
    /// Computes cosine similarity in Rust (no sqlite-vec dependency here)
    /// and returns the top `limit` results above `min_similarity`.
    pub fn search_project_files(
        &self,
        project_id: &str,
        query_embedding: &[u8],
        limit: usize,
    ) -> Result<Vec<ProjectFile>> {
        let all_files = self.get_project_files(project_id)?;

        let query_floats = blob_to_f32_vec(query_embedding);

        let mut scored: Vec<(f32, ProjectFile)> = all_files
            .into_iter()
            .filter_map(|f| {
                let emb = f.embedding.as_ref()?;
                let file_floats = blob_to_f32_vec(emb);
                if file_floats.len() != query_floats.len() {
                    return None;
                }
                let sim = cosine_similarity(&query_floats, &file_floats);
                Some((sim, f))
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        Ok(scored.into_iter().take(limit).map(|(_, f)| f).collect())
    }

    /// Delete a single indexed file.
    pub fn delete_project_file(&self, file_id: &str) -> Result<bool> {
        let rows = self
            .connection()
            .execute("DELETE FROM project_files WHERE id = ?1", params![file_id])
            .map_err(|e| Error::Database(format!("failed to delete project file: {e}")))?;
        Ok(rows > 0)
    }

    /// Check whether a file needs re-indexing by comparing its content hash.
    /// Returns `true` if the file is not indexed or the hash differs.
    pub fn needs_reindex(
        &self,
        project_id: &str,
        file_path: &str,
        current_hash: &str,
    ) -> Result<bool> {
        let stored_hash: Option<String> = self
            .connection()
            .query_row(
                "SELECT content_hash FROM project_files
                 WHERE project_id = ?1 AND file_path = ?2",
                params![project_id, file_path],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| Error::Database(format!("failed to check reindex: {e}")))?
            .flatten();

        Ok(stored_hash.as_deref() != Some(current_hash))
    }

    // ========================================================================
    // Phase 2.4 — Project Templates
    // ========================================================================

    /// Create a new project template.
    pub fn create_template(
        &self,
        name: &str,
        description: Option<&str>,
        system_prompt: Option<&str>,
        tools_enabled: Option<&str>,
        default_mode: Option<&str>,
    ) -> Result<ProjectTemplate> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

        self.connection()
            .execute(
                "INSERT INTO project_templates (id, name, description, system_prompt, tools_enabled, default_mode, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![id, name, description, system_prompt, tools_enabled, default_mode, now],
            )
            .map_err(|e| Error::Database(format!("failed to create template: {e}")))?;

        Ok(ProjectTemplate {
            id,
            name: name.to_string(),
            description: description.map(String::from),
            system_prompt: system_prompt.map(String::from),
            tools_enabled: tools_enabled.map(String::from),
            default_mode: default_mode.map(String::from),
            created_at: now,
        })
    }

    /// List all project templates.
    pub fn list_templates(&self) -> Result<Vec<ProjectTemplate>> {
        let conn = self.connection();
        let mut stmt = conn
            .prepare(
                "SELECT id, name, description, system_prompt, tools_enabled, default_mode, created_at
                 FROM project_templates ORDER BY name",
            )
            .map_err(|e| Error::Database(format!("failed to prepare list templates: {e}")))?;

        let rows = stmt
            .query_map([], row_to_template)
            .map_err(|e| Error::Database(format!("failed to list templates: {e}")))?;

        let mut templates = Vec::new();
        for row in rows {
            templates.push(
                row.map_err(|e| Error::Database(format!("failed to read template row: {e}")))?,
            );
        }
        Ok(templates)
    }

    /// Get a template by ID.
    pub fn get_template(&self, template_id: &str) -> Result<Option<ProjectTemplate>> {
        self.connection()
            .query_row(
                "SELECT id, name, description, system_prompt, tools_enabled, default_mode, created_at
                 FROM project_templates WHERE id = ?1",
                params![template_id],
                row_to_template,
            )
            .optional()
            .map_err(|e| Error::Database(format!("failed to get template: {e}")))
    }

    /// Delete a template.
    pub fn delete_template(&self, template_id: &str) -> Result<bool> {
        let rows = self
            .connection()
            .execute(
                "DELETE FROM project_templates WHERE id = ?1",
                params![template_id],
            )
            .map_err(|e| Error::Database(format!("failed to delete template: {e}")))?;
        Ok(rows > 0)
    }

    /// Create a new project pre-populated from a template.
    pub fn create_project_from_template(
        &self,
        template_id: &str,
        name: &str,
        path: &str,
        owner_id: Option<&str>,
    ) -> Result<Project> {
        let template = self
            .get_template(template_id)?
            .ok_or_else(|| Error::Database(format!("template not found: {template_id}")))?;

        let settings = serde_json::json!({
            "system_prompt": template.system_prompt,
            "tools_enabled": template.tools_enabled,
            "default_mode": template.default_mode,
            "template_id": template.id,
        });

        self.create_project(
            name,
            path,
            template.description.as_deref(),
            owner_id,
            Some(&settings),
        )
    }

    // ========================================================================
    // Phase 7.4 — GDPR Compliance
    // ========================================================================

    /// Export all user data as a JSON value (sessions, messages, memory, projects).
    pub fn export_user_data(&self, user_id: &str) -> Result<serde_json::Value> {
        let conn = self.connection();

        // Sessions owned by user
        let mut stmt = conn
            .prepare(
                "SELECT id, tenant_id, channel_id, user_id, created_at, updated_at, metadata, project_id
                 FROM sessions WHERE user_id = ?1",
            )
            .map_err(|e| Error::Database(format!("export_user_data sessions prepare: {e}")))?;

        let sessions: Vec<serde_json::Value> = stmt
            .query_map(params![user_id], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "tenant_id": row.get::<_, String>(1)?,
                    "channel_id": row.get::<_, String>(2)?,
                    "user_id": row.get::<_, String>(3)?,
                    "created_at": row.get::<_, String>(4)?,
                    "updated_at": row.get::<_, Option<String>>(5)?,
                    "metadata": row.get::<_, String>(6)?,
                    "project_id": row.get::<_, Option<String>>(7)?,
                }))
            })
            .map_err(|e| Error::Database(format!("export_user_data sessions: {e}")))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Database(format!("export_user_data sessions collect: {e}")))?;

        // Collect session IDs for message export
        let session_ids: Vec<String> = sessions
            .iter()
            .filter_map(|s| s.get("id").and_then(|v| v.as_str()).map(String::from))
            .collect();

        // Messages for all user sessions
        let mut all_messages = Vec::new();
        for sid in &session_ids {
            let mut msg_stmt = conn
                .prepare(
                    "SELECT id, session_id, direction, content, timestamp, metadata, source, provider, model, tokens_in, tokens_out
                     FROM messages WHERE session_id = ?1 ORDER BY timestamp",
                )
                .map_err(|e| Error::Database(format!("export_user_data messages prepare: {e}")))?;

            let msgs: Vec<serde_json::Value> = msg_stmt
                .query_map(params![sid], |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "session_id": row.get::<_, String>(1)?,
                        "direction": row.get::<_, String>(2)?,
                        "content": row.get::<_, String>(3)?,
                        "timestamp": row.get::<_, String>(4)?,
                        "metadata": row.get::<_, String>(5)?,
                        "source": row.get::<_, Option<String>>(6)?,
                        "provider": row.get::<_, Option<String>>(7)?,
                        "model": row.get::<_, Option<String>>(8)?,
                        "tokens_in": row.get::<_, Option<i32>>(9)?,
                        "tokens_out": row.get::<_, Option<i32>>(10)?,
                    }))
                })
                .map_err(|e| Error::Database(format!("export_user_data messages: {e}")))?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| Error::Database(format!("export_user_data messages collect: {e}")))?;

            all_messages.extend(msgs);
        }

        // Projects owned by user
        let projects = self.list_projects(Some(user_id))?;

        // Mobile user record (if any)
        let mobile_user = self.find_mobile_user_by_id(user_id)?;

        Ok(serde_json::json!({
            "user_id": user_id,
            "exported_at": chrono::Utc::now().to_rfc3339(),
            "sessions": sessions,
            "messages": all_messages,
            "projects": projects,
            "mobile_user": mobile_user,
        }))
    }

    /// Delete all data belonging to a user (GDPR right to erasure).
    ///
    /// Cascade-deletes: messages (via FK), session keys, summaries, session
    /// tokens, projects (and project_files via FK), mobile_user record.
    pub fn delete_user_data(&self, user_id: &str) -> Result<usize> {
        let conn = self.connection();
        let mut total_deleted: usize = 0;

        // 1. Collect session IDs owned by this user
        let session_ids: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT id FROM sessions WHERE user_id = ?1")
                .map_err(|e| Error::Database(format!("delete_user_data session ids: {e}")))?;
            stmt.query_map(params![user_id], |row| row.get::<_, String>(0))
                .map_err(|e| Error::Database(format!("delete_user_data session ids query: {e}")))?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| Error::Database(format!("delete_user_data session ids collect: {e}")))?
        };

        for sid in &session_ids {
            // Delete messages
            let n = conn
                .execute("DELETE FROM messages WHERE session_id = ?1", params![sid])
                .map_err(|e| Error::Database(format!("delete_user_data messages: {e}")))?;
            total_deleted += n;

            // Delete session keys
            let n = conn
                .execute(
                    "DELETE FROM chat_session_keys WHERE session_id = ?1",
                    params![sid],
                )
                .map_err(|e| Error::Database(format!("delete_user_data session keys: {e}")))?;
            total_deleted += n;

            // Delete summaries
            let n = conn
                .execute(
                    "DELETE FROM chat_summaries WHERE session_id = ?1",
                    params![sid],
                )
                .map_err(|e| Error::Database(format!("delete_user_data summaries: {e}")))?;
            total_deleted += n;

            // Delete session tokens
            let n = conn
                .execute(
                    "DELETE FROM session_tokens WHERE session_id = ?1",
                    params![sid],
                )
                .map_err(|e| Error::Database(format!("delete_user_data session tokens: {e}")))?;
            total_deleted += n;

            // Delete scheduled tasks
            let n = conn
                .execute(
                    "DELETE FROM scheduled_tasks WHERE session_id = ?1",
                    params![sid],
                )
                .map_err(|e| Error::Database(format!("delete_user_data tasks: {e}")))?;
            total_deleted += n;
        }

        // 2. Delete sessions themselves
        let n = conn
            .execute("DELETE FROM sessions WHERE user_id = ?1", params![user_id])
            .map_err(|e| Error::Database(format!("delete_user_data sessions: {e}")))?;
        total_deleted += n;

        // 3. Delete projects owned by this user (project_files cascade via FK)
        let n = conn
            .execute("DELETE FROM projects WHERE owner_id = ?1", params![user_id])
            .map_err(|e| Error::Database(format!("delete_user_data projects: {e}")))?;
        total_deleted += n;

        // 4. Delete custom modes
        let n = conn
            .execute("DELETE FROM custom_modes WHERE user_id = ?1", params![user_id])
            .map_err(|e| Error::Database(format!("delete_user_data custom_modes: {e}")))?;
        total_deleted += n;

        // 5. Delete mobile user record
        let n = conn
            .execute("DELETE FROM mobile_users WHERE id = ?1", params![user_id])
            .map_err(|e| Error::Database(format!("delete_user_data mobile_user: {e}")))?;
        total_deleted += n;

        // 6. Record deletion in data_retention
        let retention_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        conn.execute(
            "INSERT INTO data_retention (id, entity_type, entity_id, expires_at, deleted_at)
             VALUES (?1, 'user', ?2, ?3, ?3)",
            params![retention_id, user_id, now],
        )
        .map_err(|e| Error::Database(format!("delete_user_data retention record: {e}")))?;

        Ok(total_deleted)
    }

    /// Create a data-retention record (e.g., for scheduling future deletion).
    pub fn create_data_retention(
        &self,
        entity_type: &str,
        entity_id: &str,
        expires_at: &str,
    ) -> Result<DataRetentionRecord> {
        let id = uuid::Uuid::new_v4().to_string();
        self.connection()
            .execute(
                "INSERT INTO data_retention (id, entity_type, entity_id, expires_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![id, entity_type, entity_id, expires_at],
            )
            .map_err(|e| Error::Database(format!("failed to create data retention: {e}")))?;

        Ok(DataRetentionRecord {
            id,
            entity_type: entity_type.to_string(),
            entity_id: entity_id.to_string(),
            expires_at: expires_at.to_string(),
            deleted_at: None,
        })
    }

    /// List data-retention records that have expired but not yet been deleted.
    pub fn list_expired_retention_records(&self) -> Result<Vec<DataRetentionRecord>> {
        let conn = self.connection();
        let mut stmt = conn
            .prepare(
                "SELECT id, entity_type, entity_id, expires_at, deleted_at
                 FROM data_retention
                 WHERE deleted_at IS NULL AND datetime(expires_at) <= datetime('now')
                 ORDER BY expires_at",
            )
            .map_err(|e| Error::Database(format!("failed to list expired retention: {e}")))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(DataRetentionRecord {
                    id: row.get(0)?,
                    entity_type: row.get(1)?,
                    entity_id: row.get(2)?,
                    expires_at: row.get(3)?,
                    deleted_at: row.get(4)?,
                })
            })
            .map_err(|e| Error::Database(format!("failed to query expired retention: {e}")))?;

        let mut records = Vec::new();
        for row in rows {
            records.push(
                row.map_err(|e| Error::Database(format!("failed to read retention row: {e}")))?,
            );
        }
        Ok(records)
    }

    /// Mark a retention record as deleted.
    pub fn mark_retention_deleted(&self, retention_id: &str) -> Result<()> {
        self.connection()
            .execute(
                "UPDATE data_retention SET deleted_at = datetime('now') WHERE id = ?1",
                params![retention_id],
            )
            .map_err(|e| Error::Database(format!("failed to mark retention deleted: {e}")))?;
        Ok(())
    }
}

// ============================================================================
// Row mappers
// ============================================================================

fn row_to_project(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    let settings_raw: String = row.get(7)?;
    Ok(Project {
        id: row.get(0)?,
        name: row.get(1)?,
        path: row.get(2)?,
        description: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        owner_id: row.get(6)?,
        settings: serde_json::from_str(&settings_raw).unwrap_or(serde_json::json!({})),
    })
}

fn row_to_project_file(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectFile> {
    Ok(ProjectFile {
        id: row.get(0)?,
        project_id: row.get(1)?,
        file_path: row.get(2)?,
        content_hash: row.get(3)?,
        embedding: row.get(4)?,
        indexed_at: row.get(5)?,
        file_size: row.get(6)?,
    })
}

fn row_to_template(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectTemplate> {
    Ok(ProjectTemplate {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        system_prompt: row.get(3)?,
        tools_enabled: row.get(4)?,
        default_mode: row.get(5)?,
        created_at: row.get(6)?,
    })
}

// ============================================================================
// Helpers
// ============================================================================

fn blob_to_f32_vec(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a.sqrt() * norm_b.sqrt())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use crate::session_store::SessionStore;

    fn make_store() -> SessionStore {
        SessionStore::in_memory().expect("in-memory store should open")
    }

    // ── Phase 2.1: Project CRUD ─────────────────────────────────────────

    #[test]
    fn create_and_get_project() {
        let store = make_store();
        let project = store
            .create_project("test-proj", "/tmp/test", Some("a description"), Some("user-1"), None)
            .expect("create_project");

        assert_eq!(project.name, "test-proj");
        assert_eq!(project.path, "/tmp/test");

        let fetched = store.get_project(&project.id).expect("get_project").unwrap();
        assert_eq!(fetched.name, "test-proj");
        assert_eq!(fetched.owner_id, Some("user-1".to_string()));
    }

    #[test]
    fn list_projects_by_owner() {
        let store = make_store();
        store.create_project("p1", "/a", None, Some("u1"), None).unwrap();
        store.create_project("p2", "/b", None, Some("u2"), None).unwrap();
        store.create_project("p3", "/c", None, Some("u1"), None).unwrap();

        let u1_projects = store.list_projects(Some("u1")).unwrap();
        assert_eq!(u1_projects.len(), 2);

        let all_projects = store.list_projects(None).unwrap();
        assert_eq!(all_projects.len(), 3);
    }

    #[test]
    fn update_project() {
        let store = make_store();
        let project = store.create_project("orig", "/orig", None, None, None).unwrap();
        let updated = store
            .update_project(&project.id, Some("renamed"), None, None, None)
            .unwrap()
            .unwrap();
        assert_eq!(updated.name, "renamed");
        assert_eq!(updated.path, "/orig");
    }

    #[test]
    fn delete_project() {
        let store = make_store();
        let project = store.create_project("del-me", "/tmp", None, None, None).unwrap();
        assert!(store.delete_project(&project.id).unwrap());
        assert!(store.get_project(&project.id).unwrap().is_none());
    }

    #[test]
    fn associate_session_to_project() {
        let store = make_store();
        let project = store.create_project("proj", "/p", None, None, None).unwrap();
        store
            .upsert_session("s1", "api", "user-1", &serde_json::json!({}))
            .unwrap();
        store.associate_session_to_project("s1", &project.id).unwrap();

        let sessions = store.get_project_sessions(&project.id).unwrap();
        assert_eq!(sessions, vec!["s1".to_string()]);
    }

    // ── Phase 2.3: RAG Indexing ─────────────────────────────────────────

    #[test]
    fn index_and_list_project_files() {
        let store = make_store();
        let project = store.create_project("proj", "/p", None, None, None).unwrap();

        store
            .index_project_file(&project.id, "src/main.rs", Some("abc123"), None, Some(1024))
            .unwrap();
        store
            .index_project_file(&project.id, "Cargo.toml", Some("def456"), None, Some(256))
            .unwrap();

        let files = store.get_project_files(&project.id).unwrap();
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn needs_reindex_checks_hash() {
        let store = make_store();
        let project = store.create_project("proj", "/p", None, None, None).unwrap();

        // Not indexed yet -> needs reindex
        assert!(store.needs_reindex(&project.id, "foo.rs", "hash1").unwrap());

        // Index it
        store.index_project_file(&project.id, "foo.rs", Some("hash1"), None, None).unwrap();

        // Same hash -> no reindex
        assert!(!store.needs_reindex(&project.id, "foo.rs", "hash1").unwrap());

        // Different hash -> needs reindex
        assert!(store.needs_reindex(&project.id, "foo.rs", "hash2").unwrap());
    }

    #[test]
    fn delete_project_file() {
        let store = make_store();
        let project = store.create_project("proj", "/p", None, None, None).unwrap();
        let file = store
            .index_project_file(&project.id, "a.rs", Some("h"), None, None)
            .unwrap();

        assert!(store.delete_project_file(&file.id).unwrap());
        assert_eq!(store.get_project_files(&project.id).unwrap().len(), 0);
    }

    // ── Phase 2.4: Templates ────────────────────────────────────────────

    #[test]
    fn create_and_list_templates() {
        let store = make_store();
        let t = store
            .create_template("rust-cli", Some("Rust CLI template"), Some("You are a Rust expert"), Some("bash,edit"), Some("code"))
            .unwrap();

        assert_eq!(t.name, "rust-cli");

        let all = store.list_templates().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "rust-cli");
    }

    #[test]
    fn get_and_delete_template() {
        let store = make_store();
        let t = store.create_template("tmp", None, None, None, None).unwrap();

        let fetched = store.get_template(&t.id).unwrap().unwrap();
        assert_eq!(fetched.name, "tmp");

        assert!(store.delete_template(&t.id).unwrap());
        assert!(store.get_template(&t.id).unwrap().is_none());
    }

    #[test]
    fn create_project_from_template() {
        let store = make_store();
        let template = store
            .create_template("tmpl", Some("a tmpl"), Some("system prompt"), Some("bash"), Some("code"))
            .unwrap();

        let project = store
            .create_project_from_template(&template.id, "my-proj", "/my/proj", Some("owner-1"))
            .unwrap();

        assert_eq!(project.name, "my-proj");
        assert_eq!(
            project.settings.get("template_id").and_then(|v| v.as_str()),
            Some(template.id.as_str())
        );
        assert_eq!(
            project.settings.get("system_prompt").and_then(|v| v.as_str()),
            Some("system prompt")
        );
    }

    // ── Phase 7.4: GDPR ────────────────────────────────────────────────

    #[test]
    fn export_user_data_returns_all_user_entities() {
        let store = make_store();

        // Create session + messages for user
        store.upsert_session("s1", "api", "user-gdpr", &serde_json::json!({})).unwrap();
        store
            .append_message("s1", "user", "hello", chrono::Utc::now(), &serde_json::json!({}))
            .unwrap();

        // Create a project for the user
        store.create_project("proj", "/p", None, Some("user-gdpr"), None).unwrap();

        let export = store.export_user_data("user-gdpr").unwrap();
        assert_eq!(export["user_id"], "user-gdpr");
        assert_eq!(export["sessions"].as_array().unwrap().len(), 1);
        assert_eq!(export["messages"].as_array().unwrap().len(), 1);
        assert_eq!(export["projects"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn delete_user_data_cascades_everything() {
        let store = make_store();

        store.upsert_session("s1", "api", "user-del", &serde_json::json!({})).unwrap();
        store
            .append_message("s1", "user", "bye", chrono::Utc::now(), &serde_json::json!({}))
            .unwrap();
        store.create_project("proj", "/p", None, Some("user-del"), None).unwrap();

        let deleted = store.delete_user_data("user-del").unwrap();
        assert!(deleted > 0);

        // Verify everything is gone
        let export = store.export_user_data("user-del").unwrap();
        assert_eq!(export["sessions"].as_array().unwrap().len(), 0);
        assert_eq!(export["messages"].as_array().unwrap().len(), 0);
        assert_eq!(export["projects"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn data_retention_lifecycle() {
        let store = make_store();

        // Create a retention record that expires in the past
        let _record = store
            .create_data_retention("user", "user-123", "2020-01-01 00:00:00")
            .unwrap();

        let expired = store.list_expired_retention_records().unwrap();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].entity_id, "user-123");

        store.mark_retention_deleted(&expired[0].id).unwrap();

        let expired_after = store.list_expired_retention_records().unwrap();
        assert_eq!(expired_after.len(), 0);
    }
}
