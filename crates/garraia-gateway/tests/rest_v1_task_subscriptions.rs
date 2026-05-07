//! Integration tests for task subscriptions REST API (plan 0079, GAR-539).
//!
//! All scenarios bundled into ONE `#[tokio::test]` — same pattern as
//! `rest_v1_task_assignees.rs`. Splitting triggers the sqlx runtime-teardown
//! race documented in plan 0016 M3.
//!
//! Scenarios (8 total):
//!
//!   S1.  POST 201 — subscribe caller to task; assert response shape + audit row.
//!   S2.  POST 409 — duplicate subscribe returns 409 Conflict.
//!   S3.  GET 200 — list subscriptions; includes S1 entry.
//!   S4.  DELETE 204 — unsubscribe; verify audit row emitted.
//!   S5.  DELETE 204 — idempotent: user already unsubscribed; returns 204 again.
//!   S6.  POST 404 — task not in group (cross-group task isolation).
//!   S7.  GET 404 + DELETE 404 + POST 404 — unknown task_id returns 404.
//!   S8.  POST 403 — path group_id ≠ principal group_id returns 403.

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

async fn create_task_list_and_task(h: &Harness, token: &str, gid: &str) -> (String, String) {
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{gid}/task-lists"),
            Some(token),
            Some(gid),
            Some(json!({ "name": "Sub Test List", "type": "list" })),
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
            &format!("/v1/groups/{gid}/task-lists/{list_id}/tasks"),
            Some(token),
            Some(gid),
            Some(json!({ "title": "Watch me", "status": "todo", "priority": "none" })),
        ))
        .await
        .expect("create task");
    assert_eq!(resp.status(), StatusCode::CREATED, "setup: task 201");
    let t_body = body_json(resp).await;
    let task_id = t_body["id"].as_str().expect("task id").to_string();

    (list_id, task_id)
}

// ─── Test ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn task_subscriptions_api_all_scenarios() {
    let h = Harness::get().await;

    // Primary user (group owner).
    let (_user_id, group_id, token) = seed_user_with_group(&h, "alice@subs.test")
        .await
        .expect("seed alice+group");

    // Second user / group for cross-group isolation.
    let (_user_id2, group_id2, token2) = seed_user_with_group(&h, "bob@subs.test")
        .await
        .expect("seed bob+group2");

    let gid = group_id.to_string();
    let g2id = group_id2.to_string();

    let (_, task_id) = create_task_list_and_task(&h, &token, &gid).await;

    // ── S1. POST 201 — subscribe caller ──────────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{gid}/tasks/{task_id}/subscriptions"),
            Some(&token),
            Some(&gid),
            None,
        ))
        .await
        .expect("S1");
    assert_eq!(resp.status(), StatusCode::CREATED, "S1: 201 subscribe");
    let s1_body = body_json(resp).await;
    assert_eq!(s1_body["task_id"].as_str().unwrap(), task_id, "S1: task_id");
    assert!(s1_body["user_id"].as_str().is_some(), "S1: user_id present");
    assert!(s1_body["subscribed_at"].as_str().is_some(), "S1: subscribed_at present");
    assert!(!s1_body["muted"].as_bool().unwrap(), "S1: muted defaults false");

    // Verify audit row.
    let audit = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("S1 fetch audit");
    let s1_audit = audit
        .iter()
        .find(|e| e.0 == "task.subscribed")
        .expect("S1: task.subscribed audit row");
    let (_, _, s1_res_type, s1_res_id, s1_meta) = s1_audit;
    assert_eq!(s1_res_type, "task_subscriptions", "S1: audit resource_type");
    assert_eq!(s1_res_id, &task_id, "S1: audit resource_id = task_id");
    assert!(
        s1_meta["subscriber_user_id_len"].as_u64().is_some(),
        "S1: metadata has subscriber_user_id_len"
    );
    assert!(
        s1_meta.get("user_id").is_none(),
        "S1: PII guard — user_id value absent from metadata"
    );

    // ── S2. POST 409 — duplicate subscribe ───────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{gid}/tasks/{task_id}/subscriptions"),
            Some(&token),
            Some(&gid),
            None,
        ))
        .await
        .expect("S2");
    assert_eq!(resp.status(), StatusCode::CONFLICT, "S2: 409 duplicate");

    // ── S3. GET 200 — list subscriptions includes S1 ─────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "GET",
            &format!("/v1/groups/{gid}/tasks/{task_id}/subscriptions"),
            Some(&token),
            Some(&gid),
            None,
        ))
        .await
        .expect("S3");
    assert_eq!(resp.status(), StatusCode::OK, "S3: 200 list");
    let s3_body = body_json(resp).await;
    let items = s3_body.as_array().expect("S3: array response");
    assert_eq!(items.len(), 1, "S3: exactly 1 subscriber");
    assert_eq!(items[0]["task_id"].as_str().unwrap(), task_id, "S3: task_id matches");

    // ── S4. DELETE 204 — unsubscribe + audit ─────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "DELETE",
            &format!("/v1/groups/{gid}/tasks/{task_id}/subscriptions"),
            Some(&token),
            Some(&gid),
            None,
        ))
        .await
        .expect("S4");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT, "S4: 204 unsubscribe");

    let audit2 = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("S4 fetch audit");
    assert!(
        audit2.iter().any(|e| e.0 == "task.unsubscribed"),
        "S4: task.unsubscribed audit row present"
    );

    // ── S5. DELETE 204 — idempotent (already unsubscribed) ───────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "DELETE",
            &format!("/v1/groups/{gid}/tasks/{task_id}/subscriptions"),
            Some(&token),
            Some(&gid),
            None,
        ))
        .await
        .expect("S5");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT, "S5: 204 idempotent");

    // GET returns empty list after unsubscribe.
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "GET",
            &format!("/v1/groups/{gid}/tasks/{task_id}/subscriptions"),
            Some(&token),
            Some(&gid),
            None,
        ))
        .await
        .expect("S5b");
    assert_eq!(resp.status(), StatusCode::OK, "S5b: 200");
    assert_eq!(
        body_json(resp).await.as_array().unwrap().len(),
        0,
        "S5b: empty after unsubscribe"
    );

    // ── S6. POST 404 — task in group1, accessed via group2 creds ─────────────
    // Bob (group2) tries to subscribe to task in group1 using group2 path.
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{g2id}/tasks/{task_id}/subscriptions"),
            Some(&token2),
            Some(&g2id),
            None,
        ))
        .await
        .expect("S6");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "S6: 404 cross-group task");

    // ── S7. GET 404 + DELETE 404 + POST 404 — unknown task_id ────────────────
    let bad_id = uuid::Uuid::new_v4();
    for method in ["GET", "DELETE", "POST"] {
        let resp = h
            .router
            .clone()
            .oneshot(auth_req(
                method,
                &format!("/v1/groups/{gid}/tasks/{bad_id}/subscriptions"),
                Some(&token),
                Some(&gid),
                None,
            ))
            .await
            .unwrap_or_else(|_| panic!("S7 {method}"));
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "S7: {method} 404 unknown task"
        );
    }

    // ── S8. POST 403 — path group_id ≠ principal group_id ───────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            // path says group2 but token/header says group1
            &format!("/v1/groups/{g2id}/tasks/{task_id}/subscriptions"),
            Some(&token),
            Some(&gid),
            None,
        ))
        .await
        .expect("S8");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "S8: 403 group mismatch");
}
