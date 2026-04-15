//! Authed integration tests for `GET /v1/me` (plan 0016 M3-T3).
//!
//! Exercises the full Principal extractor chain against a real
//! pgvector/pg16 container (via `common::Harness`) and a JWT minted
//! by the same `JwtIssuer` that the router verifies against (via
//! `fixtures::seed_user_with_group`). Covers:
//!
//!   1. No bearer                 -> 401
//!   2. Valid bearer, no group    -> 200 (group_id + role absent)
//!   3. Valid bearer, owner group -> 200 (group_id + role='owner')
//!   4. Valid bearer, foreign grp -> 403
//!   5. `/v1/openapi.json`        -> 200 + bearer SecurityScheme + /v1/me path
//!
//! Separate from `rest_v1_me.rs` on purpose: that file contains the
//! env-mutating fail-soft test guarded by `#[serial]`, and mixing it
//! with these non-mutating tests would force all of them to
//! serialize. This file is pure read-only oneshot — no `#[serial]`.

mod common;

use axum::http::{HeaderName, HeaderValue, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use common::fixtures::seed_user_with_group;
use common::{harness_get, Harness};

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("failed to collect response body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response body is not JSON")
}

#[tokio::test]
async fn get_v1_me_without_bearer_returns_401() {
    let h = Harness::get().await;
    let resp = h
        .router
        .clone()
        .oneshot(harness_get("/v1/me"))
        .await
        .expect("oneshot should succeed");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn get_v1_me_with_valid_bearer_no_group_returns_200() {
    let h = Harness::get().await;
    let (user_id, _group_id, token) =
        seed_user_with_group(&h, "no-group@m3.test").await.expect("seed");

    let mut req = harness_get("/v1/me");
    req.headers_mut().insert(
        HeaderName::from_static("authorization"),
        HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
    );

    let resp = h.router.clone().oneshot(req).await.expect("oneshot");
    assert_eq!(resp.status(), StatusCode::OK);

    let v = body_json(resp).await;
    assert_eq!(v["user_id"], user_id.to_string());
    assert!(
        v.get("group_id").is_none(),
        "absent X-Group-Id -> group_id must be skipped in response, got {v}"
    );
    assert!(
        v.get("role").is_none(),
        "absent X-Group-Id -> role must be skipped in response, got {v}"
    );
}

#[tokio::test]
async fn get_v1_me_with_valid_bearer_and_member_group_returns_200_with_role() {
    let h = Harness::get().await;
    let (user_id, group_id, token) =
        seed_user_with_group(&h, "member@m3.test").await.expect("seed");

    let mut req = harness_get("/v1/me");
    req.headers_mut().insert(
        HeaderName::from_static("authorization"),
        HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
    );
    req.headers_mut().insert(
        HeaderName::from_static("x-group-id"),
        HeaderValue::from_str(&group_id.to_string()).unwrap(),
    );

    let resp = h.router.clone().oneshot(req).await.expect("oneshot");
    assert_eq!(resp.status(), StatusCode::OK);

    let v = body_json(resp).await;
    assert_eq!(v["user_id"], user_id.to_string());
    assert_eq!(v["group_id"], group_id.to_string());
    assert_eq!(v["role"], "owner");
}

#[tokio::test]
async fn get_v1_me_with_valid_bearer_non_member_group_returns_403() {
    let h = Harness::get().await;
    let (_user_id, _group_id, token) =
        seed_user_with_group(&h, "foreign@m3.test").await.expect("seed");

    // Fresh random UUID that was never inserted into groups — the
    // Principal extractor should fail the membership lookup and
    // return 403 Forbidden.
    let foreign_group = uuid::Uuid::new_v4();

    let mut req = harness_get("/v1/me");
    req.headers_mut().insert(
        HeaderName::from_static("authorization"),
        HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
    );
    req.headers_mut().insert(
        HeaderName::from_static("x-group-id"),
        HeaderValue::from_str(&foreign_group.to_string()).unwrap(),
    );

    let resp = h.router.clone().oneshot(req).await.expect("oneshot");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn openapi_spec_exposes_bearer_security_scheme_and_get_me_path() {
    let h = Harness::get().await;
    let resp = h
        .router
        .clone()
        .oneshot(harness_get("/v1/openapi.json"))
        .await
        .expect("oneshot");
    assert_eq!(resp.status(), StatusCode::OK);

    let v = body_json(resp).await;
    // Plan 0016 M3-T1: SecurityAddon registers a bearer HTTP scheme.
    assert_eq!(
        v["components"]["securitySchemes"]["bearer"]["scheme"], "bearer",
        "bearer security scheme missing from OpenAPI components, got {v}"
    );
    assert_eq!(
        v["components"]["securitySchemes"]["bearer"]["bearerFormat"], "JWT",
        "bearer scheme is present but bearerFormat != JWT"
    );
    // Plan 0015 M4: /v1/me is reachable under full router state.
    assert!(
        v["paths"]["/v1/me"]["get"].is_object(),
        "/v1/me GET handler missing from OpenAPI paths"
    );
}
