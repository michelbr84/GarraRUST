//! Skills Editor API handlers (Phase 3.3).
//!
//! Provides CRUD endpoints for managing skills (SKILL.md files):
//! - `GET /api/skills` — list all skills
//! - `GET /api/skills/{name}` — get skill content
//! - `POST /api/skills` — create a new skill
//! - `PUT /api/skills/{name}` — update skill content
//! - `DELETE /api/skills/{name}` — delete skill
//! - `POST /api/skills/import` — import skill from JSON/URL
//! - `GET /api/skills/{name}/export` — export skill as JSON
//! - `POST /api/skills/{name}/triggers` — set auto-triggers

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::path_validation::{NameError, validate_skill_name};
use crate::state::SharedState;

/// Build a 400 response for a rejected skill basename.
///
/// Centralizes the audit log + response shape used by every handler in this
/// module. Keeps handler bodies focused on their own happy path while
/// preserving the existing `{status, message}` envelope.
fn bad_skill_name(name: &str, err: NameError) -> (StatusCode, Json<serde_json::Value>) {
    // Log the *kind* of rejection but never the raw byte sequence — control
    // chars in the name could corrupt downstream log consumers.
    warn!(reason = ?err, name_len = name.len(), "rejected skill name");
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({
            "status": "error",
            "message": err.to_string(),
        })),
    )
}

// ── Types ───────────────────────────────────────────────────────────────────

/// Skill info returned by the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub triggers: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
}

/// Request to create or update a skill.
#[derive(Debug, Deserialize)]
pub struct CreateSkillRequest {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub triggers: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    pub body: String,
}

/// Request to import a skill.
#[derive(Debug, Deserialize)]
pub struct ImportSkillRequest {
    /// Import from a URL.
    #[serde(default)]
    pub url: Option<String>,
    /// Import from inline JSON content.
    #[serde(default)]
    pub content: Option<String>,
}

/// Request to set triggers for a skill.
#[derive(Debug, Deserialize)]
pub struct SetTriggersRequest {
    pub triggers: Vec<TriggerConfig>,
}

/// A trigger configuration for auto-activating a skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerConfig {
    /// Event type: "project_open", "command", "schedule", etc.
    pub event: String,
    /// Pattern or condition for the trigger.
    pub pattern: String,
    #[serde(default)]
    pub enabled: bool,
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Get the skills directory path.
fn skills_dir() -> std::path::PathBuf {
    garraia_config::ConfigLoader::default_config_dir().join("skills")
}

/// Build a SKILL.md content string from components.
fn build_skill_content(
    name: &str,
    description: &str,
    triggers: &[String],
    dependencies: &[String],
    body: &str,
) -> String {
    let mut yaml = format!("name: {name}\ndescription: {description}\n");
    if !triggers.is_empty() {
        yaml.push_str("triggers:\n");
        for t in triggers {
            yaml.push_str(&format!("  - {t}\n"));
        }
    }
    if !dependencies.is_empty() {
        yaml.push_str("dependencies:\n");
        for d in dependencies {
            yaml.push_str(&format!("  - {d}\n"));
        }
    }
    format!("---\n{yaml}---\n\n{body}\n")
}

// ── Handlers ────────────────────────────────────────────────────────────────

/// GET /api/skills — list all skills from the skills directory.
pub async fn list_skills(State(_state): State<SharedState>) -> Json<serde_json::Value> {
    let scanner = garraia_skills::SkillScanner::new(skills_dir());

    let skills: Vec<SkillInfo> = match scanner.discover() {
        Ok(discovered) => discovered
            .into_iter()
            .map(|s| SkillInfo {
                name: s.frontmatter.name,
                description: s.frontmatter.description,
                triggers: s.frontmatter.triggers,
                dependencies: s.frontmatter.dependencies,
                body: s.body,
                source_path: s.source_path.map(|p| p.display().to_string()),
            })
            .collect(),
        Err(e) => {
            warn!(error = %e, "failed to discover skills");
            Vec::new()
        }
    };

    Json(serde_json::json!({
        "skills": skills,
        "total": skills.len(),
    }))
}

