//! Integration tests for `GET /v1/search` (plan 0084 + plan 0085;
//! GAR-549 + GAR-551).
//!
//! All scenarios bundled into ONE `#[tokio::test]` — same pattern as other
//! rest_v1_* tests (sqlx runtime-teardown race documented in plan 0016 M3).
//!
//! Scenarios:
//!
//!   ── Slice 1 (group scope) — GAR-549 ──
//!   S1.  GET 200 — message search: matching message returned.
//!   S2.  GET 200 — memory search: matching memory_item returned.
//!   S3.  GET 200 — combined types: both messages and memory in results.
//!   S4.  GET 200 — empty results: no match returns empty items array.
//!   S5.  GET 404 — cross-group: scope_id ≠ principal.group_id → 404.
//!   S6.  GET 400 — empty q → 400.
//!   S7.  GET 400 — unknown type in types → 400.
//!   S8.  GET 400 — `scope_type=user` + `types=messages` rejected (plan 0085).
//!   S9.  GET 401 — missing bearer → 401.
//!   S10. GET 200 — sensitivity='secret' memory excluded.
//!   S11. GET 200 — cross-group RLS: group2 cannot see group1 messages.
//!
//!   ── Slice 2 (chat + user scope) — GAR-551 ──
//!   S12. GET 200 — `scope_type=chat`: returns only messages from that chat.
//!   S13. GET 404 — `scope_type=chat` with chat in another group → 404.
//!   S14. GET 200 — `scope_type=chat` memory: only chat-scope rows, not group-scope.
//!   S15. GET 200 — `scope_type=user` (memory): returns only caller's personal memory.
//!   S16. GET 404 — `scope_type=user` with another user's id → 404.
//!   S17. GET 400 — `scope_type=user` with default types (messages,memory) → 400.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

use common::Harness;
use common::fixtures::seed_user_with_group;

// ─── Response helpers ─────────────────────────────────────────────────────────

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("collect response body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response is not JSON")
}

// ─── Request builders ─────────────────────────────────────────────────────────

fn req(
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
    let mut r = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(body_bytes)
        .expect("request builder");
    r.extensions_mut()
        .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
            "127.0.0.1:1".parse().unwrap(),
        ));
    if let Some(t) = token {
        r.headers_mut().insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_str(&format!("Bearer {t}")).unwrap(),
        );
    }
    if let Some(g) = x_group_id {
        r.headers_mut().insert(
            HeaderName::from_static("x-group-id"),
            HeaderValue::from_str(g).unwrap(),
        );
    }
    r
}

