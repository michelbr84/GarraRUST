//! Integration tests for message_attachments REST API (plan 0182, GAR-700).
//!
//! All scenarios bundled into ONE `#[tokio::test]` — same pattern as
//! `rest_v1_task_attachments.rs`. Splitting triggers the sqlx runtime-teardown
//! race documented in plan 0016 M3.
//!
//! Scenarios (8 total):
//!
//!   MA1. POST 201 — attach a file; response shape + audit row.
//!   MA2. GET 200 — list attachments; returns MA1 entry (cursor pagination).
//!   MA3. POST 409 — duplicate attach returns 409 Conflict.
//!   MA4. DELETE 204 — detach the file; audit row emitted.
//!   MA5. DELETE 204 idempotent — same file_id just detached; 204, no new audit.
//!   MA6. POST 404 — file_id belongs to Bob's group (cross-group file guard).
//!   MA7. POST 404 — file is soft-deleted in Alice's group.
//!   MA8. GET/DELETE 404 — message_id is from Bob's group (cross-group message guard).

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

use common::Harness;
use common::fixtures::{fetch_audit_events_for_group, seed_user_with_group};

// ─── Helpers ─────────────────────────────────────────────────────────────────

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("collect response body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response body is not JSON")
}

fn auth_req(
    method: &str,
    uri: &str,
    token: Option<&str>,
    x_group_id: Option<&str>,
    body: Option<serde_json::Value>,
) -> Request<Body> {
    let body_bytes = match body {
        Some(v) => Body::from(v.to_string()),
        None => Body::empty(),
    };
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(body_bytes)
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

/// Creates a chat and sends one message. Returns (chat_id, message_id).
async fn create_chat_and_message(h: &Harness, token: &str, group_id: &str) -> (String, String) {
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{group_id}/chats"),
            Some(token),
            Some(group_id),
            Some(json!({ "name": "attachments-test-chat", "type": "channel" })),
        ))
        .await
        .expect("create chat");
    assert_eq!(resp.status(), StatusCode::CREATED, "setup: chat 201");
    let chat_body = body_json(resp).await;
    let chat_id = chat_body["id"].as_str().expect("chat id").to_string();

    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/chats/{chat_id}/messages"),
            Some(token),
            Some(group_id),
            Some(json!({ "body": "message with attachments" })),
        ))
        .await
        .expect("send message");
    assert_eq!(resp.status(), StatusCode::CREATED, "setup: message 201");
    let msg_body = body_json(resp).await;
    let message_id = msg_body["id"].as_str().expect("message id").to_string();

    (chat_id, message_id)
}

