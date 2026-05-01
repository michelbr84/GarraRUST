//! Integration tests for the Skins API (Phase 1.3).
//!
//! Tests create, list, get, and delete skin operations.
//! Spins up a real gateway server and uses a temp directory for skins storage.

use std::net::TcpListener;

use garraia_config::AppConfig;
use garraia_gateway::GatewayServer;
use serde_json::json;
use serial_test::serial;

/// Pick a random available port.
fn random_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind to random port");
    listener.local_addr().unwrap().port()
}

/// Start the gateway with a temp skins directory and return the base URL.
/// Waits up to 30 seconds for the server to accept connections.
async fn start_test_gateway_with_skins_dir(skins_dir: &str) -> String {
    let port = random_port();
    let mut config = AppConfig::default();
    config.gateway.port = port;
    config.memory.enabled = false;
    // Disable MCP servers to speed up test startup
    config.mcp.clear();

    // Set skins directory via env var before spawning
    // Point config dir to a temp location to avoid loading real MCP configs
    let tmp_config = tempfile::tempdir().expect("create temp config dir");
    // SAFETY: we are in a test and no other threads are reading these env vars yet.
    unsafe {
        std::env::set_var("GARRAIA_CONFIG_DIR", tmp_config.path().to_str().unwrap());
        std::env::set_var("GARRAIA_SKINS_DIR", skins_dir);
    }

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
#[serial]
async fn skin_crud_lifecycle() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let skins_path = tmp.path().join("skins");
    let base =
        start_test_gateway_with_skins_dir(skins_path.to_str().expect("valid utf8 path")).await;
    let client = reqwest::Client::new();

    // ── List (initially empty) ───────────────────────────────────────────
    let list_resp = client
        .get(format!("{base}/api/skins"))
        .send()
        .await
        .expect("list should succeed");

    assert_eq!(list_resp.status(), 200);
    let list_body: serde_json::Value = list_resp.json().await.expect("valid JSON");
    let skins = list_body["skins"].as_array().expect("should be array");
    assert!(skins.is_empty(), "skins should be empty initially");

    // ── Create ────────────────────────────────────────────────────────────
    let create_resp = client
        .post(format!("{base}/api/skins"))
        .json(&json!({
            "name": "dark-theme",
            "primary_color": "#1a1a2e",
            "background": "#16213e",
            "text_color": "#e0e0e0"
        }))
        .send()
        .await
        .expect("create should succeed");

    assert_eq!(create_resp.status(), 201, "create should return 201");
    let create_body: serde_json::Value = create_resp.json().await.expect("valid JSON");
    assert_eq!(create_body["skin"]["name"], "dark-theme");

    // ── List (should contain our skin) ───────────────────────────────────
    let list_resp2 = client
        .get(format!("{base}/api/skins"))
        .send()
        .await
        .expect("list should succeed");

    assert_eq!(list_resp2.status(), 200);
    let list_body2: serde_json::Value = list_resp2.json().await.expect("valid JSON");
    let skins2 = list_body2["skins"].as_array().expect("should be array");
    assert_eq!(skins2.len(), 1, "should have one skin");
    assert_eq!(skins2[0]["name"], "dark-theme");

    // ── Get ───────────────────────────────────────────────────────────────
    let get_resp = client
        .get(format!("{base}/api/skins/dark-theme"))
        .send()
        .await
        .expect("get should succeed");

    assert_eq!(get_resp.status(), 200);
    let get_body: serde_json::Value = get_resp.json().await.expect("valid JSON");
    assert_eq!(get_body["skin"]["name"], "dark-theme");
    assert_eq!(get_body["skin"]["primary_color"], "#1a1a2e");

    // ── Delete ────────────────────────────────────────────────────────────
    let delete_resp = client
        .delete(format!("{base}/api/skins/dark-theme"))
        .send()
        .await
        .expect("delete should succeed");

    assert_eq!(delete_resp.status(), 200);
    let delete_body: serde_json::Value = delete_resp.json().await.expect("valid JSON");
    assert_eq!(delete_body["ok"], true);

    // ── Verify deletion ──────────────────────────────────────────────────
    let gone_resp = client
        .get(format!("{base}/api/skins/dark-theme"))
        .send()
        .await
        .expect("gone request should succeed");
    assert_eq!(gone_resp.status(), 404);
}

