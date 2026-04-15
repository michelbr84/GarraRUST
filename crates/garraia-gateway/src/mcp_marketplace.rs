//! MCP Marketplace API handlers (Phase 3.2).
//!
//! Provides a catalog of popular MCP servers with one-click install:
//! - `GET /api/mcp/marketplace` — catalog of popular MCP servers
//! - `POST /api/mcp/marketplace/install` — one-click install
//! - `GET /api/mcp/{id}/health` — health check for a specific MCP server
//! - `GET /api/mcp/{id}/config-schema` — JSON Schema for config form

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::state::SharedState;

// ── Catalog types ───────────────────────────────────────────────────────────

/// Category for marketplace MCP servers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpCategory {
    Filesystem,
    Developer,
    Database,
    Communication,
    Search,
    Automation,
    Productivity,
}

/// A marketplace catalog entry for an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpCatalogEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub install_command: String,
    pub install_args: Vec<String>,
    pub config_schema: serde_json::Value,
    pub category: McpCategory,
    pub popularity: u32,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub env_vars: Vec<McpEnvVarSpec>,
}

/// Environment variable specification for MCP server config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpEnvVarSpec {
    pub name: String,
    pub description: String,
    pub required: bool,
    #[serde(default)]
    pub sensitive: bool,
}

// ── Built-in catalog ────────────────────────────────────────────────────────

