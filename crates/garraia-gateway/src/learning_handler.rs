//! Learning Agent Web UI handlers (plan 0156 / GAR-651).
//!
//! Exposes the garraia-learning registry and versioning modules through a
//! REST API consumed by the Garra Glass Web Console. Auth-free, secret-free
//! (same policy as /api/health, /api/diagnostics).
//!
//! URL namespace: /api/learning/skills/* and /api/learning/logs/*

use axum::{
    Json,
    extract::{Path, Query},
    http::StatusCode,
    response::{Html, IntoResponse},
};
use garraia_learning::updater::ProcessShellRunner;
use garraia_learning::{
    registry::{self, RegistryOptions},
    safety::SafetyIntent,
    versioning::{self, VersioningOptions},
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::warn;

use crate::path_validation::{NameError, validate_skill_name};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn default_registry_opts() -> RegistryOptions {
    let project_dir = registry::project_skills_dir();
    let global_dir =
        registry::global_skills_dir().unwrap_or_else(|_| PathBuf::from("/tmp/.garra/skills"));
    RegistryOptions::new(global_dir, project_dir)
}

fn bad_name(name: &str, err: NameError) -> (StatusCode, Json<serde_json::Value>) {
    warn!(reason = ?err, name_len = name.len(), "rejected learning skill name");
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": err.to_string() })),
    )
}

fn internal_err(msg: impl std::fmt::Display) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": msg.to_string() })),
    )
}

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SkillSummary {
    pub name: String,
    pub version: String,
    pub scope: String,
    pub score: f32,
    pub locked: bool,
    pub deprecated: bool,
    pub fail_count: u32,
    pub source: String,
}

#[derive(Debug, Serialize)]
pub struct SkillDetail {
    #[serde(flatten)]
    pub summary: SkillSummary,
    pub body: String,
    pub source_path: Option<String>,
    pub history: Vec<VersionSummary>,
}

#[derive(Debug, Serialize)]
pub struct VersionSummary {
    pub sha: String,
    pub short_sha: String,
    pub date: String,
    pub author: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct CandidateSummary {
    pub slug: String,
    pub occurrence_count: usize,
    pub normalized_commands: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ScoresQuery {
    pub since: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RollbackRequest {
    pub sha: String,
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RejectRequest {
    pub reason: Option<String>,
}

// ── Handlers: skills ──────────────────────────────────────────────────────────

/// GET /api/learning/skills — list all managed skills (global + project).
pub async fn list_learning_skills() -> impl IntoResponse {
    let opts = default_registry_opts();
    let skills = match registry::list_skills(&opts, None) {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "list_learning_skills failed");
            return (StatusCode::OK, Json(serde_json::json!({ "skills": [] }))).into_response();
        }
    };
    let summaries: Vec<SkillSummary> = skills
        .into_iter()
        .map(|s| SkillSummary {
            name: s.frontmatter.name,
            version: s.frontmatter.version,
            scope: format!("{:?}", s.frontmatter.scope).to_lowercase(),
            score: s.frontmatter.score,
            locked: s.frontmatter.locked,
            deprecated: s.frontmatter.deprecated,
            fail_count: s.frontmatter.fail_count,
            source: format!("{:?}", s.frontmatter.source).to_lowercase(),
        })
        .collect();
    Json(serde_json::json!({ "skills": summaries })).into_response()
}

/// GET /api/learning/skills/:name — skill detail including git history.
pub async fn get_learning_skill(Path(name): Path<String>) -> impl IntoResponse {
    if let Err(e) = validate_skill_name(&name) {
        return bad_name(&name, e).into_response();
    }
    let opts = default_registry_opts();
    let skill = match registry::get_skill(&opts, &name, None) {
        Ok(Some(s)) => s,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "skill not found" })),
            )
                .into_response();
        }
        Err(e) => return internal_err(e).into_response(),
    };

    let skills_dir = skill
        .source_path
        .as_ref()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| opts.project_dir.clone());
    let repo_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let vopts = VersioningOptions::new(skills_dir, repo_root);
    let runner = ProcessShellRunner;
    let history = versioning::history(&name, &vopts, &runner)
        .unwrap_or_default()
        .into_iter()
        .map(|v| VersionSummary {
            sha: v.sha,
            short_sha: v.short_sha,
            date: v.date,
            author: v.author,
            message: v.message,
        })
        .collect::<Vec<_>>();

    let detail = SkillDetail {
        summary: SkillSummary {
            name: skill.frontmatter.name,
            version: skill.frontmatter.version,
            scope: format!("{:?}", skill.frontmatter.scope).to_lowercase(),
            score: skill.frontmatter.score,
            locked: skill.frontmatter.locked,
            deprecated: skill.frontmatter.deprecated,
            fail_count: skill.frontmatter.fail_count,
            source: format!("{:?}", skill.frontmatter.source).to_lowercase(),
        },
        body: skill.body,
        source_path: skill.source_path.map(|p| p.display().to_string()),
        history,
    };
    Json(detail).into_response()
}

