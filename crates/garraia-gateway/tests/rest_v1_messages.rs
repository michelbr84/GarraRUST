//! Integration tests for `/v1/chats/{chat_id}/messages` (plan 0055, GAR-507)
//! and `/v1/messages/{message_id}/threads` (plan 0057, GAR-509).
//!
//! All scenarios bundled into ONE `#[tokio::test]` — same pattern as
//! `rest_v1_chats.rs` / `rest_v1_groups.rs`. Splitting into multiple
//! `#[tokio::test]`s triggers the sqlx runtime-teardown race documented
//! in plan 0016 M3 commit `4f8be37`.
//!
//! Scenarios (15 total):
//!
//! POST /messages scenarios (5):
//!   M1. POST 201 — happy path: owner sends a message; asserts response
//!       shape, DB row, sender_label match, and audit row with structural
//!       metadata only (no body content — invariant 5 PII guard).
//!   M2. POST 400 — empty body rejected.
//!   M3. POST 401 — missing bearer.
//!   M4. POST 400 — `X-Group-Id` header missing.
//!   M5. POST 404 — chat belongs to a different group (cross-tenant).
//!
//! GET /messages scenarios (5):
//!   G1. GET 200 — happy path: 3 messages returned newest-first.
//!   G2. GET 200 — cursor pagination: `after=<mid_id>` returns only older.
//!   G3. GET 200 — empty chat returns `items: []`.
//!   G4. GET 401 — missing bearer.
//!   G5. GET 404 — chat of a different group (cross-tenant).
//!
//! POST /threads scenarios (5):
//!   T1. POST 201 — happy path no title: asserts response shape + audit
//!       row with `{has_title: false}`. No PII in audit metadata.
//!   T2. POST 201 — with title: asserts title stored + audit `{has_title: true}`.
//!   T3. POST 409 — create thread twice on same message.
//!   T4. POST 404 — message_id belongs to a different group (cross-tenant).
//!   T5. POST 401 — missing bearer.

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

fn post_message(
    token: Option<&str>,
    chat_id: &str,
    x_group_id: Option<&str>,
    body: serde_json::Value,
) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri(format!("/v1/chats/{chat_id}/messages"))
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

