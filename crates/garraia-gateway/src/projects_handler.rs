//! Phase 1.3 — Project/Folder Concept: CRUD for projects and session creation
//! with optional working directory / project context.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::warn;
use uuid::Uuid;

use crate::state::SharedState;

// ── In-memory project store ─────────────────────────────────────────────────

/// A registered project with a name, path, and optional description.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

// We store projects in a thread-safe global map.  A future iteration may
// persist them to SQLite via SessionStore, but for the initial Phase 1.3
// a `DashMap` keeps the implementation simple.
use dashmap::DashMap;
use std::sync::LazyLock;

static PROJECTS: LazyLock<DashMap<String, Project>> = LazyLock::new(DashMap::new);

// ── Request / Response types ────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub path: String,
    pub description: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateProjectRequest {
    pub name: Option<String>,
    pub path: Option<String>,
    pub description: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateSessionWithProjectRequest {
    /// Optional named agent for this session.
    pub agent_id: Option<String>,
    /// Optional working directory to set on the session.
    pub working_dir: Option<String>,
    /// Optional project name.
    pub project_name: Option<String>,
    /// Optional project ID (must reference an existing project).
    pub project_id: Option<String>,
}

// ── Handlers ────────────────────────────────────────────────────────────────

/// POST /api/projects — create a new project.
pub async fn create_project(
    Json(body): Json<CreateProjectRequest>,
) -> impl IntoResponse {
    let now = chrono::Utc::now().to_rfc3339();
    let project = Project {
        id: Uuid::new_v4().to_string(),
        name: body.name,
        path: body.path,
        description: body.description,
        created_at: now.clone(),
        updated_at: now,
    };
    let id = project.id.clone();
    PROJECTS.insert(id.clone(), project.clone());

    (StatusCode::CREATED, Json(serde_json::json!({ "project": project })))
}

/// GET /api/projects — list all projects.
pub async fn list_projects() -> impl IntoResponse {
    let projects: Vec<Project> = PROJECTS.iter().map(|e| e.value().clone()).collect();
    Json(serde_json::json!({ "projects": projects }))
}

/// GET /api/projects/{id} — get a single project.
pub async fn get_project(Path(id): Path<String>) -> impl IntoResponse {
    match PROJECTS.get(&id) {
        Some(entry) => (
            StatusCode::OK,
            Json(serde_json::json!({ "project": entry.value().clone() })),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "project not found" })),
        ),
    }
}

/// PUT /api/projects/{id} — update a project.
pub async fn update_project(
    Path(id): Path<String>,
    Json(body): Json<UpdateProjectRequest>,
) -> impl IntoResponse {
    let Some(mut entry) = PROJECTS.get_mut(&id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "project not found" })),
        );
    };
    let project = entry.value_mut();
    if let Some(name) = body.name {
        project.name = name;
    }
    if let Some(path) = body.path {
        project.path = path;
    }
    if let Some(desc) = body.description {
        project.description = Some(desc);
    }
    project.updated_at = chrono::Utc::now().to_rfc3339();
    let updated = project.clone();
    drop(entry);

    (StatusCode::OK, Json(serde_json::json!({ "project": updated })))
}

/// DELETE /api/projects/{id} — delete a project.
pub async fn delete_project(Path(id): Path<String>) -> impl IntoResponse {
    match PROJECTS.remove(&id) {
        Some(_) => (
            StatusCode::OK,
            Json(serde_json::json!({ "ok": true })),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "project not found" })),
        ),
    }
}

/// GET /api/projects/{id}/files — list files in the project directory,
/// respecting `.garraignore` if present.
pub async fn list_project_files(Path(id): Path<String>) -> impl IntoResponse {
    let Some(entry) = PROJECTS.get(&id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "project not found" })),
        );
    };
    let project_path = PathBuf::from(&entry.path);
    drop(entry);

    if !project_path.is_dir() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "project path is not a directory" })),
        );
    }

    // Attempt to use garraia_glob scanner if available; otherwise fall back to
    // a simple readdir.
    let files = match collect_project_files(&project_path).await {
        Ok(f) => f,
        Err(e) => {
            warn!("failed to list project files: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("failed to list files: {e}") })),
            );
        }
    };

    (StatusCode::OK, Json(serde_json::json!({ "files": files })))
}

