// Gated so `cargo clippy --all-targets` without `test-helpers` skips this
#![cfg(feature = "test-helpers")]

//! Integration tests for SSE per-user rate limit on
//! `GET /v1/chats/{chat_id}/stream` (plan 0163, GAR-679).
//!
//! Verifies that a single user cannot open more than `MAX_SSE_PER_USER` (5)
//! concurrent SSE connections and receives **429 Too Many Requests** with a
//! `Retry-After: 60` header on the 6th attempt.
//!
//! All scenarios in ONE `#[tokio::test]` to avoid the sqlx runtime-teardown
//! race documented in plan 0016 M3 commit `4f8be37`.
//!
//! Scenarios:
//!   S1. 5 concurrent SSE connections from the same user → all 200.
//!   S2. 6th SSE connection from the same user → 429 + Retry-After: 60.
//!   S3. After dropping one open connection, next attempt → 200.
//!   S4. Different user on the same chat is not affected by first user's cap.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use tower::ServiceExt;
use uuid::Uuid;

use common::Harness;
use common::fixtures::seed_user_with_group;

fn connect_info() -> axum::extract::ConnectInfo<std::net::SocketAddr> {
    axum::extract::ConnectInfo("127.0.0.1:1".parse().unwrap())
}

fn stream_req(chat_id: Uuid, token: &str, group_id: Uuid) -> Request<Body> {
    let mut req = Request::builder()
        .method("GET")
        .uri(format!("/v1/chats/{chat_id}/stream"))
        .body(Body::empty())
        .expect("SSE stream request builder");
    req.extensions_mut().insert(connect_info());
    req.headers_mut().insert(
        HeaderName::from_static("authorization"),
        HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
    );
    req.headers_mut().insert(
        HeaderName::from_static("x-group-id"),
        HeaderValue::from_str(&group_id.to_string()).unwrap(),
    );
    req
}

async fn seed_chat(h: &Harness, group_id: Uuid, creator_id: Uuid, name: &str) -> Uuid {
    let chat_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO chats (id, group_id, type, name, created_by) \
         VALUES ($1, $2, 'channel', $3, $4)",
    )
    .bind(chat_id)
    .bind(group_id)
    .bind(name)
    .bind(creator_id)
    .execute(&h.admin_pool)
    .await
    .expect("seed_chat: insert chats");
    sqlx::query(
        "INSERT INTO chat_members (chat_id, user_id, role) \
         VALUES ($1, $2, 'owner')",
    )
    .bind(chat_id)
    .bind(creator_id)
    .execute(&h.admin_pool)
    .await
    .expect("seed_chat: insert chat_members");
    chat_id
}

#[tokio::test]
async fn v1_chats_sse_rate_limit() {
    let h = Harness::get().await;

    // Seed user_a and user_b in the same group.
    let (user_a_id, group_id, token_a) = seed_user_with_group(&h, "owner@sse-rate-limit-a.test")
        .await
        .expect("seed user_a");

    let (user_b_id, _group_b_id, token_b) =
        seed_user_with_group(&h, "member@sse-rate-limit-b.test")
            .await
            .expect("seed user_b");

    // Give user_b membership in group_a so they can access the same chat.
    sqlx::query(
        "INSERT INTO group_members (group_id, user_id, role) \
         VALUES ($1, $2, 'member') ON CONFLICT DO NOTHING",
    )
    .bind(group_id)
    .bind(user_b_id)
    .execute(&h.admin_pool)
    .await
    .expect("add user_b to group_a");

    let chat_id = seed_chat(&h, group_id, user_a_id, "sse-rate-limit-chat").await;

    // ── S1. Five concurrent SSE connections from user_a → all 200 ─────────
    //
    // Each `oneshot` creates an independent Response that holds the SSE stream.
    // The `ChatStreamGuard` inside each stream increments the per-user counter
    // at construction and will decrement it when the Response is dropped.
    let resp1 = h
        .router
        .clone()
        .oneshot(stream_req(chat_id, &token_a, group_id))
        .await
        .expect("S1 conn 1");
    assert_eq!(resp1.status(), StatusCode::OK, "S1: conn 1 must be 200");

    let resp2 = h
        .router
        .clone()
        .oneshot(stream_req(chat_id, &token_a, group_id))
        .await
        .expect("S1 conn 2");
    assert_eq!(resp2.status(), StatusCode::OK, "S1: conn 2 must be 200");

    let resp3 = h
        .router
        .clone()
        .oneshot(stream_req(chat_id, &token_a, group_id))
        .await
        .expect("S1 conn 3");
    assert_eq!(resp3.status(), StatusCode::OK, "S1: conn 3 must be 200");

    let resp4 = h
        .router
        .clone()
        .oneshot(stream_req(chat_id, &token_a, group_id))
        .await
        .expect("S1 conn 4");
    assert_eq!(resp4.status(), StatusCode::OK, "S1: conn 4 must be 200");

    let resp5 = h
        .router
        .clone()
        .oneshot(stream_req(chat_id, &token_a, group_id))
        .await
        .expect("S1 conn 5");
    assert_eq!(resp5.status(), StatusCode::OK, "S1: conn 5 must be 200");

    // ── S2. 6th connection → 429 + Retry-After: 60 ────────────────────────
    //
    // user_a already holds 5 open streams. MAX_SSE_PER_USER = 5, so the
    // 6th attempt is rejected before any DB access.
    let resp_429 = h
        .router
        .clone()
        .oneshot(stream_req(chat_id, &token_a, group_id))
        .await
        .expect("S2 conn 6");
    assert_eq!(
        resp_429.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "S2: 6th SSE connection must return 429"
    );
    let retry_after = resp_429
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(
        retry_after, "60",
        "S2: Retry-After header must be 60 seconds"
    );

    // ── S3. Drop one open connection → next attempt succeeds ──────────────
    //
    // Dropping `resp1` drops the Response<Body>, which drops the SSE stream,
    // which drops ChatStreamGuard::drop — decrementing the per-user counter
    // from 5 to 4. The 6th connection attempt (now the 5th active) succeeds.
    drop(resp1);

    // Give the Tokio runtime a chance to run the spawned audit task from
    // ChatStreamGuard::drop (which may briefly hold the counter's DashMap entry).
    tokio::task::yield_now().await;

    let resp_new = h
        .router
        .clone()
        .oneshot(stream_req(chat_id, &token_a, group_id))
        .await
        .expect("S3 new conn after drop");
    assert_eq!(
        resp_new.status(),
        StatusCode::OK,
        "S3: connection after dropping one must be 200"
    );

    // ── S4. Different user is not affected by user_a's cap ─────────────────
    //
    // user_b has no open SSE connections; their limit is independent of
    // user_a's counter. The first connection from user_b must succeed even
    // while user_a holds 5 open streams.
    let resp_b = h
        .router
        .clone()
        .oneshot(stream_req(chat_id, &token_b, group_id))
        .await
        .expect("S4 user_b conn");
    assert_eq!(
        resp_b.status(),
        StatusCode::OK,
        "S4: different user must not be affected by user_a's cap"
    );

    // Keep remaining responses alive until end of test to ensure their
    // ChatStreamGuard has the full lifetime of the test's tokio runtime.
    drop(resp2);
    drop(resp3);
    drop(resp4);
    drop(resp5);
    drop(resp_429);
    drop(resp_new);
    drop(resp_b);
}
