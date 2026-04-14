//! Fail-soft integration test for `GET /v1/me` (plan 0015, Task 6, Opção 2+3).
//!
//! Boots the gateway with `AppConfig::default()` and NO auth env vars set,
//! so `AuthConfig::from_env` returns `None` and every `/v1` route answers
//! 503 Problem Details via `RestError::AuthUnconfigured`. This exercises
//! the full wiring: `router.rs` merge → `rest_v1::router` fail-soft branch
//! → `unconfigured_handler` → `RestError::IntoResponse` → on-the-wire HTTP.
//!
//! The authenticated happy path (200 + JWT + real Postgres via
//! testcontainers) is intentionally NOT covered here. It lands in a
//! follow-up slice together with the first REST write handler
//! (`POST /v1/groups`), where a shared test harness is justified.

use std::net::TcpListener;
use std::time::Duration;

use garraia_config::AppConfig;
use garraia_gateway::GatewayServer;

fn random_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind to random port");
    listener.local_addr().unwrap().port()
}

/// Strip auth env vars that might leak from the dev shell so the gateway
/// reliably boots in fail-soft mode. `unsafe` is required on Edition 2024
/// because `std::env::remove_var` is marked unsafe for multi-threaded
/// programs — acceptable here because this runs before the server task
/// is spawned, so no other thread touches the env block.
fn clear_auth_env() {
    unsafe {
        std::env::remove_var("GARRAIA_JWT_SECRET");
        std::env::remove_var("GARRAIA_REFRESH_HMAC_SECRET");
        std::env::remove_var("GARRAIA_LOGIN_DATABASE_URL");
        std::env::remove_var("GARRAIA_SIGNUP_DATABASE_URL");
    }
}

#[tokio::test]
async fn get_v1_me_fails_soft_with_503_problem_details_when_auth_unconfigured() {
    clear_auth_env();

    let port = random_port();
    let mut config = AppConfig::default();
    config.gateway.port = port;
    config.memory.enabled = false;

    tokio::spawn(async move {
        let server = GatewayServer::new(config);
        let _ = server.run().await;
    });

    // Wait for the listener to actually bind. Gateway bootstrap pulls
    // in channels, MCP registry and tools, so 500ms is not enough — poll
    // the port with exponential backoff up to ~10s.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("reqwest client");
    let url = format!("http://127.0.0.1:{port}/v1/me");
    let mut attempt = 0u32;
    let resp = loop {
        match client.get(&url).send().await {
            Ok(resp) => break resp,
            Err(err) if attempt < 40 => {
                attempt += 1;
                tokio::time::sleep(Duration::from_millis(250)).await;
                let _ = err;
                continue;
            }
            Err(err) => panic!("gateway never came up: {err}"),
        }
    };

    assert_eq!(
        resp.status().as_u16(),
        503,
        "unconfigured gateway must answer /v1/me with 503"
    );
    assert_eq!(
        resp.headers()
            .get("content-type")
            .expect("content-type header present"),
        "application/problem+json",
        "RFC 9457 content-type missing",
    );
    let v: serde_json::Value = resp.json().await.expect("body is JSON");
    assert_eq!(v["type"], "about:blank", "RFC 9457 default type URI");
    assert_eq!(v["title"], "Service Unavailable");
    assert_eq!(v["status"], 503);
    assert!(
        v["detail"].is_string(),
        "Problem Details body must include a detail string"
    );
}
