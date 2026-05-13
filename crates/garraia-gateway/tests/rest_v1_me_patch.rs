//! Integration tests for `PATCH /v1/me` (plan 0110 T8, GAR-599).
//!
//! All scenarios bundled into ONE `#[tokio::test]` — same pattern as
//! `rest_v1_me_authed.rs`. Splitting triggers the sqlx runtime-teardown race.
//!
//! Scenarios (5):
//!   ME-PATCH-1. 200 success — updates display_name, verifies field returned.
//!   ME-PATCH-2. 200 no-op — empty body `{}`, returns current data without error.
//!   ME-PATCH-3. 422 display_name too long (129 chars).
//!   ME-PATCH-4. 422 unknown field in body (deny_unknown_fields).
//!   ME-PATCH-5. 401 no JWT.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

use common::Harness;
use common::fixtures::seed_user_with_group;

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("collect response body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response body is not JSON")
}

fn patch_me_req(token: Option<&str>, body: serde_json::Value) -> Request<Body> {
    let mut req = Request::builder()
        .method("PATCH")
        .uri("/v1/me")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request builder");
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
            "127.0.0.1:1".parse().unwrap(),
        ));
    if let Some(t) = token {
        req.headers_mut().insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_str(&format!("Bearer {t}")).unwrap(),
        );
    }
    req
}

#[cfg(feature = "test-helpers")]
#[tokio::test]
async fn rest_v1_me_patch_scenarios() {
    let h = Harness::get().await;

    let (user_id, _group_id, token) = seed_user_with_group(&h, "alice@me-patch.test")
        .await
        .expect("seed alice");

    // ── ME-PATCH-1: 200 success — updates display_name ──────────────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(patch_me_req(
                Some(&token),
                json!({ "display_name": "Alice Updated" }),
            ))
            .await
            .expect("ME-PATCH-1 oneshot");
        assert_eq!(resp.status(), StatusCode::OK, "ME-PATCH-1 status");
        let v = body_json(resp).await;
        assert_eq!(
            v["user_id"],
            user_id.to_string(),
            "ME-PATCH-1 user_id matches"
        );
        assert_eq!(
            v["display_name"], "Alice Updated",
            "ME-PATCH-1 display_name updated"
        );
        assert!(
            v.get("email").is_some(),
            "ME-PATCH-1 email present in response"
        );
        assert!(
            v.get("created_at").is_some(),
            "ME-PATCH-1 created_at present"
        );
        assert!(
            v.get("updated_at").is_some(),
            "ME-PATCH-1 updated_at present"
        );
    }

    // ── ME-PATCH-2: 200 no-op — empty body `{}` ─────────────────────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(patch_me_req(Some(&token), json!({})))
            .await
            .expect("ME-PATCH-2 oneshot");
        assert_eq!(resp.status(), StatusCode::OK, "ME-PATCH-2 status");
        let v = body_json(resp).await;
        // display_name should still be the one set in ME-PATCH-1
        assert_eq!(
            v["display_name"], "Alice Updated",
            "ME-PATCH-2 display_name unchanged"
        );
    }

    // ── ME-PATCH-3: 422 display_name too long (129 chars) ───────────────────
    {
        let long_name = "x".repeat(129);
        let resp = h
            .router
            .clone()
            .oneshot(patch_me_req(
                Some(&token),
                json!({ "display_name": long_name }),
            ))
            .await
            .expect("ME-PATCH-3 oneshot");
        // Validation returns 400 from our handler validate() path
        assert!(
            resp.status() == StatusCode::BAD_REQUEST
                || resp.status() == StatusCode::UNPROCESSABLE_ENTITY,
            "ME-PATCH-3 too-long name must return 400 or 422, got {}",
            resp.status()
        );
    }

    // ── ME-PATCH-4: 422 unknown field ────────────────────────────────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(patch_me_req(
                Some(&token),
                json!({ "display_name": "Alice", "status": "deleted" }),
            ))
            .await
            .expect("ME-PATCH-4 oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "ME-PATCH-4 unknown field must return 422"
        );
    }

    // ── ME-PATCH-5: 401 no JWT ───────────────────────────────────────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(patch_me_req(None, json!({ "display_name": "No auth" })))
            .await
            .expect("ME-PATCH-5 oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "ME-PATCH-5 missing JWT must return 401"
        );
    }
}