/// Seeds a minimal `files` + `file_versions` row directly via `admin_pool`,
/// bypassing RLS. Returns the new `file_id`.
///
/// When `deleted` is true, sets `deleted_at` so the file counts as soft-deleted
/// (MA7 scenario).
async fn seed_file(h: &Harness, group_id: Uuid, uploader_id: Uuid, deleted: bool) -> Uuid {
    let file_id = Uuid::new_v4();
    let object_key = format!("test/{file_id}/v1/payload.bin");
    let hex64 = "a".repeat(64);

    let deleted_at_expr = if deleted {
        "'2000-01-01T00:00:00Z'::timestamptz"
    } else {
        "NULL"
    };

    sqlx::query(&format!(
        "INSERT INTO files \
            (id, group_id, name, size_bytes, mime_type, \
             created_by, created_by_label, deleted_at) \
         VALUES ($1, $2, 'test-file.bin', 1024, 'application/octet-stream', \
                 $3, 'Test User', {deleted_at_expr})",
    ))
    .bind(file_id)
    .bind(group_id)
    .bind(uploader_id)
    .execute(&h.admin_pool)
    .await
    .expect("seed files row");

    sqlx::query(
        "INSERT INTO file_versions \
            (file_id, group_id, version, object_key, etag, \
             checksum_sha256, integrity_hmac, size_bytes, mime_type, \
             created_by, created_by_label) \
         VALUES ($1, $2, 1, $3, 'etag-test', $4, $4, 1024, \
                 'application/octet-stream', $5, 'Test User')",
    )
    .bind(file_id)
    .bind(group_id)
    .bind(object_key)
    .bind(&hex64)
    .bind(uploader_id)
    .execute(&h.admin_pool)
    .await
    .expect("seed file_versions row");

    file_id
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(feature = "test-helpers")]
#[tokio::test]
async fn rest_v1_message_attachments_scenarios() {
    let h = Harness::get().await;

    // Alice — primary actor with her own group.
    // Bob — separate group for cross-group isolation.
    let (alice_id, alice_group_id, alice_token) = seed_user_with_group(&h, "alice@msgattach.test")
        .await
        .expect("seed alice+group");
    let (bob_id, bob_group_id, bob_token) = seed_user_with_group(&h, "bob@msgattach.test")
        .await
        .expect("seed bob+group");

    let gid = alice_group_id.to_string();
    let g2id = bob_group_id.to_string();

    // Create a chat + message under Alice's group.
    let (_chat_id, message_id) = create_chat_and_message(&h, &alice_token, &gid).await;

    // Create a message under Bob's group (cross-group message guard, MA8).
    let (_bob_chat_id, bob_message_id) = create_chat_and_message(&h, &bob_token, &g2id).await;

    // Seed a live file in Alice's group (the one we'll attach in MA1).
    let file_id = seed_file(&h, alice_group_id, alice_id, false).await;
    let file_str = file_id.to_string();

    // Seed a soft-deleted file in Alice's group (MA7).
    let deleted_file_id = seed_file(&h, alice_group_id, alice_id, true).await;
    let deleted_file_str = deleted_file_id.to_string();

    // Seed a live file in Bob's group (cross-group file guard, MA6).
    let bob_file_id = seed_file(&h, bob_group_id, bob_id, false).await;
    let bob_file_str = bob_file_id.to_string();

    // ── MA1. POST 201 — attach file; response shape + audit row ──────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/messages/{message_id}/attachments"),
            Some(&alice_token),
            Some(&gid),
            Some(json!({ "file_id": file_str })),
        ))
        .await
        .expect("MA1 oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "MA1 must return 201");
    let ma1_body = body_json(resp).await;
    assert_eq!(ma1_body["message_id"], message_id, "MA1 message_id matches");
    assert_eq!(ma1_body["file_id"], file_str, "MA1 file_id matches");
    assert!(
        ma1_body.get("attached_at").is_some(),
        "MA1 attached_at present"
    );
    assert_eq!(
        ma1_body["file_name"], "test-file.bin",
        "MA1 file_name from files table"
    );
    assert_eq!(
        ma1_body["mime_type"], "application/octet-stream",
        "MA1 mime_type from files table"
    );
    assert_eq!(
        ma1_body["size_bytes"], 1024,
        "MA1 size_bytes from files table"
    );

    // Audit row: action = "message.file.attached", resource_type = "message_attachments".
    let audit = fetch_audit_events_for_group(&h, alice_group_id)
        .await
        .expect("MA1 fetch audit");
    let ma1_audit = audit
        .iter()
        .find(|e| e.0 == "message.file.attached")
        .expect("MA1 audit row message.file.attached must exist");
    let (_, _, ma1_res_type, ma1_res_id, ma1_meta) = ma1_audit;
    assert_eq!(
        ma1_res_type, "message_attachments",
        "MA1 audit resource_type"
    );
    assert_eq!(
        ma1_res_id, &message_id,
        "MA1 audit resource_id is message_id"
    );
    assert!(
        ma1_meta.get("message_id").is_some(),
        "MA1 audit metadata has message_id"
    );
    assert!(
        ma1_meta.get("file_id").is_some(),
        "MA1 audit metadata has file_id"
    );
    assert!(
        ma1_meta.get("file_name").is_none(),
        "MA1 audit must NOT contain file_name (PII guard)"
    );

    // ── MA2. GET 200 — list attachments; MA1 entry present ───────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "GET",
            &format!("/v1/messages/{message_id}/attachments"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("MA2 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "MA2 must return 200");
    let ma2_body = body_json(resp).await;
    let items = ma2_body["items"].as_array().expect("MA2 items array");
    assert!(
        items.iter().any(|it| it["file_id"] == file_str),
        "MA2 should contain the MA1 attachment"
    );
    assert!(
        ma2_body["next_cursor"].is_null(),
        "MA2 next_cursor is null (single page)"
    );

    // ── MA3. POST 409 — duplicate attach ──────────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/messages/{message_id}/attachments"),
            Some(&alice_token),
            Some(&gid),
            Some(json!({ "file_id": file_str })),
        ))
        .await
        .expect("MA3 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "MA3 duplicate attach must return 409"
    );

    // ── MA4. DELETE 204 — detach file; audit row emitted ─────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "DELETE",
            &format!("/v1/messages/{message_id}/attachments/{file_str}"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("MA4 oneshot");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT, "MA4 must return 204");

    // Verify the file no longer appears in GET list.
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "GET",
            &format!("/v1/messages/{message_id}/attachments"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("MA4 list after detach");
    let ma4_list = body_json(resp).await;
    let after_del = ma4_list["items"].as_array().expect("MA4 items array");
    assert!(
        !after_del.iter().any(|it| it["file_id"] == file_str),
        "MA4 detached file must not appear in GET list"
    );

    // Audit row: action = "message.file.detached".
    let audit2 = fetch_audit_events_for_group(&h, alice_group_id)
        .await
        .expect("MA4 fetch audit");
    assert!(
        audit2
            .iter()
            .any(|e| e.0 == "message.file.detached" && e.3 == message_id),
        "MA4 audit row message.file.detached must be present"
    );

    // ── MA5. DELETE 204 idempotent — same file_id, not attached ──────────────
    let audit_count_before = audit2
        .iter()
        .filter(|e| e.0 == "message.file.detached")
        .count();

    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "DELETE",
            &format!("/v1/messages/{message_id}/attachments/{file_str}"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("MA5 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "MA5 idempotent DELETE must return 204"
    );

    // No new audit row on idempotent detach (rows_affected = 0).
    let audit3 = fetch_audit_events_for_group(&h, alice_group_id)
        .await
        .expect("MA5 fetch audit");
    let audit_count_after = audit3
        .iter()
        .filter(|e| e.0 == "message.file.detached")
        .count();
    assert_eq!(
        audit_count_before, audit_count_after,
        "MA5 no-op DELETE must not emit duplicate audit"
    );

    // ── MA6. POST 404 — file belongs to Bob's group (cross-group guard) ───────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/messages/{message_id}/attachments"),
            Some(&alice_token),
            Some(&gid),
            Some(json!({ "file_id": bob_file_str })),
        ))
        .await
        .expect("MA6 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "MA6 cross-group file must return 404 (never 403)"
    );

    // ── MA7. POST 404 — soft-deleted file ─────────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/messages/{message_id}/attachments"),
            Some(&alice_token),
            Some(&gid),
            Some(json!({ "file_id": deleted_file_str })),
        ))
        .await
        .expect("MA7 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "MA7 deleted file must return 404"
    );

    // ── MA8. GET/DELETE 404 — message_id from Bob's group ─────────────────────
    // Alice uses her own group context but references bob_message_id → message
    // not found (RLS filters it out) → 404 on all verbs.
    // Re-attach alice's file for this check.
    let _ = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/messages/{message_id}/attachments"),
            Some(&alice_token),
            Some(&gid),
            Some(json!({ "file_id": file_str })),
        ))
        .await
        .expect("MA8 re-attach setup");

    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/messages/{bob_message_id}/attachments"),
            Some(&alice_token),
            Some(&gid),
            Some(json!({ "file_id": file_str })),
        ))
        .await
        .expect("MA8 POST oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "MA8 POST cross-group message must return 404"
    );

    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "GET",
            &format!("/v1/messages/{bob_message_id}/attachments"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("MA8 GET oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "MA8 GET cross-group message must return 404"
    );

    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "DELETE",
            &format!("/v1/messages/{bob_message_id}/attachments/{file_str}"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("MA8 DELETE oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "MA8 DELETE cross-group message must return 404"
    );

    // Suppress unused variable warnings from seeding Bob's resources.
    let _ = bob_token;
    let _ = g2id;
}
