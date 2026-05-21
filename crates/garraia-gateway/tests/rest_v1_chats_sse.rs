//! Integration tests for `GET /v1/chats/{chat_id}/stream` — SSE cross-tenant
//! isolation (plan 0162 / GAR-670, security audit finding F-2).
//!
//! Verifies that FORCE RLS (`chats_group_isolation` policy, migration 007) +
//! the handler's `WHERE id = $chat_id AND group_id = $caller_group_id`
//! converts a cross-tenant SSE subscription attempt into 404 Not Found, not
//! 403 Forbidden (no existence leak).
//!
//! All scenarios in ONE `#[tokio::test]` to avoid the sqlx runtime-teardown
//! race documented in plan 0016 M3 commit `4f8be37`.
//!
//! Scenarios (S1–S4):
//!   S1. 404 — user from Group A, X-Group-Id: A, subscribes to Chat B
//!       (different group). FORCE RLS + WHERE returns 0 rows → NotFound,
//!       not Forbidden. No existence leak.
//!   S2. 200 text/event-stream — user from Group A subscribes to Chat A
//!       (own group, happy path). Response starts streaming immediately.
//!   S3. 400 — missing X-Group-Id header → BadRequest before any DB access.
//!   S4. 404 — archived chat (archived_at IS NOT NULL) → NotFound.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use tower::ServiceExt;
use uuid::Uuid;

use common::Harness;
use common::fixtures::{fetch_audit_events_for_group, seed_user_with_group};

fn connect_info() -> axum::extract::ConnectInfo<std::net::SocketAddr> {
    axum::extract::ConnectInfo("127.0.0.1:1".parse().unwrap())
}

/// Build a `GET /v1/chats/{chat_id}/stream` request.
///
/// `x_group_id: Some(gid)` sends the header; `None` omits it (→ 400).
fn stream_req(chat_id: Uuid, token: &str, x_group_id: Option<Uuid>) -> Request<Body> {
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
    if let Some(gid) = x_group_id {
        req.headers_mut().insert(
            HeaderName::from_static("x-group-id"),
            HeaderValue::from_str(&gid.to_string()).unwrap(),
        );
    }
    req
}

