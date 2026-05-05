//! Integration tests for task-lists + tasks REST API (plan 0066/0067, GAR-516/GAR-518).
//!
//! All scenarios bundled into ONE `#[tokio::test]` — same pattern as
//! `rest_v1_memory.rs` / `rest_v1_chats.rs`. Splitting triggers the
//! sqlx runtime-teardown race documented in plan 0016 M3.
//!
//! Scenarios (21 total):
//!
//! Task-list scenarios (10):
//!   TL1.  POST 201 — create task list; assert response shape + audit row.
//!   TL2.  POST 400 — invalid list type.
//!   TL3.  POST 401 — missing bearer.
//!   TL4.  POST 403 — path group_id ≠ principal group_id.
//!   TL5.  GET 200 — list task lists; returns created list; excludes other groups.
//!   TL6.  PATCH 200 — update name + type; audit row present.
//!   TL7.  PATCH 404 — unknown list_id.
//!   TL8.  PATCH 200 — description null-clear (`{"description": null}`).
//!   TL9.  DELETE 204 — archive task list; no longer in GET /task-lists.
//!   TL10. DELETE 204 — already-archived list (idempotent).
//!
//! Task scenarios (11):
//!   T1.  POST 201 — create task; assert response shape + audit row.
//!   T2.  POST 400 — title too long (>500 chars).
//!   T3.  POST 404 — unknown list_id.
//!   T4.  GET 200 — list tasks; includes created task; excludes deleted.
//!   T5.  PATCH 200 — update status; verify status changed in response.
//!   T6.  DELETE 204 — soft-delete; task no longer returned in list.
//!   T7.  DELETE 404 — cross-tenant task returns 404 (RLS isolation).
//!   T8.  GET 200 — get single task by ID.
//!   T9.  GET 404 — cross-group task returns 404 (RLS isolation).
//!   T10. GET 404 — deleted task returns 404.
//!   T11. PATCH 403 — path group_id ≠ principal group_id returns 403.

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

fn post_task_list(
    token: Option<&str>,
    x_group_id: Option<&str>,
    path_group_id: &str,
    body: serde_json::Value,
) -> Request<Body> {
    auth_req(
        "POST",
        &format!("/v1/groups/{path_group_id}/task-lists"),
        token,
        x_group_id,
        Some(body),
    )
}

fn get_task_lists(
    token: Option<&str>,
    x_group_id: Option<&str>,
    path_group_id: &str,
) -> Request<Body> {
    auth_req(
        "GET",
        &format!("/v1/groups/{path_group_id}/task-lists"),
        token,
        x_group_id,
        None,
    )
}

fn patch_task_list_req(
    token: Option<&str>,
    x_group_id: Option<&str>,
    path_group_id: &str,
    list_id: &str,
    body: serde_json::Value,
) -> Request<Body> {
    auth_req(
        "PATCH",
        &format!("/v1/groups/{path_group_id}/task-lists/{list_id}"),
        token,
        x_group_id,
        Some(body),
    )
}

fn delete_task_list_req(
    token: Option<&str>,
    x_group_id: Option<&str>,
    path_group_id: &str,
    list_id: &str,
) -> Request<Body> {
    auth_req(
        "DELETE",
        &format!("/v1/groups/{path_group_id}/task-lists/{list_id}"),
        token,
        x_group_id,
        None,
    )
}

fn post_task(
    token: Option<&str>,
    x_group_id: Option<&str>,
    path_group_id: &str,
    list_id: &str,
    body: serde_json::Value,
) -> Request<Body> {
    auth_req(
        "POST",
        &format!("/v1/groups/{path_group_id}/task-lists/{list_id}/tasks"),
        token,
        x_group_id,
        Some(body),
    )
}

fn get_tasks(
    token: Option<&str>,
    x_group_id: Option<&str>,
    path_group_id: &str,
    list_id: &str,
    status_filter: Option<&str>,
) -> Request<Body> {
    let mut uri = format!("/v1/groups/{path_group_id}/task-lists/{list_id}/tasks");
    if let Some(s) = status_filter {
        uri.push_str(&format!("?status={s}"));
    }
    auth_req("GET", &uri, token, x_group_id, None)
}

