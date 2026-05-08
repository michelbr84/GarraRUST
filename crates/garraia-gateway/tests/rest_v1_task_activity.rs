//! Integration tests for task activity REST API (plan 0080, GAR-541, slice 7).
//!
//! All scenarios bundled into ONE `#[tokio::test]` — same pattern as previous
//! task-slice tests. Splitting triggers the sqlx runtime-teardown race
//! documented in plan 0016 M3.
//!
//! Scenarios (8 total):
//!
//!   A1. create_task → GET /activity returns 1 row, kind = `created`.
//!   A2. patch_task status → GET /activity includes `status_changed` row
//!       with `old`/`new` payload.
//!   A3. patch_task priority → GET /activity includes `priority_changed` row.
//!   A4. delete_task (soft-delete) → GET /activity includes `deleted` row.
//!   A5. create_task_comment → GET /activity includes `commented` row with
//!       `body_len` in payload.
//!   A6. add_task_assignee → GET /activity includes `assigned` row with
//!       `assignee_id` in payload (no raw email/name — PII guard).
//!   A7. Cross-group: Alice cannot GET /activity for a task in Bob's group
//!       (404 — task not visible via RLS).
//!   A8. Cursor pagination: limit=1 returns one item + non-null `next_cursor`;
//!       fetching with cursor returns the next item.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

use common::Harness;
use common::fixtures::seed_user_with_group;

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

/// Create a task-list and task; return (list_id, task_id).
async fn create_task_list_and_task(h: &Harness, token: &str, group_id: &str) -> (String, String) {
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{group_id}/task-lists"),
            Some(token),
            Some(group_id),
            Some(json!({ "name": "Activity Test List", "type": "list" })),
        ))
        .await
        .expect("create task-list");
    assert_eq!(resp.status(), StatusCode::CREATED, "setup: task-list 201");
    let tl_body = body_json(resp).await;
    let list_id = tl_body["id"].as_str().expect("list id").to_string();

    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{group_id}/task-lists/{list_id}/tasks"),
            Some(token),
            Some(group_id),
            Some(json!({ "title": "Task for activity tests" })),
        ))
        .await
        .expect("create task");
    assert_eq!(resp.status(), StatusCode::CREATED, "setup: task 201");
    let task_body = body_json(resp).await;
    let task_id = task_body["id"].as_str().expect("task id").to_string();

    (list_id, task_id)
}

