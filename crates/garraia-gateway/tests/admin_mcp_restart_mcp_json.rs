//! Issue #1242, blocker found in the second review: an allowlist written into
//! `mcp.json` must survive an admin restart.
//!
//! `admin_restart_mcp` resolved its fallback against `config.mcp` alone — the
//! `mcp:` section of `config.yml`. The **boot** path does not read that: it
//! reads `ConfigLoader::merged_mcp_config`, which is `mcp.json` merged with
//! that section, and `mcp.json` deserializes into
//! `garraia_config::McpServerConfig`, which carries `allowed_tools`. So an
//! allowlist declared in `mcp.json` was honoured at boot and invisible to the
//! restart: the resolution fell through to `NeverRestricted`, handed the
//! reconnect `vec![]` — "allow every discovered tool" — and the server came
//! back open, with `tool_count: 2` and `write_file` callable again.
//!
//! This test therefore uses a real `mcp.json` on disk, not a hand-built
//! `AppConfig`. An `AppConfig` fixture would prove nothing about the source
//! the boot path actually reads, which is the whole point of the defect.
//!
//! It lives in its own test binary because it needs a config dir with a
//! specific `mcp.json` in it, and the sibling binary
//! (`admin_mcp_restart_allowlist.rs`) needs one without.

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

const SERVER: &str = "declared-in-mcp-json";

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

#[tokio::test]
async fn restart_honours_an_allowlist_declared_in_mcp_json() {
    let dir = tempfile::tempdir().expect("temp config dir");
    // SAFETY: dedicated test binary with a single test, so no other thread is
    // reading the environment concurrently.
    unsafe {
        std::env::set_var("GARRAIA_CONFIG_DIR", dir.path());
        std::env::set_var(
            garraia_gateway::mcp::McpPersistenceService::DISABLE_AUTOPROVISION_ENV,
            "1",
        );
    }

    // The file exactly as an operator (or the wizard) writes it — Claude
    // Desktop shape, with the GAR-190 allowlist.
    let mcp_json = serde_json::json!({
        "mcpServers": {
            SERVER: {
                "command": "python3",
                "args": fixture_args(),
                "transport": "stdio",
                "timeout": 10,
                "allowed_tools": ["read_file"],
            }
        }
    });
    std::fs::write(
        dir.path().join("mcp.json"),
        serde_json::to_vec_pretty(&mcp_json).expect("serialize mcp.json"),
    )
    .expect("write mcp.json");

    let body = async {
        // The manager knows nothing: this is a gateway that has not connected
        // the server yet, or one whose boot handshake failed. The registry
        // knows it, which is how it reaches the handler at all.
        let manager = Arc::new(McpManager::new());
        assert!(
            manager.allowed_tools_for(SERVER).await.is_none(),
            "precondition: the manager must not know this server"
        );

        // `config.yml` declares nothing — `mcp.json` is the only source.
        let mut state = AppState::new(
            AppConfig::default(),
            Arc::new(AgentRuntime::new()),
            ChannelRegistry::new(),
        );
        assert!(
            state.config.mcp.is_empty(),
            "precondition: the 'mcp:' section of config.yml must be empty, or \
             this test would pass through the old code path too"
        );
        state.mcp_manager_arc = Some(Arc::clone(&manager));
        state
            .mcp_registry
            .add_server(
                SERVER,
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
                },
            )
            .await;

        let admin_state = AdminState {
            store: Arc::new(Mutex::new(
                AdminStore::in_memory().expect("in-memory admin store"),
            )),
            app_state: Arc::new(state),
            encryption_key: Arc::new(vec![0u8; 32]),
        };

        let response = admin_restart_mcp(
            State(admin_state),
            axum::Extension(AuthenticatedAdmin {
                user_id: "u-1242".to_string(),
                username: "operador".to_string(),
                role: Role::Admin,
                csrf_token: "csrf".to_string(),
                session_token: "sess".to_string(),
            }),
            Path(SERVER.to_string()),
        )
        .await
        .into_response();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("response body");
        let json: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON body");

        assert_eq!(status, axum::http::StatusCode::OK, "restart body: {json}");
        assert_eq!(
            json["tool_count"], 1,
            "the allowlist declared in mcp.json — the file the boot path reads \
             — must be applied to the reconnect: {json}"
        );
        let err = manager
            .call_tool(SERVER, "write_file", HashMap::new())
            .await
            .expect_err("write_file must be blocked by the mcp.json allowlist");
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
