//! Issue #1242: `admin_restart_mcp` must not lose the GAR-190 tool allowlist.
//!
//! This drives the **real handler**, not a hand-rolled reimplementation of its
//! steps. That distinction is the whole point: `crates/garraia-gateway/src/
//! admin/mcp.rs` had 0% coverage, which is exactly how the boot path (passes
//! the configured allowlist), the health-monitor reconnect (clones it) and
//! this restart (dropped it) drifted apart without a single test noticing. A
//! test that only exercises `McpManager` would leave the defect site
//! unexecuted and let the next refactor of the handler reintroduce the bug.
//!
//! The handler is called directly rather than through `build_router` so the
//! test does not have to mint an admin session and a CSRF token; the auth
//! layering is already covered by `authz_http_matrix.rs`, and what is under
//! test here is the body of the handler.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::response::IntoResponse;
use garraia_agents::{AgentRuntime, McpManager};
use garraia_channels::ChannelRegistry;
use garraia_config::AppConfig;
use garraia_gateway::admin::mcp::admin_restart_mcp;
use garraia_gateway::admin::middleware::AuthenticatedAdmin;
use garraia_gateway::admin::rbac::Role;
use garraia_gateway::admin::shared::AdminState;
use garraia_gateway::admin::store::AdminStore;
use garraia_gateway::mcp::McpServerConfig;
use garraia_gateway::state::AppState;
use tokio::sync::Mutex;

const SERVER: &str = "fake-allowlisted";

/// The stdio fixture lives in `garraia-agents`; it is the same child process
/// the MCP lifecycle tests drive, so there is only one fake server to keep
/// honest.
fn fixture_args() -> Vec<String> {
    vec![
        format!(
            "{}/../garraia-agents/tests/fixtures/fake_mcp_server.py",
            env!("CARGO_MANIFEST_DIR")
        ),
        "--tools".to_string(),
        "read_file,write_file".to_string(),
    ]
}

fn server_config() -> McpServerConfig {
    McpServerConfig {
        command: Some("python3".to_string()),
        args: fixture_args(),
        env: HashMap::new(),
        url: None,
        transport: None,
        timeout_secs: 10,
        memory_limit_mb: None,
        max_restarts: Some(5),
        restart_delay_secs: Some(1),
    }
}

fn admin() -> AuthenticatedAdmin {
    AuthenticatedAdmin {
        user_id: "u-1242".to_string(),
        username: "operador".to_string(),
        role: Role::Admin,
        csrf_token: "csrf".to_string(),
        session_token: "sess".to_string(),
    }
}

/// What the `mcp:` section of the running `config.yml` declares for `SERVER`.
///
/// This is the only place an allowlist can be *recovered* from once the
/// manager has forgotten it, so the tests need to be able to set it and to
/// leave it absent.
fn config_yml_entry(command: &str, allowed_tools: Vec<String>) -> garraia_config::McpServerConfig {
    garraia_config::McpServerConfig {
        command: command.to_string(),
        args: fixture_args(),
        env: HashMap::new(),
        transport: "stdio".to_string(),
        url: None,
        enabled: Some(true),
        timeout: Some(10),
        allowed_tools,
        memory_limit_mb: None,
        max_restarts: Some(5),
        restart_delay_secs: Some(1),
    }
}

/// Build the `AdminState` the handler receives, with `manager` already wired
/// in and `SERVER` registered exactly as the registry would hold it.
///
/// `registry_command` is what the *registry* will tell the handler to spawn —
/// pointing it at a command that does not exist is how a test reproduces a
/// restart whose reconnect fails. `declared` is the `config.yml` entry, absent
/// by default.
async fn admin_state_with(
    manager: &Arc<McpManager>,
    registry_command: &str,
    declared: Option<garraia_config::McpServerConfig>,
) -> AdminState {
    let mut app_config = AppConfig::default();
    if let Some(entry) = declared {
        app_config.mcp.insert(SERVER.to_string(), entry);
    }
    let mut state = AppState::new(
        app_config,
        Arc::new(AgentRuntime::new()),
        ChannelRegistry::new(),
    );
    state.mcp_manager_arc = Some(Arc::clone(manager));
    let mut registry_config = server_config();
    registry_config.command = Some(registry_command.to_string());
    state.mcp_registry.add_server(SERVER, registry_config).await;

    AdminState {
        store: Arc::new(Mutex::new(
            AdminStore::in_memory().expect("in-memory admin store"),
        )),
        app_state: Arc::new(state),
        encryption_key: Arc::new(vec![0u8; 32]),
    }
}

