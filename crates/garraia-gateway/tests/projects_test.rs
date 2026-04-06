//! Integration tests for the Projects API (Phase 1.3).
//!
//! Tests the full CRUD lifecycle: create, list, get, update, delete.
//! Spins up a real gateway server on a random port and uses HTTP requests.

use std::net::TcpListener;

use garraia_config::AppConfig;
use garraia_gateway::GatewayServer;
use serde_json::json;

/// Pick a random available port.
fn random_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind to random port");
    listener.local_addr().unwrap().port()
}

/// Start the gateway in the background and return the base URL.
/// Waits up to 30 seconds for the server to accept connections.
///
/// Uses a temporary config directory to avoid loading real MCP server
/// configs from the user's home directory during tests.
async fn start_test_gateway() -> String {
    let port = random_port();
    let mut config = AppConfig::default();
    config.gateway.port = port;
    config.memory.enabled = false;
    config.mcp.clear();

    // Point config dir to a temp location so the loader won't find
    // any disk-based MCP configs (garraia.yaml, mcp.json, etc.)
    let tmp = tempfile::tempdir().expect("create temp config dir");
    // SAFETY: we are in a test and no other threads are reading this env var yet.
    unsafe { std::env::set_var("GARRAIA_CONFIG_DIR", tmp.path().to_str().unwrap()) };

    tokio::spawn(async move {
        let server = GatewayServer::new(config);
        let _ = server.run().await;
    });

    // Wait for the server to actually accept TCP connections
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .expect("build reqwest client");

    for _ in 0..60 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        if client
            .get(format!("http://127.0.0.1:{port}/health"))
            .send()
            .await
            .is_ok()
        {
            break;
        }
    }

    format!("http://127.0.0.1:{port}")
}

#[tokio::test]
async fn project_crud_lifecycle() {
    let base = start_test_gateway().await;
    let client = reqwest::Client::new();

    // ── Create ────────────────────────────────────────────────────────────
    let create_resp = client
        .post(format!("{base}/api/projects"))
        .json(&json!({
            "name": "test-project",
            "path": "/tmp/test-project",
            "description": "A test project"
        }))
        .send()
        .await
        .expect("create request should succeed");

    assert_eq!(create_resp.status(), 201, "create should return 201");

    let create_body: serde_json::Value = create_resp.json().await.expect("valid JSON");
    let project_id = create_body["project"]["id"]
        .as_str()
        .expect("should have project id")
        .to_string();
    assert_eq!(create_body["project"]["name"], "test-project");
    assert_eq!(create_body["project"]["path"], "/tmp/test-project");
    assert_eq!(create_body["project"]["description"], "A test project");

    // ── List ──────────────────────────────────────────────────────────────
    let list_resp = client
        .get(format!("{base}/api/projects"))
        .send()
        .await
        .expect("list request should succeed");

    assert_eq!(list_resp.status(), 200);
    let list_body: serde_json::Value = list_resp.json().await.expect("valid JSON");
    let projects = list_body["projects"].as_array().expect("should be array");
    assert!(
        projects.iter().any(|p| p["id"].as_str() == Some(&project_id)),
        "created project should appear in list"
    );

    // ── Get ───────────────────────────────────────────────────────────────
    let get_resp = client
        .get(format!("{base}/api/projects/{project_id}"))
        .send()
        .await
        .expect("get request should succeed");

    assert_eq!(get_resp.status(), 200);
    let get_body: serde_json::Value = get_resp.json().await.expect("valid JSON");
    assert_eq!(get_body["project"]["id"], project_id);
    assert_eq!(get_body["project"]["name"], "test-project");

    // ── Update ────────────────────────────────────────────────────────────
    let update_resp = client
        .put(format!("{base}/api/projects/{project_id}"))
        .json(&json!({
            "name": "renamed-project",
            "description": "Updated description"
        }))
        .send()
        .await
        .expect("update request should succeed");

    assert_eq!(update_resp.status(), 200);
    let update_body: serde_json::Value = update_resp.json().await.expect("valid JSON");
    assert_eq!(update_body["project"]["name"], "renamed-project");

    // Verify update persisted via GET
    let verify_resp = client
        .get(format!("{base}/api/projects/{project_id}"))
        .send()
        .await
        .expect("verify request should succeed");
    let verify_body: serde_json::Value = verify_resp.json().await.expect("valid JSON");
    assert_eq!(verify_body["project"]["name"], "renamed-project");

    // ── Delete ────────────────────────────────────────────────────────────
    let delete_resp = client
        .delete(format!("{base}/api/projects/{project_id}"))
        .send()
        .await
        .expect("delete request should succeed");

    assert_eq!(delete_resp.status(), 200);
    let delete_body: serde_json::Value = delete_resp.json().await.expect("valid JSON");
    assert_eq!(delete_body["ok"], true);

    // Verify deletion via GET (should 404)
    let gone_resp = client
        .get(format!("{base}/api/projects/{project_id}"))
        .send()
        .await
        .expect("gone request should succeed");
    assert_eq!(gone_resp.status(), 404, "deleted project should return 404");
}

#[tokio::test]
async fn get_nonexistent_project_returns_404() {
    let base = start_test_gateway().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{base}/api/projects/nonexistent-id"))
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn update_nonexistent_project_returns_404() {
    let base = start_test_gateway().await;
    let client = reqwest::Client::new();

    let resp = client
        .put(format!("{base}/api/projects/nonexistent-id"))
        .json(&json!({"name": "new-name"}))
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn delete_nonexistent_project_returns_404() {
    let base = start_test_gateway().await;
    let client = reqwest::Client::new();

    let resp = client
        .delete(format!("{base}/api/projects/nonexistent-id"))
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), 404);
}
