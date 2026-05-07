//! Integration tests for task subscriptions REST API (plan 0079, GAR-539).
//!
//! All scenarios bundled into ONE `#[tokio::test]` — same pattern as
//! `rest_v1_task_assignees.rs` and `rest_v1_task_labels.rs`. Splitting
//! triggers the sqlx runtime-teardown race documented in plan 0016 M3.
//!
//! Scenarios (7 total):
//!
//!   S1. POST 201 — Alice subscribes herself; response shape + audit row
//!       (`task.subscribed`, resource `task_subscriptions/{task_id}`,
//!       metadata `subscriber_user_id_len: 36` and NO PII).
//!   S2. POST 409 — second subscribe by same user.
//!   S3. GET 200  — list returns 1 entry with Alice as subscriber.
//!   S4. DELETE 204 — Alice unsubscribes; audit `task.unsubscribed`;
//!       second DELETE on same task → 204 idempotent.
//!   S5. POST 404 — Alice tries to subscribe to a task that lives in Bob's
//!       group (cross-group `task_id` injection guard).
//!   S6. POST 403 — path `group_id` ≠ principal `group_id`
//!       (covered by `check_group_match`).
//!   S7. GET 200  — empty array after the unsubscribe in S4.

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
            Some(json!({ "name": "Subscription Test List", "type": "list" })),
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
            Some(json!({ "title": "Task that gets subscribers" })),
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
async fn rest_v1_task_subscriptions_scenarios() {
    let h = Harness::get().await;

    // Seed Alice as group owner; Bob owns a separate group used by S5.
    let (alice_id, group_id, alice_token) = seed_user_with_group(&h, "alice@subs.test")
        .await
        .expect("seed alice+group");
    let (_bob_id, bob_group_id, bob_token) = seed_user_with_group(&h, "bob@subs.test")
        .await
        .expect("seed bob+group2");

    let gid = group_id.to_string();
    let g2id = bob_group_id.to_string();
    let alice_id_s = alice_id.to_string();

    // Set up a task under Alice's group (S1–S4, S6, S7 act on this one).
    let (_list_id, task_id) = create_task_list_and_task(&h, &alice_token, &gid).await;

    // Set up a task under Bob's group for the cross-group injection test (S5).
    let (_b_list_id, bob_task_id) = create_task_list_and_task(&h, &bob_token, &g2id).await;

    // ── S1. POST 201 — Alice subscribes herself; verify shape + audit ────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{gid}/tasks/{task_id}/subscriptions"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("S1 oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "S1 status");
    let s1_body = body_json(resp).await;
    assert_eq!(s1_body["task_id"], task_id, "S1 task_id");
    assert_eq!(s1_body["user_id"], alice_id_s, "S1 user_id is principal");
    assert_eq!(s1_body["muted"], false, "S1 muted defaults to false");
    assert!(
        s1_body.get("subscribed_at").is_some(),
        "S1 subscribed_at present"
    );

    // Audit row.
    let audit1 = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("S1 fetch audit");
    let s1_audit = audit1
        .iter()
        .find(|e| e.0 == "task.subscribed" && e.3 == task_id)
        .expect("S1 audit row task.subscribed");
    let (_, _, s1_res_type, _, s1_meta) = s1_audit;
    assert_eq!(s1_res_type, "task_subscriptions", "S1 resource_type");
    assert_eq!(
        s1_meta["subscriber_user_id_len"].as_u64(),
        Some(36),
        "S1 metadata has subscriber_user_id_len"
    );
    assert!(
        s1_meta.get("user_id").is_none(),
        "S1 audit must not contain raw user_id (PII guard)"
    );
    assert!(
        s1_meta.get("email").is_none(),
        "S1 audit must not contain email (PII guard)"
    );

    // ── S2. POST 409 — second subscribe by same user ─────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{gid}/tasks/{task_id}/subscriptions"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("S2 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "S2 duplicate subscribe must be 409"
    );

    // ── S3. GET 200 — list returns Alice as subscriber ───────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "GET",
            &format!("/v1/groups/{gid}/tasks/{task_id}/subscriptions"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("S3 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "S3 status");
    let s3_body = body_json(resp).await;
    let items = s3_body.as_array().expect("S3 response is array");
    assert_eq!(items.len(), 1, "S3 list should have exactly 1 subscriber");
    assert_eq!(items[0]["user_id"], alice_id_s, "S3 subscriber is alice");
    assert_eq!(items[0]["task_id"], task_id, "S3 task_id");
    assert_eq!(items[0]["muted"], false, "S3 muted defaults to false");

    // ── S4. DELETE 204 — Alice unsubscribes (idempotent on second call) ──
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "DELETE",
            &format!("/v1/groups/{gid}/tasks/{task_id}/subscriptions"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("S4 first oneshot");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT, "S4 first DELETE 204");

    // Audit row for unsubscribe.
    let audit4 = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("S4 fetch audit");
    assert!(
        audit4
            .iter()
            .any(|e| e.0 == "task.unsubscribed" && e.3 == task_id),
        "S4 audit row task.unsubscribed must be present"
    );

    // Second DELETE → 204 idempotent.
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "DELETE",
            &format!("/v1/groups/{gid}/tasks/{task_id}/subscriptions"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("S4 idempotent oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "S4 second DELETE must be 204 idempotent"
    );

    // ── S5. POST 404 — Alice subscribes to a task in Bob's group ─────────
    // Even though Alice's principal carries her own group_id, the path is
    // still under her group_id (since check_group_match enforces that).
    // The task_id is from Bob's group — RLS + the explicit `group_id =`
    // filter in the existence query should yield 404.
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{gid}/tasks/{bob_task_id}/subscriptions"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("S5 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "S5 cross-group task_id must be 404"
    );

    // ── S6. POST 403 — path group_id ≠ principal group_id ────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/v1/groups/{g2id}/tasks/{task_id}/subscriptions"),
            Some(&alice_token),
            Some(&gid), // X-Group-Id matches alice; path is bob's group.
            None,
        ))
        .await
        .expect("S6 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "S6 path group mismatch must be 403"
    );

    // ── S7. GET 200 — empty array after unsubscribe in S4 ────────────────
    let resp = h
        .router
        .clone()
        .oneshot(auth_req(
            "GET",
            &format!("/v1/groups/{gid}/tasks/{task_id}/subscriptions"),
            Some(&alice_token),
            Some(&gid),
            None,
        ))
        .await
        .expect("S7 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "S7 status");
    let s7_body = body_json(resp).await;
    let items = s7_body.as_array().expect("S7 response is array");
    assert!(
        items.is_empty(),
        "S7 list must be empty after unsubscribe (got {})",
        items.len()
    );
}
