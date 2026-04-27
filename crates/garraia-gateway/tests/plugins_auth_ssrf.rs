//! Integration tests for `/api/plugins/*` admin auth gating
//! (GAR-459 / PR-A of GAR-454).
//!
//! Unit-level coverage of `is_blocked_ip`, `host_in_allowlist`, and the
//! `download_and_validate_manifest` reject paths (HTTPS-only, empty
//! allowlist, malformed URL) lives next to the helpers in
//! `crates/garraia-gateway/src/plugins_handler.rs::tests`. This file
//! covers the router-level gate: anonymous requests without an admin
//! session cookie MUST receive 401 from every plugin route, before the
//! handler logic runs.
//!
//! Why integration over unit: `require_admin_auth` is wired via
//! `axum::middleware::from_fn` on the sub-router, so the only honest
//! way to verify "no cookie → 401" is to drive the router through Tower
//! and inspect the response status — exactly mirroring how production
//! traffic flows.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use garraia_agents::AgentRuntime;
use garraia_channels::ChannelRegistry;
use garraia_config::AppConfig;
use garraia_gateway::admin::store::AdminStore;
use garraia_gateway::plugins_handler::build_plugin_routes;
use garraia_gateway::state::AppState;
use http_body_util::BodyExt;
use tokio::sync::Mutex;
use tower::ServiceExt;

fn build_test_router() -> Router {
    let state = Arc::new(AppState::new(
        AppConfig::default(),
        AgentRuntime::new(),
        ChannelRegistry::new(),
    ));
    let admin_store = Arc::new(Mutex::new(
        AdminStore::in_memory().expect("in-memory admin store"),
    ));
    build_plugin_routes(state, admin_store)
}

async fn unauthenticated_request(method: &str, uri: &str, body: Body) -> StatusCode {
    let req = Request::builder()
        .uri(uri)
        .method(method)
        .header("content-type", "application/json")
        .body(body)
        .expect("request build");
    let router = build_test_router();
    let resp = router.oneshot(req).await.expect("oneshot");
    let status = resp.status();
    // Drain the body so the connection completes cleanly even if test
    // assertions fail later — the body is irrelevant for the 401 contract.
    let _ = resp.into_body().collect().await;
    status
}

#[tokio::test]
async fn install_without_cookie_returns_401() {
    let body = Body::from(r#"{"url":"https://plugins.example.com/m.json"}"#);
    let status = unauthenticated_request("POST", "/api/plugins/install", body).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "POST /api/plugins/install must reject anonymous callers"
    );
}

#[tokio::test]
async fn list_without_cookie_returns_401() {
    let status = unauthenticated_request("GET", "/api/plugins", Body::empty()).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "GET /api/plugins must reject anonymous callers"
    );
}

#[tokio::test]
async fn get_by_id_without_cookie_returns_401() {
    let status = unauthenticated_request("GET", "/api/plugins/some-id", Body::empty()).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "GET /api/plugins/{{id}} must reject anonymous callers"
    );
}

#[tokio::test]
async fn delete_without_cookie_returns_401() {
    let status = unauthenticated_request("DELETE", "/api/plugins/some-id", Body::empty()).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "DELETE /api/plugins/{{id}} must reject anonymous callers"
    );
}

#[tokio::test]
async fn toggle_without_cookie_returns_401() {
    let body = Body::from(r#"{"enabled":true}"#);
    let status = unauthenticated_request("POST", "/api/plugins/some-id/toggle", body).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "POST /api/plugins/{{id}}/toggle must reject anonymous callers"
    );
}

#[tokio::test]
async fn install_with_garbage_cookie_returns_401() {
    // A cookie with an unknown session token must still be rejected — the
    // middleware validates against the AdminStore, not just presence.
    let req = Request::builder()
        .uri("/api/plugins/install")
        .method("POST")
        .header("content-type", "application/json")
        .header("cookie", "garraia_admin_session=not-a-valid-token")
        .body(Body::from(
            r#"{"url":"https://plugins.example.com/m.json"}"#,
        ))
        .expect("request build");
    let router = build_test_router();
    let resp = router.oneshot(req).await.expect("oneshot");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
