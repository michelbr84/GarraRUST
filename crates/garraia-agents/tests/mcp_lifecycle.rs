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
        )
        .await
        .expect("fixture server should connect");
}

fn ctx() -> ToolContext {
    ToolContext {
        session_id: "mcp-lifecycle-test".to_string(),
        user_id: None,
        is_heartbeat: false,
        is_confirmation_approved: false,
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