/// GET /activity for a task and return the parsed body.
async fn get_activity(
    h: &Harness,
    token: &str,
    group_id: &str,
    task_id: &str,
    query: &str,
) -> serde_json::Value {
    let uri = if query.is_empty() {
        format!("/v1/groups/{group_id}/tasks/{task_id}/activity")
    } else {
        format!("/v1/groups/{group_id}/tasks/{task_id}/activity?{query}")
    };
    let resp = h
        .router
        .clone()
        .oneshot(auth_req("GET", &uri, Some(token), Some(group_id), None))
        .await
        .expect("GET /activity oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "GET /activity 200");
    body_json(resp).await
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(feature = "test-helpers")]
#[tokio::test]
async fn rest_v1_task_activity_scenarios() {
    let h = Harness::get().await;

    // Seed Alice (owner of group A) and Bob (owner of group B).
    let (alice_id, group_id, alice_token) = seed_user_with_group(&h, "alice@activity.test")
        .await
        .expect("seed alice+group");
    let (_bob_id, bob_group_id, bob_token) = seed_user_with_group(&h, "bob@activity.test")
        .await
        .expect("seed bob+group2");

    let gid = group_id.to_string();
    let g2id = bob_group_id.to_string();
    let alice_id_s = alice_id.to_string();

    // ── A1. create_task → activity kind=`created` ─────────────────────────
    // The task creation itself emits the `created` activity entry.
    let (_, task_id) = create_task_list_and_task(&h, &alice_token, &gid).await;

    let body = get_activity(&h, &alice_token, &gid, &task_id, "").await;
    let items = body["items"].as_array().expect("A1 items is array");
    assert!(
        !items.is_empty(),
        "A1 activity must have at least one entry after create"
    );
    let created_row = items
        .iter()
        .find(|it| it["kind"] == "created")
        .expect("A1 must have kind=created row");
    assert!(
        !created_row["actor_label"].as_str().unwrap_or("").is_empty(),
        "A1 actor_label must be non-empty"
    );
    assert!(created_row.get("payload").is_some(), "A1 payload present");

    // ── A2. patch_task status → `status_changed` row ─────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "PATCH",
            &format!("/v1/groups/{gid}/tasks/{task_id}"),
            Some(&alice_token),
            Some(&gid),
            Some(json!({ "status": "in_progress" })),
        ))
        .await
        .expect("A2 PATCH task");
    assert_eq!(resp.status(), StatusCode::OK, "A2 PATCH 200");

    let body = get_activity(&h, &alice_token, &gid, &task_id, "").await;
    let items = body["items"].as_array().expect("A2 items");
    let sc_row = items
        .iter()
        .find(|it| it["kind"] == "status_changed")
        .expect("A2 status_changed row");
    assert_eq!(
        sc_row["payload"]["new"].as_str(),
        Some("in_progress"),
        "A2 payload.new = in_progress"
    );
    assert!(
        sc_row["payload"].get("old").is_some(),
        "A2 payload.old present"
    );

    // ── A3. patch_task priority → `priority_changed` row ─────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "PATCH",
            &format!("/v1/groups/{gid}/tasks/{task_id}"),
            Some(&alice_token),
            Some(&gid),
            Some(json!({ "priority": "high" })),
        ))
        .await
        .expect("A3 PATCH task priority");
    assert_eq!(resp.status(), StatusCode::OK, "A3 PATCH 200");

    let body = get_activity(&h, &alice_token, &gid, &task_id, "").await;
    let items = body["items"].as_array().expect("A3 items");
    assert!(
        items.iter().any(|it| it["kind"] == "priority_changed"),
        "A3 priority_changed row must be present"
    );

    // ── A4. delete_task → `deleted` row ──────────────────────────────────
    // First create a separate task so the deletion doesn't break A5/A6/A8.
    let (_, task_to_delete) = create_task_list_and_task(&h, &alice_token, &gid).await;

    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "DELETE",
            &format!("/v1/groups/{gid}/tasks/{task_to_delete}"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("A4 DELETE task");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT, "A4 DELETE 204");

    // GET /activity on the deleted task → 404 (task soft-deleted).
    let uri = format!("/v1/groups/{gid}/tasks/{task_to_delete}/activity");
    let resp = h
        .router
        .clone()
        .oneshot(auth_req("GET", &uri, Some(&alice_token), Some(&gid), None))
        .await
        .expect("A4 GET activity on deleted task");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "A4 deleted task /activity returns 404"
    );

    // ── A5. create_task_comment → `commented` row ────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{gid}/tasks/{task_id}/comments"),
            Some(&alice_token),
            Some(&gid),
            Some(json!({ "body_md": "Hello from A5" })),
        ))
        .await
        .expect("A5 POST comment");
    assert_eq!(resp.status(), StatusCode::CREATED, "A5 comment 201");

    let body = get_activity(&h, &alice_token, &gid, &task_id, "").await;
    let items = body["items"].as_array().expect("A5 items");
    let commented_row = items
        .iter()
        .find(|it| it["kind"] == "commented")
        .expect("A5 commented row");
    let body_len = commented_row["payload"]["body_len"].as_u64();
    assert!(body_len.is_some(), "A5 payload.body_len present");
    assert!(body_len.unwrap() > 0, "A5 payload.body_len > 0");
    // PII guard: body_md must NOT appear in payload.
    assert!(
        commented_row["payload"].get("body_md").is_none(),
        "A5 payload must not contain body_md (PII guard)"
    );

    // ── A6. add_task_assignee → `assigned` row ────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{gid}/tasks/{task_id}/assignees"),
            Some(&alice_token),
            Some(&gid),
            Some(json!({ "user_id": alice_id_s })),
        ))
        .await
        .expect("A6 POST assignee");
    assert_eq!(resp.status(), StatusCode::CREATED, "A6 assignee 201");

    let body = get_activity(&h, &alice_token, &gid, &task_id, "").await;
    let items = body["items"].as_array().expect("A6 items");
    let assigned_row = items
        .iter()
        .find(|it| it["kind"] == "assigned")
        .expect("A6 assigned row");
    // assignee_id is a UUID (not PII); it should be present.
    assert!(
        assigned_row["payload"].get("assignee_id").is_some(),
        "A6 payload.assignee_id present"
    );
    // PII guard: no raw email or display_name in payload.
    assert!(
        assigned_row["payload"].get("email").is_none(),
        "A6 no email in payload"
    );
    assert!(
        assigned_row["payload"].get("display_name").is_none(),
        "A6 no display_name in payload"
    );

    // ── A7. Cross-group: Alice cannot see Bob's task activity ─────────────
    let (_, bob_task_id) = create_task_list_and_task(&h, &bob_token, &g2id).await;
    let uri = format!("/v1/groups/{gid}/tasks/{bob_task_id}/activity");
    let resp = h
        .router
        .clone()
        .oneshot(auth_req("GET", &uri, Some(&alice_token), Some(&gid), None))
        .await
        .expect("A7 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "A7 cross-group activity must return 404"
    );

    // ── A8. Cursor pagination: limit=1 returns next_cursor ───────────────
    // At this point task_id has several activity rows (created, status_changed,
    // priority_changed, commented, assigned). limit=1 should return exactly
    // 1 item and a non-null next_cursor.
    let body = get_activity(&h, &alice_token, &gid, &task_id, "limit=1").await;
    let items = body["items"].as_array().expect("A8 items");
    assert_eq!(items.len(), 1, "A8 limit=1 returns exactly 1 item");
    let next_cursor = body["next_cursor"]
        .as_str()
        .expect("A8 next_cursor must be non-null when more items exist");
    assert!(!next_cursor.is_empty(), "A8 next_cursor non-empty");

    // Fetch the second page using the cursor.
    let body2 = get_activity(
        &h,
        &alice_token,
        &gid,
        &task_id,
        &format!("limit=1&cursor={next_cursor}"),
    )
    .await;
    let items2 = body2["items"].as_array().expect("A8 page2 items");
    assert_eq!(items2.len(), 1, "A8 page 2 also has 1 item");
    // Verify the two pages return different IDs.
    assert_ne!(
        items[0]["id"], items2[0]["id"],
        "A8 page 1 and page 2 return different activity rows"
    );
}
