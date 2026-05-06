//! Integration tests for `GET /v1/memory/{id}` + `PATCH /v1/memory/{id}`
//! (plan 0074, GAR-528, epic GAR-WS-MEMORY slice 3).
//!
//! All scenarios bundled into ONE `#[tokio::test]` — same pattern as
//! `rest_v1_memory.rs`. Splitting into multiple `#[tokio::test]`s triggers
//! the sqlx runtime-teardown race documented in plan 0016 M3.
//!
//! Scenarios (9 total):
//!
//! GET /memory/{id} scenarios (3):
//!   GI1. GET 200 — happy path: returns full content (not 200-char preview).
//!   GI2. GET 404 — item not found.
//!   GI3. GET 404 — cross-tenant (Eve cannot see Alice's item).
//!
//! PATCH /memory/{id} scenarios (6):
//!   PA1. PATCH 200 — update content: returns updated item.
//!   PA2. PATCH 200 — update kind + sensitivity.
//!   PA3. PATCH 200 — update TTL to a future date.
//!   PA4. PATCH 400 — empty body rejected.
//!   PA5. PATCH 404 — item not found.
//!   PA6. PATCH 404 — cross-tenant: Eve cannot patch Alice's item.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

use common::Harness;
use common::fixtures::seed_user_with_group;

// ─── Request builders ─────────────────────────────────────────────────────────

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("failed to collect response body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response body is not JSON")
}

fn post_memory(
    token: Option<&str>,
    x_group_id: Option<&str>,
    body: serde_json::Value,
) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri("/v1/memory")
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
    if let Some(g) = x_group_id {
        req.headers_mut().insert(
            HeaderName::from_static("x-group-id"),
            HeaderValue::from_str(g).unwrap(),
        );
    }
    req
}

fn get_memory_by_id(
    token: Option<&str>,
    memory_id: &str,
    x_group_id: Option<&str>,
) -> Request<Body> {
    let mut req = Request::builder()
        .method("GET")
        .uri(format!("/v1/memory/{memory_id}"))
        .body(Body::empty())
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
    if let Some(g) = x_group_id {
        req.headers_mut().insert(
            HeaderName::from_static("x-group-id"),
            HeaderValue::from_str(g).unwrap(),
        );
    }
    req
}

