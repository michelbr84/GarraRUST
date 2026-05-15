//! Admin MCP server management handlers.
//!
//! Slice 9.c of GAR-471 / Q9 of EPIC GAR-430 (Quality Gates Phase 3.6).
//! Extracted from `admin/handlers.rs` (lines 2203-2557) without behavior change.
//! Covers MCP server listing, creation, restart, and deletion.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;

use super::middleware::AuthenticatedAdmin;
use super::rbac::{Action, Resource, check_permission};
use super::shared::AdminState;

// ── MCP server management ─────────────────────────────────────────────────────

/// GET /admin/api/mcp — list all configured MCP servers with live status.
pub async fn admin_list_mcp(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::McpServers, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let servers = state.app_state.mcp_registry.list().await;
    let list: Vec<serde_json::Value> = servers
        .iter()
        .map(|s| {
            serde_json::json!({
                "name": s.name,
                "transport": s.config.infer_transport(),
                "command": s.config.command,
                "args": s.config.args,
                "url": s.config.url,
                "timeout_secs": s.config.timeout_secs,
                "status": s.status,
                "tool_count": s.tool_count,
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!({"servers": list})))
}

/// Request body for POST /admin/api/mcp.
#[derive(serde::Deserialize)]
pub struct CreateMcpRequest {
    /// Unique name for the server (e.g. "my-tool").
    pub name: String,
    /// Shell command to launch (stdio transport).
    pub command: Option<String>,
    /// Arguments for `command`.
    #[serde(default)]
    pub args: Vec<String>,
    /// Extra environment variables.
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    /// URL for HTTP/SSE/StreamableHttp transports.
    pub url: Option<String>,
    /// Explicit transport override.
    pub transport: Option<crate::mcp::McpTransportType>,
    /// Handshake timeout in seconds (default: 30).
    pub timeout_secs: Option<u64>,
}

/// POST /admin/api/mcp — add a new MCP server configuration.
///
/// Adds the server to the in-memory registry and persists it to `mcp.json`.
/// The server starts in `Stopped` state; use the restart endpoint (GAR-287)
/// to connect it without restarting the gateway.
pub async fn admin_create_mcp(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    Json(body): Json<CreateMcpRequest>,
) -> impl IntoResponse {
    use crate::mcp::{McpPersistenceService, McpServerConfig};

    if !check_permission(admin.role, Resource::McpServers, Action::Create) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let name = body.name.trim().to_string();
    if name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "name must not be empty"})),
        );
    }

    if body.command.is_none() && body.url.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "either command or url is required"})),
        );
    }

    let config = McpServerConfig {
        command: body.command,
        args: body.args,
        env: body.env,
        url: body.url,
        transport: body.transport,
        timeout_secs: body.timeout_secs.unwrap_or(30),
        memory_limit_mb: None,
        max_restarts: None,
        restart_delay_secs: None,
    };

    // Add to registry
    state
        .app_state
        .mcp_registry
        .add_server(name.clone(), config)
        .await;

    // Persist to mcp.json (GAR-291: with vault for credential encryption).
    let svc = McpPersistenceService::with_default_path();
    let svc = if let Some(vp) = crate::bootstrap::default_vault_path() {
        svc.with_vault(vp)
    } else {
        svc
    };
    if let Err(e) = svc.save_from_registry(&state.app_state.mcp_registry).await {
        tracing::warn!("admin_create_mcp: failed to persist mcp.json: {e}");
    }

    let server = state.app_state.mcp_registry.get(&name).await;
    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "ok": true,
            "name": name,
            "status": server.map(|s| s.status),
        })),
    )
}