/// The common case: registry points at the working fixture, `config.yml`
/// declares nothing.
async fn admin_state(manager: &Arc<McpManager>) -> AdminState {
    admin_state_with(manager, "python3", None).await
}

async fn connect(manager: &Arc<McpManager>, allowed_tools: Vec<String>) {
    manager
        .connect(
            SERVER,
            "python3",
            &fixture_args(),
            &HashMap::new(),
            10,
            allowed_tools,
            None,
            5,
            1,
        )
        .await
        .expect("fixture server should connect");
}

async fn restart(state: AdminState) -> (axum::http::StatusCode, serde_json::Value) {
    let response = admin_restart_mcp(
        State(state),
        axum::Extension(admin()),
        Path(SERVER.to_string()),
    )
    .await
    .into_response();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("response body");
    let json = serde_json::from_slice(&bytes).expect("JSON body");
    (status, json)
}

/// RED before the fix: the handler reconnected with `vec![]`, an empty
/// allowlist means "allow every discovered tool", and `disconnect` had already
/// dropped the only copy of the real allowlist — so `write_file` came back
/// callable and `tool_count` reported 2.
#[tokio::test]
async fn restart_preserves_the_allowlist() {
    let body = async {
        let manager = Arc::new(McpManager::new());
        connect(&manager, vec!["read_file".to_string()]).await;
        assert!(
            manager
                .call_tool(SERVER, "write_file", HashMap::new())
                .await
                .is_err(),
            "allowlist must block write_file before the restart"
        );

        let (status, json) = restart(admin_state(&manager).await).await;
        assert_eq!(status, axum::http::StatusCode::OK, "restart body: {json}");
        assert_eq!(
            json["tool_count"], 1,
            "tool_count must not count an allowlist-blocked tool: {json}"
        );

        let err = manager
            .call_tool(SERVER, "write_file", HashMap::new())
            .await
            .expect_err("write_file must stay blocked across an admin restart");
        assert!(
            err.contains("blocked by the allowed_tools allowlist"),
            "expected the GAR-190 allowlist error, got: {err}"
        );

        let names: Vec<String> = manager
            .take_tools(SERVER, std::time::Duration::from_secs(10))
            .await
            .iter()
            .map(|t| t.name().to_string())
            .collect();
        assert!(
            names.iter().any(|n| n.ends_with("read_file")),
            "allowed tool must survive the restart, got: {names:?}"
        );
        assert!(
            !names.iter().any(|n| n.ends_with("write_file")),
            "blocked tool must not be re-registered by a restart, got: {names:?}"
        );

        manager.disconnect_all().await;
    };
    tokio::time::timeout(std::time::Duration::from_secs(60), body)
        .await
        .expect("test must not hang");
}

/// The other half, and the assertion that makes it worth running: a server
/// the manager *knows* and that nobody restricted must not be tightened by a
/// restart, **even when `config.yml` declares a narrower list**.
///
/// The previous version of this test connected with `vec![]` and asserted
/// that everything stayed open — which `unwrap_or_default()` satisfied just
/// as happily as the fix did, so it passed on both sides of the change and
/// proved nothing. The live answer from the manager (`Some(vec![])`) is a
/// real answer, not a missing one, and it has to win over the config
/// fallback; reordering `resolve_allowlist` to consult the config first
/// turns the two assertions below red.
#[tokio::test]
async fn live_empty_allowlist_wins_over_a_narrower_config_entry() {
    let body = async {
        let manager = Arc::new(McpManager::new());
        connect(&manager, vec![]).await;

        let state = admin_state_with(
            &manager,
            "python3",
            Some(config_yml_entry("python3", vec!["read_file".to_string()])),
        )
        .await;
        let (status, json) = restart(state).await;
        assert_eq!(status, axum::http::StatusCode::OK, "restart body: {json}");
        assert_eq!(
            json["tool_count"], 2,
            "the live (empty) allowlist is authoritative — the config entry \
             must not retroactively restrict a running server: {json}"
        );
        assert!(
            manager
                .call_tool(SERVER, "write_file", HashMap::new())
                .await
                .is_ok(),
            "no allowlist in force means every discovered tool stays callable"
        );

        manager.disconnect_all().await;
    };
    tokio::time::timeout(std::time::Duration::from_secs(60), body)
        .await
        .expect("test must not hang");
}