/// Collect files in `dir`, respecting `.garraignore` at the project root.
async fn collect_project_files(dir: &std::path::Path) -> std::result::Result<Vec<String>, String> {
    let dir = dir.to_path_buf();
    // Do the blocking I/O on a dedicated thread.
    tokio::task::spawn_blocking(move || {
        let ignore_path = dir.join(".garraignore");
        let mut ignored: Vec<String> = Vec::new();

        // Parse .garraignore patterns (simple line-based glob matching).
        if ignore_path.is_file() {
            if let Ok(contents) = std::fs::read_to_string(&ignore_path) {
                for line in contents.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with('#') {
                        continue;
                    }
                    ignored.push(trimmed.to_string());
                }
            }
        }

        let mut files = Vec::new();
        collect_dir_recursive(&dir, &dir, &ignored, &mut files)?;
        Ok(files)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

fn collect_dir_recursive(
    base: &std::path::Path,
    current: &std::path::Path,
    ignored_patterns: &[String],
    out: &mut Vec<String>,
) -> std::result::Result<(), String> {
    let entries =
        std::fs::read_dir(current).map_err(|e| format!("read_dir {}: {e}", current.display()))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
        let path = entry.path();
        let rel = path
            .strip_prefix(base)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

        // Simple pattern matching against .garraignore entries.
        let should_ignore = ignored_patterns.iter().any(|pat| {
            if pat.starts_with('!') {
                return false; // negation — skip
            }
            simple_glob_match(pat, &rel)
        });

        if should_ignore {
            continue;
        }

        if path.is_dir() {
            collect_dir_recursive(base, &path, ignored_patterns, out)?;
        } else {
            out.push(rel);
        }
    }
    Ok(())
}

/// Minimal glob match supporting `*` and `**` — good enough for ignore files.
fn simple_glob_match(pattern: &str, path: &str) -> bool {
    let pat = pattern.trim_start_matches('/');
    // If pattern has no slash, match against the filename only.
    if !pat.contains('/') {
        let filename = path.rsplit('/').next().unwrap_or(path);
        return wildcard_match(pat, filename);
    }
    wildcard_match(pat, path)
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    wildcard_match_inner(pattern, text)
}

fn wildcard_match_inner(pattern: &str, text: &str) -> bool {
    if pattern.is_empty() {
        return text.is_empty();
    }
    let mut p_chars = pattern.chars();
    let pc = p_chars.next().unwrap(); // safe — pattern is non-empty
    let rest_pattern = p_chars.as_str();

    match pc {
        '*' => {
            // "**" matches everything including path separators.
            if rest_pattern.starts_with('*') {
                let after_stars = rest_pattern.trim_start_matches('*').trim_start_matches('/');
                // Try matching after_stars at every position.
                for i in 0..=text.len() {
                    if wildcard_match_inner(after_stars, &text[i..]) {
                        return true;
                    }
                }
                return false;
            }
            // Single '*' matches everything except '/'.
            for i in 0..=text.len() {
                if i > 0 && text.as_bytes()[i - 1] == b'/' {
                    break;
                }
                if wildcard_match_inner(rest_pattern, &text[i..]) {
                    return true;
                }
            }
            false
        }
        '?' => {
            if text.is_empty() || text.starts_with('/') {
                false
            } else {
                let next_char_len = text.chars().next().map_or(0, |c| c.len_utf8());
                wildcard_match_inner(rest_pattern, &text[next_char_len..])
            }
        }
        c => {
            if text.is_empty() {
                false
            } else {
                let tc = text.chars().next().unwrap();
                if c == tc {
                    let next_char_len = tc.len_utf8();
                    wildcard_match_inner(rest_pattern, &text[next_char_len..])
                } else {
                    false
                }
            }
        }
    }
}

/// POST /api/sessions (enhanced) — create a session with optional project context.
///
/// This handler augments the base session creation with working_dir / project_name / project_id.
pub async fn create_session_with_project(
    State(state): State<SharedState>,
    Json(body): Json<CreateSessionWithProjectRequest>,
) -> impl IntoResponse {
    let session_id = state.create_session();

    // Apply project context.
    if let Some(mut session) = state.sessions.get_mut(&session_id) {
        if let Some(ref agent_id) = body.agent_id {
            session.channel_id = Some(format!("api:{agent_id}"));
        }

        // If project_id is provided, look up the project and populate fields from it.
        if let Some(ref pid) = body.project_id {
            if let Some(project) = PROJECTS.get(pid) {
                session.project_id = Some(pid.clone());
                session.project_name = Some(project.name.clone());
                session.working_dir = Some(project.path.clone());
            }
        }

        // Explicit overrides take precedence over project lookups.
        if let Some(ref wd) = body.working_dir {
            session.working_dir = Some(wd.clone());
        }
        if let Some(ref pn) = body.project_name {
            session.project_name = Some(pn.clone());
        }
    }

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "session_id": session_id,
            "agent_id": body.agent_id,
            "working_dir": body.working_dir,
            "project_name": body.project_name,
            "project_id": body.project_id,
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_glob_matching() {
        assert!(simple_glob_match("*.rs", "main.rs"));
        assert!(simple_glob_match("*.rs", "src/lib.rs"));
        assert!(!simple_glob_match("*.rs", "main.toml"));
        assert!(simple_glob_match("target/**", "target/debug/build"));
        assert!(simple_glob_match(".git", ".git"));
    }

    #[test]
    fn wildcard_match_basic() {
        assert!(wildcard_match_inner("hello", "hello"));
        assert!(!wildcard_match_inner("hello", "world"));
        assert!(wildcard_match_inner("h*o", "hello"));
        assert!(wildcard_match_inner("*", "anything"));
        assert!(wildcard_match_inner("*.rs", "main.rs"));
    }
}
