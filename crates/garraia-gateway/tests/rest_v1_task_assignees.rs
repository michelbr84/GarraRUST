//! Integration tests for task assignees REST API (plan 0077, GAR-533).
//!
//! All scenarios bundled into ONE `#[tokio::test]` — same pattern as
//! `rest_v1_tasks.rs` / `rest_v1_memory.rs`. Splitting triggers the
//! sqlx runtime-teardown race documented in plan 0016 M3.
//!
//! Scenarios (9 total):
//!
//!   A1.  POST 201 — assign group member; assert response shape + audit row.
//!   A2.  POST 409 — duplicate assignee returns 409 Conflict.
//!   A3.  POST 404 — target user not an active member of the group (cross-group injection guard).
//!   A4.  POST 404 — task not in group (cross-group task isolation).
//!   A5.  GET 200 — list assignees; returns A1 entry.
//!   A6.  DELETE 204 — remove assignee; verify audit row emitted.
//!   A7.  DELETE 204 — idempotent: user was just removed; returns 204 again.
//!   A8.  GET 404 + DELETE 404 — unknown task_id returns 404 on both endpoints.
//!   A9.  POST 403 — path group_id ≠ principal group_id returns 403.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

use common::Harness;
use common::fixtures::{fetch_audit_events_for_group, seed_member_via_admin, seed_user_with_group};

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

// Create a task list, returns (task_list_id, task_id) for the created entries.
async fn create_task_list_and_task(h: &Harness, token: &str, group_id: &str) -> (String, String) {
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{group_id}/task-lists"),
            Some(token),
            Some(group_id),
            Some(json!({ "name": "Assignee Test List", "type": "list" })),
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
            Some(json!({ "title": "Task with assignees" })),
        ))
        .await
        .expect("create task");
    assert_eq!(resp.status(), StatusCode::CREATED, "setup: task 201");
    let task_body = body_json(resp).await;
    let task_id = task_body["id"].as_str().expect("task id").to_string();

    (list_id, task_id)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(feature = "test-helpers")]
