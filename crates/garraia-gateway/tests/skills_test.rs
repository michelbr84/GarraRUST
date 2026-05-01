//! Integration tests for the Skills API (Phase 3.3).
//!
//! GAR-490 PR A — exercises the centralized `validate_skill_name` helper
//! against the real Axum router for every handler that takes a name from
//! a path or body parameter. Each negative case in the unit-test matrix
//! (`crates/garraia-gateway/src/path_validation.rs`) is mirrored here for
//! the handler that exposes it to the network.
//!
//! These tests do not require the `test-helpers` feature — they boot a
//! real `GatewayServer` and use its public HTTP surface, mirroring the
//! pattern already used in `skins_test.rs`.

use std::net::TcpListener;

use garraia_config::AppConfig;
use garraia_gateway::GatewayServer;
use serde_json::json;
use serial_test::serial;

fn random_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind to random port");
    listener.local_addr().unwrap().port()
}

/// Boot a gateway whose `default_config_dir()` (and therefore `skills_dir()`)
/// points at a fresh temp directory. The skill files are written directly
/// to disk and isolated per-test by the caller's `tempfile::TempDir`.
async fn start_test_gateway_with_config_dir(config_dir: &str) -> String {
    let port = random_port();
    let mut config = AppConfig::default();
    config.gateway.port = port;
    config.memory.enabled = false;
    config.mcp.clear();

    // SAFETY: we are in a #[tokio::test] guarded by `#[serial]`; no other
    // thread is reading these env vars concurrently.
    unsafe {
        std::env::set_var("GARRAIA_CONFIG_DIR", config_dir);
    }

    tokio::spawn(async move {
        let server = GatewayServer::new(config);
        let _ = server.run().await;
    });

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

/// Minimal valid CreateSkillRequest payload — used by the positive cases.
fn valid_skill_payload(name: &str) -> serde_json::Value {
    json!({
        "name": name,
        "description": "test skill for GAR-490 PR A",
        "body": "Test body content.",
    })
}

// ── Positive: CRUD lifecycle through the centralized validator ──────────────

#[tokio::test]
#[serial]
async fn skill_crud_lifecycle() {
    // End-to-end CRUD using a name that satisfies both this PR's helper
    // (`[A-Za-z0-9-]+`) and the downstream `garraia_skills::validate_skill`
    // convention. See `path_validation.rs` for the alignment rationale.
    let tmp = tempfile::tempdir().expect("create temp dir");
    let base =
        start_test_gateway_with_config_dir(tmp.path().to_str().expect("valid utf8 path")).await;
    let client = reqwest::Client::new();

    // Create
    let create = client
        .post(format!("{base}/api/skills"))
        .json(&valid_skill_payload("valid-skill"))
        .send()
        .await
        .expect("create");
    assert_eq!(create.status(), 201);

    // Get
    let get = client
        .get(format!("{base}/api/skills/valid-skill"))
        .send()
        .await
        .expect("get");
    assert_eq!(get.status(), 200);

    // Update — same handler, body.name must also be valid.
    let update = client
        .put(format!("{base}/api/skills/valid-skill"))
        .json(&valid_skill_payload("valid-skill"))
        .send()
        .await
        .expect("update");
    assert_eq!(update.status(), 200);

    // Delete
    let delete = client
        .delete(format!("{base}/api/skills/valid-skill"))
        .send()
        .await
        .expect("delete");
    assert_eq!(delete.status(), 200);

    // Confirm deletion
    let gone = client
        .get(format!("{base}/api/skills/valid-skill"))
        .send()
        .await
        .expect("gone");
    assert_eq!(gone.status(), 404);
}

#[tokio::test]
#[serial]
async fn create_skill_rejects_underscore_per_project_convention() {
    // The downstream rule in `garraia_skills::validate_skill` rejects
    // underscores. The helper aligns: underscore at the path layer
    // returns 400 immediately rather than 400 later from the body
    // parser. This test pins the convention so a future loosening of
    // either layer is caught.
    let tmp = tempfile::tempdir().expect("create temp dir");
    let base =
        start_test_gateway_with_config_dir(tmp.path().to_str().expect("valid utf8 path")).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/api/skills"))
        .json(&valid_skill_payload("valid_skill"))
        .send()
        .await
        .expect("request");
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
#[serial]
async fn create_skill_accepts_hyphen_and_digits() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let base =
        start_test_gateway_with_config_dir(tmp.path().to_str().expect("valid utf8 path")).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/api/skills"))
        .json(&valid_skill_payload("valid-skill-123"))
        .send()
        .await
        .expect("create");
    assert_eq!(resp.status(), 201);
}

// ── Negative: matrix-by-handler ─────────────────────────────────────────────
// The shape of these tests is intentionally repetitive — one rejection
// per (handler × attack vector) combination — because each row maps to a
// CodeQL `rust/path-injection` alert that this PR closes. Compactness via
// macros would obscure that mapping.

#[tokio::test]
#[serial]
async fn create_skill_rejects_path_traversal() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let base =
        start_test_gateway_with_config_dir(tmp.path().to_str().expect("valid utf8 path")).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/api/skills"))
        .json(&valid_skill_payload("../evil"))
        .send()
        .await
        .expect("request");
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
#[serial]
async fn create_skill_rejects_empty_name() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let base =
        start_test_gateway_with_config_dir(tmp.path().to_str().expect("valid utf8 path")).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/api/skills"))
        .json(&valid_skill_payload(""))
        .send()
        .await
        .expect("request");
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
#[serial]
async fn create_skill_rejects_nul_byte() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let base =
        start_test_gateway_with_config_dir(tmp.path().to_str().expect("valid utf8 path")).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/api/skills"))
        .json(&valid_skill_payload("abc\0def"))
        .send()
        .await
        .expect("request");
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
#[serial]
async fn create_skill_rejects_windows_drive() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let base =
        start_test_gateway_with_config_dir(tmp.path().to_str().expect("valid utf8 path")).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/api/skills"))
        .json(&valid_skill_payload("C:foo"))
        .send()
        .await
        .expect("request");
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
#[serial]
async fn get_skill_rejects_path_traversal() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let base =
        start_test_gateway_with_config_dir(tmp.path().to_str().expect("valid utf8 path")).await;
    let client = reqwest::Client::new();

    // %2E%2E%2F = "../"
    let resp = client
        .get(format!("{base}/api/skills/%2E%2E%2Fetc"))
        .send()
        .await
        .expect("request");
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
#[serial]
async fn update_skill_rejects_dot_in_name() {
    // Using `evil.md` instead of literal `..` because clients (and matchit)
    // collapse `..` segments before they reach the handler. The dot is
    // sufficient: the helper rejects any name containing `.`.
    let tmp = tempfile::tempdir().expect("create temp dir");
    let base =
        start_test_gateway_with_config_dir(tmp.path().to_str().expect("valid utf8 path")).await;
    let client = reqwest::Client::new();

    let resp = client
        .put(format!("{base}/api/skills/evil.md"))
        .json(&valid_skill_payload("anything"))
        .send()
        .await
        .expect("request");
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
#[serial]
async fn delete_skill_rejects_dot_in_name() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let base =
        start_test_gateway_with_config_dir(tmp.path().to_str().expect("valid utf8 path")).await;
    let client = reqwest::Client::new();

    let resp = client
        .delete(format!("{base}/api/skills/evil.md"))
        .send()
        .await
        .expect("request");
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
#[serial]
async fn export_skill_rejects_dot_in_name() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let base =
        start_test_gateway_with_config_dir(tmp.path().to_str().expect("valid utf8 path")).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{base}/api/skills/evil.md/export"))
        .send()
        .await
        .expect("request");
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
#[serial]
async fn import_skill_with_malicious_frontmatter_name_returns_400() {
    // Defense-in-depth coverage flagged by the security audit:
    // `import_skill` parses the YAML frontmatter and was using
    // `skill.frontmatter.name` directly in `format!("{}.md", ...)` —
    // CodeQL did not flag this because the value transits through
    // `parse_skill`, but the path-injection vector was real.
    let tmp = tempfile::tempdir().expect("create temp dir");
    let base =
        start_test_gateway_with_config_dir(tmp.path().to_str().expect("valid utf8 path")).await;
    let client = reqwest::Client::new();

    // Frontmatter `name` violates the helper (path traversal segment)
    // even though the YAML itself parses cleanly.
    let malicious_content = "---\nname: ../evil\ndescription: looks ok\n---\n\nbody.\n";

    let resp = client
        .post(format!("{base}/api/skills/import"))
        .json(&json!({ "content": malicious_content }))
        .send()
        .await
        .expect("request");
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
#[serial]
async fn set_skill_triggers_rejects_dot_in_name() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let base =
        start_test_gateway_with_config_dir(tmp.path().to_str().expect("valid utf8 path")).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/api/skills/evil.md/triggers"))
        .json(&json!({ "triggers": [] }))
        .send()
        .await
        .expect("request");
    assert_eq!(resp.status(), 400);
}