#[tokio::test]
#[serial]
async fn get_nonexistent_skin_returns_404() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let skins_path = tmp.path().join("skins_404");
    let base =
        start_test_gateway_with_skins_dir(skins_path.to_str().expect("valid utf8 path")).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{base}/api/skins/nonexistent"))
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), 404);
}

#[tokio::test]
#[serial]
async fn create_skin_with_path_traversal_returns_400() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let skins_path = tmp.path().join("skins_traversal");
    let base =
        start_test_gateway_with_skins_dir(skins_path.to_str().expect("valid utf8 path")).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/api/skins"))
        .json(&json!({
            "name": "../evil",
            "color": "#000"
        }))
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), 400, "path traversal should be rejected");
}

// GAR-490 PR A: extra negative-matrix cases that exercise the centralized
// `validate_skill_name` helper end-to-end. Each rejection must travel
// through the real Axum router (Path extractor + handler) so we know the
// fix actually fires in production, not just in unit tests.

#[tokio::test]
#[serial]
async fn get_skin_with_dot_in_name_returns_400() {
    // Using `evil.json` instead of literal `..` because clients (and
    // matchit) collapse `..` segments before they reach the handler.
    // Any `.` is rejected by the helper, so this still exercises the
    // path-injection guard end-to-end on the GET handler.
    let tmp = tempfile::tempdir().expect("create temp dir");
    let skins_path = tmp.path().join("skins_get_dot");
    let base =
        start_test_gateway_with_skins_dir(skins_path.to_str().expect("valid utf8 path")).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{base}/api/skins/evil.json"))
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), 400);
}

#[tokio::test]
#[serial]
async fn delete_skin_with_backslash_returns_400() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let skins_path = tmp.path().join("skins_del_back");
    let base =
        start_test_gateway_with_skins_dir(skins_path.to_str().expect("valid utf8 path")).await;
    let client = reqwest::Client::new();

    // %5C = '\' — the prior narrow validation in create_skin caught the
    // literal but the get/delete handlers did not validate at all.
    let resp = client
        .delete(format!("{base}/api/skins/a%5Cb"))
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), 400);
}

#[tokio::test]
#[serial]
async fn create_skin_rejects_underscore_per_project_convention() {
    // Aligns with `garraia_skills::validate_skill` (parser.rs:64):
    // underscore is not part of the project's name-charset convention.
    // The unified helper applies the same rule to skin basenames.
    let tmp = tempfile::tempdir().expect("create temp dir");
    let skins_path = tmp.path().join("skins_underscore");
    let base =
        start_test_gateway_with_skins_dir(skins_path.to_str().expect("valid utf8 path")).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/api/skins"))
        .json(&json!({
            "name": "valid_skin_name",
            "color": "#abcdef"
        }))
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), 400);
}

#[tokio::test]
#[serial]
async fn create_skin_with_dot_in_name_returns_400() {
    // Matches `foo.md` test in path_validation::tests — `.` is rejected
    // because a malicious caller could embed an extension that confuses
    // the on-disk layout (e.g., `evil.json.md` collisions).
    let tmp = tempfile::tempdir().expect("create temp dir");
    let skins_path = tmp.path().join("skins_dot");
    let base =
        start_test_gateway_with_skins_dir(skins_path.to_str().expect("valid utf8 path")).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/api/skins"))
        .json(&json!({
            "name": "foo.json",
            "color": "#000"
        }))
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), 400);
}