#[tokio::test]
async fn rest_v1_task_assignees_scenarios() {
    let h = Harness::get().await;

    // Seed Alice as group owner, Charlie as a member of Alice's group.
    // Bob owns a separate group for isolation tests.
    let (_alice_id, group_id, alice_token) = seed_user_with_group(&h, "alice@assignees.test")
        .await
        .expect("seed alice+group");
    let (charlie_id, _charlie_token) =
        seed_member_via_admin(&h, group_id, "member", "charlie@assignees.test")
            .await
            .expect("seed charlie as member");
    let (_, bob_group_id, bob_token) = seed_user_with_group(&h, "bob@assignees.test")
        .await
        .expect("seed bob+group2");

    let gid = group_id.to_string();
    let g2id = bob_group_id.to_string();
    let charlie_str = charlie_id.to_string();

    // Set up a task list + task under Alice's group.
    let (_list_id, task_id) = create_task_list_and_task(&h, &alice_token, &gid).await;

    // Set up a task under Bob's group (for cross-group isolation).
    let (_bob_list_id, bob_task_id) = create_task_list_and_task(&h, &bob_token, &g2id).await;

    // ── A1. POST 201 — assign group member; response shape + audit row ─────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{gid}/tasks/{task_id}/assignees"),
            Some(&alice_token),
            Some(&gid),
            Some(json!({ "user_id": charlie_str })),
        ))
        .await
        .expect("A1 oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "A1 status");
    let a1_body = body_json(resp).await;
    assert_eq!(a1_body["task_id"], task_id, "A1 task_id matches");
    assert_eq!(a1_body["user_id"], charlie_str, "A1 user_id matches");
    assert!(
        a1_body.get("assigned_at").is_some(),
        "A1 assigned_at present"
    );
    // assigned_by should be Alice's user_id (non-null since caller assigned).
    assert!(
        !a1_body["assigned_by"].is_null(),
        "A1 assigned_by is non-null"
    );

    // Verify audit row: action = "task.assignee.added", resource = task_id.
    let audit = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("A1 fetch audit");
    let a1_audit = audit
        .iter()
        .find(|e| e.0 == "task.assignee.added")
        .expect("A1 audit row task.assignee.added");
    let (_, _, a1_res_type, a1_res_id, a1_meta) = a1_audit;
    assert_eq!(a1_res_type, "task_assignees", "A1 audit resource_type");
    assert_eq!(a1_res_id, &task_id, "A1 audit resource_id is task_id");
    assert!(
        a1_meta["assignee_user_id_len"].as_u64().is_some(),
        "A1 audit metadata has assignee_user_id_len"
    );
    assert!(
        a1_meta.get("user_id").is_none(),
        "A1 audit must not contain user_id value (PII guard)"
    );

    // ── A2. POST 409 — duplicate assignee ─────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{gid}/tasks/{task_id}/assignees"),
            Some(&alice_token),
            Some(&gid),
            Some(json!({ "user_id": charlie_str })),
        ))
        .await
        .expect("A2 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "A2 duplicate assignee must be 409"
    );

    // ── A3. POST 404 — target user not a member of this group ─────────────
    // Use a freshly generated UUID that is not in Alice's group_members.
    let outsider_id = uuid::Uuid::new_v4().to_string();
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{gid}/tasks/{task_id}/assignees"),
            Some(&alice_token),
            Some(&gid),
            Some(json!({ "user_id": outsider_id })),
        ))
        .await
        .expect("A3 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "A3 non-member user must be 404 (never 403)"
    );

    // ── A4. POST 404 — task not in caller's group (cross-group task) ───────
    // Alice uses her own group context but references Bob's task_id.
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{gid}/tasks/{bob_task_id}/assignees"),
            Some(&alice_token),
            Some(&gid),
            Some(json!({ "user_id": charlie_str })),
        ))
        .await
        .expect("A4 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "A4 cross-group task must be 404"
    );

    // ── A5. GET 200 — list assignees; includes A1 entry ──────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "GET",
            &format!("/v1/groups/{gid}/tasks/{task_id}/assignees"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("A5 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "A5 status");
    let a5_body = body_json(resp).await;
    let items = a5_body.as_array().expect("A5 response is array");
    assert!(
        items.iter().any(|it| it["user_id"] == charlie_str),
        "A5 should contain charlie's assignee entry"
    );
    for item in items {
        assert_eq!(
            item["task_id"], task_id,
            "A5 all items reference correct task_id"
        );
    }

    // ── A6. DELETE 204 — remove assignee; verify audit row ────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "DELETE",
            &format!("/v1/groups/{gid}/tasks/{task_id}/assignees/{charlie_str}"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("A6 oneshot");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT, "A6 status");

    // Assignee must no longer appear in the list.
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "GET",
            &format!("/v1/groups/{gid}/tasks/{task_id}/assignees"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("A6 list after remove");
    let a6_list = body_json(resp).await;
    let after_del = a6_list.as_array().expect("A6 items array");
    assert!(
        !after_del.iter().any(|it| it["user_id"] == charlie_str),
        "A6 removed assignee must not appear in GET list"
    );

    // Verify audit row: action = "task.assignee.removed".
    let audit2 = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("A6 fetch audit");
    assert!(
        audit2
            .iter()
            .any(|e| e.0 == "task.assignee.removed" && e.3 == task_id),
        "A6 audit row task.assignee.removed must be present"
    );

    // ── A7. DELETE 204 — idempotent: charlie already removed → 204 again ──
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "DELETE",
            &format!("/v1/groups/{gid}/tasks/{task_id}/assignees/{charlie_str}"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("A7 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "A7 idempotent DELETE must return 204"
    );

    // ── A8. GET 404 + DELETE 404 — unknown task_id ────────────────────────
    let unknown_task = uuid::Uuid::new_v4().to_string();

    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "GET",
            &format!("/v1/groups/{gid}/tasks/{unknown_task}/assignees"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("A8 GET oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "A8 GET unknown task must be 404"
    );

    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "DELETE",
            &format!("/v1/groups/{gid}/tasks/{unknown_task}/assignees/{charlie_str}"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("A8 DELETE oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "A8 DELETE unknown task must be 404"
    );

    // ── A9. POST 403 — path group_id ≠ principal group_id ─────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{g2id}/tasks/{task_id}/assignees"),
            Some(&alice_token),
            Some(&gid), // X-Group-Id = alice's group
            Some(json!({ "user_id": charlie_str })),
        ))
        .await
        .expect("A9 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "A9 path group_id mismatch must return 403"
    );
}
