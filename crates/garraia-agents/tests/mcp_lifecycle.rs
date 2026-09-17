//! Lifecycle regression tests for the MCP client.
//!
//! Every test is wrapped in an outer timeout so a regression fails the suite
//! instead of hanging CI. They drive a real child process (the Python fixture
//! in `tests/fixtures/`) because the defects these pin — dead-transport
//! detection and peer staleness across reconnects — only appear with a real
//! transport that dies.
#![cfg(feature = "mcp")]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use garraia_agents::McpManager;
use garraia_agents::tools::{ToolContext, ToolOutput};

fn fixture_args(extra: &[&str]) -> Vec<String> {
    let script = format!(
        "{}/tests/fixtures/fake_mcp_server.py",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut args = vec![script];
    args.extend(extra.iter().map(|s| s.to_string()));
    args
}

async fn connect(manager: &Arc<McpManager>, name: &str, extra: &[&str]) {
    manager
        .connect(
            name,
            "python3",
            &fixture_args(extra),
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

fn ctx() -> ToolContext {
    ToolContext {
        session_id: "mcp-lifecycle-test".to_string(),
        user_id: None,
        is_heartbeat: false,
        approval: garraia_agents::tools::approval::ToolApproval::None,
        working_dir: None,
        project_id: None,
    }
}

async fn call_echo(tool: &dyn garraia_agents::tools::Tool) -> ToolOutput {
    tool.execute(&ctx(), serde_json::json!({}))
        .await
        .expect("echo tool call should succeed")
}

/// Pins the §0 defect: `RunningService::is_closed()` never flips when the
/// child dies on its own, so the manager reported dead servers as alive and
/// the whole auto-restart machinery was a no-op. Fails before the fix.
#[tokio::test]
async fn detects_dead_child_and_reconnects() {
    let body = async {
        let manager = Arc::new(McpManager::new());
        connect(&manager, "fake", &["--crash-after-calls", "1"]).await;
        assert!(manager.is_connected("fake").await, "should start connected");

        let tools = manager.take_tools("fake", Duration::from_secs(10)).await;
        let _ = call_echo(tools[0].as_ref()).await; // triggers the crash

        // Give the serve loop a moment to observe the closed pipe.
        for _ in 0..40 {
            if !manager.is_connected("fake").await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(
            !manager.is_connected("fake").await,
            "manager must notice the child died"
        );

        manager.health_tick().await;
        assert!(
            manager.is_connected("fake").await,
            "health tick must reconnect the dead server"
        );
        manager.disconnect_all().await;
    };
    tokio::time::timeout(Duration::from_secs(30), body)
        .await
        .expect("test must not hang");
}

/// Pins the §3 defect: tools captured an `Arc<Peer>` at registration, so after
/// a reconnect every LLM-visible MCP tool talked to a dead transport forever
/// (the AgentRuntime is immutable after boot). Fails before the fix.
#[tokio::test]
async fn tool_survives_reconnect() {
    let body = async {
        let manager = Arc::new(McpManager::new());
        connect(&manager, "fake", &["--crash-after-calls", "1"]).await;

        // Registered once, exactly like the gateway does at boot.
        let tools = manager.take_tools("fake", Duration::from_secs(10)).await;
        let tool = tools[0].as_ref();

        let first = call_echo(tool).await; // crashes the child
        assert!(first.content.contains("pong"));

        for _ in 0..40 {
            if !manager.is_connected("fake").await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        manager.health_tick().await;
        assert!(
            manager.is_connected("fake").await,
            "should have reconnected"
        );

        // The same tool object must reach the NEW peer.
        let second = call_echo(tool).await;
        assert!(
            second.content.contains("pong"),
            "tool must follow the reconnect, got: {}",
            second.content
        );
        manager.disconnect_all().await;
    };
    tokio::time::timeout(Duration::from_secs(30), body)
        .await
        .expect("test must not hang");
}

/// A server that ignores stdin EOF must not hold shutdown open forever.
#[tokio::test]
async fn disconnect_all_is_bounded_with_stubborn_child() {
    let body = async {
        let manager = Arc::new(McpManager::new());
        connect(&manager, "stubborn", &["--ignore-eof"]).await;

        tokio::time::timeout(Duration::from_secs(20), manager.disconnect_all())
            .await
            .expect("disconnect_all must not hang on a child that ignores EOF");
        assert!(!manager.is_connected("stubborn").await);
    };
    tokio::time::timeout(Duration::from_secs(40), body)
        .await
        .expect("test must not hang");
}

/// Connect exactly like the boot path does, with a GAR-190 allowlist in force.
async fn connect_allowlisted(
    manager: &Arc<McpManager>,
    name: &str,
    extra: &[&str],
    allowed_tools: Vec<String>,
) {
    manager
        .connect(
            name,
            "python3",
            &fixture_args(extra),
            &HashMap::new(),
            10,
            allowed_tools,
            None,
            5,
            1,
            // #1236: `false` e o default de producao — ambiente do filho
            // filtrado pela allowlist. O gemeo `connect_fixture` acima usa o
            // mesmo valor; testar allowlist de TOOL sob heranca de ambiente
            // ligada misturaria dois controles de seguranca num teste so.
            false,
        )
        .await
        .expect("fixture server should connect");
}

/// Issue #1242: `admin_restart_mcp` reconnected with `vec![]` as the
/// allowlist, and `is_tool_allowed` reads an empty allowlist as "allow
/// everything" — so a routine hot-reload silently re-opened every discovered
/// tool to the LLM. `disconnect` had already dropped the only copy of the
/// allowlist, so nothing downstream could notice.
///
/// This is the manager-side half: it pins that `allowed_tools_for` reports
/// the allowlist a reconnect needs, and that `take_tools`/`call_tool`/
/// `tool_info` all agree once it is passed back in. The handler itself is
/// covered by `garraia-gateway/tests/admin_mcp_restart_allowlist.rs`, which
/// calls `admin_restart_mcp` for real — `admin/mcp.rs` had no test executing
/// it at all, which is how this path drifted from the boot path and the
/// health-monitor reconnect without anything going red.
#[tokio::test]
async fn restart_preserves_tool_allowlist() {
    let body = async {
        let manager = Arc::new(McpManager::new());
        let fixture = ["--tools", "read_file,write_file"];
        connect_allowlisted(&manager, "fake", &fixture, vec!["read_file".to_string()]).await;

        // Baseline: the allowlist binds before the restart.
        assert!(
            manager
                .call_tool("fake", "write_file", HashMap::new())
                .await
                .is_err(),
            "allowlist must block write_file before the restart"
        );

        // --- what admin_restart_mcp does ---
        let captured = manager
            .allowed_tools_for("fake")
            .await
            .expect("a connected server must report its allowlist");
        manager.disconnect("fake").await;
        connect_allowlisted(&manager, "fake", &fixture, captured).await;
        // --- end of restart ---

        let err = manager
            .call_tool("fake", "write_file", HashMap::new())
            .await
            .expect_err("write_file must stay blocked across a restart");
        assert!(
            err.contains("blocked by the allowed_tools allowlist"),
            "expected the GAR-190 allowlist error, got: {err}"
        );

        let names: Vec<String> = manager
            .take_tools("fake", Duration::from_secs(10))
            .await
            .iter()
            .map(|t| t.name().to_string())
            .collect();
        assert!(
            names.iter().any(|n| n.ends_with("read_file")),
            "allowed tool must still be registered, got: {names:?}"
        );
        assert!(
            !names.iter().any(|n| n.ends_with("write_file")),
            "blocked tool must not be registered after a restart, got: {names:?}"
        );

        // The restart response reports `tool_info().len()` as `tool_count`;
        // counting a blocked tool tells the operator the allowlist is off.
        assert_eq!(
            manager.tool_info("fake").await.len(),
            1,
            "tool_count must not include allowlist-blocked tools"
        );

        manager.disconnect_all().await;
    };
    tokio::time::timeout(Duration::from_secs(60), body)
        .await
        .expect("test must not hang");
}

/// The other half of #1242: a server that never had an allowlist must keep
/// behaving as before. `Some(vec![])` (known, no allowlist) and `None`
/// (unknown server) are different answers, and the restart handler relies on
/// the difference to avoid tightening a server nobody restricted.
#[tokio::test]
async fn restart_leaves_an_unrestricted_server_unrestricted() {
    let body = async {
        let manager = Arc::new(McpManager::new());
        let fixture = ["--tools", "read_file,write_file"];
        connect_allowlisted(&manager, "fake", &fixture, vec![]).await;

        assert_eq!(
            manager.allowed_tools_for("fake").await,
            Some(vec![]),
            "a known server without an allowlist reports an empty one, not None"
        );
        assert_eq!(
            manager.allowed_tools_for("never-registered").await,
            None,
            "an unknown server must be distinguishable from an unrestricted one"
        );

        let captured = manager
            .allowed_tools_for("fake")
            .await
            .expect("a connected server must report its allowlist");
        manager.disconnect("fake").await;
        connect_allowlisted(&manager, "fake", &fixture, captured).await;

        assert!(
            manager
                .call_tool("fake", "write_file", HashMap::new())
                .await
                .is_ok(),
            "no allowlist means every discovered tool stays callable"
        );
        assert_eq!(
            manager.tool_info("fake").await.len(),
            2,
            "tool_count must still report every tool when nothing is blocked"
        );

        manager.disconnect_all().await;
    };
    tokio::time::timeout(Duration::from_secs(60), body)
        .await
        .expect("test must not hang");
}
