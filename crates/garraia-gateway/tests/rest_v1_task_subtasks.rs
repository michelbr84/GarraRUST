//! Integration tests for the subtasks REST API (plan 0083, GAR-546, slice 9).
//!
//! All scenarios bundled into ONE `#[tokio::test]` — same pattern as previous
//! task-slice tests. Splitting triggers the sqlx runtime-teardown race
//! documented in plan 0016 M3.
//!
//! Scenarios (8 total):
//!
//!   S1. Happy path — create a subtask with `parent_task_id`; response includes
//!       `parent_task_id`; `task_activity` has a `created` row with the parent ID.
//!   S2. GET /subtasks — returns the child created in S1; `parent_task_id` absent
//!       from `TaskSummary` items (compact shape).
//!   S3. GET /subtasks — unknown parent task → 404.
//!   S4. GET /subtasks — soft-deleted parent → 404.
//!   S5. POST create with parent_task_id from Bob's group → 404
//!       (RLS + group_id check: cross-group injection).
//!   S6. POST create with depth > 1 (grandchild) → 400 "max nesting depth exceeded".
//!   S7. GET /subtasks — cursor pagination: 3 subtasks, limit=2, cursor page returns remaining.
//!   S8. GET /subtasks — status filter: only matching subtasks returned.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

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

/// POST a task-list and return the new list_id string.
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
    assert_eq!(resp.status(), StatusCode::CREATED, "setup: task-list 201");
    body_json(resp).await["id"]
        .as_str()
        .expect("list id")
        .to_string()
}

/// POST a task (no parent) and return the task_id string.
async fn create_root_task(
    h: &Harness,
    token: &str,
    group_id: &str,
    list_id: &str,
    title: &str,
) -> String {
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{group_id}/task-lists/{list_id}/tasks"),
            Some(token),
            Some(group_id),
            Some(json!({ "title": title })),
        ))
        .await
        .expect("POST task oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "setup: root task 201");
    body_json(resp).await["id"]
        .as_str()
        .expect("task id")
        .to_string()
}

/// POST a task with a parent and return the full response body.
async fn create_subtask(
    h: &Harness,
    token: &str,
    group_id: &str,
    list_id: &str,
    title: &str,
    parent_task_id: &str,
    expected_status: StatusCode,
) -> serde_json::Value {
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{group_id}/task-lists/{list_id}/tasks"),
            Some(token),
            Some(group_id),
            Some(json!({ "title": title, "parent_task_id": parent_task_id })),
        ))
        .await
        .expect("POST subtask oneshot");
    assert_eq!(
        resp.status(),
        expected_status,
        "create_subtask {expected_status}"
    );
    body_json(resp).await
}

/// GET /subtasks and return the full response body.
async fn get_subtasks(
    h: &Harness,
    token: &str,
    group_id: &str,
    task_id: &str,
    query: &str,
) -> axum::response::Response {
    h.router
        .clone()
        .oneshot(auth_req(
            "GET",
            &format!("/v1/groups/{group_id}/tasks/{task_id}/subtasks{query}"),
            Some(token),
            Some(group_id),
            None,
        ))
        .await
        .expect("GET subtasks oneshot")
}