fn built_in_catalog() -> Vec<McpCatalogEntry> {
    vec![
        McpCatalogEntry {
            id: "filesystem".into(),
            name: "Filesystem".into(),
            description: "Read, write, and manage files on the local filesystem".into(),
            install_command: "npx".into(),
            install_args: vec![
                "-y".into(),
                "@modelcontextprotocol/server-filesystem".into(),
            ],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "allowed_directories": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Directories the server can access"
                    }
                }
            }),
            category: McpCategory::Filesystem,
            popularity: 95,
            homepage: Some("https://github.com/modelcontextprotocol/servers".into()),
            env_vars: vec![],
        },
        McpCatalogEntry {
            id: "github".into(),
            name: "GitHub".into(),
            description: "Interact with GitHub repositories, issues, pull requests, and more"
                .into(),
            install_command: "npx".into(),
            install_args: vec!["-y".into(), "@modelcontextprotocol/server-github".into()],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "owner": { "type": "string", "description": "GitHub owner/org" },
                    "repo": { "type": "string", "description": "Repository name" }
                }
            }),
            category: McpCategory::Developer,
            popularity: 90,
            homepage: Some("https://github.com/modelcontextprotocol/servers".into()),
            env_vars: vec![McpEnvVarSpec {
                name: "GITHUB_PERSONAL_ACCESS_TOKEN".into(),
                description: "GitHub Personal Access Token for API access".into(),
                required: true,
                sensitive: true,
            }],
        },
        McpCatalogEntry {
            id: "postgres".into(),
            name: "PostgreSQL".into(),
            description: "Query and manage PostgreSQL databases".into(),
            install_command: "npx".into(),
            install_args: vec!["-y".into(), "@modelcontextprotocol/server-postgres".into()],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "connection_string": {
                        "type": "string",
                        "description": "PostgreSQL connection string"
                    }
                },
                "required": ["connection_string"]
            }),
            category: McpCategory::Database,
            popularity: 85,
            homepage: Some("https://github.com/modelcontextprotocol/servers".into()),
            env_vars: vec![McpEnvVarSpec {
                name: "POSTGRES_CONNECTION_STRING".into(),
                description: "PostgreSQL connection URI".into(),
                required: true,
                sensitive: true,
            }],
        },
        McpCatalogEntry {
            id: "slack".into(),
            name: "Slack".into(),
            description: "Send messages, manage channels, and interact with Slack workspaces"
                .into(),
            install_command: "npx".into(),
            install_args: vec!["-y".into(), "@modelcontextprotocol/server-slack".into()],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "workspace": { "type": "string", "description": "Slack workspace name" }
                }
            }),
            category: McpCategory::Communication,
            popularity: 80,
            homepage: Some("https://github.com/modelcontextprotocol/servers".into()),
            env_vars: vec![McpEnvVarSpec {
                name: "SLACK_BOT_TOKEN".into(),
                description: "Slack Bot User OAuth Token".into(),
                required: true,
                sensitive: true,
            }],
        },
        McpCatalogEntry {
            id: "notion".into(),
            name: "Notion".into(),
            description: "Read and write Notion pages, databases, and blocks".into(),
            install_command: "npx".into(),
            install_args: vec!["-y".into(), "@notionhq/notion-mcp-server".into()],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            category: McpCategory::Productivity,
            popularity: 78,
            homepage: Some("https://github.com/makenotion/notion-mcp-server".into()),
            env_vars: vec![McpEnvVarSpec {
                name: "NOTION_API_KEY".into(),
                description: "Notion Integration API Key".into(),
                required: true,
                sensitive: true,
            }],
        },
        McpCatalogEntry {
            id: "brave-search".into(),
            name: "Brave Search".into(),
            description: "Web search and local search using Brave Search API".into(),
            install_command: "npx".into(),
            install_args: vec![
                "-y".into(),
                "@modelcontextprotocol/server-brave-search".into(),
            ],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            category: McpCategory::Search,
            popularity: 75,
            homepage: Some("https://github.com/modelcontextprotocol/servers".into()),
            env_vars: vec![McpEnvVarSpec {
                name: "BRAVE_API_KEY".into(),
                description: "Brave Search API key".into(),
                required: true,
                sensitive: true,
            }],
        },
        McpCatalogEntry {
            id: "puppeteer".into(),
            name: "Puppeteer".into(),
            description: "Browser automation — navigate, screenshot, and interact with web pages"
                .into(),
            install_command: "npx".into(),
            install_args: vec!["-y".into(), "@modelcontextprotocol/server-puppeteer".into()],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "headless": {
                        "type": "boolean",
                        "description": "Run browser in headless mode",
                        "default": true
                    }
                }
            }),
            category: McpCategory::Automation,
            popularity: 70,
            homepage: Some("https://github.com/modelcontextprotocol/servers".into()),
            env_vars: vec![],
        },
        McpCatalogEntry {
            id: "sqlite".into(),
            name: "SQLite".into(),
            description: "Query and manage SQLite databases".into(),
            install_command: "npx".into(),
            install_args: vec!["-y".into(), "@modelcontextprotocol/server-sqlite".into()],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "db_path": {
                        "type": "string",
                        "description": "Path to SQLite database file"
                    }
                },
                "required": ["db_path"]
            }),
            category: McpCategory::Database,
            popularity: 72,
            homepage: Some("https://github.com/modelcontextprotocol/servers".into()),
            env_vars: vec![],
        },
    ]
}

// ── Request types ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct InstallMcpRequest {
    /// ID of the catalog entry to install.
    pub id: String,
    /// Environment variables to configure.
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    /// Extra arguments to append.
    #[serde(default)]
    pub extra_args: Vec<String>,
}

// ── Handlers ────────────────────────────────────────────────────────────────

/// GET /api/mcp/marketplace — catalog of popular MCP servers with metadata.
pub async fn marketplace_catalog(State(_state): State<SharedState>) -> Json<serde_json::Value> {
    let catalog = built_in_catalog();

    Json(serde_json::json!({
        "catalog": catalog,
        "total": catalog.len(),
    }))
}

