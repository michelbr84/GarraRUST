//! Integration tests for the task-move REST endpoint (plan 0082, GAR-544, slice 8).
//!
//! All scenarios bundled into ONE `#[tokio::test]` — same pattern as previous
//! task-slice tests. Splitting triggers the sqlx runtime-teardown race
//! documented in plan 0016 M3.
//!
//! Scenarios (8 total):
//!
//!   M1. Happy path — Alice moves a task from list A to list B in her own
//!       group; response carries the new `list_id`; `task_activity` has a
//!       `moved` row with `{from_list_id, to_list_id}`; `audit_events` has a
//!       `task.moved` row whose `metadata` carries both UUIDs.
//!   M2. Idempotent self-move — `target_list_id == current` returns 200
//!       with no new activity row, no new audit row, and `updated_at`
//!       unchanged.
//!   M3. Unknown `task_id` returns 404.
//!   M4. Soft-deleted task returns 404.
//!   M5. Unknown `target_list_id` returns 404.
//!   M6. Archived target list returns 404 (archive via `DELETE` on the list).
//!   M7. Cross-group `target_list_id` (list belongs to Bob's group) returns
//!       404 from Alice's principal — RLS filter collapses the SELECT to 0
//!       rows.
//!   M8. Cross-group `task_id` (task in Bob's group, called with Alice's
//!       principal + her own group header) returns 404.

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

/// POST a task-list and return the new list_id.
async fn create_task_list(h: &Harness, token: &str, group_id: &str, name: &str) -> String {
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{group_id}/task-lists"),
            Some(token),
            Some(group_id),
            Some(json!({ "name": name, "type": "list" })),
        ))
        .await
        .expect("POST task-list oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "setup: task-list 201 ({name})"
    );
    let body = body_json(resp).await;
    body["id"].as_str().expect("list id").to_string()
}

/// POST a task into the given list and return the new task_id.
async fn create_task(h: &Harness, token: &str, group_id: &str, list_id: &str) -> String {
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{group_id}/task-lists/{list_id}/tasks"),
            Some(token),
            Some(group_id),
            Some(json!({ "title": "Task for move tests" })),
        ))
        .await
        .expect("POST task oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "setup: task 201");
    let body = body_json(resp).await;
    body["id"].as_str().expect("task id").to_string()
}

/// POST a move and return the response.
async fn move_task(
    h: &Harness,
    token: &str,
    group_id: &str,
    task_id: &str,
    target_list_id: &str,
) -> axum::response::Response {
    h.router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{group_id}/tasks/{task_id}/move"),
            Some(token),
            Some(group_id),
            Some(json!({ "target_list_id": target_list_id })),
        ))
        .await
        .expect("POST move oneshot")
}

/// SELECT the raw `tasks` row for the given task_id.
/// Used to assert `updated_at` invariance in M2.
async fn fetch_task_updated_at(h: &Harness, task_id: Uuid) -> chrono::DateTime<chrono::Utc> {
    let (ts,): (chrono::DateTime<chrono::Utc>,) =
        sqlx::query_as("SELECT updated_at FROM tasks WHERE id = $1")
            .bind(task_id)
            .fetch_one(&h.admin_pool)
            .await
            .expect("SELECT updated_at");
    ts
}

