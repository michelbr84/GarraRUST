//! Admin MCP template management handlers.
//!
//! Slice 9.d of GAR-472 / Q9 of EPIC GAR-430 (Quality Gates Phase 3.6).
//! Extracted from `admin/handlers.rs` (lines 2309-2537) without behavior change.
//! Covers MCP template listing (built-in + user-saved), saving, and deletion.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;

use super::middleware::AuthenticatedAdmin;
use super::rbac::{Action, Resource, check_permission};
use super::shared::AdminState;

// ── MCP Templates (GAR-296 / GAR-297) ───────────────────────────────

/// A built-in or user-saved MCP template.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct McpTemplate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub transport: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub url: Option<String>,
    /// Env keys with placeholder values — never contains real tokens.
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    pub timeout_secs: u64,
    /// true = shipped with GarraIA; false = user-created
    #[serde(default)]
    pub builtin: bool,
}

/// Returns the 5 built-in MCP templates (GAR-296).
fn builtin_templates() -> Vec<McpTemplate> {
    vec![
        McpTemplate {
            id: "filesystem".into(),
            name: "Filesystem MCP".into(),
            description: "Acesso a arquivos locais: ler, escrever e listar diretórios. \
                          Substitua o último argumento pelo caminho que deseja permitir \
                          (ex: /home/user ou C:\\Users\\user)."
                .into(),
            transport: "stdio".into(),
            command: Some("npx".into()),
            args: vec![
                "-y".into(),
                "@modelcontextprotocol/server-filesystem".into(),
                // Default to the user's home directory; customise before saving.
                std::env::var("HOME")
                    .or_else(|_| std::env::var("USERPROFILE"))
                    .unwrap_or_else(|_| "/".to_string()),
            ],
            url: None,
            env: Default::default(),
            timeout_secs: 30,
            builtin: true,
        },
        McpTemplate {
            id: "github".into(),
            name: "GitHub MCP".into(),
            description: "Integração com GitHub API: repos, issues, PRs e commits.".into(),
            transport: "stdio".into(),
            command: Some("npx".into()),
            args: vec!["-y".into(), "@modelcontextprotocol/server-github".into()],
            url: None,
            env: {
                let mut m = std::collections::HashMap::new();
                m.insert(
                    "GITHUB_PERSONAL_ACCESS_TOKEN".into(),
                    "<your-github-token>".into(),
                );
                m
            },
            timeout_secs: 30,
            builtin: true,
        },
        McpTemplate {
            id: "linear".into(),
            name: "Linear MCP".into(),
            description: "Gerenciamento de issues no Linear.app via MCP.".into(),
            transport: "stdio".into(),
            command: Some("npx".into()),
            args: vec!["-y".into(), "@linear/mcp-server".into()],
            url: None,
            env: {
                let mut m = std::collections::HashMap::new();
                m.insert("LINEAR_API_KEY".into(), "<your-linear-api-key>".into());
                m
            },
            timeout_secs: 30,
            builtin: true,
        },
        McpTemplate {
            id: "lmstudio".into(),
            name: "LM Studio MCP".into(),
            description: "Conecta ao LM Studio rodando localmente (modelos locais via HTTP)."
                .into(),
            transport: "http".into(),
            command: None,
            args: vec![],
            url: Some("http://localhost:1234/v1".into()),
            env: Default::default(),
            timeout_secs: 60,
            builtin: true,
        },
        McpTemplate {
            id: "n8n".into(),
            name: "n8n MCP".into(),
            description: "Automação de workflows via n8n. Requer n8n com MCP trigger ativo.".into(),
            transport: "streamableHttp".into(),
            command: None,
            args: vec![],
            url: Some("http://localhost:5678/mcp".into()),
            env: {
                let mut m = std::collections::HashMap::new();
                m.insert("N8N_API_KEY".into(), "<your-n8n-api-key>".into());
                m
            },
            timeout_secs: 60,
            builtin: true,
        },
    ]
}

/// Path to the user-saved templates file: `~/.garraia/mcp-templates.json`.
fn user_templates_path() -> std::path::PathBuf {
    garraia_config::ConfigLoader::default_config_dir().join("mcp-templates.json")
}

fn load_user_templates() -> Vec<McpTemplate> {
    let path = user_templates_path();
    let Ok(content) = std::fs::read_to_string(&path) else {
        return vec![];
    };
    serde_json::from_str(&content).unwrap_or_default()
}

fn save_user_templates(templates: &[McpTemplate]) -> std::io::Result<()> {
    let path = user_templates_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(templates).unwrap_or_default();
    std::fs::write(path, json)
}

/// GET /admin/api/mcp/templates — list built-in + user MCP templates (GAR-296/297).
pub async fn list_mcp_templates(
    State(_state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::McpServers, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }
    let mut templates = builtin_templates();
    templates.extend(load_user_templates());
    (
        StatusCode::OK,
        Json(serde_json::json!({ "templates": templates })),
    )
}

/// POST /admin/api/mcp/templates — save a user MCP template (GAR-297).
pub async fn save_mcp_template(
    State(_state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    Json(mut body): Json<McpTemplate>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::McpServers, Action::Create) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }
    if body.id.trim().is_empty() || body.name.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "id and name are required"})),
        );
    }
    // Never persist plaintext tokens — strip non-placeholder env values.
    for val in body.env.values_mut() {
        if !val.starts_with('<') {
            *val = "<redacted>".into();
        }
    }
    body.builtin = false;

    let mut templates = load_user_templates();
    // Replace if same id, else append.
    if let Some(pos) = templates.iter().position(|t| t.id == body.id) {
        templates[pos] = body.clone();
    } else {
        templates.push(body.clone());
    }
    if let Err(e) = save_user_templates(&templates) {
        tracing::warn!("failed to save user templates: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "failed to persist template"})),
        );
    }
    (
        StatusCode::CREATED,
        Json(serde_json::json!({ "template": body })),
    )
}

/// DELETE /admin/api/mcp/templates/{id} — remove a user MCP template (GAR-297).
pub async fn delete_mcp_template(
    State(_state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::McpServers, Action::Delete) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }
    let mut templates = load_user_templates();
    let before = templates.len();
    templates.retain(|t| t.id != id);
    if templates.len() == before {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "template not found"})),
        );
    }
    if let Err(e) = save_user_templates(&templates) {
        tracing::warn!("failed to save user templates after delete: {e}");
    }
    (StatusCode::OK, Json(serde_json::json!({"deleted": id})))
}