/// Seed a chat via the superuser admin pool, bypassing RLS.
/// Inserts into `chats` + one `chat_members` owner row.
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
async fn v1_chats_sse_cross_tenant_isolation() {
    let h = Harness::get().await;

    // Group A — the caller for all scenarios.
    let (user_a_id, group_a_id, token_a) =
        seed_user_with_group(&h, "owner@sse-cross-tenant-a.test")
            .await
            .expect("seed group A");

    // Group B — a different tenant; user_b owns chat_b.
    let (user_b_id, group_b_id, _token_b) =
        seed_user_with_group(&h, "owner@sse-cross-tenant-b.test")
            .await
            .expect("seed group B");

    let chat_a_id = seed_chat(&h, group_a_id, user_a_id, "sse-test-chat-a").await;
    let chat_b_id = seed_chat(&h, group_b_id, user_b_id, "sse-test-chat-b").await;

    // ── S1. Cross-tenant: Group A caller requests Chat B → 404 ──────────
    //
    // The Principal extractor looks up membership for (group_a_id, user_a_id)
    // → found → Principal { group_id: Some(group_a_id), role: Some(Owner) }.
    // The handler then issues:
    //   SET LOCAL app.current_group_id = group_a_id
    //   SELECT group_id FROM chats WHERE id = chat_b_id AND group_id = group_a_id
    // which returns 0 rows because chat_b belongs to group_b.
    // FORCE RLS (migration 007 `chats_group_isolation`) adds a second layer:
    // USING (group_id = current_setting('app.current_group_id')::uuid).
    // Result: 404 Not Found, not 403 Forbidden — no existence leak.
    let resp = h
        .router
        .clone()
        .oneshot(stream_req(chat_b_id, &token_a, Some(group_a_id)))
        .await
        .expect("S1 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "S1: cross-tenant SSE subscription must return 404, not 403 or 200"
    );

    // ── S2. Happy path: Group A caller subscribes to Chat A → 200 ───────
    //
    // Same group context: chat_a is in group_a, caller is in group_a.
    // Handler finds 1 row → returns 200 text/event-stream.
    let resp = h
        .router
        .clone()
        .oneshot(stream_req(chat_a_id, &token_a, Some(group_a_id)))
        .await
        .expect("S2 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "S2: own-group SSE must return 200"
    );
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.starts_with("text/event-stream"),
        "S2: content-type must be text/event-stream, got: {ct}"
    );

    // ── S3. Missing X-Group-Id → 400 ────────────────────────────────────
    //
    // Without X-Group-Id, Principal extractor sets group_id = None.
    // stream_chat returns BadRequest before any DB access.
    let resp = h
        .router
        .clone()
        .oneshot(stream_req(chat_a_id, &token_a, None))
        .await
        .expect("S3 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "S3: missing X-Group-Id must return 400"
    );

    // ── S4. Archived chat → 404 ──────────────────────────────────────────
    //
    // Soft-archived chats have `archived_at IS NOT NULL`; the handler's
    // WHERE clause includes `AND archived_at IS NULL`, so they return 0 rows.
    let archived_id = seed_chat(&h, group_a_id, user_a_id, "sse-test-archived").await;
    sqlx::query("UPDATE chats SET archived_at = now() WHERE id = $1")
        .bind(archived_id)
        .execute(&h.admin_pool)
        .await
        .expect("S4 archive chat");
    let resp = h
        .router
        .clone()
        .oneshot(stream_req(archived_id, &token_a, Some(group_a_id)))
        .await
        .expect("S4 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "S4: archived chat SSE subscription must return 404"
    );

    // Force the S4 response to drop now so any audit emission triggered by
    // RAII guard cleanup from earlier scenarios is well-defined before S5.
    // (S2 was the only scenario that actually built a guard.)
    drop(resp);

    // ── S5. Audit-log emission (F-4 / GAR-680) ───────────────────────────
    //
    // S2 was the only scenario that crossed both the existence check and
    // the broadcast subscribe. It must produce exactly one
    // `chat.subscribed` row when the handler runs and one
    // `chat.unsubscribed` row when the response body drops at end of scope.
    // Cross-tenant 404 (S1), missing-header 400 (S3), and archived 404 (S4)
    // must NOT emit anything to group_a's audit_events.
    //
    // The `chat.unsubscribed` row is emitted via `tokio::spawn` from
    // `ChatStreamGuard::drop`, so we briefly poll for it instead of asserting
    // synchronously — Drop happens immediately when the prior `resp` is
    // shadowed, but the spawned task runs asynchronously on the runtime.
    let mut subscribed_seen = false;
    let mut unsubscribed_seen = false;
    for _ in 0..30 {
        let rows = fetch_audit_events_for_group(&h, group_a_id)
            .await
            .expect("S5: fetch audit_events for group_a");
        subscribed_seen = false;
        unsubscribed_seen = false;
        let mut chat_a_resource_id_count = 0usize;
        for (action, actor, resource_type, resource_id, metadata) in &rows {
            if resource_id != &chat_a_id.to_string() {
                continue;
            }
            chat_a_resource_id_count += 1;
            assert_eq!(
                actor,
                &Some(user_a_id),
                "S5: actor_user_id must be the subscribing caller"
            );
            assert_eq!(
                resource_type, "chats",
                "S5: SSE audit rows must point to the `chats` resource"
            );
            // PII safety — metadata is structural only.
            let count = metadata
                .get("subscriber_count")
                .and_then(|v| v.as_u64())
                .unwrap_or_else(|| {
                    panic!("S5: missing/non-integer subscriber_count in metadata: {metadata:?}")
                });
            match action.as_str() {
                "chat.subscribed" => {
                    subscribed_seen = true;
                    assert_eq!(
                        count, 1,
                        "S5: first subscriber sees projected subscriber_count = 1"
                    );
                }
                "chat.unsubscribed" => {
                    unsubscribed_seen = true;
                    assert_eq!(
                        count, 0,
                        "S5: last subscriber leaving sees remaining_subscribers = 0"
                    );
                }
                other => panic!(
                    "S5: unexpected audit action on chat_a SSE flow: {other} (metadata={metadata:?})"
                ),
            }
            // Defensive — make sure no PII keys leaked into metadata.
            for forbidden in ["body", "name", "topic", "message"] {
                assert!(
                    metadata.get(forbidden).is_none(),
                    "S5: metadata must not carry `{forbidden}` (got {metadata:?})"
                );
            }
        }
        if subscribed_seen && unsubscribed_seen {
            // Cross-tenant + 400 + 404 paths must not pollute group_a's audit.
            assert_eq!(
                chat_a_resource_id_count, 2,
                "S5: only subscribed+unsubscribed rows expected for chat_a (got {chat_a_resource_id_count} rows)"
            );
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(
        subscribed_seen,
        "S5: chat.subscribed audit row must be emitted on happy-path SSE"
    );
    assert!(
        unsubscribed_seen,
        "S5: chat.unsubscribed audit row must be emitted when the response is dropped"
    );
}
