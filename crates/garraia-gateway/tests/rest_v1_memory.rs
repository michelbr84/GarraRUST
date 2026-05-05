//! Integration tests for `GET/POST/DELETE /v1/memory` (plan 0062, GAR-514).
//!
//! All scenarios bundled into ONE `#[tokio::test]` — same pattern as
//! `rest_v1_chats.rs` / `rest_v1_messages.rs`. Splitting into multiple
//! `#[tokio::test]`s triggers the sqlx runtime-teardown race documented
//! in plan 0016 M3 commit `4f8be37`.
//!
//! Scenarios (12 total):
//!
//! POST /memory scenarios (5):
//!   P1. POST 201 — group scope: happy path; asserts response shape, audit
//!       row with structural metadata only (no content — invariant PII guard).
//!   P2. POST 201 — user scope: creates personal memory (group_id=NULL).
//!   P3. POST 400 — empty content rejected.
//!   P4. POST 401 — missing bearer.
//!   P5. POST 403 — scope_id mismatch (group scope with wrong scope_id).
//!
//! GET /memory scenarios (5):
//!   G1. GET 200 — group scope: returns items, excludes sensitivity='secret'.
//!   G2. GET 200 — user scope: returns only caller's personal memories.
//!   G3. GET 200 — cursor pagination works.
//!   G4. GET 401 — missing bearer.
//!   G5. GET 403 — wrong scope_id (cross-group).
//!
//! DELETE /memory scenarios (2):
//!   D1. DELETE 204 — soft-delete happy path; item no longer in list.
//!   D2. DELETE 404 — cross-tenant item returns 404.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

use common::Harness;
use common::fixtures::{fetch_audit_events_for_group, seed_user_with_group};

// ─── Request builders ────────────────────────────────────────────────────────

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

