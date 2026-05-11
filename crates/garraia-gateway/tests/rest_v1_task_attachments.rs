//! Integration tests for task_attachments REST API (plan 0096, GAR-572).
//!
//! All scenarios bundled into ONE `#[tokio::test]` — same pattern as
//! `rest_v1_task_assignees.rs`. Splitting triggers the sqlx runtime-teardown
//! race documented in plan 0016 M3.
//!
//! Scenarios (8 total):
//!
//!   AT1. POST 201 — attach a file; response shape + audit row.
//!   AT2. GET 200 — list attachments; returns AT1 entry (cursor pagination).
//!   AT3. POST 409 — duplicate attach returns 409 Conflict.
//!   AT4. DELETE 204 — detach the file; audit row emitted.
//!   AT5. DELETE 204 idempotent — same file_id just detached; 204, no new audit.
//!   AT6. POST 404 — file_id belongs to Bob's group (cross-group file guard).
//!   AT7. POST 404 — file is soft-deleted in Alice's group.
//!   AT8. POST/GET/DELETE 404 — task_id is from Bob's group (cross-group task guard).

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

/// Creates a task list + task under `group_id`. Returns (list_id, task_id).
async fn create_task_list_and_task(h: &Harness, token: &str, group_id: &str) -> (String, String) {
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{group_id}/task-lists"),
            Some(token),
            Some(group_id),
            Some(json!({ "name": "Attachment Test List", "type": "list" })),
        ))
        .await
        .expect("create task-list");
    assert_eq!(resp.status(), StatusCode::CREATED, "setup: task-list 201");
    let tl = body_json(resp).await;
    let list_id = tl["id"].as_str().expect("list id").to_string();

    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{group_id}/task-lists/{list_id}/tasks"),
            Some(token),
            Some(group_id),
            Some(json!({ "title": "Task with attachments" })),
        ))
        .await
        .expect("create task");
    assert_eq!(resp.status(), StatusCode::CREATED, "setup: task 201");
    let task = body_json(resp).await;
    let task_id = task["id"].as_str().expect("task id").to_string();

    (list_id, task_id)
}

