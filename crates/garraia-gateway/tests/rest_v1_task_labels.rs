//! Integration tests for task labels REST API (plan 0078, GAR-536).
//!
//! All scenarios bundled into ONE `#[tokio::test]` — same pattern as
//! `rest_v1_task_assignees.rs`. Splitting triggers the sqlx runtime-teardown
//! race documented in plan 0016 M3.
//!
//! Scenarios (10 total):
//!
//!   L1.  POST /task-labels 201 — create label; assert response shape + audit row.
//!   L2.  POST /task-labels 409 — duplicate name in same group returns 409.
//!   L3.  POST /task-labels 400 — invalid color format (not #RRGGBB) returns 400.
//!   L4.  GET /task-labels 200 — list labels; includes L1 entry.
//!   L5.  POST .../labels 201 — assign label to task; audit row emitted.
//!   L6.  POST .../labels 409 — same label assigned again returns 409.
//!   L7.  POST .../labels 404 — label from another group returns 404 (cross-group guard).
//!   L8.  DELETE .../labels/{label_id} 204 — remove label from task (idempotent).
//!   L9.  DELETE /task-labels/{label_id} 204 — delete label (idempotent, CASCADE).
//!   L10. POST /task-labels 403 — path group_id ≠ principal group_id returns 403.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

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

