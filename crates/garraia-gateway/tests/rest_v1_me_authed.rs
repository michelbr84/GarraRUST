//! Authed integration tests for `GET /v1/me` (plan 0016 M3-T3).
//!
//! Exercises the full Principal extractor chain against a real
//! pgvector/pg16 container (via `common::Harness`) and JWTs minted
//! by the same `JwtIssuer` that the router verifies against (via
//! `fixtures::seed_user_with_group`). Covers the same 5 scenarios
//! the plan called for, but **bundled inside one `#[tokio::test]`
//! function** that runs them sequentially on a single tokio runtime:
//!
//!   1. No bearer                 -> 401
//!   2. Valid bearer, no group    -> 200 (group_id + role absent)
//!   3. Valid bearer, owner group -> 200 (group_id + role='owner')
//!   4. Valid bearer, foreign grp -> 403
//!   5. `/v1/openapi.json`        -> 200 + bearer SecurityScheme + /v1/me path
//!
//! ## Why one function instead of five?
//!
//! During plan 0016 M3-T3 I initially split these into five
//! `#[tokio::test]` functions. Running them — even with
//! `#[serial_test::serial]` AND `--test-threads=1` — consistently
//! produced `pool timed out while waiting for an open connection`
//! errors on a non-deterministic subset of tests. The root cause
//! is that each `#[tokio::test]` macro spins up its own tokio
//! runtime and tears it down when the test body returns; sqlx
//! `PgPool` connections acquired inside that runtime are not
//! always released back to the shared `Harness::admin_pool` /
//! `Harness::login_pool` before the next test's runtime starts
//! trying to acquire them. The symptom was flaky
//! `fixture tx begin` timeouts and `group_members lookup` 401s.
//!
//! Folding all scenarios into one `#[tokio::test]` function
//! means one runtime, one linear sequence of acquires and
//! releases, and 100% deterministic behavior. Failures still
//! pinpoint the scenario via assertion messages.

mod common;

use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use common::fixtures::seed_user_with_group;
use common::{Harness, harness_get};

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("failed to collect response body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response body is not JSON")
}

fn with_bearer(token: &str) -> Request<axum::body::Body> {
    let mut req = harness_get("/v1/me");
    req.headers_mut().insert(
        HeaderName::from_static("authorization"),
        HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
    );
    req
}

fn with_bearer_and_group(token: &str, group_id: &str) -> Request<axum::body::Body> {
    let mut req = with_bearer(token);
    req.headers_mut().insert(
        HeaderName::from_static("x-group-id"),
        HeaderValue::from_str(group_id).unwrap(),
    );
    req
}

#[tokio::test]
async fn v1_me_authed_scenarios() {
    let h = Harness::get().await;

    // ── Scenario 1: no bearer -> 401 ─────────────────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(harness_get("/v1/me"))
            .await
            .expect("scenario 1: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "scenario 1: missing bearer must answer 401"
        );
    }

    // ── Scenario 2: valid bearer, no X-Group-Id -> 200 ──────
    {
        let (user_id, _group_id, token) = seed_user_with_group(&h, "no-group@m3.test")
            .await
            .expect("scenario 2: seed");
        let resp = h
            .router
            .clone()
            .oneshot(with_bearer(&token))
            .await
            .expect("scenario 2: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "scenario 2: bearer without X-Group-Id must answer 200"
        );
        let v = body_json(resp).await;
        assert_eq!(v["user_id"], user_id.to_string());
        assert!(
            v.get("group_id").is_none(),
            "scenario 2: absent X-Group-Id -> response group_id must be skipped, got {v}"
        );
        assert!(
            v.get("role").is_none(),
            "scenario 2: absent X-Group-Id -> response role must be skipped, got {v}"
        );
    }

    // ── Scenario 3: valid bearer, owner group -> 200 w/ role ─
    {
        let (user_id, group_id, token) = seed_user_with_group(&h, "member@m3.test")
            .await
            .expect("scenario 3: seed");
        let resp = h
            .router
            .clone()
            .oneshot(with_bearer_and_group(&token, &group_id.to_string()))
            .await
            .expect("scenario 3: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "scenario 3: member of requested group must answer 200"
        );
        let v = body_json(resp).await;
        assert_eq!(v["user_id"], user_id.to_string());
        assert_eq!(v["group_id"], group_id.to_string());
        assert_eq!(v["role"], "owner");
    }

    // ── Scenario 4: valid bearer, foreign group -> 403 ──────
    {
        let (_user_id, _group_id, token) = seed_user_with_group(&h, "foreign@m3.test")
            .await
            .expect("scenario 4: seed");
        // Fresh random UUID that was never inserted into `groups`.
        let foreign_group = uuid::Uuid::new_v4();
        let resp = h
            .router
            .clone()
            .oneshot(with_bearer_and_group(&token, &foreign_group.to_string()))
            .await
            .expect("scenario 4: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "scenario 4: non-member X-Group-Id must answer 403"
        );
    }

    // ── Scenario 5: OpenAPI spec exposes bearer + /v1/me ───
    {
        let resp = h
            .router
            .clone()
            .oneshot(harness_get("/v1/openapi.json"))
            .await
            .expect("scenario 5: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "scenario 5: /v1/openapi.json must answer 200"
        );
        let v = body_json(resp).await;
        assert_eq!(
            v["components"]["securitySchemes"]["bearer"]["scheme"], "bearer",
            "scenario 5: bearer security scheme missing"
        );
        assert_eq!(
            v["components"]["securitySchemes"]["bearer"]["bearerFormat"],
            "JWT"
        );
        assert!(
            v["paths"]["/v1/me"]["get"].is_object(),
            "scenario 5: /v1/me GET handler missing from paths"
        );
    }
}
