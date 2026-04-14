//! Smoke test for the gateway integration harness (plan 0016 M2-T4).
//!
//! Validates the whole stack end-to-end over a real testcontainer
//! pgvector/pg16 + migrations 001..010 + typed pools + prebuilt
//! Router, by doing one `GET /v1/openapi.json` oneshot call and
//! asserting a 200 with a parseable OpenAPI body.
//!
//! This is deliberately the ONLY test in M2. Authed `/v1/me` paths
//! (401/200/403), seed fixtures, and Swagger `SecurityScheme::Bearer`
//! wire checks all land in M3 once `fixtures::seed_user_with_group`
//! is added.

mod common;

use axum::http::StatusCode;
use http_body_util::BodyExt;
use tower::ServiceExt;

use common::{harness_get, Harness};

#[tokio::test]
async fn harness_boots_and_router_responds_to_openapi_json() {
    let h = Harness::get().await;

    let resp = h
        .router
        .clone()
        .oneshot(harness_get("/v1/openapi.json"))
        .await
        .expect("oneshot should succeed");

    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    if status != StatusCode::OK {
        let body_str = String::from_utf8_lossy(&body);
        panic!("openapi.json expected 200, got {status}. body: {body_str}");
    }
    let v: serde_json::Value = serde_json::from_slice(&body).expect("openapi body is JSON");
    assert_eq!(
        v["info"]["title"], "GarraIA REST /v1",
        "unexpected OpenAPI info.title — router wiring regression?"
    );
    assert_eq!(v["info"]["version"], "0.1.0");
    // Proves `GET /v1/me` is wired under full state (not 503 stub).
    assert!(
        v["paths"]["/v1/me"]["get"].is_object(),
        "/v1/me must be listed in the OpenAPI paths"
    );
}