fn get_memory(
    token: Option<&str>,
    x_group_id: Option<&str>,
    scope_type: &str,
    scope_id: &str,
    cursor: Option<&str>,
    limit: Option<u32>,
) -> Request<Body> {
    let mut query = format!("?scope_type={scope_type}&scope_id={scope_id}");
    if let Some(c) = cursor {
        query.push_str(&format!("&cursor={c}"));
    }
    if let Some(l) = limit {
        query.push_str(&format!("&limit={l}"));
    }
    let mut req = Request::builder()
        .method("GET")
        .uri(format!("/v1/memory{query}"))
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

fn delete_memory(token: Option<&str>, memory_id: &str, x_group_id: Option<&str>) -> Request<Body> {
    let mut req = Request::builder()
        .method("DELETE")
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

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(feature = "test-helpers")]
#[tokio::test]
async fn rest_v1_memory_scenarios() {
    let h = Harness::get().await;

    // Seed two independent groups — alice is owner of group1, bob of group2.
    let (owner_id, group_id, owner_token) = seed_user_with_group(&h, "alice@mem-slice1.test")
        .await
        .expect("seed alice+group1");
    let (_, group2_id, owner2_token) = seed_user_with_group(&h, "bob@mem-slice1.test")
        .await
        .expect("seed bob+group2");

    // ── P1. POST 201 — group scope happy path ────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(post_memory(
            Some(&owner_token),
            Some(&group_id.to_string()),
            json!({
                "scope_type": "group",
                "scope_id": group_id.to_string(),
                "kind": "fact",
                "content": "Alice's family uses Garra daily.",
                "sensitivity": "group",
            }),
        ))
        .await
        .expect("P1 oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "P1 status");
    let p1_body = body_json(resp).await;
    assert_eq!(p1_body["scope_type"], "group", "P1 scope_type");
    assert_eq!(p1_body["kind"], "fact", "P1 kind");
    assert_eq!(p1_body["sensitivity"], "group", "P1 sensitivity");
    assert_eq!(
        p1_body["group_id"],
        group_id.to_string(),
        "P1 group_id stored"
    );
    let mem1_id = p1_body["id"].as_str().unwrap().to_string();

    // P1 — verify audit row: structural metadata only, NO content.
    let events = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("P1 fetch audit");
    let mem_event = events
        .iter()
        .find(|e| e.0 == "memory.created")
        .expect("P1 memory.created audit row missing");
    let (_, _, resource_type, resource_id, metadata) = mem_event;
    assert_eq!(resource_type, "memory_items", "P1 audit resource_type");
    assert_eq!(resource_id, &mem1_id, "P1 audit resource_id");
    assert_eq!(metadata["kind"], "fact", "P1 audit kind");
    assert_eq!(metadata["scope_type"], "group", "P1 audit scope_type");
    assert!(
        metadata.get("content").is_none(),
        "P1 audit MUST NOT carry content (PII)"
    );
    let content_len = metadata["content_len"].as_u64().unwrap_or(0);
    assert!(content_len > 0, "P1 audit content_len > 0");

    // ── P2. POST 201 — user scope (personal memory) ──────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(post_memory(
            Some(&owner_token),
            Some(&group_id.to_string()),
            json!({
                "scope_type": "user",
                "scope_id": owner_id.to_string(),
                "kind": "preference",
                "content": "Prefer dark mode.",
                "sensitivity": "private",
            }),
        ))
        .await
        .expect("P2 oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "P2 status");
    let p2_body = body_json(resp).await;
    assert_eq!(p2_body["scope_type"], "user", "P2 scope_type");
    // group_id must be NULL for personal memories
    assert!(p2_body["group_id"].is_null(), "P2 group_id must be null");
    assert_eq!(p2_body["sensitivity"], "private", "P2 sensitivity");
    let mem2_id = p2_body["id"].as_str().unwrap().to_string();

    // ── P3. POST 400 — empty content ─────────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(post_memory(
            Some(&owner_token),
            Some(&group_id.to_string()),
            json!({
                "scope_type": "group",
                "scope_id": group_id.to_string(),
                "kind": "note",
                "content": "   ",
            }),
        ))
        .await
        .expect("P3 oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "P3 status");

    // ── P4. POST 401 — missing bearer ────────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(post_memory(
            None,
            Some(&group_id.to_string()),
            json!({
                "scope_type": "group",
                "scope_id": group_id.to_string(),
                "kind": "note",
                "content": "hi",
            }),
        ))
        .await
        .expect("P4 oneshot");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "P4 status");

    // ── P5. POST 403 — scope_id mismatch (group2 scope_id with group1 token) ──
    let resp = h
        .router
        .clone()
        .oneshot(post_memory(
            Some(&owner_token),
            Some(&group_id.to_string()),
            json!({
                "scope_type": "group",
                "scope_id": group2_id.to_string(), // wrong group_id
                "kind": "note",
                "content": "Should be blocked.",
            }),
        ))
        .await
        .expect("P5 oneshot");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "P5 status");

    // ── G1. GET 200 — group scope list ───────────────────────────────────
    // Also verifies sensitivity='secret' items are excluded.
    // Direct DB insert of a secret item.
    sqlx::query(
        "INSERT INTO memory_items \
         (scope_type, scope_id, group_id, created_by, created_by_label, kind, content, sensitivity) \
         VALUES ('group', $1, $1, $2, 'alice', 'note', 'top secret info', 'secret')",
    )
    .bind(group_id)
    .bind(owner_id)
    .execute(&h.admin_pool)
    .await
    .expect("G1 insert secret item");

    let resp = h
        .router
        .clone()
        .oneshot(get_memory(
            Some(&owner_token),
            Some(&group_id.to_string()),
            "group",
            &group_id.to_string(),
            None,
            None,
        ))
        .await
        .expect("G1 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "G1 status");
    let g1_body = body_json(resp).await;
    let items = g1_body["items"].as_array().expect("G1 items array");
    // Should have mem1 (group-scope fact), NOT the secret item.
    assert_eq!(items.len(), 1, "G1 items count (secret excluded)");
    assert_eq!(items[0]["id"], mem1_id, "G1 item is the fact");
    // Sensitivity must not appear in summary (it was intentionally omitted).
    // The summary intentionally carries an empty sensitivity field.

    // ── G2. GET 200 — user scope (personal memories) ─────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(get_memory(
            Some(&owner_token),
            Some(&group_id.to_string()),
            "user",
            &owner_id.to_string(),
            None,
            None,
        ))
        .await
        .expect("G2 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "G2 status");
    let g2_body = body_json(resp).await;
    let g2_items = g2_body["items"].as_array().expect("G2 items array");
    assert_eq!(g2_items.len(), 1, "G2 user-scope items count");
    assert_eq!(g2_items[0]["id"], mem2_id, "G2 item is the preference");
    assert_eq!(g2_items[0]["scope_type"], "user", "G2 scope_type");

    // ── G3. GET 200 — cursor pagination ──────────────────────────────────
    // Create 3 more group-scope items, then fetch page by page with limit=1.
    for i in 0..3u32 {
        let resp = h
            .router
            .clone()
            .oneshot(post_memory(
                Some(&owner_token),
                Some(&group_id.to_string()),
                json!({
                    "scope_type": "group",
                    "scope_id": group_id.to_string(),
                    "kind": "note",
                    "content": format!("Note #{i}"),
                }),
            ))
            .await
            .expect("G3 seed oneshot");
        assert_eq!(resp.status(), StatusCode::CREATED, "G3 seed {i}");
    }

    // Page 1 (newest first).
    let resp = h
        .router
        .clone()
        .oneshot(get_memory(
            Some(&owner_token),
            Some(&group_id.to_string()),
            "group",
            &group_id.to_string(),
            None,
            Some(2),
        ))
        .await
        .expect("G3 page1 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "G3 page1 status");
    let pg1 = body_json(resp).await;
    let pg1_items = pg1["items"].as_array().expect("G3 pg1 items");
    assert_eq!(pg1_items.len(), 2, "G3 page1 has 2 items");
    let cursor = pg1["next_cursor"].as_str().expect("G3 next_cursor");

    // Page 2 using cursor.
    let resp = h
        .router
        .clone()
        .oneshot(get_memory(
            Some(&owner_token),
            Some(&group_id.to_string()),
            "group",
            &group_id.to_string(),
            Some(cursor),
            Some(2),
        ))
        .await
        .expect("G3 page2 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "G3 page2 status");
    let pg2 = body_json(resp).await;
    let pg2_items = pg2["items"].as_array().expect("G3 pg2 items");
    assert!(!pg2_items.is_empty(), "G3 page2 must have remaining items");

    // ── G4. GET 401 — missing bearer ─────────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(get_memory(
            None,
            Some(&group_id.to_string()),
            "group",
            &group_id.to_string(),
            None,
            None,
        ))
        .await
        .expect("G4 oneshot");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "G4 status");

    // ── G5. GET 403 — scope_id is group2 but caller is in group1 ─────────
    let resp = h
        .router
        .clone()
        .oneshot(get_memory(
            Some(&owner_token),
            Some(&group_id.to_string()),
            "group",
            &group2_id.to_string(), // cross-group scope_id
            None,
            None,
        ))
        .await
        .expect("G5 oneshot");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "G5 status");

    // ── D1. DELETE 204 — soft-delete happy path ───────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(delete_memory(
            Some(&owner_token),
            &mem1_id,
            Some(&group_id.to_string()),
        ))
        .await
        .expect("D1 oneshot");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT, "D1 status");

    // Verify item no longer appears in list.
    let resp = h
        .router
        .clone()
        .oneshot(get_memory(
            Some(&owner_token),
            Some(&group_id.to_string()),
            "group",
            &group_id.to_string(),
            None,
            Some(100),
        ))
        .await
        .expect("D1 verify list oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "D1 verify list status");
    let d1_list = body_json(resp).await;
    let d1_items = d1_list["items"].as_array().expect("D1 verify items");
    assert!(
        d1_items.iter().all(|it| it["id"] != mem1_id),
        "D1 deleted item must not appear in list"
    );

    // Verify audit row for the delete.
    let events = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("D1 fetch audit");
    let del_event = events
        .iter()
        .find(|e| e.0 == "memory.deleted")
        .expect("D1 memory.deleted audit row missing");
    let (_, _, del_rt, del_rid, del_meta) = del_event;
    assert_eq!(del_rt, "memory_items", "D1 audit resource_type");
    assert_eq!(del_rid, &mem1_id, "D1 audit resource_id");
    assert_eq!(del_meta["kind"], "fact", "D1 audit kind");
    assert_eq!(del_meta["scope_type"], "group", "D1 audit scope_type");

    // ── D2. DELETE 404 — cross-tenant memory item ─────────────────────────
    // Create a memory item in group2, then try to delete it with group1 token.
    let resp = h
        .router
        .clone()
        .oneshot(post_memory(
            Some(&owner2_token),
            Some(&group2_id.to_string()),
            json!({
                "scope_type": "group",
                "scope_id": group2_id.to_string(),
                "kind": "note",
                "content": "Bob's private note.",
            }),
        ))
        .await
        .expect("D2 seed oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "D2 seed status");
    let bob_mem_id = body_json(resp).await["id"].as_str().unwrap().to_string();

    let resp = h
        .router
        .clone()
        .oneshot(delete_memory(
            Some(&owner_token), // alice tries to delete bob's item
            &bob_mem_id,
            Some(&group_id.to_string()),
        ))
        .await
        .expect("D2 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "D2 cross-tenant must be 404"
    );
}