/// POST /api/learning/skills/:name/approve — promote skill through safety gate.
pub async fn approve_skill(Path(name): Path<String>) -> impl IntoResponse {
    if let Err(e) = validate_skill_name(&name) {
        return bad_name(&name, e).into_response();
    }
    let opts = default_registry_opts();
    let skill = match registry::get_skill(&opts, &name, None) {
        Ok(Some(s)) => s,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "skill not found" })),
            )
                .into_response();
        }
        Err(e) => return internal_err(e).into_response(),
    };
    match registry::promote_with_intent(&skill, &opts, &SafetyIntent::default()) {
        Ok(_) => Json(serde_json::json!({ "status": "approved" })).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /api/learning/skills/:name/reject — deprecate a skill.
pub async fn reject_skill(
    Path(name): Path<String>,
    body: Option<Json<RejectRequest>>,
) -> impl IntoResponse {
    if let Err(e) = validate_skill_name(&name) {
        return bad_name(&name, e).into_response();
    }
    let _reason = body.and_then(|b| b.0.reason);
    let opts = default_registry_opts();
    for scope in [
        garraia_learning::SkillScope::Project,
        garraia_learning::SkillScope::Global,
    ] {
        match registry::deprecate(&opts, &name, scope) {
            Ok(true) => return Json(serde_json::json!({ "status": "rejected" })).into_response(),
            Ok(false) => continue,
            Err(e) => return internal_err(e).into_response(),
        }
    }
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": "skill not found" })),
    )
        .into_response()
}

/// POST /api/learning/skills/:name/lock — set locked=true to prevent auto-update.
pub async fn lock_skill(Path(name): Path<String>) -> impl IntoResponse {
    if let Err(e) = validate_skill_name(&name) {
        return bad_name(&name, e).into_response();
    }
    let opts = default_registry_opts();
    match registry::set_locked(&opts, &name, true) {
        Ok(true) => Json(serde_json::json!({ "status": "locked" })).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "skill not found" })),
        )
            .into_response(),
        Err(e) => internal_err(e).into_response(),
    }
}

/// POST /api/learning/skills/:name/rollback — git revert to a specific SHA.
pub async fn rollback_skill(
    Path(name): Path<String>,
    Json(req): Json<RollbackRequest>,
) -> impl IntoResponse {
    if let Err(e) = validate_skill_name(&name) {
        return bad_name(&name, e).into_response();
    }
    let opts = default_registry_opts();
    let repo_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let vopts = VersioningOptions::new(opts.project_dir.clone(), repo_root);
    let runner = ProcessShellRunner;
    let reason = req.reason.as_deref().unwrap_or("web-console rollback");
    match versioning::rollback(&name, &req.sha, reason, &vopts, &runner) {
        Ok(()) => {
            Json(serde_json::json!({ "status": "rolled_back", "sha": req.sha })).into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// DELETE /api/learning/skills/:name — soft-delete (deprecate) a skill.
pub async fn delete_learning_skill(Path(name): Path<String>) -> impl IntoResponse {
    if let Err(e) = validate_skill_name(&name) {
        return bad_name(&name, e).into_response();
    }
    let opts = default_registry_opts();
    for scope in [
        garraia_learning::SkillScope::Project,
        garraia_learning::SkillScope::Global,
    ] {
        match registry::deprecate(&opts, &name, scope) {
            Ok(true) => return Json(serde_json::json!({ "status": "deleted" })).into_response(),
            Ok(false) => continue,
            Err(e) => return internal_err(e).into_response(),
        }
    }
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": "skill not found" })),
    )
        .into_response()
}

// ── Handlers: learning logs ───────────────────────────────────────────────────

/// GET /api/learning/logs/sessions — list observed session log files.
pub async fn get_log_sessions() -> impl IntoResponse {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let sessions_dir = PathBuf::from(home).join(".garra").join("sessions");
    if !sessions_dir.exists() {
        return Json(serde_json::json!({ "sessions": [] })).into_response();
    }
    let entries: Vec<String> = std::fs::read_dir(&sessions_dir)
        .map(|rd| {
            rd.filter_map(|e| {
                let e = e.ok()?;
                let name = e.file_name();
                let s = name.to_string_lossy();
                if s.ends_with(".json") {
                    Some(s.trim_end_matches(".json").to_string())
                } else {
                    None
                }
            })
            .collect()
        })
        .unwrap_or_default();
    Json(serde_json::json!({ "sessions": entries })).into_response()
}

/// GET /api/learning/logs/candidates — list pending candidate skills.
pub async fn get_log_candidates() -> impl IntoResponse {
    let opts = default_registry_opts();
    let candidates_dir = opts.project_dir.join("_candidates");
    let candidates = match registry::list_candidates(&candidates_dir) {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "list_candidates failed");
            return Json(serde_json::json!({ "candidates": [] })).into_response();
        }
    };
    let items: Vec<CandidateSummary> = candidates
        .into_iter()
        .map(|c| CandidateSummary {
            slug: c.slug,
            occurrence_count: c.occurrence_count,
            normalized_commands: c.normalized_commands,
        })
        .collect();
    Json(serde_json::json!({ "candidates": items })).into_response()
}

