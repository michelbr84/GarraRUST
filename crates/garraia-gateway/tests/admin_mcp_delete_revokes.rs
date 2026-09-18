//! Issue #1262: `admin_delete_mcp` must actually revoke an MCP server.
//!
//! Drives the **real handler** (`admin_delete_mcp`) and the **real health
//! monitor** (`McpManager::health_tick`), not a hand-rolled reimplementation.
//! That distinction is the whole point of the acceptance criteria: the defect
//! was that `remove_server` touched only the registry while the live
//! connection lived in the `McpManager` and a `pending` entry resurrected the
//! server from the health loop — so a test that only exercised the registry
//! would leave both escape hatches unexecuted.
//!
//! Mutation contract (acceptance criterion 3):
//! - Remove `manager.disconnect` from the handler → `delete_drops_the_live_connection`
//!   goes red (the tool stays callable).
//! - Remove `manager.forget` from the handler → `delete_clears_pending_so_health_tick_does_not_resurrect`
//!   goes red (the health monitor reconnects the deleted server).

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use axum::extract::{Path, State};
use axum::response::IntoResponse;
use garraia_agents::{AgentRuntime, McpManager};
use garraia_channels::ChannelRegistry;
use garraia_config::AppConfig;
use garraia_gateway::admin::mcp::admin_delete_mcp;
use garraia_gateway::admin::middleware::AuthenticatedAdmin;
use garraia_gateway::admin::rbac::Role;
use garraia_gateway::admin::shared::AdminState;
use garraia_gateway::admin::store::AdminStore;
use garraia_gateway::mcp::{McpPersistenceService, McpServerConfig};
use garraia_gateway::state::AppState;
use tokio::sync::Mutex;

const SERVER: &str = "fake-deletable";

/// Point this whole test binary at a throwaway config dir before any test
/// builds an `AppState`. See `admin_mcp_restart_allowlist.rs` for the full
/// rationale: `AppState::new` writes `mcp.json` into the config dir, and
/// without this the tests would race the developer's real `~/.config/garraia`
/// and merge whatever `mcp.json` that machine happens to have.
static TEST_ENV: LazyLock<tempfile::TempDir> = LazyLock::new(|| {
    let dir = tempfile::tempdir().expect("temp config dir");
    // SAFETY: the `LazyLock` serialises this against the other test threads in
    // this binary — every test calls `test_env()` as its first statement.
    unsafe {
        std::env::set_var("GARRAIA_CONFIG_DIR", dir.path());
        std::env::set_var(McpPersistenceService::DISABLE_AUTOPROVISION_ENV, "1");
    }
    dir
});

fn test_env() {
    LazyLock::force(&TEST_ENV);
}

/// The stdio fixture lives in `garraia-agents`; same fake server the lifecycle
/// and restart tests drive, so there is one fixture to keep honest.
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
        allowed_tools: Vec::new(),
        inherit_env: false,
        enabled: None,
    }
}

fn admin() -> AuthenticatedAdmin {
    AuthenticatedAdmin {
        user_id: "u-1262".to_string(),
        username: "operador".to_string(),
        role: Role::Admin,
        csrf_token: "csrf".to_string(),
        session_token: "sess".to_string(),
    }
}

/// Build a fresh `AppState` with `manager` wired in and `SERVER` registered,
/// wrapped in an `Arc` so several deletes can share the same registry.
async fn app_state(manager: &Arc<McpManager>) -> Arc<AppState> {
    let app_config = AppConfig::default();
    let mut state = AppState::new(
        app_config,
        Arc::new(AgentRuntime::new()),
        ChannelRegistry::new(),
    );
    state.mcp_manager_arc = Some(Arc::clone(manager));
    state.mcp_registry.add_server(SERVER, server_config()).await;
    Arc::new(state)
}

/// Wrap a shared `Arc<AppState>` into the `AdminState` the handler receives.
fn admin_state(app: Arc<AppState>) -> AdminState {
    AdminState {
        store: Arc::new(Mutex::new(
            AdminStore::in_memory().expect("in-memory admin store"),
        )),
        app_state: app,
        encryption_key: Arc::new(vec![0u8; 32]),
    }
}

async fn connect(manager: &Arc<McpManager>) {
    manager
        .connect(
            SERVER,
            "python3",
            &fixture_args(),
            &HashMap::new(),
            10,
            vec![],
            None,
            5,
            1,
            false,
        )
        .await
        .expect("fixture server should connect");
}

