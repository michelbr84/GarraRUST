//! Integration tests for `GET /v1/messages/{id}` and
//! `GET /v1/messages/{id}/threads` (plan 0109, GAR-595).
//!
//! All scenarios are bundled into ONE `#[tokio::test]` function to
//! avoid the sqlx runtime-teardown race documented in plan 0016 M3.
//!
//! GET single message scenarios (5):
//!   MG1. 200 — fetch own group's message; fields correct.
//!   MG2. 404 — random UUID not in DB.
//!   MG3. 404 — message is soft-deleted.
//!   MG4. 404 — cross-group: message belongs to group B, caller in group A.
//!   MG5. 400 — missing X-Group-Id header.
//!
//! GET thread messages scenarios (5):
//!   MT1. 200 — root message exists, no thread created → thread=null, messages=[].
//!   MT2. 200 — thread created, no replies yet → thread metadata, messages=[].
//!   MT3. 200 — thread with 2 replies → replies in created_at ASC order.
//!   MT4. 404 — root message_id is random UUID.
//!   MT5. 404 — cross-group: root message in group B, caller in group A.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

use common::Harness;
use common::fixtures::seed_user_with_group;

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("failed to collect response body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response body is not JSON")
}

fn get_request(
    method: &str,
    path: &str,
    token: Option<&str>,
    x_group_id: Option<&str>,
) -> Request<Body> {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .body(Body::empty())
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

/// Create a chat via POST /v1/groups/{group_id}/chats, return chat_id.
async fn create_chat(h: &Harness, token: &str, group_id: &str, name: &str) -> String {
    let resp = h
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/groups/{group_id}/chats"))
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .header("x-group-id", group_id)
                .extension(axum::extract::ConnectInfo::<std::net::SocketAddr>(
                    "127.0.0.1:1".parse().unwrap(),
                ))
                .body(Body::from(
                    json!({"name": name, "type": "channel"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("create_chat");
    assert_eq!(resp.status(), StatusCode::CREATED);
    body_json(resp).await["id"].as_str().unwrap().to_string()
}

/// Send a message to a chat, return message_id.
async fn send_message(
    h: &Harness,
    token: &str,
    chat_id: &str,
    group_id: &str,
    text: &str,
) -> String {
    let resp = h
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/chats/{chat_id}/messages"))
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .header("x-group-id", group_id)
                .extension(axum::extract::ConnectInfo::<std::net::SocketAddr>(
                    "127.0.0.1:1".parse().unwrap(),
                ))
                .body(Body::from(json!({"body": text}).to_string()))
                .unwrap(),
        )
        .await
        .expect("send_message");
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "seed message should 201"
    );
    body_json(resp).await["id"].as_str().unwrap().to_string()
}

/// Create a thread on a message, return thread_id.
async fn create_thread(h: &Harness, token: &str, message_id: &str, group_id: &str) -> String {
    let resp = h
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/messages/{message_id}/threads"))
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .header("x-group-id", group_id)
                .extension(axum::extract::ConnectInfo::<std::net::SocketAddr>(
                    "127.0.0.1:1".parse().unwrap(),
                ))
                .body(Body::from(json!({}).to_string()))
                .unwrap(),
        )
        .await
        .expect("create_thread");
    assert_eq!(resp.status(), StatusCode::CREATED, "thread should 201");
    body_json(resp).await["id"].as_str().unwrap().to_string()
}

/// Soft-delete a message via the admin pool (bypasses RLS) for test setup.
async fn admin_soft_delete_message(h: &Harness, message_id: &str) {
    sqlx::query("UPDATE messages SET deleted_at = now() WHERE id = $1")
        .bind(Uuid::parse_str(message_id).unwrap())
        .execute(&h.admin_pool)
        .await
        .expect("admin_soft_delete_message");
}

/// Insert a thread reply directly via admin pool (bypasses RLS for test seeding).
/// Returns the inserted message id.
async fn admin_seed_thread_reply(
    h: &Harness,
    chat_id: &str,
    group_id: &str,
    sender_user_id: &str,
    thread_id: &str,
    body: &str,
) -> String {
    let row: (Uuid,) = sqlx::query_as(
        "INSERT INTO messages (chat_id, group_id, sender_user_id, sender_label, body, thread_id) \
         VALUES ($1, $2, $3, 'test-user', $4, $5) RETURNING id",
    )
    .bind(Uuid::parse_str(chat_id).unwrap())
    .bind(Uuid::parse_str(group_id).unwrap())
    .bind(Uuid::parse_str(sender_user_id).unwrap())
    .bind(body)
    .bind(Uuid::parse_str(thread_id).unwrap())
    .fetch_one(&h.admin_pool)
    .await
    .expect("admin_seed_thread_reply");
    row.0.to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn rest_v1_messages_get_threads_scenarios() {
    let h = Harness::get().await;

    // ── Setup: two groups ─────────────────────────────────────────────────────────────────────────────
    let (owner_a_id, group_a_id, token_a) =
        seed_user_with_group(&h, "owner@msg-get-threads-a.test")
            .await
            .expect("seed group A");
    let group_a = group_a_id.to_string();
    let owner_a = owner_a_id.to_string();

    let (_owner_b_id, group_b_id, token_b) =
        seed_user_with_group(&h, "owner@msg-get-threads-b.test")
            .await
            .expect("seed group B");
    let group_b = group_b_id.to_string();

    let _ = token_b; // only used for seeding

    let chat_a = create_chat(&h, &token_a, &group_a, "get-threads-chat-a").await;
    let chat_b = create_chat(&h, &token_b, &group_b, "get-threads-chat-b").await;

    // Seed a message in group A
    let msg_a = send_message(&h, &token_a, &chat_a, &group_a, "hello from A").await;

    // Seed a message in group B (for cross-group tests)
    let msg_b = send_message(&h, &token_b, &chat_b, &group_b, "hello from B").await;

    // ──────────────────────────────────────────────────────────────────────────────────
    // MG1. GET 200 — fetch own group's message; verify fields
    // ──────────────────────────────────────────────────────────────────────────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(get_request(
                "GET",
                &format!("/v1/messages/{msg_a}"),
                Some(&token_a),
                Some(&group_a),
            ))
            .await
            .expect("MG1");
        assert_eq!(resp.status(), StatusCode::OK, "MG1 status");
        let body = body_json(resp).await;
        assert_eq!(body["id"].as_str().unwrap(), msg_a, "MG1 id");
        assert_eq!(body["group_id"].as_str().unwrap(), group_a, "MG1 group_id");
        assert!(body["body"].as_str().is_some(), "MG1 has body");
        assert!(body["sender_user_id"].as_str().is_some(), "MG1 has sender");
    }

    // ──────────────────────────────────────────────────────────────────────────────────
    // MG2. GET 404 — random UUID not in DB
    // ──────────────────────────────────────────────────────────────────────────────────
    {
        let random_id = Uuid::new_v4().to_string();
        let resp = h
            .router
            .clone()
            .oneshot(get_request(
                "GET",
                &format!("/v1/messages/{random_id}"),
                Some(&token_a),
                Some(&group_a),
            ))
            .await
            .expect("MG2");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "MG2 status");
    }

    // ──────────────────────────────────────────────────────────────────────────────────
    // MG3. GET 404 — soft-deleted message
    // ──────────────────────────────────────────────────────────────────────────────────
    {
        let msg_del = send_message(&h, &token_a, &chat_a, &group_a, "will be deleted").await;
        admin_soft_delete_message(&h, &msg_del).await;

        let resp = h
            .router
            .clone()
            .oneshot(get_request(
                "GET",
                &format!("/v1/messages/{msg_del}"),
                Some(&token_a),
                Some(&group_a),
            ))
            .await
            .expect("MG3");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "MG3 status");
    }

    // ──────────────────────────────────────────────────────────────────────────────────
    // MG4. GET 404 — cross-group: message in group B, caller in group A
    // ──────────────────────────────────────────────────────────────────────────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(get_request(
                "GET",
                &format!("/v1/messages/{msg_b}"),
                Some(&token_a),
                Some(&group_a),
            ))
            .await
            .expect("MG4");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "MG4 status");
    }

    // ──────────────────────────────────────────────────────────────────────────────────
    // MG5. GET 400 — missing X-Group-Id header
    // ──────────────────────────────────────────────────────────────────────────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(get_request(
                "GET",
                &format!("/v1/messages/{msg_a}"),
                Some(&token_a),
                None,
            ))
            .await
            .expect("MG5");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "MG5 status");
    }

    // ──────────────────────────────────────────────────────────────────────────────────
    // MT1. GET /threads 200 — root message exists, no thread → thread=null
    // ──────────────────────────────────────────────────────────────────────────────────
    {
        let msg_no_thread = send_message(&h, &token_a, &chat_a, &group_a, "no thread yet").await;
        let resp = h
            .router
            .clone()
            .oneshot(get_request(
                "GET",
                &format!("/v1/messages/{msg_no_thread}/threads"),
                Some(&token_a),
                Some(&group_a),
            ))
            .await
            .expect("MT1");
        assert_eq!(resp.status(), StatusCode::OK, "MT1 status");
        let body = body_json(resp).await;
        assert!(body["thread"].is_null(), "MT1 thread should be null");
        assert_eq!(
            body["messages"].as_array().unwrap().len(),
            0,
            "MT1 messages empty"
        );
        assert!(body["next_cursor"].is_null(), "MT1 next_cursor null");
    }

    // ──────────────────────────────────────────────────────────────────────────────────
    // MT2. GET /threads 200 — thread exists, no replies → thread metadata present
    // ──────────────────────────────────────────────────────────────────────────────────
    {
        let msg_root = send_message(&h, &token_a, &chat_a, &group_a, "thread root").await;
        let thread_id = create_thread(&h, &token_a, &msg_root, &group_a).await;

        let resp = h
            .router
            .clone()
            .oneshot(get_request(
                "GET",
                &format!("/v1/messages/{msg_root}/threads"),
                Some(&token_a),
                Some(&group_a),
            ))
            .await
            .expect("MT2");
        assert_eq!(resp.status(), StatusCode::OK, "MT2 status");
        let body = body_json(resp).await;
        let thread = &body["thread"];
        assert!(!thread.is_null(), "MT2 thread should not be null");
        assert_eq!(thread["id"].as_str().unwrap(), thread_id, "MT2 thread id");
        assert_eq!(
            thread["root_message_id"].as_str().unwrap(),
            msg_root,
            "MT2 root_message_id"
        );
        assert_eq!(
            body["messages"].as_array().unwrap().len(),
            0,
            "MT2 no replies yet"
        );
        assert!(body["next_cursor"].is_null(), "MT2 next_cursor null");
    }

    // ──────────────────────────────────────────────────────────────────────────────────
    // MT3. GET /threads 200 — thread with 2 replies in ASC order
    // ──────────────────────────────────────────────────────────────────────────────────
    {
        let msg_root2 = send_message(&h, &token_a, &chat_a, &group_a, "thread root 2").await;
        let thread_id2 = create_thread(&h, &token_a, &msg_root2, &group_a).await;

        // Seed 2 replies directly via admin pool (send_message doesn't expose thread_id yet)
        let reply1 =
            admin_seed_thread_reply(&h, &chat_a, &group_a, &owner_a, &thread_id2, "first reply")
                .await;
        let reply2 =
            admin_seed_thread_reply(&h, &chat_a, &group_a, &owner_a, &thread_id2, "second reply")
                .await;

        let resp = h
            .router
            .clone()
            .oneshot(get_request(
                "GET",
                &format!("/v1/messages/{msg_root2}/threads"),
                Some(&token_a),
                Some(&group_a),
            ))
            .await
            .expect("MT3");
        assert_eq!(resp.status(), StatusCode::OK, "MT3 status");
        let body = body_json(resp).await;
        assert!(!body["thread"].is_null(), "MT3 thread present");
        let messages = body["messages"].as_array().expect("MT3 messages array");
        assert_eq!(messages.len(), 2, "MT3 two replies");
        // Replies should be in ASC order (oldest first)
        assert_eq!(
            messages[0]["id"].as_str().unwrap(),
            reply1,
            "MT3 first reply first"
        );
        assert_eq!(
            messages[1]["id"].as_str().unwrap(),
            reply2,
            "MT3 second reply second"
        );
        assert!(body["next_cursor"].is_null(), "MT3 no more pages");
    }

    // ──────────────────────────────────────────────────────────────────────────────────
    // MT4. GET /threads 404 — root message_id is random UUID
    // ──────────────────────────────────────────────────────────────────────────────────
    {
        let random_id = Uuid::new_v4().to_string();
        let resp = h
            .router
            .clone()
            .oneshot(get_request(
                "GET",
                &format!("/v1/messages/{random_id}/threads"),
                Some(&token_a),
                Some(&group_a),
            ))
            .await
            .expect("MT4");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "MT4 status");
    }

    // ──────────────────────────────────────────────────────────────────────────────────
    // MT5. GET /threads 404 — cross-group: root message in group B, caller in A
    // ──────────────────────────────────────────────────────────────────────────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(get_request(
                "GET",
                &format!("/v1/messages/{msg_b}/threads"),
                Some(&token_a),
                Some(&group_a),
            ))
            .await
            .expect("MT5");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "MT5 status");
    }
}