/// GET /api/skills/{name} — get skill content.
pub async fn get_skill(
    State(_state): State<SharedState>,
    Path(name): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_skill_name(&name) {
        return bad_skill_name(&name, e);
    }
    let skill_path = skills_dir().join(format!("{name}.md"));

    if !skill_path.exists() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("skill '{name}' not found"),
            })),
        );
    }

    let content = match std::fs::read_to_string(&skill_path) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "status": "error",
                    "message": format!("failed to read skill: {e}"),
                })),
            );
        }
    };

    match garraia_skills::parse_skill(&content) {
        Ok(skill) => {
            let info = SkillInfo {
                name: skill.frontmatter.name,
                description: skill.frontmatter.description,
                triggers: skill.frontmatter.triggers,
                dependencies: skill.frontmatter.dependencies,
                body: skill.body,
                source_path: Some(skill_path.display().to_string()),
            };
            (
                StatusCode::OK,
                Json(serde_json::to_value(info).unwrap_or_default()),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("failed to parse skill: {e}"),
            })),
        ),
    }
}

/// POST /api/skills — create a new skill.
pub async fn create_skill(
    State(_state): State<SharedState>,
    Json(body): Json<CreateSkillRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_skill_name(&body.name) {
        return bad_skill_name(&body.name, e);
    }

    let dir = skills_dir();
    let skill_path = dir.join(format!("{}.md", body.name));

    if skill_path.exists() {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("skill '{}' already exists", body.name),
            })),
        );
    }

    let content = build_skill_content(
        &body.name,
        &body.description,
        &body.triggers,
        &body.dependencies,
        &body.body,
    );

    // Validate before writing
    if let Err(e) = garraia_skills::parse_skill(&content).and_then(|s| {
        garraia_skills::validate_skill(&s)?;
        Ok(s)
    }) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("invalid skill: {e}"),
            })),
        );
    }

    if let Err(e) = std::fs::create_dir_all(&dir) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("failed to create skills directory: {e}"),
            })),
        );
    }

    if let Err(e) = std::fs::write(&skill_path, &content) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("failed to write skill file: {e}"),
            })),
        );
    }

    info!(skill = %body.name, "created skill");

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "status": "ok",
            "message": format!("skill '{}' created", body.name),
            "skill": {
                "name": body.name,
                "description": body.description,
            },
        })),
    )
}

/// PUT /api/skills/{name} — update skill content.
pub async fn update_skill(
    State(_state): State<SharedState>,
    Path(name): Path<String>,
    Json(body): Json<CreateSkillRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_skill_name(&name) {
        return bad_skill_name(&name, e);
    }
    // The body.name is unused for the path here (the URL `name` is the
    // canonical identifier), but a malicious body could still smuggle
    // a bad name into `build_skill_content`'s YAML frontmatter. Reject
    // it for the same reason.
    if let Err(e) = validate_skill_name(&body.name) {
        return bad_skill_name(&body.name, e);
    }
    let skill_path = skills_dir().join(format!("{name}.md"));

    if !skill_path.exists() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("skill '{name}' not found"),
            })),
        );
    }

    let content = build_skill_content(
        &body.name,
        &body.description,
        &body.triggers,
        &body.dependencies,
        &body.body,
    );

    // Validate before writing
    if let Err(e) = garraia_skills::parse_skill(&content).and_then(|s| {
        garraia_skills::validate_skill(&s)?;
        Ok(s)
    }) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("invalid skill: {e}"),
            })),
        );
    }

    if let Err(e) = std::fs::write(&skill_path, &content) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("failed to update skill file: {e}"),
            })),
        );
    }

    info!(skill = %name, "updated skill");

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "message": format!("skill '{name}' updated"),
        })),
    )
}

/// DELETE /api/skills/{name} — delete a skill.
pub async fn delete_skill(
    State(_state): State<SharedState>,
    Path(name): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_skill_name(&name) {
        return bad_skill_name(&name, e);
    }
    let installer = garraia_skills::SkillInstaller::new(skills_dir());

    match installer.remove(&name) {
        Ok(true) => {
            info!(skill = %name, "deleted skill");
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "ok",
                    "message": format!("skill '{name}' deleted"),
                })),
            )
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("skill '{name}' not found"),
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("failed to delete skill: {e}"),
            })),
        ),
    }
}

