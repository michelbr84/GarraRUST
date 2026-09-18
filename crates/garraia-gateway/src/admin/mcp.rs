//! Admin MCP server management handlers.
//!
//! Slice 9.c of GAR-471 / Q9 of EPIC GAR-430 (Quality Gates Phase 3.6).
//! Extracted from `admin/handlers.rs` (lines 2203-2557) without behavior change.
//! Covers MCP server listing, creation, restart, and deletion.

use std::collections::HashMap;

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
                // #1273: the allowlist is now carried by the registry type —
                // exposing it lets the admin surface the GAR-190 restriction
                // instead of it living only inside the manager.
                "allowed_tools": s.config.allowed_tools,
                "inherit_env": s.config.inherit_env,
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
    /// GAR-190: restrict the server to these tool names. Empty/absent = every
    /// discovered tool is allowed.
    ///
    /// #1273: accepted here so an allowlist created through the admin API
    /// lands in `mcp.json` and is honoured at boot and at restart (Config
    /// origin) instead of leaving the server permanently in
    /// `NeverRestricted`.
    #[serde(default)]
    pub allowed_tools: Vec<String>,
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
        allowed_tools: body.allowed_tools,
        // #1075/#1273: POST-created servers are always env-isolated — same
        // stance as the admin restart (`env_isolation = "forced"`). The
        // field exists on the type for round-trip fidelity of hand-written
        // configs; the admin API deliberately does not accept it.
        inherit_env: false,
        enabled: None,
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
/// Issue #924: the restarted server's tools are also re-registered into
/// `AgentRuntime`. This used to be an admitted gap — the note here said the
/// runtime list "is updated at startup and is not patched", which meant a
/// restarted server's tools were reachable through `McpManager::call_tool`
/// (admin, CLI) but invisible to `tool_definitions()`, i.e. to the LLM.
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
        // #1075: um restart pela admin API reconecta sempre isolado, mesmo
        // que o arquivo declare `inherit_env: true`. Desde #1273 o registro
        // CARREGA o campo, então a divergência deixou de ser falta de
        // informação e passou a ser política: reconnect pela admin API
        // nunca entrega o ambiente do gateway ao filho. Registrar isso
        // aqui é o que torna a política diagnosticável a partir do log.
        env_isolation = "forced",
        "admin: restarting MCP server"
    );

    // Issue #1242, path 1: everything the reconnect needs is resolved BEFORE
    // anything is torn down. `command`/`url` used to be read *after* the
    // `disconnect` below, and both reads could `return` a 400 from there — at
    // which point the connection was already gone and the allowlist existed
    // nowhere but this stack frame, so the NEXT restart resolved `None` and
    // brought the server back wide open. That is the very fail-open the `Err`
    // arm parks in `pending`; a validation failure must not be the hole left
    // open. Reachable in one call: `POST /admin/api/mcp` accepts
    // `{"url": ..., "transport": "stdio"}` and overwrites a live entry.
    let reconnect = match transport {
        McpTransportType::Stdio => match config.command.as_deref() {
            Some(command) => ReconnectWith::Stdio(command.to_string()),
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "stdio transport requires 'command'"})),
                );
            }
        },
        McpTransportType::StreamableHttp | McpTransportType::Http | McpTransportType::Sse => {
            match config.url.as_deref() {
                Some(url) => ReconnectWith::Http(url.to_string()),
                None => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({"error": "HTTP transport requires 'url'"})),
                    );
                }
            }
        }
    };

    // Issue #1242: capture the GAR-190 allowlist BEFORE tearing the
    // connection down. `disconnect` drops the `McpConnection` that holds it,
    // and an empty `allowed_tools` means "allow every discovered tool"
    // (`is_tool_allowed`), so the answer to "what was the allowlist?" has to
    // be resolved here, not flattened away.
    let live_config = state.app_state.current_config();
    let declared = declared_mcp_servers(&live_config);
    let (allowed_tools, allowlist_origin) =
        resolve_allowlist(manager, &declared, &server_name).await;
    tracing::info!(
        server = %server_name,
        origin = ?allowlist_origin,
        allowed_tools = allowed_tools.len(),
        "admin: resolved the MCP tool allowlist for the restart"
    );
    // `connect`/`connect_http` take the Vec by value; keep a copy so a failed
    // reconnect can still hand the allowlist to `pending` (see the Err arm).
    let allowlist_for_pending = allowed_tools.clone();

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

    // Reconnect with what was resolved before the teardown.
    let result = match reconnect {
        ReconnectWith::Stdio(command) => {
            manager
                .connect(
                    &server_name,
                    &command,
                    &config.args,
                    &config.env,
                    config.timeout_secs,
                    allowed_tools,
                    memory_limit_mb,
                    max_restarts,
                    restart_delay_secs,
                    // #1075 (continuação): um servidor reiniciado pela
                    // admin API reconecta sempre isolado — `inherit_env:
                    // true` declarado em `config.yml`/`mcp.json` é honrado
                    // no boot (`build_mcp_tools`) mas não aqui. Desde #1273
                    // o tipo do registro carrega o campo, de modo que a
                    // divergência deixou de ser falta de informação e
                    // passou a ser política fail-safe (isola mais, nunca
                    // menos): honrar a declaração no restart é mudança de
                    // stance própria, não parte do arranjo do tipo.
                    false,
                )
                .await
        }
        #[cfg(feature = "mcp-http")]
        ReconnectWith::Http(url) => {
            manager
                .connect_http(
                    &server_name,
                    &url,
                    config.timeout_secs,
                    allowed_tools,
                    max_restarts,
                    restart_delay_secs,
                )
                .await
        }
        #[cfg(not(feature = "mcp-http"))]
        ReconnectWith::Http(_url) => Err(garraia_common::Error::Mcp(
            "HTTP/SSE MCP transports require the 'mcp-http' feature".into(),
        )),
    };

    match result {
        Ok(()) => {
            // Count discovered tools and sync registry status.
            let tool_count = manager.tool_info(&server_name).await.len();

            // Issue #924: push the new inventory into the runtime too, so the
            // LLM sees exactly what the registry just reported.
            state.app_state.agents.sync_mcp_tools(manager).await;
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

            // Issue #1242, path 1: `disconnect` above already removed the
            // connection, so at this point the allowlist we just resolved
            // exists nowhere but this stack frame. Without the line below the
            // NEXT restart resolves `None` for a server that *was* restricted
            // — a failed restart silently unlocking the server on the retry
            // is the same fail-open this handler exists to close. `pending`
            // is where the boot path already parks a server that could not
            // connect, and it gives the health monitor the same backoff-driven
            // retry a boot failure gets.
            register_pending_after_failure(
                manager,
                &server_name,
                config,
                &transport,
                allowlist_for_pending,
                memory_limit_mb,
                max_restarts,
                restart_delay_secs,
            )
            .await;

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

// ── Issue #1242: allowlist resolution for a restart ──────────────────────────

/// What `admin_restart_mcp` will reconnect with, resolved from the registry
/// entry *before* `disconnect` runs.
///
/// It exists so the two "the registry entry is not usable" answers (stdio
/// without `command`, HTTP without `url`) are given while the live connection
/// — and the allowlist it holds — is still intact.
enum ReconnectWith {
    Stdio(String),
    Http(String),
}

/// Where the GAR-190 allowlist used by a restart came from.
///
/// This exists so the answer is *named* in the logs and in the code. The
/// original defect was `allowed_tools_for(..).unwrap_or_default()`: the
/// `Option` is load-bearing (its doc comment says so), `None` became
/// `vec![]`, and `vec![]` is how `is_tool_allowed` spells "allow every
/// discovered tool". One `unwrap_or_default` therefore turned a hot-reload
/// into the exact fail-open the allowlist is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AllowlistOrigin {
    /// The manager holds a non-empty list — a live connection, or a `pending`
    /// entry left by a boot failure or by an earlier failed restart. This is
    /// what is restricting the server right now, so it wins.
    Manager,
    /// Nothing is in force, and the merged declaration (`mcp.json` +
    /// the `mcp:` section of `config.yml`) names a non-empty `allowed_tools`
    /// — the same merge the boot path reads.
    Config,
    /// Nothing restricts this server anywhere. See `resolve_allowlist`.
    NeverRestricted,
}

/// The MCP declarations the **boot path** reads: `mcp.json` merged with the
/// `mcp:` section of `config.yml`, exactly as `bootstrap::build_mcp_tools`
/// merges them (`config.yml` wins a name collision).
///
/// Reading only `config.mcp` here was a fail-open of its own: `mcp.json`
/// deserializes into `garraia_config::McpServerConfig`, which *does* carry
/// `allowed_tools`, and the boot path honours it. An allowlist declared there
/// was therefore in force at boot and invisible to the restart, which fell
/// through to `NeverRestricted` and reconnected the server unrestricted.
///
/// This reads a file from an async handler. It is a single small file that
/// the boot path already reads the same way, on an operator-triggered
/// endpoint, so the block is not worth a `spawn_blocking` hop.
fn declared_mcp_servers(
    config: &garraia_config::AppConfig,
) -> HashMap<String, garraia_config::McpServerConfig> {
    match garraia_config::ConfigLoader::new() {
        Ok(loader) => loader.merged_mcp_config(config),
        Err(e) => {
            // Fail *narrow*, not open: without the config dir we still know
            // what `config.yml` declares.
            tracing::warn!(
                error = %e,
                "admin: could not open the config dir to read mcp.json; \
                 restarting with only the 'mcp:' section of config.yml"
            );
            config.mcp.clone()
        }
    }
}

/// Resolve the allowlist to reconnect `server_name` with.
///
/// The invariant is precise: **a restart never widens what is in force, and
/// applies a declared list when nothing is in force.**
///
/// 1. A **non-empty** live list from the manager (live connection, or a
///    `pending` entry left by a boot failure or an earlier failed restart) is
///    what is actually restricting the server right now. It wins.
/// 2. Otherwise — `None`, or `Some(vec![])`, which is how `is_tool_allowed`
///    spells "allow everything" — a non-empty declared list is applied.
///    `Some(vec![])` means "nobody ever restricted this server", not "the
///    operator chose not to restrict it": the manager cannot tell those
///    apart, and the operator's written `allowed_tools` can. Treating the
///    live empty list as authoritative was a real hole — a server that
///    happened to connect before the operator declared an allowlist could
///    never be tightened by a restart.
/// 3. Only when neither knows of a restriction is the list empty.
///
/// What this deliberately does **not** do: apply a declared list that is
/// narrower than a non-empty live one. Rule 1 keeps the live list, so the
/// restart still never widens, but that tightening needs a gateway restart.
/// Picking between two real allowlists is a policy question this handler
/// should not answer silently.
async fn resolve_allowlist(
    manager: &garraia_agents::McpManager,
    declared: &HashMap<String, garraia_config::McpServerConfig>,
    server_name: &str,
) -> (Vec<String>, AllowlistOrigin) {
    if let Some(live) = manager
        .allowed_tools_for(server_name)
        .await
        .filter(|live| !live.is_empty())
    {
        return (live, AllowlistOrigin::Manager);
    }

    if let Some(entry) = declared.get(server_name)
        && !entry.allowed_tools.is_empty()
    {
        return (entry.allowed_tools.clone(), AllowlistOrigin::Config);
    }

    // Deliberately empty, and this is NOT the flattened `None` of #1242.
    // Reaching here means nothing restricts this server: the manager holds no
    // allowlist (or an empty one) *and* no merged declaration names it.
    //
    // The servers that live here permanently are the ones created through
    // `POST /admin/api/mcp`, which cannot carry an allowlist at all today;
    // refusing to start them would break the documented "create, then
    // restart to connect" flow without protecting anything.
    //
    // #1274: an HTTP entry of `mcp.json` used to land here too — the loader
    // dropped it for lacking `command`, so the declared merge never saw the
    // name and a restart reconnected the server with every tool exposed,
    // discarding the `allowed_tools` the operator had written. The loader now
    // keeps those entries (`command` is `#[serde(default)]` and a stdio entry
    // without one is refused loudly instead), so what still reaches this arm
    // from a file is an entry the loader skipped **with a `warn!`** — never
    // one it dropped silently. This comment used to name only the
    // API-created servers; the incomplete claim is what made the #1274 hole
    // survive three audits.
    (Vec::new(), AllowlistOrigin::NeverRestricted)
}

/// Park a server whose restart failed in the manager's `pending` map, with
/// the allowlist that was in force, so the next restart can still find it.
#[allow(clippy::too_many_arguments)]
async fn register_pending_after_failure(
    manager: &garraia_agents::McpManager,
    server_name: &str,
    config: &crate::mcp::McpServerConfig,
    transport: &crate::mcp::McpTransportType,
    allowed_tools: Vec<String>,
    memory_limit_mb: Option<u64>,
    max_restarts: u32,
    restart_delay_secs: u64,
) {
    use crate::mcp::McpTransportType;

    match transport {
        McpTransportType::Stdio => {
            let Some(command) = config.command.as_deref() else {
                return;
            };
            manager
                .register_pending_stdio(
                    server_name,
                    command,
                    &config.args,
                    &config.env,
                    config.timeout_secs,
                    allowed_tools,
                    memory_limit_mb,
                    max_restarts,
                    restart_delay_secs,
                    // `false`: a política do restart é isolamento forçado
                    // (`env_isolation = "forced"`, no início do handler).
                    // Desde #1273 o tipo carrega `inherit_env`, então
                    // parkar `true` aqui poderia fazer a entrada em
                    // `pending` prometer uma herança que o próximo restart
                    // não honraria — parkar `false` parka a promessa que de
                    // fato é mantida.
                    false,
                )
                .await;
        }
        #[cfg(feature = "mcp-http")]
        McpTransportType::StreamableHttp | McpTransportType::Http | McpTransportType::Sse => {
            let Some(url) = config.url.as_deref() else {
                return;
            };
            manager
                .register_pending_http(
                    server_name,
                    url,
                    config.timeout_secs,
                    allowed_tools,
                    max_restarts,
                    restart_delay_secs,
                )
                .await;
        }
        // Without `mcp-http` the reconnect could not have been attempted over
        // HTTP in the first place, so there is nothing to park.
        #[cfg(not(feature = "mcp-http"))]
        _ => {}
    }
}

/// DELETE /admin/api/mcp/:id — remove a configured MCP server.
///
/// Tears down the live `McpManager` connection, clears any `pending` entry so
/// the health monitor cannot resurrect the server, drops the server's tools
/// from the `AgentRuntime` inventory, removes it from the in-memory registry,
/// deletes its entry from `mcp.json`, and purges associated vault credentials
/// (GAR-291).
///
/// The manager teardown runs **before** the registry removal: `remove_server`
/// only touches the registry, while the live connection and the `pending`
/// resurrect-from-boot-failure path live in the manager. Doing them in the
/// other order left a deleted server serving tools and, after #1242, coming
/// back from `pending` with credentials the DELETE had just purged (issue
/// #1262). Returns 404 if the server is not in the registry — but the manager
/// teardown still runs, so a second delete after a prior silent failure now
/// actually clears the orphaned connection instead of 404-ing with the server
/// still serving.
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

    // Issue #1262: tear down the live connection and clear `pending` BEFORE
    // removing the server from the registry. `remove_server` only touches the
    // registry — the live connection lives in the `McpManager`, and a server
    // parked in `pending` is resurrected by the health monitor on its next
    // `check_and_reconnect` pass. Without these two calls a deleted server
    // kept serving tools to the agent and, worse, came back from `pending`
    // with `env` already resolved from vault credentials the DELETE had just
    // purged — so "delete and revoke" did not revoke. `forget` also drops the
    // `restart_states` counter so a future `register_pending_*` under the same
    // name does not inherit a stale backoff budget.
    //
    // Teardown first means a persistence failure below can leave the manager
    // without the server (safe — nothing resurrects), never the registry
    // without the manager (the dangerous pair that kept the bug alive). If the
    // manager is not wired there is nothing to tear down; the registry removal
    // below still proceeds so the API contract (delete removes the entry) is
    // honored, and the warning makes the missing teardown diagnosable.
    if let Some(manager) = state.app_state.mcp_manager_arc.as_ref() {
        manager.disconnect(&server_name).await;
        manager.forget(&server_name).await;
        // Drop the deleted server's tools from the AgentRuntime inventory, so
        // the LLM stops seeing them. `sync_mcp_tools` only refreshes servers
        // that are still in the manager — a server absent from the manager is
        // intentionally left alone (a read failure must not strip working
        // tools), so after `disconnect` it would never be revisited and its
        // `McpTool` objects would keep pointing at a dead transport until the
        // next gateway restart. `replace_mcp_tools` with an empty vec is the
        // explicit removal path: it retains every tool whose source is not
        // this server and adds none.
        state
            .app_state
            .agents
            .replace_mcp_tools(&server_name, Vec::new());
    } else {
        tracing::warn!(
            server = %server_name,
            "admin_delete_mcp: MCP manager not wired — live connection and pending entries cannot be torn down"
        );
    }

    let removed = state
        .app_state
        .mcp_registry
        .remove_server(&server_name)
        .await;
    if !removed {
        // The registry never had this name, but the manager teardown above
        // already cleared any orphaned live/pending state — which, after a
        // prior delete failed to call `disconnect`, was the only handle the
        // operator had left (the second delete used to 404 with the server
        // still serving). Keep the 404 contract for the API surface.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// #1274: an HTTP entry of `mcp.json` carries an `allowed_tools` the
    /// operator wrote. The loader used to drop the entry (its `command` was
    /// required), so `declared_mcp_servers` never saw the name, the restart
    /// resolution fell through to `NeverRestricted`, and the server came back
    /// with every tool exposed. This exercises the real composition the
    /// handler uses — a file on disk through `merged_mcp_config` (the `Ok`
    /// branch of `declared_mcp_servers`), into `resolve_allowlist` with a
    /// manager that knows nothing — not a hand-built declaration map.
    #[tokio::test]
    async fn restart_resolves_the_allowlist_of_an_http_entry_declared_in_mcp_json() {
        let dir = tempfile::tempdir().expect("temp config dir");
        let mcp_json = serde_json::json!({
            "mcpServers": {
                "remote": {
                    "url": "http://127.0.0.1:9/mcp",
                    "transport": "http",
                    "allowed_tools": ["read_file"],
                    "timeout": 10
                }
            }
        });
        std::fs::write(
            dir.path().join("mcp.json"),
            serde_json::to_vec_pretty(&mcp_json).expect("serialize mcp.json"),
        )
        .expect("write mcp.json");

        // The manager holds nothing: no live connection, no `pending` entry —
        // the restart must find the allowlist in the declared merge or not at
        // all.
        let manager = Arc::new(garraia_agents::McpManager::new());
        assert!(
            manager.allowed_tools_for("remote").await.is_none(),
            "precondition: the manager must not know this server"
        );

        let loader = garraia_config::ConfigLoader::with_dir(dir.path());
        let declared = loader.merged_mcp_config(&garraia_config::AppConfig::default());
        assert!(
            declared.contains_key("remote"),
            "the HTTP entry must reach the declared merge — without it the \
             restart resolution is blind to the name (#1274)"
        );

        let (allowed_tools, origin) = resolve_allowlist(&manager, &declared, "remote").await;
        assert_eq!(
            origin,
            AllowlistOrigin::Config,
            "the declared allowlist must win over NeverRestricted, or the \
             restart reconnects the server unrestricted (#1274)"
        );
        assert_eq!(allowed_tools, vec!["read_file".to_string()]);
    }
}
