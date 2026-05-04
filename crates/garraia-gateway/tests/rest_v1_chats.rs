//! Integration tests for `/v1/groups/{group_id}/chats` (plan 0054, GAR-506).
//!
//! All scenarios bundled into ONE `#[tokio::test]` function — same pattern
//! as `rest_v1_groups.rs`/`rest_v1_invites.rs`. Splitting into multiple
//! `#[tokio::test]`s historically triggered the sqlx runtime-teardown race
//! documented in plan 0016 M3 commit `4f8be37`.
//!
//! Scenarios covered (10 total):
//!
//! POST scenarios (5):
//!   C1. POST 201 — happy path: owner creates a 'channel'; asserts
//!       response shape, `chats` row, `chat_members[owner]`, and
//!       `audit_events` row with action=`chat.created` + structural-only
//!       metadata (no name/topic literal — invariant 7).
//!   C2. POST 400 — type='dm' rejected with the slice-2 deferral message.
//!   C3. POST 400 — empty name (after trim).
//!   C4. POST 401 — missing bearer.
//!   C5. POST 400 — `X-Group-Id` missing.
//!
//! GET scenarios (5):
//!   G1. GET 200 — happy path: 2 channels returned newest-first.
//!   G2. GET 200 — archived chat (manually flipped via admin pool) is
//!       NOT in the response, proving the `archived_at IS NULL` filter.
//!   G3. GET 400 — `X-Group-Id` mismatch.
//!   G4. GET 401 — missing bearer.
//!   G5. GET 200 — empty group returns `items: []`.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

use common::Harness;
use common::fixtures::{fetch_audit_events_for_group, seed_user_with_group};

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("failed to collect response body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response body is not JSON")
}

fn post_chat(
    token: Option<&str>,
    group_path: &str,
    x_group_id: Option<&str>,
    body: serde_json::Value,
) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri(format!("/v1/groups/{group_path}/chats"))
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

