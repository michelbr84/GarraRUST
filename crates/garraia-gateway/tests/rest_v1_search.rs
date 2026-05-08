//! Integration tests for `GET /v1/search` (plan 0084, GAR-549).
//!
//! All scenarios bundled into ONE `#[tokio::test]` — same pattern as other
//! rest_v1_* tests (sqlx runtime-teardown race documented in plan 0016 M3).
//!
//! Scenarios (11 total):
//!
//!   S1.  GET 200 — message search: matching message returned.
//!   S2.  GET 200 — memory search: matching memory_item returned.
//!   S3.  GET 200 — combined types: both messages and memory in results.
//!   S4.  GET 200 — empty results: no match returns empty items array.
//!   S5.  GET 404 — cross-group: scope_id ≠ principal.group_id → 404.
//!   S6.  GET 400 — empty q → 400.
//!   S7.  GET 400 — unknown type in types → 400.
//!   S8.  GET 400 — unsupported scope_type (user) → 400.
//!   S9.  GET 401 — missing bearer → 401.
//!   S10. GET 200 — sensitivity='secret' memory excluded.
//!   S11. GET 200 — cross-group RLS: group2 cannot see group1 messages.

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

/// Helper: create a memory item.
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

/// Helper: GET /v1/search with the given query string.
fn search_req(token: Option<&str>, x_group_id: Option<&str>, qs: &str) -> Request<Body> {
    req("GET", &format!("/v1/search?{qs}"), token, x_group_id, None)
}

// ─── Test suite ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn search_scenarios() {
    let h = Harness::get().await;

    // Seed two isolated groups.
    let (_, group_id, token) = seed_user_with_group(&h, "owner@search-test.test")
        .await
        .expect("seed owner");
    let (_, group2_id, token2) = seed_user_with_group(&h, "owner2@search-test.test")
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

    // ── S8. Unsupported scope_type=user → 400 ────────────────────────────
    let qs8 = format!(
        "q=hello&scope_type=user&scope_id={}&types=messages",
        group_id
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
}