/// Seeds a minimal `files` + `file_versions` row directly via `admin_pool`,
/// bypassing RLS. Returns the new `file_id`.
///
/// When `deleted` is true, sets `deleted_at` so the file counts as soft-deleted
/// (AT7 scenario).
async fn seed_file(h: &Harness, group_id: Uuid, uploader_id: Uuid, deleted: bool) -> Uuid {
    let file_id = Uuid::new_v4();
    // object_key includes file_id to satisfy the UNIQUE constraint across parallel tests.
    let object_key = format!("test/{file_id}/v1/payload.bin");
    // 64 lowercase hex chars satisfy both checksum_sha256 and integrity_hmac CHECK constraints.
    let hex64 = "a".repeat(64);

    let deleted_at_expr = if deleted {
        "'2000-01-01T00:00:00Z'::timestamptz"
    } else {
        "NULL"
    };

    // files row — no folder (NULL folder_id is fine per migration 003 MATCH SIMPLE FK).
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

    // file_versions row for version 1 (referenced by files.current_version default 1).
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
async fn rest_v1_task_attachments_scenarios() {
    let h = Harness::get().await;

    // Seed Alice as group owner (primary actor).
    // Bob owns a separate group for cross-group isolation tests.
    let (alice_id, alice_group_id, alice_token) =
        seed_user_with_group(&h, "alice@attachments.test")
            .await
            .expect("seed alice+group");
    let (bob_id, bob_group_id, bob_token) = seed_user_with_group(&h, "bob@attachments.test")
        .await
        .expect("seed bob+group");

    let gid = alice_group_id.to_string();
    let g2id = bob_group_id.to_string();

    // Create a task under Alice's group.
    let (_list_id, task_id) = create_task_list_and_task(&h, &alice_token, &gid).await;

    // Create a task under Bob's group (cross-group task guard, AT8).
    let (_bob_list_id, bob_task_id) = create_task_list_and_task(&h, &bob_token, &g2id).await;

    // Seed a live file in Alice's group (the one we'll attach in AT1).
    let file_id = seed_file(&h, alice_group_id, alice_id, false).await;
    let file_str = file_id.to_string();

    // Seed a soft-deleted file in Alice's group (AT7).
    let deleted_file_id = seed_file(&h, alice_group_id, alice_id, true).await;
    let deleted_file_str = deleted_file_id.to_string();

    // Seed a live file in Bob's group (cross-group file guard, AT6).
    let bob_file_id = seed_file(&h, bob_group_id, bob_id, false).await;
    let bob_file_str = bob_file_id.to_string();

    // ── AT1. POST 201 — attach file; response shape + audit row ──────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{gid}/tasks/{task_id}/attachments"),
            Some(&alice_token),
            Some(&gid),
            Some(json!({ "file_id": file_str })),
        ))
        .await
        .expect("AT1 oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "AT1 must return 201");
    let at1_body = body_json(resp).await;
    assert_eq!(at1_body["task_id"], task_id, "AT1 task_id matches");
    assert_eq!(at1_body["file_id"], file_str, "AT1 file_id matches");
    assert!(
        at1_body.get("attached_at").is_some(),
        "AT1 attached_at present"
    );
    assert_eq!(
        at1_body["file_name"], "test-file.bin",
        "AT1 file_name from files table"
    );
    assert_eq!(
        at1_body["mime_type"], "application/octet-stream",
        "AT1 mime_type from files table"
    );
    assert_eq!(
        at1_body["size_bytes"], 1024,
        "AT1 size_bytes from files table"
    );

    // Audit row: action = "task.file.attached", resource_type = "task_attachments".
    let audit = fetch_audit_events_for_group(&h, alice_group_id)
        .await
        .expect("AT1 fetch audit");
    let at1_audit = audit
        .iter()
        .find(|e| e.0 == "task.file.attached")
        .expect("AT1 audit row task.file.attached must exist");
    let (_, _, at1_res_type, at1_res_id, at1_meta) = at1_audit;
    assert_eq!(at1_res_type, "task_attachments", "AT1 audit resource_type");
    assert_eq!(at1_res_id, &task_id, "AT1 audit resource_id is task_id");
    // Metadata must carry task_id + file_id but no PII (no file_name, no email).
    assert!(
        at1_meta.get("task_id").is_some(),
        "AT1 audit metadata has task_id"
    );
    assert!(
        at1_meta.get("file_id").is_some(),
        "AT1 audit metadata has file_id"
    );
    assert!(
        at1_meta.get("file_name").is_none(),
        "AT1 audit must NOT contain file_name (PII guard)"
    );

    // ── AT2. GET 200 — list attachments; AT1 entry present ───────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "GET",
            &format!("/v1/groups/{gid}/tasks/{task_id}/attachments"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("AT2 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "AT2 must return 200");
    let at2_body = body_json(resp).await;
    let items = at2_body["items"].as_array().expect("AT2 items array");
    assert!(
        items.iter().any(|it| it["file_id"] == file_str),
        "AT2 should contain the AT1 attachment"
    );
    // next_cursor should be null (only one item, well below limit).
    assert!(
        at2_body["next_cursor"].is_null(),
        "AT2 next_cursor is null (single page)"
    );

    // ── AT3. POST 409 — duplicate attach ─────────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{gid}/tasks/{task_id}/attachments"),
            Some(&alice_token),
            Some(&gid),
            Some(json!({ "file_id": file_str })),
        ))
        .await
        .expect("AT3 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "AT3 duplicate attach must return 409"
    );

    // ── AT4. DELETE 204 — detach file; audit row emitted ─────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "DELETE",
            &format!("/v1/groups/{gid}/tasks/{task_id}/attachments/{file_str}"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("AT4 oneshot");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT, "AT4 must return 204");

    // Verify the file no longer appears in GET list.
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "GET",
            &format!("/v1/groups/{gid}/tasks/{task_id}/attachments"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("AT4 list after detach");
    let at4_list = body_json(resp).await;
    let after_del = at4_list["items"].as_array().expect("AT4 items array");
    assert!(
        !after_del.iter().any(|it| it["file_id"] == file_str),
        "AT4 detached file must not appear in GET list"
    );

    // Audit row: action = "task.file.detached".
    let audit2 = fetch_audit_events_for_group(&h, alice_group_id)
        .await
        .expect("AT4 fetch audit");
    assert!(
        audit2
            .iter()
            .any(|e| e.0 == "task.file.detached" && e.3 == task_id),
        "AT4 audit row task.file.detached must be present"
    );

    // ── AT5. DELETE 204 idempotent — same file_id, not attached ──────────────
    let audit_count_before = audit2
        .iter()
        .filter(|e| e.0 == "task.file.detached")
        .count();

    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "DELETE",
            &format!("/v1/groups/{gid}/tasks/{task_id}/attachments/{file_str}"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("AT5 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "AT5 idempotent DELETE must return 204"
    );

    // No new audit row should have been emitted (rows_affected = 0).
    let audit3 = fetch_audit_events_for_group(&h, alice_group_id)
        .await
        .expect("AT5 fetch audit");
    let audit_count_after = audit3
        .iter()
        .filter(|e| e.0 == "task.file.detached")
        .count();
    assert_eq!(
        audit_count_before, audit_count_after,
        "AT5 no-op DELETE must not emit duplicate audit"
    );

    // ── AT6. POST 404 — file belongs to Bob's group (cross-group guard) ───────
    // Alice references bob_file_id while acting in alice's group → 404 (never 403).
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{gid}/tasks/{task_id}/attachments"),
            Some(&alice_token),
            Some(&gid),
            Some(json!({ "file_id": bob_file_str })),
        ))
        .await
        .expect("AT6 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "AT6 cross-group file must return 404 (never 403)"
    );

    // ── AT7. POST 404 — soft-deleted file ────────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{gid}/tasks/{task_id}/attachments"),
            Some(&alice_token),
            Some(&gid),
            Some(json!({ "file_id": deleted_file_str })),
        ))
        .await
        .expect("AT7 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "AT7 deleted file must return 404"
    );

    // ── AT8. POST/GET/DELETE 404 — task_id from Bob's group ──────────────────
    // Alice uses her own group context but references bob_task_id → task not
    // found in alice's group → 404 on all three verbs.
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{gid}/tasks/{bob_task_id}/attachments"),
            Some(&alice_token),
            Some(&gid),
            Some(json!({ "file_id": file_str })),
        ))
        .await
        .expect("AT8 POST oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "AT8 POST cross-group task must return 404"
    );

    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "GET",
            &format!("/v1/groups/{gid}/tasks/{bob_task_id}/attachments"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("AT8 GET oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "AT8 GET cross-group task must return 404"
    );

    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "DELETE",
            &format!("/v1/groups/{gid}/tasks/{bob_task_id}/attachments/{file_str}"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("AT8 DELETE oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "AT8 DELETE cross-group task must return 404"
    );
}