/// SELECT all `task_activity` rows for the given task_id, newest first.
async fn fetch_task_activity(h: &Harness, task_id: Uuid) -> Vec<(String, serde_json::Value)> {
    sqlx::query_as(
        "SELECT kind, payload FROM task_activity \
         WHERE task_id = $1 \
         ORDER BY created_at DESC, id DESC",
    )
    .bind(task_id)
    .fetch_all(&h.admin_pool)
    .await
    .expect("SELECT task_activity")
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(feature = "test-helpers")]
#[tokio::test]
async fn rest_v1_task_move_scenarios() {
    let h = Harness::get().await;

    // Seed Alice (owner of group A) and Bob (owner of group B).
    let (_alice_id, alice_group_id, alice_token) = seed_user_with_group(&h, "alice@move.test")
        .await
        .expect("seed alice+group");
    let (_bob_id, bob_group_id, bob_token) = seed_user_with_group(&h, "bob@move.test")
        .await
        .expect("seed bob+group");

    let agid = alice_group_id.to_string();
    let bgid = bob_group_id.to_string();

    // Two lists in Alice's group.
    let list_a = create_task_list(&h, &alice_token, &agid, "List A — origin").await;
    let list_b = create_task_list(&h, &alice_token, &agid, "List B — target").await;

    // Task starts in list_a.
    let task_id_str = create_task(&h, &alice_token, &agid, &list_a).await;
    let task_id_uuid: Uuid = task_id_str.parse().expect("task uuid");

    // ── M1. Happy path ─────────────────────────────────────────────────────
    let resp = move_task(&h, &alice_token, &agid, &task_id_str, &list_b).await;
    assert_eq!(resp.status(), StatusCode::OK, "M1 move 200");
    let body = body_json(resp).await;
    assert_eq!(
        body["list_id"].as_str(),
        Some(list_b.as_str()),
        "M1 response carries the new list_id"
    );
    assert_eq!(
        body["id"].as_str(),
        Some(task_id_str.as_str()),
        "M1 response is the same task"
    );

    // task_activity has a `moved` row with both UUIDs.
    let activity = fetch_task_activity(&h, task_id_uuid).await;
    let moved_row = activity
        .iter()
        .find(|(k, _)| k == "moved")
        .expect("M1 task_activity has a `moved` row");
    let moved_payload = &moved_row.1;
    assert_eq!(
        moved_payload["from_list_id"].as_str(),
        Some(list_a.as_str()),
        "M1 activity payload.from_list_id"
    );
    assert_eq!(
        moved_payload["to_list_id"].as_str(),
        Some(list_b.as_str()),
        "M1 activity payload.to_list_id"
    );

    // audit_events has a `task.moved` row scoped to Alice's group.
    let audit_rows = fetch_audit_events_for_group(&h, alice_group_id)
        .await
        .expect("M1 audit fetch");
    let audit_moved = audit_rows
        .iter()
        .find(|(action, _, rt, rid, _)| {
            action == "task.moved" && rt == "tasks" && rid == &task_id_str
        })
        .expect("M1 audit_events has task.moved row");
    let audit_meta = &audit_moved.4;
    assert_eq!(
        audit_meta["from_list_id"].as_str(),
        Some(list_a.as_str()),
        "M1 audit metadata.from_list_id"
    );
    assert_eq!(
        audit_meta["to_list_id"].as_str(),
        Some(list_b.as_str()),
        "M1 audit metadata.to_list_id"
    );
    // PII guard: no list names in metadata.
    assert!(
        audit_meta.get("from_name").is_none(),
        "M1 audit metadata must not carry list names"
    );
    assert!(
        audit_meta.get("to_name").is_none(),
        "M1 audit metadata must not carry list names"
    );

    // ── M2. Idempotent self-move ────────────────────────────────────────────
    // Task is now in list_b. Move it to list_b again — should be a no-op.
    let updated_before = fetch_task_updated_at(&h, task_id_uuid).await;
    let activity_before = fetch_task_activity(&h, task_id_uuid).await;
    let audit_before = fetch_audit_events_for_group(&h, alice_group_id)
        .await
        .expect("M2 audit before");
    let moved_count_before = audit_before
        .iter()
        .filter(|(action, _, _, rid, _)| action == "task.moved" && rid == &task_id_str)
        .count();

    let resp = move_task(&h, &alice_token, &agid, &task_id_str, &list_b).await;
    assert_eq!(resp.status(), StatusCode::OK, "M2 self-move 200");

    let updated_after = fetch_task_updated_at(&h, task_id_uuid).await;
    assert_eq!(
        updated_after, updated_before,
        "M2 updated_at must NOT change on self-move"
    );
    let activity_after = fetch_task_activity(&h, task_id_uuid).await;
    assert_eq!(
        activity_after.len(),
        activity_before.len(),
        "M2 no new task_activity row on self-move"
    );
    let audit_after = fetch_audit_events_for_group(&h, alice_group_id)
        .await
        .expect("M2 audit after");
    let moved_count_after = audit_after
        .iter()
        .filter(|(action, _, _, rid, _)| action == "task.moved" && rid == &task_id_str)
        .count();
    assert_eq!(
        moved_count_after, moved_count_before,
        "M2 no new task.moved audit row on self-move"
    );

    // ── M3. Unknown task_id → 404 ──────────────────────────────────────────
    let unknown_task = Uuid::new_v4().to_string();
    let resp = move_task(&h, &alice_token, &agid, &unknown_task, &list_a).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "M3 unknown task 404");

    // ── M4. Soft-deleted task → 404 ────────────────────────────────────────
    // Create a fresh task in list_a, soft-delete it via DELETE, then attempt
    // to move it.
    let doomed_task = create_task(&h, &alice_token, &agid, &list_a).await;
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "DELETE",
            &format!("/v1/groups/{agid}/tasks/{doomed_task}"),
            Some(&alice_token),
            Some(&agid),
            None,
        ))
        .await
        .expect("M4 DELETE task");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT, "M4 DELETE 204");

    let resp = move_task(&h, &alice_token, &agid, &doomed_task, &list_b).await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "M4 soft-deleted task move 404"
    );

    // ── M5. Unknown target_list_id → 404 ───────────────────────────────────
    let unknown_list = Uuid::new_v4().to_string();
    let resp = move_task(&h, &alice_token, &agid, &task_id_str, &unknown_list).await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "M5 unknown target_list_id 404"
    );

    // ── M6. Archived target list → 404 ─────────────────────────────────────
    // Create a third list, archive it via DELETE on the list, then attempt
    // to move our happy-path task into it.
    let list_c_archived = create_task_list(&h, &alice_token, &agid, "List C — to archive").await;
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "DELETE",
            &format!("/v1/groups/{agid}/task-lists/{list_c_archived}"),
            Some(&alice_token),
            Some(&agid),
            None,
        ))
        .await
        .expect("M6 archive list-c");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT, "M6 archive list 204");

    let resp = move_task(&h, &alice_token, &agid, &task_id_str, &list_c_archived).await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "M6 archived target list 404"
    );

    // ── M7. Cross-group target_list_id → 404 ───────────────────────────────
    // Bob owns a list in his group; Alice (with her own JWT + her own
    // x-group-id) cannot use Bob's list_id as a target.
    let bob_list = create_task_list(&h, &bob_token, &bgid, "Bob's list").await;
    let resp = move_task(&h, &alice_token, &agid, &task_id_str, &bob_list).await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "M7 cross-group target 404"
    );

    // ── M8. Cross-group task_id → 404 ──────────────────────────────────────
    // Bob has a task in his own group. Alice attempts to move it using her
    // own principal but Bob's task UUID — RLS filters the lookup to 0 rows.
    let bob_task = create_task(&h, &bob_token, &bgid, &bob_list).await;
    let resp = move_task(&h, &alice_token, &agid, &bob_task, &list_b).await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "M8 cross-group task 404"
    );
}