/// POST /api/mcp/marketplace/install — one-click install of an MCP server.
pub async fn marketplace_install(
    State(state): State<SharedState>,
    Json(body): Json<InstallMcpRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let catalog = built_in_catalog();
    let entry = match catalog.iter().find(|e| e.id == body.id) {
        Some(e) => e,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "status": "error",
                    "message": format!("MCP server '{}' not found in catalog", body.id),
                })),
            );
        }
    };

    // Check required env vars
    for var in &entry.env_vars {
        if var.required && !body.env.contains_key(&var.name) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "status": "error",
                    "message": format!("required environment variable '{}' not provided", var.name),
                })),
            );
        }
    }

    // Build the MCP server config
    let mut args = entry.install_args.clone();
    args.extend(body.extra_args.clone());

    let config = crate::mcp::McpServerConfig {
        command: Some(entry.install_command.clone()),
        args,
        env: body.env.clone(),
        url: None,
        transport: None,
        timeout_secs: 30,
        memory_limit_mb: None,
        max_restarts: Some(5),
        restart_delay_secs: Some(5),
    };

    // Register in the MCP runtime registry
    state.mcp_registry.add_server(&entry.id, config).await;

    info!(
        mcp_server = %entry.id,
        name = %entry.name,
        "MCP server installed from marketplace"
    );

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "status": "ok",
            "message": format!("MCP server '{}' installed", entry.name),
            "server": {
                "id": entry.id,
                "name": entry.name,
                "command": entry.install_command,
                "status": "stopped",
            },
        })),
    )
}

/// GET /api/mcp/{id}/health — health check for a specific MCP server.
pub async fn mcp_server_health(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    // Check the MCP runtime registry
    match state.mcp_registry.get(&id).await {
        Some(server) => {
            let status_str = match &server.status {
                crate::mcp::McpStatus::Running => "running",
                crate::mcp::McpStatus::Stopped => "stopped",
                crate::mcp::McpStatus::Error { .. } => "error",
            };

            let mut response = serde_json::json!({
                "id": id,
                "status": status_str,
                "tool_count": server.tool_count,
                "transport": serde_json::to_value(server.config.infer_transport()).unwrap_or_default(),
            });

            if let crate::mcp::McpStatus::Error { message } = &server.status {
                response["error"] = serde_json::Value::String(message.clone());
            }

            (StatusCode::OK, Json(response))
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("MCP server '{id}' not found"),
            })),
        ),
    }
}

/// GET /api/mcp/{id}/config-schema — returns JSON Schema for config form.
pub async fn mcp_config_schema(
    State(_state): State<SharedState>,
    Path(id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let catalog = built_in_catalog();
    match catalog.iter().find(|e| e.id == id) {
        Some(entry) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "id": entry.id,
                "name": entry.name,
                "config_schema": entry.config_schema,
                "env_vars": entry.env_vars,
            })),
        ),
        None => {
            // Return a generic schema for unknown servers
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "id": id,
                    "name": id,
                    "config_schema": {
                        "type": "object",
                        "properties": {
                            "command": { "type": "string", "description": "Command to run" },
                            "args": { "type": "array", "items": { "type": "string" } },
                            "env": { "type": "object", "additionalProperties": { "type": "string" } }
                        }
                    },
                    "env_vars": [],
                })),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_all_entries() {
        let catalog = built_in_catalog();
        assert_eq!(catalog.len(), 8);

        let ids: Vec<&str> = catalog.iter().map(|e| e.id.as_str()).collect();
        assert!(ids.contains(&"filesystem"));
        assert!(ids.contains(&"github"));
        assert!(ids.contains(&"postgres"));
        assert!(ids.contains(&"slack"));
        assert!(ids.contains(&"notion"));
        assert!(ids.contains(&"brave-search"));
        assert!(ids.contains(&"puppeteer"));
        assert!(ids.contains(&"sqlite"));
    }

    #[test]
    fn catalog_sorted_by_popularity() {
        let catalog = built_in_catalog();
        // The first entry should have the highest popularity
        assert_eq!(catalog[0].id, "filesystem");
        assert_eq!(catalog[0].popularity, 95);
    }

    #[test]
    fn github_requires_token() {
        let catalog = built_in_catalog();
        let github = catalog.iter().find(|e| e.id == "github").unwrap();
        assert!(!github.env_vars.is_empty());
        assert!(github.env_vars[0].required);
        assert!(github.env_vars[0].sensitive);
    }
}