fn get_messages(
    token: Option<&str>,
    chat_id: &str,
    x_group_id: Option<&str>,
    after: Option<&str>,
    limit: Option<u32>,
) -> Request<Body> {
    let mut query = String::new();
    let mut sep = '?';
    if let Some(a) = after {
        query.push_str(&format!("{sep}after={a}"));
        sep = '&';
    }
    if let Some(l) = limit {
        query.push_str(&format!("{sep}limit={l}"));
    }
    let mut req = Request::builder()
        .method("GET")
        .uri(format!("/v1/chats/{chat_id}/messages{query}"))
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

fn post_thread(
    token: Option<&str>,
    message_id: &str,
    x_group_id: Option<&str>,
    body: serde_json::Value,
) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri(format!("/v1/messages/{message_id}/threads"))
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

/// Helper: create a chat via POST /v1/groups/{group_id}/chats and return chat_id.
async fn create_chat(h: &Harness, token: &str, group_id: &str, name: &str) -> String {
    let resp = h
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/groups/{group_id}/chats"))
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .header("x-group-id", group_id)
                .extension(axum::extract::ConnectInfo::<std::net::SocketAddr>(
                    "127.0.0.1:1".parse().unwrap(),
                ))
                .body(Body::from(
                    json!({"name": name, "type": "channel"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("create_chat oneshot");
    let b = body_json(resp).await;
    b["id"].as_str().unwrap().to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn rest_v1_messages_scenarios() {
    let h = Harness::get().await;

    let (owner_id, group_id, owner_token) = seed_user_with_group(&h, "owner@msg-slice2.test")
        .await
        .expect("seed owner+group");

    // Create a chat for this group.
    let chat_id = create_chat(&h, &owner_token, &group_id.to_string(), "general").await;

    // Create a second group+owner for cross-tenant tests.
    let (_, group2_id, owner2_token) = seed_user_with_group(&h, "owner2@msg-slice2.test")
        .await
        .expect("seed owner2+group2");
    let chat2_id = create_chat(&h, &owner2_token, &group2_id.to_string(), "other").await;

    // ── M1. POST 201 happy path ──────────────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(post_message(
            Some(&owner_token),
            &chat_id,
            Some(&group_id.to_string()),
            json!({"body": "Hello, world!"}),
        ))
        .await
        .expect("M1 oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "M1 status");
    let body = body_json(resp).await;
    assert_eq!(body["body"], "Hello, world!", "M1 body");
    assert_eq!(body["chat_id"], chat_id, "M1 chat_id");
    assert_eq!(body["group_id"], group_id.to_string(), "M1 group_id");
    assert_eq!(
        body["sender_user_id"],
        owner_id.to_string(),
        "M1 sender_user_id"
    );
    // sender_label must be non-empty (resolved from users.display_name)
    assert!(
        !body["sender_label"].as_str().unwrap_or("").is_empty(),
        "M1 sender_label must be resolved"
    );
    let msg1_id = body["id"].as_str().unwrap().to_string();

    // M1 — verify audit row: structural metadata only, NO body content.
    let events = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("M1 fetch audit");
    let msg_event = events
        .iter()
        .find(|e| e.0 == "message.sent")
        .expect("M1 message.sent audit row missing");
    let (_, _, resource_type, resource_id, metadata) = msg_event;
    assert_eq!(resource_type, "messages", "M1 audit resource_type");
    assert_eq!(resource_id, &msg1_id, "M1 audit resource_id");
    assert_eq!(
        metadata["body_len"], 13,
        "M1 audit body_len = len('Hello, world!')"
    );
    assert_eq!(metadata["has_reply_to"], false, "M1 audit has_reply_to");
    assert!(
        metadata.get("body").is_none(),
        "M1 audit MUST NOT carry body content"
    );

    // ── M2. POST 400 empty body ──────────────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(post_message(
            Some(&owner_token),
            &chat_id,
            Some(&group_id.to_string()),
            json!({"body": "  "}),
        ))
        .await
        .expect("M2 oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "M2 status");

    // ── M3. POST 401 missing bearer ──────────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(post_message(
            None,
            &chat_id,
            Some(&group_id.to_string()),
            json!({"body": "hi"}),
        ))
        .await
        .expect("M3 oneshot");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "M3 status");

    // ── M4. POST 400 X-Group-Id missing ─────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(post_message(
            Some(&owner_token),
            &chat_id,
            None,
            json!({"body": "hi"}),
        ))
        .await
        .expect("M4 oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "M4 status");

    // ── M5. POST 404 cross-tenant (owner1 posts to owner2's chat) ────────
    let resp = h
        .router
        .clone()
        .oneshot(post_message(
            Some(&owner_token),
            &chat2_id,                   // belongs to group2
            Some(&group_id.to_string()), // owner1's group
            json!({"body": "cross-tenant attempt"}),
        ))
        .await
        .expect("M5 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "M5 status (cross-tenant → 404)"
    );

    // ── G1. GET 200 happy path — 3 messages newest-first ────────────────
    // Send 2 more messages so we have 3 total (msg1 already sent).
    let resp2 = h
        .router
        .clone()
        .oneshot(post_message(
            Some(&owner_token),
            &chat_id,
            Some(&group_id.to_string()),
            json!({"body": "Second message"}),
        ))
        .await
        .expect("G1 post msg2");
    assert_eq!(resp2.status(), StatusCode::CREATED);
    let body2 = body_json(resp2).await;
    let _msg2_id = body2["id"].as_str().unwrap().to_string();

    let resp3 = h
        .router
        .clone()
        .oneshot(post_message(
            Some(&owner_token),
            &chat_id,
            Some(&group_id.to_string()),
            json!({"body": "Third message"}),
        ))
        .await
        .expect("G1 post msg3");
    assert_eq!(resp3.status(), StatusCode::CREATED);
    let body3 = body_json(resp3).await;
    let msg3_id = body3["id"].as_str().unwrap().to_string();

    let resp = h
        .router
        .clone()
        .oneshot(get_messages(
            Some(&owner_token),
            &chat_id,
            Some(&group_id.to_string()),
            None,
            None,
        ))
        .await
        .expect("G1 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "G1 status");
    let body = body_json(resp).await;
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 3, "G1 expected 3 messages");
    // Newest-first: msg3 → msg2 → msg1
    assert_eq!(items[0]["body"], "Third message", "G1 newest first");
    assert_eq!(items[2]["body"], "Hello, world!", "G1 oldest last");
    // With limit=3 and total=3, next_cursor should be None (not full page)
    assert!(
        body["next_cursor"].is_null(),
        "G1 next_cursor null when < limit"
    );

    // ── G2. GET 200 cursor pagination ────────────────────────────────────
    // after=msg3_id → should return only msg2 and msg1
    let resp = h
        .router
        .clone()
        .oneshot(get_messages(
            Some(&owner_token),
            &chat_id,
            Some(&group_id.to_string()),
            Some(&msg3_id),
            None,
        ))
        .await
        .expect("G2 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "G2 status");
    let body = body_json(resp).await;
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2, "G2 after cursor: should see msg2 + msg1");
    assert_eq!(items[0]["body"], "Second message", "G2 newest of remaining");
    assert_eq!(items[1]["body"], "Hello, world!", "G2 oldest");

    // ── G3. GET 200 empty chat ───────────────────────────────────────────
    let empty_chat_id = create_chat(&h, &owner_token, &group_id.to_string(), "empty").await;
    let resp = h
        .router
        .clone()
        .oneshot(get_messages(
            Some(&owner_token),
            &empty_chat_id,
            Some(&group_id.to_string()),
            None,
            None,
        ))
        .await
        .expect("G3 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "G3 status");
    let body = body_json(resp).await;
    assert_eq!(body["items"].as_array().unwrap().len(), 0, "G3 empty");
    assert!(body["next_cursor"].is_null(), "G3 next_cursor null");

    // ── G4. GET 401 missing bearer ───────────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(get_messages(
            None,
            &chat_id,
            Some(&group_id.to_string()),
            None,
            None,
        ))
        .await
        .expect("G4 oneshot");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "G4 status");

    // ── G5. GET 404 cross-tenant ─────────────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(get_messages(
            Some(&owner_token),
            &chat2_id,
            Some(&group_id.to_string()),
            None,
            None,
        ))
        .await
        .expect("G5 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "G5 status (cross-tenant → 404)"
    );

    // ── T1. POST /threads 201 — happy path, no title ─────────────────────
    // msg1_id was created in M1 above — use it as the thread root.
    let resp = h
        .router
        .clone()
        .oneshot(post_thread(
            Some(&owner_token),
            &msg1_id,
            Some(&group_id.to_string()),
            json!({}),
        ))
        .await
        .expect("T1 oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "T1 status");
    let t1_body = body_json(resp).await;
    assert_eq!(t1_body["root_message_id"], msg1_id, "T1 root_message_id");
    assert!(t1_body["title"].is_null(), "T1 title must be null");
    assert!(
        !t1_body["id"].as_str().unwrap_or("").is_empty(),
        "T1 id present"
    );
    assert!(
        !t1_body["chat_id"].as_str().unwrap_or("").is_empty(),
        "T1 chat_id present"
    );
    let thread1_id = t1_body["id"].as_str().unwrap().to_string();

    // T1 — audit row: structural only, no title content.
    let events = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("T1 fetch audit");
    let thread_event = events
        .iter()
        .find(|e| e.0 == "thread.created" && e.3 == thread1_id)
        .expect("T1 thread.created audit row missing");
    let (_, _, resource_type, _, metadata) = thread_event;
    assert_eq!(resource_type, "message_threads", "T1 audit resource_type");
    assert_eq!(metadata["has_title"], false, "T1 audit has_title=false");
    assert!(
        metadata.get("title").is_none(),
        "T1 audit MUST NOT carry title content"
    );

    // ── T2. POST /threads 201 — with title ───────────────────────────────
    // Use msg3_id (created in G1 block) as a fresh root for a titled thread.
    let resp = h
        .router
        .clone()
        .oneshot(post_thread(
            Some(&owner_token),
            &msg3_id,
            Some(&group_id.to_string()),
            json!({"title": "Design discussion"}),
        ))
        .await
        .expect("T2 oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "T2 status");
    let t2_body = body_json(resp).await;
    assert_eq!(
        t2_body["title"], "Design discussion",
        "T2 title stored correctly"
    );
    assert_eq!(t2_body["root_message_id"], msg3_id, "T2 root_message_id");
    let thread2_id = t2_body["id"].as_str().unwrap().to_string();

    // T2 — audit row: has_title=true.
    let events = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("T2 fetch audit");
    let thread2_event = events
        .iter()
        .find(|e| e.0 == "thread.created" && e.3 == thread2_id)
        .expect("T2 thread.created audit row missing");
    let (_, _, _, _, t2_metadata) = thread2_event;
    assert_eq!(t2_metadata["has_title"], true, "T2 audit has_title=true");
    assert!(
        t2_metadata.get("title").is_none(),
        "T2 audit MUST NOT carry title content"
    );

    // ── T3. POST /threads 409 — duplicate (same message twice) ───────────
    let resp = h
        .router
        .clone()
        .oneshot(post_thread(
            Some(&owner_token),
            &msg1_id, // already has a thread from T1
            Some(&group_id.to_string()),
            json!({}),
        ))
        .await
        .expect("T3 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "T3 status (duplicate → 409)"
    );

    // ── T4. POST /threads 404 — message belongs to a different group ──────
    // Send a message to group2's chat, then try to create a thread as owner1.
    let resp_cross = h
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/chats/{chat2_id}/messages"))
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {owner2_token}"))
                .header("x-group-id", group2_id.to_string())
                .extension(axum::extract::ConnectInfo::<std::net::SocketAddr>(
                    "127.0.0.1:1".parse().unwrap(),
                ))
                .body(Body::from(
                    json!({"body": "group2 message for T4"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("T4 seed msg in group2");
    assert_eq!(
        resp_cross.status(),
        StatusCode::CREATED,
        "T4 seed msg status"
    );
    let cross_msg_body = body_json(resp_cross).await;
    let cross_msg_id = cross_msg_body["id"].as_str().unwrap().to_string();

    let resp = h
        .router
        .clone()
        .oneshot(post_thread(
            Some(&owner_token),
            &cross_msg_id,               // belongs to group2
            Some(&group_id.to_string()), // owner1's group
            json!({}),
        ))
        .await
        .expect("T4 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "T4 status (cross-tenant message → 404)"
    );

    // ── T5. POST /threads 401 — missing bearer ────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(post_thread(
            None,
            &msg1_id,
            Some(&group_id.to_string()),
            json!({}),
        ))
        .await
        .expect("T5 oneshot");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "T5 status");
}