/// Soft-delete a task via DELETE endpoint.
async fn delete_task(h: &Harness, token: &str, group_id: &str, task_id: &str) {
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "DELETE",
            &format!("/v1/groups/{group_id}/tasks/{task_id}"),
            Some(token),
            Some(group_id),
            None,
        ))
        .await
        .expect("DELETE task oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "setup: delete task 204"
    );
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(feature = "test-helpers")]
#[tokio::test]
async fn rest_v1_task_subtasks_scenarios() {
    let h = Harness::get().await;

    // Seed Alice (owner of group A) and Bob (owner of group B).
    let (_alice_id, alice_group_id, alice_token) = seed_user_with_group(&h, "alice@subtasks.test")
        .await
        .expect("seed alice+group");
    let (_bob_id, bob_group_id, bob_token) = seed_user_with_group(&h, "bob@subtasks.test")
        .await
        .expect("seed bob+group");

    let agid = alice_group_id.to_string();
    let bgid = bob_group_id.to_string();

    // Setup: one task list for Alice and one for Bob.
    let alice_list = create_task_list(&h, &alice_token, &agid, "Alice's List").await;
    let bob_list = create_task_list(&h, &bob_token, &bgid, "Bob's List").await;

    // Setup: a root task in Alice's list (will be the parent).
    let parent_id = create_root_task(&h, &alice_token, &agid, &alice_list, "Parent Task").await;

    // ── S1. Happy path — create subtask ────────────────────────────────────
    let body = create_subtask(
        &h,
        &alice_token,
        &agid,
        &alice_list,
        "Child Task 1",
        &parent_id,
        StatusCode::CREATED,
    )
    .await;

    assert_eq!(
        body["parent_task_id"].as_str(),
        Some(parent_id.as_str()),
        "S1 parent_task_id in response"
    );
    let child1_id = body["id"].as_str().expect("child1 id").to_string();

    // Verify task_activity has a `created` row with parent_task_id.
    let child1_uuid: Uuid = child1_id.parse().expect("child1 uuid");
    let activity: Vec<(String, serde_json::Value)> = sqlx::query_as(
        "SELECT kind, payload FROM task_activity WHERE task_id = $1 ORDER BY created_at DESC",
    )
    .bind(child1_uuid)
    .fetch_all(&h.admin_pool)
    .await
    .expect("SELECT task_activity");
    assert!(!activity.is_empty(), "S1 activity row exists");
    assert_eq!(activity[0].0, "created", "S1 activity kind=created");
    assert_eq!(
        activity[0].1["parent_task_id"].as_str(),
        Some(parent_id.as_str()),
        "S1 activity payload has parent_task_id"
    );

    // ── S2. GET /subtasks — returns child created in S1 ────────────────────
    let resp = get_subtasks(&h, &alice_token, &agid, &parent_id, "").await;
    assert_eq!(resp.status(), StatusCode::OK, "S2 GET subtasks 200");
    let body = body_json(resp).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1, "S2 one subtask returned");
    assert_eq!(
        items[0]["id"].as_str(),
        Some(child1_id.as_str()),
        "S2 correct child id"
    );

    // ── S3. GET /subtasks — unknown parent task → 404 ──────────────────────
    let unknown_id = Uuid::new_v4().to_string();
    let resp = get_subtasks(&h, &alice_token, &agid, &unknown_id, "").await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "S3 unknown parent 404"
    );

    // ── S4. GET /subtasks — soft-deleted parent → 404 ──────────────────────
    let to_delete = create_root_task(&h, &alice_token, &agid, &alice_list, "Task to delete").await;
    delete_task(&h, &alice_token, &agid, &to_delete).await;
    let resp = get_subtasks(&h, &alice_token, &agid, &to_delete, "").await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "S4 deleted parent 404"
    );

    // ── S5. Cross-group parent injection — Bob's parent_task_id in Alice's request ──
    let bob_root = create_root_task(&h, &bob_token, &bgid, &bob_list, "Bob's Root Task").await;
    let body = create_subtask(
        &h,
        &alice_token,
        &agid,
        &alice_list,
        "Injected child",
        &bob_root, // parent in Bob's group — should fail
        StatusCode::NOT_FOUND,
    )
    .await;
    // 404 because parent lookup is scoped to Alice's group_id via RLS
    let _ = body;

    // ── S6. Depth > 1 (grandchild) → 400 ──────────────────────────────────
    // child1 already has a parent (parent_id) → child1.parent_task_id IS NOT NULL
    let body = create_subtask(
        &h,
        &alice_token,
        &agid,
        &alice_list,
        "Grandchild Task",
        &child1_id, // child1 is not a root task
        StatusCode::BAD_REQUEST,
    )
    .await;
    let detail = body["detail"].as_str().unwrap_or("");
    assert!(
        detail.contains("nesting depth"),
        "S6 error mentions nesting depth, got: {detail}"
    );

    // ── S7. Cursor pagination — 3 subtasks, limit=2 ────────────────────────
    // child1 already exists; create two more.
    let _child2_id = {
        let b = create_subtask(
            &h,
            &alice_token,
            &agid,
            &alice_list,
            "Child Task 2",
            &parent_id,
            StatusCode::CREATED,
        )
        .await;
        b["id"].as_str().expect("child2").to_string()
    };
    let _child3_id = {
        let b = create_subtask(
            &h,
            &alice_token,
            &agid,
            &alice_list,
            "Child Task 3",
            &parent_id,
            StatusCode::CREATED,
        )
        .await;
        b["id"].as_str().expect("child3").to_string()
    };

    // Page 1: limit=2, newest first.
    let resp = get_subtasks(&h, &alice_token, &agid, &parent_id, "?limit=2").await;
    assert_eq!(resp.status(), StatusCode::OK, "S7 page1 200");
    let body = body_json(resp).await;
    let page1 = body["items"].as_array().expect("page1 items");
    assert_eq!(page1.len(), 2, "S7 page1 has 2 items");
    let cursor = body["next_cursor"]
        .as_str()
        .expect("S7 next_cursor present");

    // Page 2: use cursor.
    let resp = get_subtasks(
        &h,
        &alice_token,
        &agid,
        &parent_id,
        &format!("?limit=2&cursor={cursor}"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK, "S7 page2 200");
    let body = body_json(resp).await;
    let page2 = body["items"].as_array().expect("page2 items");
    assert_eq!(page2.len(), 1, "S7 page2 has 1 remaining item");
    assert!(body["next_cursor"].is_null(), "S7 page2 no more cursor");

    // ── S8. Status filter ──────────────────────────────────────────────────
    // Patch child1 to status=done.
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "PATCH",
            &format!("/v1/groups/{agid}/tasks/{child1_id}"),
            Some(&alice_token),
            Some(&agid),
            Some(json!({ "status": "done" })),
        ))
        .await
        .expect("PATCH task oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "S8 patch to done 200");

    // Filter by status=done — should return only child1.
    let resp = get_subtasks(&h, &alice_token, &agid, &parent_id, "?status=done").await;
    assert_eq!(resp.status(), StatusCode::OK, "S8 filter=done 200");
    let body = body_json(resp).await;
    let done_items = body["items"].as_array().expect("done items");
    assert_eq!(done_items.len(), 1, "S8 only one done subtask");
    assert_eq!(
        done_items[0]["id"].as_str(),
        Some(child1_id.as_str()),
        "S8 correct done subtask"
    );

    // Filter by status=todo — should NOT include child1.
    let resp = get_subtasks(&h, &alice_token, &agid, &parent_id, "?status=todo").await;
    assert_eq!(resp.status(), StatusCode::OK, "S8 filter=todo 200");
    let body = body_json(resp).await;
    let todo_items = body["items"].as_array().expect("todo items");
    for item in todo_items {
        assert_ne!(
            item["id"].as_str(),
            Some(child1_id.as_str()),
            "S8 done child not in todo filter"
        );
    }
}