fn get_chats(
    token: Option<&str>,
    group_path: &str,
    x_group_id: Option<&str>,
) -> Request<Body> {
    let mut req = Request::builder()
        .method("GET")
        .uri(format!("/v1/groups/{group_path}/chats"))
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

#[tokio::test]
async fn v1_chats_scenarios() {
    let h = Harness::get().await;

    // Seed owner of group A used by C1..C5 + G1..G3.
    let (owner_id, group_id, owner_token) =
        seed_user_with_group(&h, "owner@chats-slice1.test")
            .await
            .expect("seed owner+group");

    // ── C1. POST 201 happy path ─────────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(post_chat(
            Some(&owner_token),
            &group_id.to_string(),
            Some(&group_id.to_string()),
            json!({"name": "general", "type": "channel", "topic": "team-wide"}),
        ))
        .await
        .expect("C1 oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "C1 status");
    let v = body_json(resp).await;
    assert_eq!(v["name"], "general", "C1 name");
    assert_eq!(v["type"], "channel", "C1 type");
    assert_eq!(v["topic"], "team-wide", "C1 topic");
    assert_eq!(v["created_by"], owner_id.to_string(), "C1 created_by");
    assert_eq!(v["group_id"], group_id.to_string(), "C1 group_id");
    let chat_id_str = v["id"].as_str().unwrap().to_string();
    let chat_id: uuid::Uuid = chat_id_str.parse().expect("C1 chat_id parses");

    // C1 — verify chat_members owner row exists in same tx.
    let (cm_role,): (String,) = sqlx::query_as(
        "SELECT role FROM chat_members WHERE chat_id = $1 AND user_id = $2",
    )
    .bind(chat_id)
    .bind(owner_id)
    .fetch_one(&h.admin_pool)
    .await
    .expect("C1 chat_members row");
    assert_eq!(cm_role, "owner", "C1 chat_members.role = owner");

    // C1 — verify audit row exists with structural metadata only (no PII).
    let events = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("C1 fetch audit");
    let chat_event = events
        .iter()
        .find(|(action, _, _, _, _)| action == "chat.created")
        .expect("C1 chat.created audit row");
    let (action, actor, resource_type, resource_id, metadata) = chat_event;
    assert_eq!(action, "chat.created", "C1 audit action");
    assert_eq!(actor.as_ref(), Some(&owner_id), "C1 audit actor");
    assert_eq!(resource_type, "chats", "C1 audit resource_type");
    assert_eq!(resource_id, &chat_id_str, "C1 audit resource_id");
    assert_eq!(metadata["name_len"], 7, "C1 audit name_len = chars in 'general'");
    assert_eq!(metadata["type"], "channel", "C1 audit type");
    assert_eq!(metadata["has_topic"], true, "C1 audit has_topic");
    assert!(
        metadata.get("name").is_none(),
        "C1 audit must NOT carry chat name (PII invariant 7)"
    );
    assert!(
        metadata.get("topic").is_none(),
        "C1 audit must NOT carry chat topic (PII invariant 7)"
    );

    // ── C2. POST 400 type='dm' rejected ─────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(post_chat(
            Some(&owner_token),
            &group_id.to_string(),
            Some(&group_id.to_string()),
            json!({"name": "dm-attempt", "type": "dm"}),
        ))
        .await
        .expect("C2 oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "C2 status");
    let v = body_json(resp).await;
    let detail = v["detail"].as_str().unwrap_or_default();
    assert!(
        detail.contains("'dm' is not yet supported"),
        "C2 detail message: {detail}"
    );

    // ── C3. POST 400 empty name ─────────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(post_chat(
            Some(&owner_token),
            &group_id.to_string(),
            Some(&group_id.to_string()),
            json!({"name": "  ", "type": "channel"}),
        ))
        .await
        .expect("C3 oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "C3 status");

    // ── C4. POST 401 missing bearer ─────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(post_chat(
            None,
            &group_id.to_string(),
            Some(&group_id.to_string()),
            json!({"name": "ok", "type": "channel"}),
        ))
        .await
        .expect("C4 oneshot");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "C4 status");

    // ── C5. POST 400 X-Group-Id missing ─────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(post_chat(
            Some(&owner_token),
            &group_id.to_string(),
            None,
            json!({"name": "ok-no-header", "type": "channel"}),
        ))
        .await
        .expect("C5 oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "C5 status");

    // ── G1. GET 200 happy path with 2 channels (newest-first) ────────
    // Create a 2nd channel so we can verify ordering.
    let resp = h
        .router
        .clone()
        .oneshot(post_chat(
            Some(&owner_token),
            &group_id.to_string(),
            Some(&group_id.to_string()),
            json!({"name": "random", "type": "channel"}),
        ))
        .await
        .expect("G1 seed second channel");
    assert_eq!(resp.status(), StatusCode::CREATED, "G1 seed second channel");

    let resp = h
        .router
        .clone()
        .oneshot(get_chats(
            Some(&owner_token),
            &group_id.to_string(),
            Some(&group_id.to_string()),
        ))
        .await
        .expect("G1 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "G1 status");
    let v = body_json(resp).await;
    let items = v["items"].as_array().expect("G1 items array");
    assert_eq!(items.len(), 2, "G1 expected 2 channels");
    // Newest-first: 'random' was created after 'general'.
    assert_eq!(items[0]["name"], "random", "G1 newest first");
    assert_eq!(items[1]["name"], "general", "G1 second");

    // ── G2. GET 200 archived chat excluded ──────────────────────────
    sqlx::query("UPDATE chats SET archived_at = now() WHERE name = 'random' AND group_id = $1")
        .bind(group_id)
        .execute(&h.admin_pool)
        .await
        .expect("G2 archive 'random' via admin");
    let resp = h
        .router
        .clone()
        .oneshot(get_chats(
            Some(&owner_token),
            &group_id.to_string(),
            Some(&group_id.to_string()),
        ))
        .await
        .expect("G2 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "G2 status");
    let v = body_json(resp).await;
    let items = v["items"].as_array().expect("G2 items array");
    assert_eq!(items.len(), 1, "G2 archived chat must be excluded");
    assert_eq!(items[0]["name"], "general", "G2 only general remains");

    // ── G3. GET 400 X-Group-Id mismatch ─────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(get_chats(
            Some(&owner_token),
            &group_id.to_string(),
            Some("00000000-0000-0000-0000-000000000000"),
        ))
        .await
        .expect("G3 oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "G3 status");

    // ── G4. GET 401 missing bearer ──────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(get_chats(
            None,
            &group_id.to_string(),
            Some(&group_id.to_string()),
        ))
        .await
        .expect("G4 oneshot");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "G4 status");

    // ── G5. GET 200 empty (fresh group with zero chats) ─────────────
    let (_, group2_id, owner2_token) =
        seed_user_with_group(&h, "owner2@chats-slice1.test")
            .await
            .expect("G5 seed second group");
    let resp = h
        .router
        .clone()
        .oneshot(get_chats(
            Some(&owner2_token),
            &group2_id.to_string(),
            Some(&group2_id.to_string()),
        ))
        .await
        .expect("G5 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "G5 status");
    let v = body_json(resp).await;
    assert_eq!(
        v["items"].as_array().expect("G5 items array").len(),
        0,
        "G5 fresh group returns empty list"
    );
}