/// GET /api/learning/logs/scores?since=<iso> — score history from skill registry.
pub async fn get_log_scores(Query(q): Query<ScoresQuery>) -> impl IntoResponse {
    let opts = default_registry_opts();
    let repo_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let vopts = VersioningOptions::new(opts.project_dir.clone(), repo_root);
    let skills = registry::list_skills(&opts, None).unwrap_or_default();
    let since_filter = q.since;
    let mut all_scores: Vec<serde_json::Value> = Vec::new();
    for skill in &skills {
        let entries =
            versioning::score_history(&skill.frontmatter.name, &vopts).unwrap_or_default();
        for entry in entries {
            if since_filter
                .as_deref()
                .is_some_and(|s| entry.timestamp_utc.as_str() < s)
            {
                continue;
            }
            all_scores.push(serde_json::json!({
                "skill": skill.frontmatter.name,
                "sha": entry.sha,
                "timestamp_utc": entry.timestamp_utc,
                "score": entry.score,
            }));
        }
    }
    Json(serde_json::json!({ "scores": all_scores })).into_response()
}

// ── HTML page ─────────────────────────────────────────────────────────────────

/// GET /learning — Garra Glass Web Console: Skills + Learning Logs page.
pub async fn learning_ui() -> impl IntoResponse {
    if let Ok(content) = std::fs::read_to_string("crates/garraia-gateway/src/learning.html") {
        return Html(content).into_response();
    }
    Html(include_str!("learning.html").to_string()).into_response()
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_name_rejects_traversal() {
        let err = validate_skill_name("../evil");
        assert!(err.is_err());
    }

    #[test]
    fn bad_name_rejects_empty() {
        let err = validate_skill_name("");
        assert!(err.is_err());
    }

    #[test]
    fn bad_name_rejects_slash() {
        let err = validate_skill_name("a/b");
        assert!(err.is_err());
    }

    #[test]
    fn good_names_pass() {
        assert!(validate_skill_name("my-skill").is_ok());
        assert!(validate_skill_name("skill01").is_ok());
        assert!(validate_skill_name("camelCase").is_ok());
    }

    #[tokio::test]
    async fn list_returns_empty_when_no_dirs() {
        // With no .garra/skills directories present the handler must return 200 + empty array.
        let resp = list_learning_skills().await.into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(v["skills"].as_array().is_some());
    }

    #[tokio::test]
    async fn candidates_returns_empty_when_no_dir() {
        let resp = get_log_candidates().await.into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(v["candidates"].as_array().is_some());
    }

    #[tokio::test]
    async fn sessions_returns_empty_when_no_dir() {
        // HOME is set to a temp dir so sessions_dir won't exist.
        let tmp = std::env::temp_dir();
        // SAFETY: test-only, single-threaded context.
        unsafe { std::env::set_var("HOME", tmp.to_str().unwrap()) };
        let resp = get_log_sessions().await.into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(v["sessions"].as_array().is_some());
    }

    #[tokio::test]
    async fn get_skill_not_found() {
        let resp = get_learning_skill(Path("nonexistent-skill".to_string()))
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn reject_skill_not_found() {
        let resp = reject_skill(Path("nonexistent-skill".to_string()), None)
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn lock_skill_not_found() {
        let resp = lock_skill(Path("nonexistent-skill".to_string()))
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn bad_skill_name_returns_400() {
        let resp = get_learning_skill(Path("../traversal".to_string()))
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