/// POST /api/skills/import — import skill from JSON content or URL.
pub async fn import_skill(
    State(_state): State<SharedState>,
    Json(body): Json<ImportSkillRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let installer = garraia_skills::SkillInstaller::new(skills_dir());

    match (&body.url, &body.content) {
        (Some(url), _) => match installer.install_from_url(url).await {
            Ok(skill) => {
                info!(skill = %skill.frontmatter.name, url = %url, "imported skill from URL");
                (
                    StatusCode::CREATED,
                    Json(serde_json::json!({
                        "status": "ok",
                        "message": format!("skill '{}' imported from URL", skill.frontmatter.name),
                        "skill": {
                            "name": skill.frontmatter.name,
                            "description": skill.frontmatter.description,
                        },
                    })),
                )
            }
            Err(e) => (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "status": "error",
                    "message": format!("failed to import skill: {e}"),
                })),
            ),
        },
        (_, Some(content)) => {
            // Parse the content directly and write it
            match garraia_skills::parse_skill(content) {
                Ok(skill) => {
                    if let Err(e) = garraia_skills::validate_skill(&skill) {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(serde_json::json!({
                                "status": "error",
                                "message": format!("invalid skill: {e}"),
                            })),
                        );
                    }

                    let dir = skills_dir();
                    if let Err(e) = std::fs::create_dir_all(&dir) {
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(serde_json::json!({
                                "status": "error",
                                "message": format!("failed to create skills directory: {e}"),
                            })),
                        );
                    }

                    let path = dir.join(format!("{}.md", skill.frontmatter.name));
                    if let Err(e) = std::fs::write(&path, content) {
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(serde_json::json!({
                                "status": "error",
                                "message": format!("failed to write skill: {e}"),
                            })),
                        );
                    }

                    info!(skill = %skill.frontmatter.name, "imported skill from content");
                    (
                        StatusCode::CREATED,
                        Json(serde_json::json!({
                            "status": "ok",
                            "message": format!("skill '{}' imported", skill.frontmatter.name),
                            "skill": {
                                "name": skill.frontmatter.name,
                                "description": skill.frontmatter.description,
                            },
                        })),
                    )
                }
                Err(e) => (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "status": "error",
                        "message": format!("failed to parse skill content: {e}"),
                    })),
                ),
            }
        }
        _ => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "message": "either 'url' or 'content' must be provided",
            })),
        ),
    }
}

/// GET /api/skills/{name}/export — export skill as JSON.
pub async fn export_skill(
    State(_state): State<SharedState>,
    Path(name): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_skill_name(&name) {
        return bad_skill_name(&name, e);
    }
    let skill_path = skills_dir().join(format!("{name}.md"));

    if !skill_path.exists() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("skill '{name}' not found"),
            })),
        );
    }

    let content = match std::fs::read_to_string(&skill_path) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "status": "error",
                    "message": format!("failed to read skill: {e}"),
                })),
            );
        }
    };

    match garraia_skills::parse_skill(&content) {
        Ok(skill) => {
            let export = serde_json::json!({
                "name": skill.frontmatter.name,
                "description": skill.frontmatter.description,
                "triggers": skill.frontmatter.triggers,
                "dependencies": skill.frontmatter.dependencies,
                "body": skill.body,
                "raw_content": content,
            });
            (StatusCode::OK, Json(export))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("failed to parse skill for export: {e}"),
            })),
        ),
    }
}

/// POST /api/skills/{name}/triggers — set auto-triggers for a skill.
pub async fn set_skill_triggers(
    State(_state): State<SharedState>,
    Path(name): Path<String>,
    Json(body): Json<SetTriggersRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_skill_name(&name) {
        return bad_skill_name(&name, e);
    }
    let skill_path = skills_dir().join(format!("{name}.md"));

    if !skill_path.exists() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("skill '{name}' not found"),
            })),
        );
    }

    // Read the existing skill
    let content = match std::fs::read_to_string(&skill_path) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "status": "error",
                    "message": format!("failed to read skill: {e}"),
                })),
            );
        }
    };

    let skill = match garraia_skills::parse_skill(&content) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "status": "error",
                    "message": format!("failed to parse skill: {e}"),
                })),
            );
        }
    };

    // Update triggers in the skill file
    let triggers: Vec<String> = body
        .triggers
        .iter()
        .filter(|t| t.enabled)
        .map(|t| format!("{}:{}", t.event, t.pattern))
        .collect();

    let updated_content = build_skill_content(
        &skill.frontmatter.name,
        &skill.frontmatter.description,
        &triggers,
        &skill.frontmatter.dependencies,
        &skill.body,
    );

    if let Err(e) = std::fs::write(&skill_path, &updated_content) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("failed to update skill triggers: {e}"),
            })),
        );
    }

    info!(skill = %name, trigger_count = body.triggers.len(), "updated skill triggers");

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "message": format!("triggers updated for skill '{name}'"),
            "triggers": body.triggers,
        })),
    )
}