async fn create_task_list_and_task(h: &Harness, token: &str, group_id: &str) -> (String, String) {
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{group_id}/task-lists"),
            Some(token),
            Some(group_id),
            Some(json!({ "name": "Label Test List", "type": "list" })),
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
            Some(json!({ "title": "Task for label assignment" })),
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
async fn rest_v1_task_labels_scenarios() {
    let h = Harness::get().await;

    // Seed Alice as group owner, Bob owns a separate group for isolation tests.
    let (_alice_id, group_id, alice_token) = seed_user_with_group(&h, "alice@labels.test")
        .await
        .expect("seed alice+group");
    let (_, bob_group_id, bob_token) = seed_user_with_group(&h, "bob@labels.test")
        .await
        .expect("seed bob+group2");

    let gid = group_id.to_string();
    let g2id = bob_group_id.to_string();

    // Set up a task under Alice's group.
    let (_list_id, task_id) = create_task_list_and_task(&h, &alice_token, &gid).await;

    // ── L1. POST /task-labels 201 — create label; response shape + audit ──
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{gid}/task-labels"),
            Some(&alice_token),
            Some(&gid),
            Some(json!({ "name": "Bug", "color": "#ff0000" })),
        ))
        .await
        .expect("L1 oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "L1 status");
    let l1_body = body_json(resp).await;
    let label_id = l1_body["id"].as_str().expect("L1 label id").to_string();
    assert_eq!(l1_body["name"], "Bug", "L1 name");
    assert_eq!(l1_body["color"], "#ff0000", "L1 color");
    assert_eq!(l1_body["group_id"], gid, "L1 group_id");
    assert!(l1_body.get("created_at").is_some(), "L1 created_at present");

    // Verify audit row.
    let audit = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("L1 fetch audit");
    let l1_audit = audit
        .iter()
        .find(|e| e.0 == "task_label.created")
        .expect("L1 audit row task_label.created");
    let (_, _, l1_res_type, l1_res_id, l1_meta) = l1_audit;
    assert_eq!(l1_res_type, "task_labels", "L1 audit resource_type");
    assert_eq!(l1_res_id, &label_id, "L1 audit resource_id is label_id");
    assert!(
        l1_meta["name_len"].as_u64().is_some(),
        "L1 audit metadata has name_len (PII-safe)"
    );
    assert!(
        l1_meta.get("name").is_none(),
        "L1 audit must not contain label name value (PII guard)"
    );

    // ── L2. POST /task-labels 409 — duplicate name ───────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{gid}/task-labels"),
            Some(&alice_token),
            Some(&gid),
            Some(json!({ "name": "Bug", "color": "#00ff00" })),
        ))
        .await
        .expect("L2 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "L2 duplicate label name must be 409"
    );

    // ── L3. POST /task-labels 400 — invalid color ────────────────────────
    for bad_color in &["red", "#gg0000", "#ff00", "#FF0000aa", ""] {
        let resp = h
            .router
            .clone()
            .oneshot(auth_req(
                "POST",
                &format!("/v1/groups/{gid}/task-labels"),
                Some(&alice_token),
                Some(&gid),
                Some(json!({ "name": "Color Test", "color": bad_color })),
            ))
            .await
            .expect("L3 oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "L3 invalid color '{bad_color}' must be 400"
        );
    }

    // ── L4. GET /task-labels 200 — list includes L1 entry ────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "GET",
            &format!("/v1/groups/{gid}/task-labels"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("L4 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "L4 status");
    let l4_body = body_json(resp).await;
    let items = l4_body.as_array().expect("L4 response is array");
    assert!(
        items.iter().any(|it| it["id"] == label_id),
        "L4 list must contain the created label"
    );

    // ── L5. POST .../labels 201 — assign label to task; audit row ─────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{gid}/tasks/{task_id}/labels"),
            Some(&alice_token),
            Some(&gid),
            Some(json!({ "label_id": label_id })),
        ))
        .await
        .expect("L5 oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "L5 status");
    let l5_body = body_json(resp).await;
    assert_eq!(l5_body["task_id"], task_id, "L5 task_id");
    assert_eq!(l5_body["label_id"], label_id, "L5 label_id");
    assert!(
        l5_body.get("assigned_at").is_some(),
        "L5 assigned_at present"
    );

    // Verify audit row.
    let audit2 = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("L5 fetch audit");
    assert!(
        audit2
            .iter()
            .any(|e| e.0 == "task.label.assigned" && e.3 == task_id),
        "L5 audit row task.label.assigned must be present"
    );

    // ── L6. POST .../labels 409 — same label assigned again ───────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{gid}/tasks/{task_id}/labels"),
            Some(&alice_token),
            Some(&gid),
            Some(json!({ "label_id": label_id })),
        ))
        .await
        .expect("L6 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "L6 duplicate assignment must be 409"
    );

    // ── L7. POST .../labels 404 — label from another group ────────────────
    // Create a label in Bob's group; try to assign it to Alice's task.
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{g2id}/task-labels"),
            Some(&bob_token),
            Some(&g2id),
            Some(json!({ "name": "Bob Label", "color": "#0000ff" })),
        ))
        .await
        .expect("L7 bob label create");
    assert_eq!(resp.status(), StatusCode::CREATED, "L7 bob label 201");
    let bob_label_body = body_json(resp).await;
    let bob_label_id = bob_label_body["id"].as_str().expect("bob label id");

    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{gid}/tasks/{task_id}/labels"),
            Some(&alice_token),
            Some(&gid),
            Some(json!({ "label_id": bob_label_id })),
        ))
        .await
        .expect("L7 cross-group assign oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "L7 cross-group label must be 404"
    );

    // ── L8. DELETE .../labels/{label_id} 204 — remove label from task ─────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "DELETE",
            &format!("/v1/groups/{gid}/tasks/{task_id}/labels/{label_id}"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("L8 oneshot");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT, "L8 status");

    // Audit row for removal.
    let audit3 = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("L8 fetch audit");
    assert!(
        audit3
            .iter()
            .any(|e| e.0 == "task.label.removed" && e.3 == task_id),
        "L8 audit row task.label.removed must be present"
    );

    // Idempotent: second DELETE on same assignment also returns 204.
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "DELETE",
            &format!("/v1/groups/{gid}/tasks/{task_id}/labels/{label_id}"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("L8 idempotent oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "L8 idempotent delete must return 204"
    );

    // ── L9. DELETE /task-labels/{label_id} 204 — delete the label itself ──
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "DELETE",
            &format!("/v1/groups/{gid}/task-labels/{label_id}"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("L9 oneshot");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT, "L9 status");

    // Audit row for label deletion.
    let audit4 = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("L9 fetch audit");
    assert!(
        audit4
            .iter()
            .any(|e| e.0 == "task_label.deleted" && e.3 == label_id),
        "L9 audit row task_label.deleted must be present"
    );

    // Idempotent: delete again → 204.
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "DELETE",
            &format!("/v1/groups/{gid}/task-labels/{label_id}"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("L9 idempotent oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "L9 idempotent delete label must return 204"
    );

    // GET list after deletion: label no longer present.
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "GET",
            &format!("/v1/groups/{gid}/task-labels"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("L9 list after delete");
    assert_eq!(resp.status(), StatusCode::OK, "L9 list status");
    let l9_list = body_json(resp).await;
    let after_del = l9_list.as_array().expect("L9 items array");
    assert!(
        !after_del.iter().any(|it| it["id"] == label_id),
        "L9 deleted label must not appear in GET list"
    );

    // ── L10. POST 403 — path group_id ≠ principal group_id ───────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{g2id}/task-labels"),
            Some(&alice_token),
            Some(&gid), // X-Group-Id = alice's group, path = bob's group
            Some(json!({ "name": "Forbidden Label", "color": "#ffffff" })),
        ))
        .await
        .expect("L10 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "L10 path group_id mismatch must return 403"
    );
}
