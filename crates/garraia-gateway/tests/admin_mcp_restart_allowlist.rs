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

/// Build the `AdminState` the handler receives, with `manager` already wired
/// in and `SERVER` registered exactly as the registry would hold it.
async fn admin_state(manager: &Arc<McpManager>) -> AdminState {
    let mut state = AppState::new(
        AppConfig::default(),
        Arc::new(AgentRuntime::new()),
        ChannelRegistry::new(),
    );
    state.mcp_manager_arc = Some(Arc::clone(manager));
    state.mcp_registry.add_server(SERVER, server_config()).await;

    AdminState {
        store: Arc::new(Mutex::new(
            AdminStore::in_memory().expect("in-memory admin store"),
        )),
        app_state: Arc::new(state),
        encryption_key: Arc::new(vec![0u8; 32]),
    }
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

/// The other half: a server nobody restricted must not be tightened by a
/// restart. `allowed_tools_for` answers `Some(vec![])` for it, which the
/// handler passes through unchanged.
#[tokio::test]
async fn restart_leaves_an_unrestricted_server_open() {
    let body = async {
        let manager = Arc::new(McpManager::new());
        connect(&manager, vec![]).await;

        let (status, json) = restart(admin_state(&manager).await).await;
        assert_eq!(status, axum::http::StatusCode::OK, "restart body: {json}");
        assert_eq!(
            json["tool_count"], 2,
            "every tool stays visible when no allowlist was ever configured: {json}"
        );
        assert!(
            manager
                .call_tool(SERVER, "write_file", HashMap::new())
                .await
                .is_ok(),
            "no allowlist means every discovered tool stays callable"
        );

        manager.disconnect_all().await;
    };
    tokio::time::timeout(std::time::Duration::from_secs(60), body)
        .await
        .expect("test must not hang");
}