/// Issue #1242, path 1 — the one the reproduction walked.
///
/// A restart whose reconnect fails leaves the manager with **no** record of
/// the server: `disconnect` removed the connection before the reconnect was
/// attempted. The handler therefore has to park the allowlist it resolved in
/// `pending`, or the *next* restart resolves `None` and reconnects the server
/// wide open — a failed restart quietly unlocking what it was protecting.
///
/// Delete the `register_pending_after_failure` call in `admin_restart_mcp`
/// and this test goes red at `tool_count`.
#[tokio::test]
async fn a_failed_restart_does_not_lose_the_allowlist_for_the_next_one() {
    let body = async {
        let manager = Arc::new(McpManager::new());
        connect(&manager, vec!["read_file".to_string()]).await;

        // Restart #1: the registry hands the handler a command that cannot be
        // spawned, so the reconnect fails after `disconnect` already ran.
        let broken = admin_state_with(&manager, "garraia-no-such-binary-1242", None).await;
        let (status, json) = restart(broken).await;
        assert_eq!(
            status,
            axum::http::StatusCode::BAD_GATEWAY,
            "a reconnect against a missing binary must fail: {json}"
        );

        // Restart #2, this time with a command that works. Nothing else has
        // re-declared the allowlist: `config.yml` is empty in this test, so
        // the only way it can survive is the `pending` entry left above.
        let (status, json) = restart(admin_state(&manager).await).await;
        assert_eq!(status, axum::http::StatusCode::OK, "restart body: {json}");
        assert_eq!(
            json["tool_count"], 1,
            "the allowlist must survive a failed restart: {json}"
        );

        let err = manager
            .call_tool(SERVER, "write_file", HashMap::new())
            .await
            .expect_err("write_file must stay blocked after a failed restart");
        assert!(
            err.contains("blocked by the allowed_tools allowlist"),
            "expected the GAR-190 allowlist error, got: {err}"
        );

        manager.disconnect_all().await;
    };
    tokio::time::timeout(std::time::Duration::from_secs(120), body)
        .await
        .expect("test must not hang");
}

/// Issue #1242, the generalisation of path 2 — a server the manager has no
/// record of at all.
///
/// Path 2 is the HTTP shape of this: `bootstrap` parked only *stdio* boot
/// failures in `pending`, so an HTTP server whose boot handshake failed left
/// the manager knowing nothing about it and the very first restart resolved
/// `None`. The resolution that fixes it lives above the transport split, so
/// it is driven here with the stdio fixture: the manager is empty, the
/// registry has the server (that is how it reaches the handler at all), and
/// the running config is the only thing that still knows the allowlist.
///
/// Delete the `config.mcp.get(..)` branch of `resolve_allowlist` and this
/// test goes red — `NeverRestricted` hands the reconnect `vec![]`, which is
/// "allow everything".
#[tokio::test]
async fn restart_recovers_the_allowlist_from_config_when_the_manager_forgot() {
    let body = async {
        // Never connected: `allowed_tools_for` answers `None` for SERVER.
        let manager = Arc::new(McpManager::new());
        assert!(
            manager.allowed_tools_for(SERVER).await.is_none(),
            "precondition: the manager must not know this server"
        );

        let state = admin_state_with(
            &manager,
            "python3",
            Some(config_yml_entry("python3", vec!["read_file".to_string()])),
        )
        .await;
        let (status, json) = restart(state).await;
        assert_eq!(status, axum::http::StatusCode::OK, "restart body: {json}");
        assert_eq!(
            json["tool_count"], 1,
            "the config-declared allowlist must be applied to the reconnect: {json}"
        );

        let err = manager
            .call_tool(SERVER, "write_file", HashMap::new())
            .await
            .expect_err("write_file must be blocked by the config-declared allowlist");
        assert!(
            err.contains("blocked by the allowed_tools allowlist"),
            "expected the GAR-190 allowlist error, got: {err}"
        );

        manager.disconnect_all().await;
    };
    tokio::time::timeout(std::time::Duration::from_secs(60), body)
        .await
        .expect("test must not hang");
}

/// The floor: a server nobody ever restricted — no manager record, no config
/// entry, which is every server created through `POST /admin/api/mcp` — still
/// starts, and starts unrestricted. This is the branch `resolve_allowlist`
/// spells `NeverRestricted`, and the test exists so that a later attempt to
/// make the handler fail-closed on *every* `None` cannot land silently: the
/// documented "create, then restart to connect" flow would stop working.
#[tokio::test]
async fn a_server_that_was_never_restricted_still_starts() {
    let body = async {
        let manager = Arc::new(McpManager::new());

        let (status, json) = restart(admin_state(&manager).await).await;
        assert_eq!(status, axum::http::StatusCode::OK, "restart body: {json}");
        assert_eq!(
            json["tool_count"], 2,
            "a server with no allowlist anywhere is unrestricted, not refused: {json}"
        );

        manager.disconnect_all().await;
    };
    tokio::time::timeout(std::time::Duration::from_secs(60), body)
        .await
        .expect("test must not hang");
}