fn patch_task_req(
    token: Option<&str>,
    x_group_id: Option<&str>,
    path_group_id: &str,
    task_id: &str,
    body: serde_json::Value,
) -> Request<Body> {
    auth_req(
        "PATCH",
        &format!("/v1/groups/{path_group_id}/tasks/{task_id}"),
        token,
        x_group_id,
        Some(body),
    )
}

fn delete_task_req(
    token: Option<&str>,
    x_group_id: Option<&str>,
    path_group_id: &str,
    task_id: &str,
) -> Request<Body> {
    auth_req(
        "DELETE",
        &format!("/v1/groups/{path_group_id}/tasks/{task_id}"),
        token,
        x_group_id,
        None,
    )
}

fn get_task_req(
    token: Option<&str>,
    x_group_id: Option<&str>,
    path_group_id: &str,
    task_id: &str,
) -> Request<Body> {
    auth_req(
        "GET",
        &format!("/v1/groups/{path_group_id}/tasks/{task_id}"),
        token,
        x_group_id,
        None,
    )
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(feature = "test-helpers")]
#[tokio::test]
async fn rest_v1_tasks_scenarios() {
    let h = Harness::get().await;

    // Seed two independent groups for cross-tenant isolation tests.
    let (_owner_id, group_id, owner_token) = seed_user_with_group(&h, "alice@tasks-slice12.test")
        .await
        .expect("seed alice+group1");
    let (_, group2_id, owner2_token) = seed_user_with_group(&h, "bob@tasks-slice12.test")
        .await
        .expect("seed bob+group2");

    let gid = group_id.to_string();
    let g2id = group2_id.to_string();

    // ── TL1. POST 201 — create task list happy path ──────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(post_task_list(
            Some(&owner_token),
            Some(&gid),
            &gid,
            json!({ "name": "Sprint Backlog", "type": "list", "description": "Initial backlog" }),
        ))
        .await
        .expect("TL1 oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "TL1 status");
    let tl1_body = body_json(resp).await;
    let list_id = tl1_body["id"].as_str().expect("TL1 id").to_string();
    assert_eq!(tl1_body["name"], "Sprint Backlog", "TL1 name");
    assert_eq!(tl1_body["type"], "list", "TL1 type");
    assert_eq!(tl1_body["group_id"], gid, "TL1 group_id");
    assert_eq!(
        tl1_body["description"], "Initial backlog",
        "TL1 description"
    );
    assert!(
        tl1_body.get("created_by_label").is_some(),
        "TL1 created_by_label present"
    );

    // Verify audit row: action = "task_list.created", no name in metadata.
    let audit = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("TL1 fetch audit");
    let tl_audit = audit
        .iter()
        .find(|e| e.0 == "task_list.created")
        .expect("TL1 audit row");
    let (_, _, resource_type, resource_id, metadata) = tl_audit;
    assert_eq!(resource_type, "task_lists", "TL1 audit resource_type");
    assert_eq!(resource_id, &list_id, "TL1 audit resource_id");
    assert!(
        metadata["name_len"].as_u64().is_some(),
        "TL1 audit metadata has name_len"
    );
    assert!(
        metadata.get("name").is_none(),
        "TL1 audit metadata must not contain name text (PII guard)"
    );

    // ── TL2. POST 400 — invalid list type ────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(post_task_list(
            Some(&owner_token),
            Some(&gid),
            &gid,
            json!({ "name": "Bad", "type": "spreadsheet" }),
        ))
        .await
        .expect("TL2 oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "TL2 status");

    // ── TL3. POST 401 — missing bearer ───────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(post_task_list(
            None,
            Some(&gid),
            &gid,
            json!({ "name": "No auth", "type": "board" }),
        ))
        .await
        .expect("TL3 oneshot");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "TL3 status");

    // ── TL4. POST 403 — path group_id ≠ principal group_id ───────────────
    let resp = h
        .router
        .clone()
        .oneshot(post_task_list(
            Some(&owner_token),
            Some(&gid),
            &g2id,
            json!({ "name": "Cross group", "type": "list" }),
        ))
        .await
        .expect("TL4 oneshot");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "TL4 status");

    // ── TL5. GET 200 — list task lists; returns TL1, excludes other groups
    let resp = h
        .router
        .clone()
        .oneshot(get_task_lists(Some(&owner_token), Some(&gid), &gid))
        .await
        .expect("TL5 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "TL5 status");
    let tl5_body = body_json(resp).await;
    let items = tl5_body["items"].as_array().expect("TL5 items array");
    assert!(
        items.iter().any(|it| it["id"] == list_id),
        "TL5 should contain the created list"
    );
    assert!(
        items.iter().all(|it| it["group_id"] == gid),
        "TL5 must not leak other groups' task lists"
    );

    // ── TL6. PATCH 200 — update name + type ──────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(patch_task_list_req(
            Some(&owner_token),
            Some(&gid),
            &gid,
            &list_id,
            json!({ "name": "Kanban Board", "type": "board" }),
        ))
        .await
        .expect("TL6 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "TL6 status");
    let tl6_body = body_json(resp).await;
    assert_eq!(tl6_body["name"], "Kanban Board", "TL6 name updated");
    assert_eq!(tl6_body["type"], "board", "TL6 type updated");
    assert_eq!(tl6_body["id"], list_id, "TL6 id unchanged");
    // Description was not sent → should remain unchanged.
    assert_eq!(
        tl6_body["description"], "Initial backlog",
        "TL6 description unchanged when key omitted"
    );

    // Verify audit: "task_list.updated", name_len present, no name text.
    let audit2 = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("TL6 fetch audit");
    let tl6_audit = audit2
        .iter()
        .find(|e| e.0 == "task_list.updated")
        .expect("TL6 audit row");
    let (_, _, _, _, tl6_meta) = tl6_audit;
    assert!(
        tl6_meta["name_len"].as_u64().is_some(),
        "TL6 audit has name_len"
    );
    assert!(
        tl6_meta.get("name").is_none(),
        "TL6 audit must not contain name text (PII guard)"
    );
    assert_eq!(tl6_meta["type"], "board", "TL6 audit type reflects new value");

    // ── TL7. PATCH 404 — unknown list_id ─────────────────────────────────
    let unknown_list = uuid::Uuid::new_v4().to_string();
    let resp = h
        .router
        .clone()
        .oneshot(patch_task_list_req(
            Some(&owner_token),
            Some(&gid),
            &gid,
            &unknown_list,
            json!({ "name": "Ghost" }),
        ))
        .await
        .expect("TL7 oneshot");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "TL7 status");

    // ── TL8. PATCH 200 — description null-clear ───────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(patch_task_list_req(
            Some(&owner_token),
            Some(&gid),
            &gid,
            &list_id,
            json!({ "description": null }),
        ))
        .await
        .expect("TL8 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "TL8 status");
    let tl8_body = body_json(resp).await;
    assert!(
        tl8_body["description"].is_null(),
        "TL8 description cleared to null"
    );
    // Name must still be the value from TL6.
    assert_eq!(tl8_body["name"], "Kanban Board", "TL8 name unchanged");

    // ── TL9. DELETE 204 — archive task list ───────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(delete_task_list_req(
            Some(&owner_token),
            Some(&gid),
            &gid,
            &list_id,
        ))
        .await
        .expect("TL9 oneshot");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT, "TL9 status");

    // Archived list should NOT appear in GET /task-lists.
    let resp = h
        .router
        .clone()
        .oneshot(get_task_lists(Some(&owner_token), Some(&gid), &gid))
        .await
        .expect("TL9 list after archive");
    let tl9_list = body_json(resp).await;
    let after_archive = tl9_list["items"].as_array().expect("TL9 items");
    assert!(
        !after_archive.iter().any(|it| it["id"] == list_id),
        "TL9 archived list must not appear in GET /task-lists"
    );

    // Verify audit: "task_list.archived" present.
    let audit3 = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("TL9 fetch audit");
    assert!(
        audit3
            .iter()
            .any(|e| e.0 == "task_list.archived" && e.3 == list_id),
        "TL9 audit row task_list.archived must be present"
    );

    // ── TL10. DELETE 204 — idempotent: already-archived list ─────────────
    let resp = h
        .router
        .clone()
        .oneshot(delete_task_list_req(
            Some(&owner_token),
            Some(&gid),
            &gid,
            &list_id,
        ))
        .await
        .expect("TL10 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "TL10 re-archive must return 204 (idempotent)"
    );

    // ── T1. POST 201 — create task in list ───────────────────────────────
    // Create a fresh list for task scenarios (the original is archived).
    let resp = h
        .router
        .clone()
        .oneshot(post_task_list(
            Some(&owner_token),
            Some(&gid),
            &gid,
            json!({ "name": "Task List 2", "type": "list" }),
        ))
        .await
        .expect("T1 create list");
    let tl2_body = body_json(resp).await;
    let list2_id = tl2_body["id"].as_str().expect("T1 list2 id").to_string();

    let resp = h
        .router
        .clone()
        .oneshot(post_task(
            Some(&owner_token),
            Some(&gid),
            &gid,
            &list2_id,
            json!({
                "title": "Implement login",
                "status": "in_progress",
                "priority": "high",
            }),
        ))
        .await
        .expect("T1 oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "T1 status");
    let t1_body = body_json(resp).await;
    let task_id = t1_body["id"].as_str().expect("T1 id").to_string();
    assert_eq!(t1_body["title"], "Implement login", "T1 title");
    assert_eq!(t1_body["status"], "in_progress", "T1 status field");
    assert_eq!(t1_body["priority"], "high", "T1 priority");
    assert_eq!(t1_body["list_id"], list2_id, "T1 list_id");
    assert_eq!(t1_body["group_id"], gid, "T1 group_id");

    // Verify audit: action = "task.created", no title text in metadata.
    let audit4 = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("T1 fetch audit");
    let t_audit = audit4
        .iter()
        .find(|e| e.0 == "task.created")
        .expect("T1 audit row");
    let (_, _, t_res_type, t_res_id, t_meta) = t_audit;
    assert_eq!(t_res_type, "tasks", "T1 audit resource_type");
    assert_eq!(t_res_id, &task_id, "T1 audit resource_id");
    assert!(
        t_meta["title_len"].as_u64().is_some(),
        "T1 audit metadata has title_len"
    );
    assert!(
        t_meta.get("title").is_none(),
        "T1 audit metadata must not contain title text (PII guard)"
    );

    // ── T2. POST 400 — title too long ────────────────────────────────────
    let long_title = "x".repeat(501);
    let resp = h
        .router
        .clone()
        .oneshot(post_task(
            Some(&owner_token),
            Some(&gid),
            &gid,
            &list2_id,
            json!({ "title": long_title }),
        ))
        .await
        .expect("T2 oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "T2 status");

    // ── T3. POST 404 — unknown list_id ───────────────────────────────────
    let unknown_list2 = uuid::Uuid::new_v4().to_string();
    let resp = h
        .router
        .clone()
        .oneshot(post_task(
            Some(&owner_token),
            Some(&gid),
            &gid,
            &unknown_list2,
            json!({ "title": "Orphan task" }),
        ))
        .await
        .expect("T3 oneshot");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "T3 status");

    // ── T4. GET 200 — list tasks; includes T1; excludes deleted ──────────
    let resp = h
        .router
        .clone()
        .oneshot(get_tasks(
            Some(&owner_token),
            Some(&gid),
            &gid,
            &list2_id,
            None,
        ))
        .await
        .expect("T4 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "T4 status");
    let t4_body = body_json(resp).await;
    let tasks = t4_body["items"].as_array().expect("T4 items array");
    assert!(
        tasks.iter().any(|it| it["id"] == task_id),
        "T4 should contain the created task"
    );

    // ── T5. PATCH 200 — update task status ───────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(patch_task_req(
            Some(&owner_token),
            Some(&gid),
            &gid,
            &task_id,
            json!({ "status": "done", "priority": "low" }),
        ))
        .await
        .expect("T5 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "T5 status");
    let t5_body = body_json(resp).await;
    assert_eq!(t5_body["status"], "done", "T5 status updated");
    assert_eq!(t5_body["priority"], "low", "T5 priority updated");
    assert_eq!(t5_body["title"], "Implement login", "T5 title unchanged");

    // ── T6. DELETE 204 — soft-delete; task no longer in list ─────────────
    let resp = h
        .router
        .clone()
        .oneshot(delete_task_req(
            Some(&owner_token),
            Some(&gid),
            &gid,
            &task_id,
        ))
        .await
        .expect("T6 oneshot");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT, "T6 status");

    let resp = h
        .router
        .clone()
        .oneshot(get_tasks(
            Some(&owner_token),
            Some(&gid),
            &gid,
            &list2_id,
            None,
        ))
        .await
        .expect("T6 list after delete");
    let t6_list_body = body_json(resp).await;
    let remaining = t6_list_body["items"].as_array().expect("T6 items array");
    assert!(
        !remaining.iter().any(|it| it["id"] == task_id),
        "T6 deleted task must not appear in list"
    );

    let audit5 = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("T6 fetch audit");
    let del_audit = audit5
        .iter()
        .find(|e| e.0 == "task.deleted" && e.3 == task_id)
        .expect("T6 audit row");
    let (_, _, _, _, del_meta) = del_audit;
    assert!(
        del_meta["title_len"].as_u64().is_some(),
        "T6 audit has title_len"
    );
    assert!(
        del_meta.get("title").is_none(),
        "T6 audit must not contain title text (PII guard)"
    );

    // ── T7. DELETE 404 — cross-tenant task ───────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(post_task_list(
            Some(&owner2_token),
            Some(&g2id),
            &g2id,
            json!({ "name": "Bob list", "type": "board" }),
        ))
        .await
        .expect("T7 create bob list");
    assert_eq!(resp.status(), StatusCode::CREATED, "T7 bob list created");
    let bob_list_body = body_json(resp).await;
    let bob_list_id = bob_list_body["id"]
        .as_str()
        .expect("T7 bob list id")
        .to_string();

    let resp = h
        .router
        .clone()
        .oneshot(post_task(
            Some(&owner2_token),
            Some(&g2id),
            &g2id,
            &bob_list_id,
            json!({ "title": "Bob's secret task" }),
        ))
        .await
        .expect("T7 create bob task");
    assert_eq!(resp.status(), StatusCode::CREATED, "T7 bob task created");
    let bob_task_body = body_json(resp).await;
    let bob_task_id = bob_task_body["id"]
        .as_str()
        .expect("T7 bob task id")
        .to_string();

    // Alice tries to delete Bob's task — RLS filters it → 404.
    let resp = h
        .router
        .clone()
        .oneshot(delete_task_req(
            Some(&owner_token),
            Some(&gid),
            &gid,
            &bob_task_id,
        ))
        .await
        .expect("T7 cross-tenant delete");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "T7 cross-tenant delete must return 404 (not 403)"
    );

    // ── T8. GET 200 — get single task by ID ──────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(post_task(
            Some(&owner_token),
            Some(&gid),
            &gid,
            &list2_id,
            json!({ "title": "GET me", "priority": "medium" }),
        ))
        .await
        .expect("T8 create task");
    assert_eq!(resp.status(), StatusCode::CREATED, "T8 task created");
    let t8_created = body_json(resp).await;
    let t8_task_id = t8_created["id"].as_str().expect("T8 task id").to_string();

    let resp = h
        .router
        .clone()
        .oneshot(get_task_req(
            Some(&owner_token),
            Some(&gid),
            &gid,
            &t8_task_id,
        ))
        .await
        .expect("T8 get task oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "T8 status");
    let t8_body = body_json(resp).await;
    assert_eq!(t8_body["id"], t8_task_id, "T8 id matches");
    assert_eq!(t8_body["title"], "GET me", "T8 title matches");
    assert_eq!(t8_body["priority"], "medium", "T8 priority matches");
    assert_eq!(t8_body["group_id"], gid, "T8 group_id matches");

    // ── T9. GET 404 — cross-group task returns 404 (RLS isolation) ───────
    let resp = h
        .router
        .clone()
        .oneshot(get_task_req(
            Some(&owner_token),
            Some(&gid),
            &gid,
            &bob_task_id,
        ))
        .await
        .expect("T9 cross-group GET oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "T9 cross-group task GET must return 404"
    );

    // ── T10. GET 404 — deleted task returns 404 ───────────────────────────
    // task_id was soft-deleted in T6.
    let resp = h
        .router
        .clone()
        .oneshot(get_task_req(
            Some(&owner_token),
            Some(&gid),
            &gid,
            &task_id,
        ))
        .await
        .expect("T10 deleted task GET oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "T10 deleted task GET must return 404"
    );

    // ── T11. PATCH 403 — path group_id ≠ principal group_id ─────────────
    let resp = h
        .router
        .clone()
        .oneshot(patch_task_req(
            Some(&owner_token),
            Some(&gid),  // X-Group-Id = alice's group
            &g2id,       // path = bob's group → 403 mismatch
            &t8_task_id,
            json!({ "status": "done" }),
        ))
        .await
        .expect("T11 cross-group PATCH oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "T11 path group_id mismatch must return 403"
    );
}