async fn delete(state: AdminState) -> (axum::http::StatusCode, serde_json::Value) {
    let response = admin_delete_mcp(
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

/// Acceptance criterion (a): a connected server, once deleted, no longer
/// serves tools to the `AgentRuntime`.
///
/// RED before the fix: `admin_delete_mcp` never called `disconnect`, so the
/// live `McpConnection` survived the registry removal and `call_tool` kept
/// working. The mutation proof is removing `manager.disconnect` from the
/// handler — `is_connected` stays `true` and `call_tool` returns `Ok`.
#[tokio::test]
async fn delete_drops_the_live_connection() {
    test_env();
    let body = async {
        let manager = Arc::new(McpManager::new());
        connect(&manager).await;
        assert!(
            manager.is_connected(SERVER).await,
            "precondition: server must start connected"
        );

        // Register the server's tools with the runtime the way boot does, so
        // the post-delete removal has something to drop. Without this, the
        // assertion on `tool_names` would pass vacuously.
        let app = app_state(&manager).await;
        let runtime = Arc::clone(&app.agents);
        let boot_count = runtime.sync_mcp_tools(&manager).await;
        assert!(
            boot_count >= 1,
            "precondition: the runtime must see at least one MCP tool before the delete"
        );
        assert!(
            runtime
                .tool_names()
                .iter()
                .any(|n| n.starts_with(&format!("{SERVER}__"))),
            "precondition: the runtime must list a tool from {SERVER} before the delete"
        );

        let (status, json) = delete(admin_state(app)).await;
        assert_eq!(status, axum::http::StatusCode::OK, "delete body: {json}");

        // (a1) The live connection is gone — `call_tool` can no longer reach
        // the server. Before the fix this returned `Ok` with the tool output.
        assert!(
            !manager.is_connected(SERVER).await,
            "disconnect must drop the live connection"
        );
        let err = manager
            .call_tool(SERVER, "read_file", HashMap::new())
            .await
            .expect_err("call_tool must fail after the delete — the connection is gone");
        assert!(
            err.contains("not connected") || err.contains("not found"),
            "expected a not-connected error, got: {err}"
        );

        // (a2) The runtime's tool inventory no longer lists the deleted
        // server. The handler calls `sync_mcp_tools` after `disconnect`, which
        // is what drops them — without that call the runtime would keep
        // serving stale `McpTool` objects pointing at a dead transport.
        let post_names = runtime.tool_names();
        assert!(
            !post_names
                .iter()
                .any(|n| n.starts_with(&format!("{SERVER}__"))),
            "the runtime must stop listing tools from the deleted server, got: {post_names:?}"
        );

        manager.disconnect_all().await;
    };
    tokio::time::timeout(std::time::Duration::from_secs(60), body)
        .await
        .expect("test must not hang");
}

/// Acceptance criterion (b): a server parked in `pending` (boot failure) is
/// not resurrected by the health monitor after a delete.
///
/// RED before the fix: `admin_delete_mcp` never cleared `pending`, so
/// `check_and_reconnect` turned the orphaned `pending` entry into a reconnect
/// target on the next `health_tick`. The mutation proof is removing
/// `manager.forget` from the handler — `health_tick` reconnects the server and
/// `is_connected` flips to `true`, failing the last assertion.
#[tokio::test]
async fn delete_clears_pending_so_health_tick_does_not_resurrect() {
    test_env();
    let body = async {
        let manager = Arc::new(McpManager::new());

        // Park SERVER in `pending` with a *working* fixture command. The
        // health monitor would therefore reconnect it on the next tick if the
        // `pending` entry survived the delete — which is exactly the
        // resurrection this test pins. A broken command would hide the bug:
        // the reconnect would fail and `is_connected` would stay false whether
        // `forget` ran or not.
        manager
            .register_pending_stdio(
                SERVER,
                "python3",
                &fixture_args(),
                &HashMap::new(),
                10,
                vec![],
                None,
                5,
                1,
                false,
            )
            .await;
        assert!(
            !manager.is_connected(SERVER).await,
            "precondition: a pending server is not connected"
        );

        let app = app_state(&manager).await;
        let (status, json) = delete(admin_state(Arc::clone(&app))).await;
        assert_eq!(status, axum::http::StatusCode::OK, "delete body: {json}");

        // Drive the real health-monitor pass. With `forget` this has no
        // `pending` entry to iterate and reconnects nothing.
        manager.health_tick().await;

        // Give the spawned child a brief moment in case the monitor attempted
        // a reconnect that is still in flight — under the fix none is
        // attempted, so this loop must never observe a connection.
        for _ in 0..20 {
            assert!(
                !manager.is_connected(SERVER).await,
                "health_tick must not resurrect a deleted server — forget cleared pending"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        manager.disconnect_all().await;
    };
    tokio::time::timeout(std::time::Duration::from_secs(60), body)
        .await
        .expect("test must not hang");
}

/// Regression for the "second delete 404s with the server still serving"
/// consequence described in the issue: after the first delete failed to call
/// `disconnect`, the server was gone from the registry but still live in the
/// manager, and a second delete returned 404 leaving the operator no handle.
///
/// With the fix, the teardown runs before the 404 check, so a second delete
/// (which finds nothing in the registry) still tears down any orphaned manager
/// state and returns 404 — and the server is actually gone, not just
/// unlisted.
#[tokio::test]
async fn second_delete_after_a_stale_state_actually_clears_it() {
    test_env();
    let body = async {
        let manager = Arc::new(McpManager::new());
        connect(&manager).await;
        assert!(manager.is_connected(SERVER).await);

        // First delete: registry has the server, handler tears down the
        // manager and removes it. 200 OK.
        let app = app_state(&manager).await;
        let (status, json) = delete(admin_state(Arc::clone(&app))).await;
        assert_eq!(status, axum::http::StatusCode::OK, "first delete: {json}");
        assert!(!manager.is_connected(SERVER).await);

        // A second delete on the SAME registry finds nothing → 404. The
        // contract is preserved. The important part is that the server is not
        // still serving tools behind the 404, which it would have been before
        // the fix (the first delete left the connection live, and the second
        // had no handle to drop it).
        let (status, json) = delete(admin_state(Arc::clone(&app))).await;
        assert_eq!(
            status,
            axum::http::StatusCode::NOT_FOUND,
            "second delete must 404: {json}"
        );
        assert!(
            !manager.is_connected(SERVER).await,
            "the server must not be live behind the 404"
        );

        manager.disconnect_all().await;
    };
    tokio::time::timeout(std::time::Duration::from_secs(60), body)
        .await
        .expect("test must not hang");
}