/// Helper: create a chat and return its id string.
async fn create_chat(h: &Harness, token: &str, group_id: &str) -> String {
    let resp = h
        .router
        .clone()
        .oneshot(req(
            "POST",
            &format!("/v1/groups/{group_id}/chats"),
            Some(token),
            Some(group_id),
            Some(json!({"name": "search-test-chat", "type": "channel"})),
        ))
        .await
        .expect("create_chat oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "setup: chat 201");
    body_json(resp).await["id"]
        .as_str()
        .expect("chat id")
        .to_string()
}

/// Helper: send a message and return its id string.
async fn send_message(
    h: &Harness,
    token: &str,
    chat_id: &str,
    group_id: &str,
    body_text: &str,
) -> String {
    let resp = h
        .router
        .clone()
        .oneshot(req(
            "POST",
            &format!("/v1/chats/{chat_id}/messages"),
            Some(token),
            Some(group_id),
            Some(json!({"body": body_text})),
        ))
        .await
        .expect("send_message oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "setup: message 201");
    body_json(resp).await["id"]
        .as_str()
        .expect("message id")
        .to_string()
}

/// Helper: create a group-scoped memory item.
async fn create_memory(h: &Harness, token: &str, group_id: &str, content: &str, sensitivity: &str) {
    let resp = h
        .router
        .clone()
        .oneshot(req(
            "POST",
            "/v1/memory",
            Some(token),
            Some(group_id),
            Some(json!({
                "scope_type": "group",
                "scope_id": group_id,
                "kind": "fact",
                "content": content,
                "sensitivity": sensitivity
            })),
        ))
        .await
        .expect("create_memory oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "setup: memory 201");
}

/// Helper: create a chat-scoped memory item.
async fn create_chat_memory(
    h: &Harness,
    token: &str,
    group_id: &str,
    chat_id: &str,
    content: &str,
) {
    let resp = h
        .router
        .clone()
        .oneshot(req(
            "POST",
            "/v1/memory",
            Some(token),
            Some(group_id),
            Some(json!({
                "scope_type": "chat",
                "scope_id": chat_id,
                "kind": "fact",
                "content": content,
                "sensitivity": "private"
            })),
        ))
        .await
        .expect("create_chat_memory oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "setup: chat memory 201");
}

/// Helper: create a user-scoped (personal) memory item.
async fn create_user_memory(
    h: &Harness,
    token: &str,
    group_id: &str,
    user_id: &str,
    content: &str,
) {
    let resp = h
        .router
        .clone()
        .oneshot(req(
            "POST",
            "/v1/memory",
            Some(token),
            Some(group_id),
            Some(json!({
                "scope_type": "user",
                "scope_id": user_id,
                "kind": "fact",
                "content": content,
                "sensitivity": "private"
            })),
        ))
        .await
        .expect("create_user_memory oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "setup: user memory 201");
}

/// Helper: GET /v1/search with the given query string.
fn search_req(token: Option<&str>, x_group_id: Option<&str>, qs: &str) -> Request<Body> {
    req("GET", &format!("/v1/search?{qs}"), token, x_group_id, None)
}

// ─── Test suite ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn search_scenarios() {
    let h = Harness::get().await;

    // Seed two isolated groups. Slice 2 also exercises user_id, so we
    // capture it for both seeds.
    let (user_id, group_id, token) = seed_user_with_group(&h, "owner@search-test.test")
        .await
        .expect("seed owner");
    let (user2_id, group2_id, token2) = seed_user_with_group(&h, "owner2@search-test.test")
        .await
        .expect("seed owner2");

    // Set up group1 data.
    let chat_id = create_chat(&h, &token, &group_id.to_string()).await;
    send_message(
        &h,
        &token,
        &chat_id,
        &group_id.to_string(),
        "fluffernutter sandwich is a delicious treat",
    )
    .await;
    // Non-matching message — must NOT appear in fluffernutter search.
    send_message(
        &h,
        &token,
        &chat_id,
        &group_id.to_string(),
        "completely unrelated weather report today",
    )
    .await;
    // Matching memory item.
    create_memory(
        &h,
        &token,
        &group_id.to_string(),
        "fluffernutter is peanut butter and marshmallow combined",
        "group",
    )
    .await;
    // Secret memory — must NEVER appear in search results.
    create_memory(
        &h,
        &token,
        &group_id.to_string(),
        "fluffernutter classified recipe top secret",
        "secret",
    )
    .await;

    // ── Slice-2 setup ────────────────────────────────────────────────────
    // Chat-scoped memory item with a unique token so we can prove that
    // `scope_type=chat` returns chat-scope rows (and not group-scope rows
    // that just happen to be in the same group).
    create_chat_memory(
        &h,
        &token,
        &group_id.to_string(),
        &chat_id,
        "cinnamonroll is the chat-scope canary phrase",
    )
    .await;
    // User-scoped (personal) memory items, one per owner. The unique
    // token "personalsecretphrase42" lets us assert that user A's search
    // returns A's personal memory and not B's.
    create_user_memory(
        &h,
        &token,
        &group_id.to_string(),
        &user_id.to_string(),
        "personalsecretphrase42 belongs to owner1 alone",
    )
    .await;
    create_user_memory(
        &h,
        &token2,
        &group2_id.to_string(),
        &user2_id.to_string(),
        "personalsecretphrase42 belongs to owner2 alone",
    )
    .await;
    // Second chat in caller's group — search by chat_id (chat 1) must NOT
    // return a message that lives only in chat 2.
    let chat2_id = create_chat(&h, &token, &group_id.to_string()).await;
    send_message(
        &h,
        &token,
        &chat2_id,
        &group_id.to_string(),
        "fluffernutter only here in chat2",
    )
    .await;

    // ── S1. Message search ────────────────────────────────────────────────
    let qs = format!(
        "q=fluffernutter&scope_type=group&scope_id={}&types=messages",
        group_id
    );
    let resp = h
        .router
        .clone()
        .oneshot(search_req(Some(&token), Some(&group_id.to_string()), &qs))
        .await
        .expect("S1 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "S1 status");
    let body = body_json(resp).await;
    let items = body["items"].as_array().expect("S1 items");
    assert!(
        !items.is_empty(),
        "S1 must return at least one message result"
    );
    assert!(
        items.iter().all(|it| it["type"] == "message"),
        "S1 all items must be type=message"
    );
    assert!(
        items.iter().any(|it| it["excerpt"]
            .as_str()
            .unwrap_or("")
            .contains("fluffernutter")),
        "S1 must find the matching message"
    );

    // ── S2. Memory search ─────────────────────────────────────────────────
    let qs2 = format!(
        "q=marshmallow&scope_type=group&scope_id={}&types=memory",
        group_id
    );
    let resp2 = h
        .router
        .clone()
        .oneshot(search_req(Some(&token), Some(&group_id.to_string()), &qs2))
        .await
        .expect("S2 oneshot");
    assert_eq!(resp2.status(), StatusCode::OK, "S2 status");
    let body2 = body_json(resp2).await;
    let items2 = body2["items"].as_array().expect("S2 items");
    assert!(
        !items2.is_empty(),
        "S2 must return at least one memory result"
    );
    assert!(
        items2.iter().all(|it| it["type"] == "memory"),
        "S2 all items must be type=memory"
    );

    // ── S3. Combined types ────────────────────────────────────────────────
    let qs3 = format!(
        "q=fluffernutter&scope_type=group&scope_id={}&types=messages,memory",
        group_id
    );
    let resp3 = h
        .router
        .clone()
        .oneshot(search_req(Some(&token), Some(&group_id.to_string()), &qs3))
        .await
        .expect("S3 oneshot");
    assert_eq!(resp3.status(), StatusCode::OK, "S3 status");
    let body3 = body_json(resp3).await;
    let items3 = body3["items"].as_array().expect("S3 items");
    assert!(
        items3.iter().any(|it| it["type"] == "message"),
        "S3 must include message results"
    );
    assert!(
        items3.iter().any(|it| it["type"] == "memory"),
        "S3 must include memory results"
    );

    // ── S4. Empty results ─────────────────────────────────────────────────
    let qs4 = format!(
        "q=xyzzy_nonexistent_12345&scope_type=group&scope_id={}&types=messages,memory",
        group_id
    );
    let resp4 = h
        .router
        .clone()
        .oneshot(search_req(Some(&token), Some(&group_id.to_string()), &qs4))
        .await
        .expect("S4 oneshot");
    assert_eq!(resp4.status(), StatusCode::OK, "S4 status");
    let body4 = body_json(resp4).await;
    assert!(
        body4["items"].as_array().expect("S4 items").is_empty(),
        "S4 must return empty items"
    );
    assert_eq!(body4["has_more"], false, "S4 has_more must be false");

    // ── S5. Cross-group 404: owner1 with scope_id=group2 ─────────────────
    let qs5 = format!(
        "q=fluffernutter&scope_type=group&scope_id={}&types=messages",
        group2_id
    );
    let resp5 = h
        .router
        .clone()
        .oneshot(search_req(Some(&token), Some(&group_id.to_string()), &qs5))
        .await
        .expect("S5 oneshot");
    assert_eq!(resp5.status(), StatusCode::NOT_FOUND, "S5 expected 404");

    // ── S6. Empty q → 400 ────────────────────────────────────────────────
    let qs6 = format!("q=&scope_type=group&scope_id={}&types=messages", group_id);
    let resp6 = h
        .router
        .clone()
        .oneshot(search_req(Some(&token), Some(&group_id.to_string()), &qs6))
        .await
        .expect("S6 oneshot");
    assert_eq!(resp6.status(), StatusCode::BAD_REQUEST, "S6 expected 400");

    // ── S7. Unknown type → 400 ────────────────────────────────────────────
    let qs7 = format!(
        "q=hello&scope_type=group&scope_id={}&types=messages,files",
        group_id
    );
    let resp7 = h
        .router
        .clone()
        .oneshot(search_req(Some(&token), Some(&group_id.to_string()), &qs7))
        .await
        .expect("S7 oneshot");
    assert_eq!(resp7.status(), StatusCode::BAD_REQUEST, "S7 expected 400");

    // ── S8. scope_type=user + types=messages → 400 (plan 0085) ───────────
    // user scope is now valid, but messages have no user scope, so
    // `types=messages` is rejected with 400 at parse_and_validate.
    let qs8 = format!(
        "q=hello&scope_type=user&scope_id={}&types=messages",
        user_id
    );
    let resp8 = h
        .router
        .clone()
        .oneshot(search_req(Some(&token), Some(&group_id.to_string()), &qs8))
        .await
        .expect("S8 oneshot");
    assert_eq!(resp8.status(), StatusCode::BAD_REQUEST, "S8 expected 400");

    // ── S9. Missing bearer → 401 ─────────────────────────────────────────
    let qs9 = format!(
        "q=hello&scope_type=group&scope_id={}&types=messages",
        group_id
    );
    let resp9 = h
        .router
        .clone()
        .oneshot(search_req(None, Some(&group_id.to_string()), &qs9))
        .await
        .expect("S9 oneshot");
    assert_eq!(resp9.status(), StatusCode::UNAUTHORIZED, "S9 expected 401");

    // ── S10. secret memory must not appear ───────────────────────────────
    // "classified" word only appears in the secret memory item.
    let qs10 = format!(
        "q=classified&scope_type=group&scope_id={}&types=memory",
        group_id
    );
    let resp10 = h
        .router
        .clone()
        .oneshot(search_req(Some(&token), Some(&group_id.to_string()), &qs10))
        .await
        .expect("S10 oneshot");
    assert_eq!(resp10.status(), StatusCode::OK, "S10 status");
    let body10 = body_json(resp10).await;
    assert!(
        body10["items"].as_array().expect("S10 items").is_empty(),
        "S10 secret memory must not appear in search results"
    );

    // ── S11. Cross-group RLS: group2 can't see group1 messages ───────────
    let qs11 = format!(
        "q=fluffernutter&scope_type=group&scope_id={}&types=messages",
        group2_id
    );
    let resp11 = h
        .router
        .clone()
        .oneshot(search_req(
            Some(&token2),
            Some(&group2_id.to_string()),
            &qs11,
        ))
        .await
        .expect("S11 oneshot");
    assert_eq!(resp11.status(), StatusCode::OK, "S11 status");
    let body11 = body_json(resp11).await;
    assert!(
        body11["items"].as_array().expect("S11 items").is_empty(),
        "S11 group2 must not see group1 messages via search"
    );

    // ── Slice 2 — chat scope (S12-S14) ───────────────────────────────────

    // ── S12. scope_type=chat happy: returns only messages from that chat
    // chat_id (chat 1) has the "fluffernutter sandwich" message. chat2_id
    // has "fluffernutter only here in chat2". A chat-scoped search by
    // chat_id must not return chat 2's message.
    let qs12 = format!(
        "q=fluffernutter&scope_type=chat&scope_id={}&types=messages",
        chat_id
    );
    let resp12 = h
        .router
        .clone()
        .oneshot(search_req(Some(&token), Some(&group_id.to_string()), &qs12))
        .await
        .expect("S12 oneshot");
    assert_eq!(resp12.status(), StatusCode::OK, "S12 status");
    let body12 = body_json(resp12).await;
    let items12 = body12["items"].as_array().expect("S12 items");
    assert!(!items12.is_empty(), "S12 must return chat 1's message");
    for it in items12 {
        assert_eq!(
            it["chat_id"].as_str().unwrap_or(""),
            chat_id,
            "S12 every result must belong to chat 1"
        );
        assert!(
            !it["excerpt"]
                .as_str()
                .unwrap_or("")
                .contains("only here in chat2"),
            "S12 chat 2's message must not appear in chat 1's search"
        );
    }

    // ── S13. scope_type=chat with chat in another group → 404 ────────────
    // Owner1 attempts to search using a chat_id that lives in group2 — but
    // we only have chat_id and chat2_id in group1. We use a UUID that
    // doesn't belong to any chat in caller's group.
    let bogus_chat = uuid::Uuid::new_v4();
    let qs13 = format!(
        "q=fluffernutter&scope_type=chat&scope_id={}&types=messages",
        bogus_chat
    );
    let resp13 = h
        .router
        .clone()
        .oneshot(search_req(Some(&token), Some(&group_id.to_string()), &qs13))
        .await
        .expect("S13 oneshot");
    assert_eq!(resp13.status(), StatusCode::NOT_FOUND, "S13 expected 404");

    // ── S14. scope_type=chat memory: only chat-scope rows ────────────────
    // The "cinnamonroll" canary lives only in the chat-scope memory; the
    // group-scope memory uses "marshmallow". Chat-scope search for
    // "cinnamonroll" must hit; chat-scope search for "marshmallow" must
    // NOT hit (group-scope memory must not leak into chat-scope results).
    let qs14a = format!(
        "q=cinnamonroll&scope_type=chat&scope_id={}&types=memory",
        chat_id
    );
    let resp14a = h
        .router
        .clone()
        .oneshot(search_req(
            Some(&token),
            Some(&group_id.to_string()),
            &qs14a,
        ))
        .await
        .expect("S14a oneshot");
    assert_eq!(resp14a.status(), StatusCode::OK, "S14a status");
    let body14a = body_json(resp14a).await;
    let items14a = body14a["items"].as_array().expect("S14a items");
    assert_eq!(
        items14a.len(),
        1,
        "S14a must return exactly the chat-scope memory item"
    );
    assert_eq!(items14a[0]["scope_type"].as_str().unwrap_or(""), "chat");

    let qs14b = format!(
        "q=marshmallow&scope_type=chat&scope_id={}&types=memory",
        chat_id
    );
    let resp14b = h
        .router
        .clone()
        .oneshot(search_req(
            Some(&token),
            Some(&group_id.to_string()),
            &qs14b,
        ))
        .await
        .expect("S14b oneshot");
    assert_eq!(resp14b.status(), StatusCode::OK, "S14b status");
    let body14b = body_json(resp14b).await;
    assert!(
        body14b["items"].as_array().expect("S14b items").is_empty(),
        "S14b group-scope memory must NOT leak into chat-scope results"
    );

    // ── Slice 2 — user scope (S15-S17) ───────────────────────────────────

    // ── S15. scope_type=user happy: returns only caller's personal memory
    let qs15 = format!(
        "q=personalsecretphrase42&scope_type=user&scope_id={}&types=memory",
        user_id
    );
    let resp15 = h
        .router
        .clone()
        .oneshot(search_req(Some(&token), Some(&group_id.to_string()), &qs15))
        .await
        .expect("S15 oneshot");
    assert_eq!(resp15.status(), StatusCode::OK, "S15 status");
    let body15 = body_json(resp15).await;
    let items15 = body15["items"].as_array().expect("S15 items");
    assert_eq!(
        items15.len(),
        1,
        "S15 must return exactly owner1's personal memory"
    );
    let it15 = &items15[0];
    assert_eq!(it15["type"].as_str().unwrap_or(""), "memory");
    assert_eq!(it15["scope_type"].as_str().unwrap_or(""), "user");
    assert_eq!(it15["scope_id"].as_str().unwrap_or(""), user_id.to_string());
    assert!(
        it15["excerpt"]
            .as_str()
            .unwrap_or("")
            .contains("owner1 alone"),
        "S15 must return owner1's personal memory body, not owner2's"
    );

    // ── S16. scope_type=user with another user's id → 404 ───────────────
    // Owner1 attempts to search using user2_id; cross-user must 404.
    let qs16 = format!(
        "q=personalsecretphrase42&scope_type=user&scope_id={}&types=memory",
        user2_id
    );
    let resp16 = h
        .router
        .clone()
        .oneshot(search_req(Some(&token), Some(&group_id.to_string()), &qs16))
        .await
        .expect("S16 oneshot");
    assert_eq!(resp16.status(), StatusCode::NOT_FOUND, "S16 expected 404");

    // ── S17. scope_type=user with default types (messages,memory) → 400
    let qs17 = format!("q=hello&scope_type=user&scope_id={}", user_id);
    let resp17 = h
        .router
        .clone()
        .oneshot(search_req(Some(&token), Some(&group_id.to_string()), &qs17))
        .await
        .expect("S17 oneshot");
    assert_eq!(resp17.status(), StatusCode::BAD_REQUEST, "S17 expected 400");
}