fn patch_memory_req(
    token: Option<&str>,
    memory_id: &str,
    x_group_id: Option<&str>,
    body: serde_json::Value,
) -> Request<Body> {
    let mut req = Request::builder()
        .method("PATCH")
        .uri(format!("/v1/memory/{memory_id}"))
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
    if let Some(g) = x_group_id {
        req.headers_mut().insert(
            HeaderName::from_static("x-group-id"),
            HeaderValue::from_str(g).unwrap(),
        );
    }
    req
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(feature = "test-helpers")]
#[tokio::test]
async fn memory_get_patch_scenarios() {
    let h = Harness::get().await;

    // Seed Alice (owner) and Eve — separate groups for cross-tenant tests.
    let (alice_user_id, alice_group_id, alice_token) =
        seed_user_with_group(&h, "alice_mgp@example.com")
            .await
            .expect("seed alice");
    let (_eve_user_id, eve_group_id, eve_token) = seed_user_with_group(&h, "eve_mgp@example.com")
        .await
        .expect("seed eve");

    let alice_group_str = alice_group_id.to_string();
    let eve_group_str = eve_group_id.to_string();

    // Seed a memory item with long content (>200 chars) to verify GET returns full content.
    let long_content = "A".repeat(500);
    let create_resp = h
        .router
        .clone()
        .oneshot(post_memory(
            Some(&alice_token),
            Some(&alice_group_str),
            json!({
                "scope_type": "group",
                "scope_id": alice_group_str,
                "kind": "fact",
                "content": long_content,
                "sensitivity": "group",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        create_resp.status(),
        StatusCode::CREATED,
        "setup: POST memory"
    );
    let created = body_json(create_resp).await;
    let memory_id = created["id"].as_str().unwrap().to_string();

    // ── GI1: GET 200 — happy path: full content returned ──────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(get_memory_by_id(
            Some(&alice_token),
            &memory_id,
            Some(&alice_group_str),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "GI1: expected 200");
    let body = body_json(resp).await;
    assert_eq!(body["id"], created["id"], "GI1: id matches");
    let returned_content = body["content"].as_str().unwrap();
    assert_eq!(
        returned_content.len(),
        500,
        "GI1: full content returned (not 200-char preview)"
    );
    assert_eq!(body["kind"], "fact", "GI1: kind correct");
    assert_eq!(
        body["sensitivity"], "group",
        "GI1: sensitivity present in GET response"
    );
    assert_eq!(body["scope_type"], "group", "GI1: scope_type correct");
    // Verify created_by matches Alice.
    assert_eq!(
        body["created_by"].as_str().unwrap(),
        alice_user_id.to_string(),
        "GI1: created_by matches alice"
    );

    // ── GI2: GET 404 — item not found ─────────────────────────────────────────
    let nonexistent_id = "00000000-0000-0000-0000-000000000001".to_string();
    let resp = h
        .router
        .clone()
        .oneshot(get_memory_by_id(
            Some(&alice_token),
            &nonexistent_id,
            Some(&alice_group_str),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "GI2: expected 404");

    // ── GI3: GET 404 — cross-tenant (Eve cannot see Alice's item) ─────────────
    let resp = h
        .router
        .clone()
        .oneshot(get_memory_by_id(
            Some(&eve_token),
            &memory_id,
            Some(&eve_group_str),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "GI3: Eve cannot see Alice's memory item"
    );

    // ── PA1: PATCH 200 — update content ───────────────────────────────────────
    let new_content = "Updated content from PA1 test scenario.";
    let resp = h
        .router
        .clone()
        .oneshot(patch_memory_req(
            Some(&alice_token),
            &memory_id,
            Some(&alice_group_str),
            json!({ "content": new_content }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "PA1: expected 200");
    let body = body_json(resp).await;
    assert_eq!(body["content"], new_content, "PA1: content updated");
    assert_eq!(body["kind"], "fact", "PA1: kind unchanged");
    assert_eq!(body["sensitivity"], "group", "PA1: sensitivity unchanged");

    // Verify GET returns the updated content.
    let resp = h
        .router
        .clone()
        .oneshot(get_memory_by_id(
            Some(&alice_token),
            &memory_id,
            Some(&alice_group_str),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "PA1: GET after PATCH is 200");
    let body = body_json(resp).await;
    assert_eq!(
        body["content"], new_content,
        "PA1: GET returns updated content"
    );

    // ── PA2: PATCH 200 — update kind + sensitivity ────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(patch_memory_req(
            Some(&alice_token),
            &memory_id,
            Some(&alice_group_str),
            json!({ "kind": "preference", "sensitivity": "private" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "PA2: expected 200");
    let body = body_json(resp).await;
    assert_eq!(body["kind"], "preference", "PA2: kind updated");
    assert_eq!(body["sensitivity"], "private", "PA2: sensitivity updated");
    assert_eq!(body["content"], new_content, "PA2: content unchanged");

    // ── PA3: PATCH 200 — set TTL to a future date ─────────────────────────────
    let future_ttl = "2099-12-31T23:59:59Z";
    let resp = h
        .router
        .clone()
        .oneshot(patch_memory_req(
            Some(&alice_token),
            &memory_id,
            Some(&alice_group_str),
            json!({ "ttl_expires_at": future_ttl }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "PA3: expected 200");
    let body = body_json(resp).await;
    assert!(
        body["ttl_expires_at"].as_str().is_some(),
        "PA3: ttl_expires_at set"
    );

    // ── PA4: PATCH 400 — empty body rejected ──────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(patch_memory_req(
            Some(&alice_token),
            &memory_id,
            Some(&alice_group_str),
            json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "PA4: empty body → 400"
    );

    // ── PA5: PATCH 404 — item not found ───────────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(patch_memory_req(
            Some(&alice_token),
            &nonexistent_id,
            Some(&alice_group_str),
            json!({ "content": "irrelevant" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "PA5: not found → 404");

    // ── PA6: PATCH 404 — cross-tenant: Eve cannot patch Alice's item ──────────
    let resp = h
        .router
        .clone()
        .oneshot(patch_memory_req(
            Some(&eve_token),
            &memory_id,
            Some(&eve_group_str),
            json!({ "content": "Eve should not be able to patch this" }),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "PA6: cross-tenant PATCH → 404"
    );
}