/// POST /admin/api/mcp/:id/restart — hot-reload an individual MCP server (GAR-287).
///
/// Disconnects the current process (if any), re-connects it using the stored
/// config, and updates the registry status. Returns 404 if the server is not
/// registered, 503 if no MCP manager is wired, or 502 if the reconnect fails.
///
/// Note: AgentRuntime tool list is updated at startup and is not patched here;
/// the restarted server's tools are available immediately through the McpManager
/// for calls made via the tool-call path.
pub async fn admin_restart_mcp(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Path(server_name): axum::extract::Path<String>,
) -> impl IntoResponse {
    use crate::mcp::McpTransportType;

    if !check_permission(admin.role, Resource::McpServers, Action::Update) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    // Verify the server exists in the registry.
    let server = state.app_state.mcp_registry.get(&server_name).await;
    let Some(server) = server else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("MCP server '{}' not found", server_name)})),
        );
    };

    let Some(manager) = state.app_state.mcp_manager_arc.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "MCP manager not available"})),
        );
    };

    let config = &server.config;
    let transport = config.infer_transport();

    tracing::info!(
        server = %server_name,
        admin = %admin.username,
        transport = ?transport,
        "admin: restarting MCP server"
    );

    // Disconnect existing connection (no-op if not connected).
    manager.disconnect(&server_name).await;
    // GAR-293: reset the crash counter so the server gets a fresh restart budget.
    manager.reset_restart_state(&server_name).await;
    state
        .app_state
        .mcp_registry
        .set_status(&server_name, crate::mcp::McpStatus::Stopped, 0)
        .await;

    // GAR-293: read resource limits from config.
    let memory_limit_mb = config.memory_limit_mb;
    let max_restarts = config.max_restarts.unwrap_or(5);
    let restart_delay_secs = config.restart_delay_secs.unwrap_or(5);

    // Reconnect based on transport type.
    let result = match transport {
        McpTransportType::Stdio => {
            let command = match config.command.as_deref() {
                Some(c) => c.to_string(),
                None => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({"error": "stdio transport requires 'command'"})),
                    );
                }
            };
            manager
                .connect(
                    &server_name,
                    &command,
                    &config.args,
                    &config.env,
                    config.timeout_secs,
                    vec![],
                    memory_limit_mb,
                    max_restarts,
                    restart_delay_secs,
                )
                .await
        }
        #[cfg(feature = "mcp-http")]
        McpTransportType::StreamableHttp | McpTransportType::Http | McpTransportType::Sse => {
            let url = match config.url.as_deref() {
                Some(u) => u.to_string(),
                None => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({"error": "HTTP transport requires 'url'"})),
                    );
                }
            };
            manager
                .connect_http(
                    &server_name,
                    &url,
                    config.timeout_secs,
                    vec![],
                    max_restarts,
                    restart_delay_secs,
                )
                .await
        }
        #[cfg(not(feature = "mcp-http"))]
        McpTransportType::StreamableHttp | McpTransportType::Http | McpTransportType::Sse => {
            Err(garraia_common::Error::Mcp(
                "HTTP/SSE MCP transports require the 'mcp-http' feature".into(),
            ))
        }
    };

    match result {
        Ok(()) => {
            // Count discovered tools and sync registry status.
            let tool_count = manager.tool_info(&server_name).await.len();
            state
                .app_state
                .mcp_registry
                .set_status(&server_name, crate::mcp::McpStatus::Running, tool_count)
                .await;

            tracing::info!(
                server = %server_name,
                tool_count,
                "admin: MCP server restarted successfully"
            );

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "ok": true,
                    "name": server_name,
                    "status": "Running",
                    "tool_count": tool_count,
                })),
            )
        }
        Err(e) => {
            let msg = e.to_string();
            tracing::error!(server = %server_name, error = %msg, "admin: MCP server restart failed");
            state
                .app_state
                .mcp_registry
                .mark_error(&server_name, &msg)
                .await;
            (
                StatusCode::BAD_GATEWAY,
                Json(
                    serde_json::json!({"error": format!("failed to restart '{}': {}", server_name, msg)}),
                ),
            )
        }
    }
}

/// DELETE /admin/api/mcp/:id — remove a configured MCP server.
///
/// Removes the server from the in-memory registry, deletes its entry from
/// `mcp.json`, and purges any associated vault credentials (GAR-291).
/// Returns 404 if the server is not found.
pub async fn admin_delete_mcp(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Path(server_name): axum::extract::Path<String>,
) -> impl IntoResponse {
    use crate::mcp::McpPersistenceService;

    if !check_permission(admin.role, Resource::McpServers, Action::Delete) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let removed = state
        .app_state
        .mcp_registry
        .remove_server(&server_name)
        .await;
    if !removed {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("MCP server '{}' not found", server_name)})),
        );
    }

    // Persist updated config (server is already gone from registry).
    let svc = McpPersistenceService::with_default_path();
    let svc = if let Some(vp) = crate::bootstrap::default_vault_path() {
        svc.with_vault(vp)
    } else {
        svc
    };

    // Clean up vault credentials for this server.
    svc.delete_server_vault_entries(&server_name);

    if let Err(e) = svc.save_from_registry(&state.app_state.mcp_registry).await {
        tracing::warn!("admin_delete_mcp: failed to persist mcp.json: {e}");
    }

    tracing::info!(
        server = %server_name,
        admin = %admin.username,
        "admin: deleted MCP server"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({"ok": true, "deleted": server_name})),
    )
}
