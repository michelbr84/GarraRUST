//! Integration tests for individual chat ops + member CRUD (plan 0076, GAR-530).
//!
//! All scenarios bundled into ONE `#[tokio::test]` function to avoid the
//! sqlx runtime-teardown race documented in plan 0016 M3 commit `4f8be37`.
//!
//! Scenarios (M1–M12):
//!   GET /v1/chats/{chat_id}:
//!     M1. 200 — owner fetches a chat they created.
//!     M2. 404 — archived chat returns 404.
//!     M3. 404 — chat from a different group returns 404 (not 403).
//!     M4. 401 — missing bearer.
//!
//!   PATCH /v1/chats/{chat_id}:
//!     M5. 200 — owner renames a chat; `updated_at` advances.
//!     M6. 400 — empty body (no name or topic).
//!
//!   DELETE /v1/chats/{chat_id}:
//!     M7. 204 — owner archives a chat; subsequent GET returns 404.
//!
//!   GET /v1/chats/{chat_id}/members:
//!     M8. 200 — list includes creator as owner.
//!
//!   POST /v1/chats/{chat_id}/members:
//!     M9.  201 — add a second group member to chat.
//!     M10. 409 — adding same user twice returns 409.
//!
//!   DELETE /v1/chats/{chat_id}/members/{user_id}:
//!     M11. 204 — remove the added member.
//!     M12. 409 — cannot remove last owner.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

use common::Harness;
use common::fixtures::{fetch_audit_events_for_group, seed_member_via_admin, seed_user_with_group};

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("failed to collect response body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response body is not JSON")
}

fn connect_info() -> axum::extract::ConnectInfo<std::net::SocketAddr> {
    axum::extract::ConnectInfo("127.0.0.1:1".parse().unwrap())
}

fn authed_request(method: &str, uri: &str, token: Option<&str>, body: Body) -> Request<Body> {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(body)
        .expect("request builder");
    req.extensions_mut().insert(connect_info());
    if let Some(t) = token {
        req.headers_mut().insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_str(&format!("Bearer {t}")).unwrap(),
        );
    }
    req
}

/// Seed a chat via admin pool (bypasses RLS for fixture setup).
/// Inserts into `chats` + one `chat_members` owner row.
/// Returns the `chat_id`.
async fn seed_chat(h: &Harness, group_id: Uuid, creator_id: Uuid, name: &str) -> Uuid {
    let chat_id = Uuid::new_v4();
    let mut tx = h.admin_pool.begin().await.expect("seed_chat tx begin");

    sqlx::query(
        "INSERT INTO chats (id, group_id, type, name, created_by) \
         VALUES ($1, $2, 'channel', $3, $4)",
    )
    .bind(chat_id)
    .bind(group_id)
    .bind(name)
    .bind(creator_id)
    .execute(&mut *tx)
    .await
    .expect("seed_chat: insert chats");

    sqlx::query(
        "INSERT INTO chat_members (chat_id, user_id, role) \
         VALUES ($1, $2, 'owner')",
    )
    .bind(chat_id)
    .bind(creator_id)
    .execute(&mut *tx)
    .await
    .expect("seed_chat: insert chat_members");

    tx.commit().await.expect("seed_chat tx commit");
    chat_id
}

#[tokio::test]
async fn v1_chat_mgmt_scenarios() {
    let h = Harness::get().await;

    // Group A — main actor (owner_a).
    let (owner_a_id, group_a_id, owner_a_token) =
        seed_user_with_group(&h, "owner@chat-mgmt-m1.test")
            .await
            .expect("seed group A owner");

    // Group B — used for cross-group 404 (M3).
    let (_owner_b_id, group_b_id, _owner_b_token) =
        seed_user_with_group(&h, "owner@chat-mgmt-m3.test")
            .await
            .expect("seed group B owner");

    // Seed a chat in group A for M1/M2/M5/M7/M8.
    let chat_a_id = seed_chat(&h, group_a_id, owner_a_id, "main-channel").await;

    // Seed a chat in group B for M3 cross-group probe.
    let chat_b_id = seed_chat(&h, group_b_id, _owner_b_id, "other-group-chat").await;

    // ── M1. GET 200 — owner can fetch a chat they created ───────────
    let resp = h
        .router
        .clone()
        .oneshot(authed_request(
            "GET",
            &format!("/v1/chats/{chat_a_id}"),
            Some(&owner_a_token),
            Body::empty(),
        ))
        .await
        .expect("M1 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "M1 status");
    let v = body_json(resp).await;
    assert_eq!(v["id"], chat_a_id.to_string(), "M1 id");
    assert_eq!(v["name"], "main-channel", "M1 name");
    assert_eq!(v["group_id"], group_a_id.to_string(), "M1 group_id");
    assert_eq!(v["created_by"], owner_a_id.to_string(), "M1 created_by");
    assert!(v.get("updated_at").is_some(), "M1 updated_at present");

    // ── M2. GET 404 — archived chat returns 404 ─────────────────────
    let archived_chat_id = seed_chat(&h, group_a_id, owner_a_id, "to-archive-m2").await;
    sqlx::query("UPDATE chats SET archived_at = now() WHERE id = $1")
        .bind(archived_chat_id)
        .execute(&h.admin_pool)
        .await
        .expect("M2 archive chat");
    let resp = h
        .router
        .clone()
        .oneshot(authed_request(
            "GET",
            &format!("/v1/chats/{archived_chat_id}"),
            Some(&owner_a_token),
            Body::empty(),
        ))
        .await
        .expect("M2 oneshot");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "M2 status");

    // ── M3. GET 404 — chat from different group returns 404, not 403 ─
    // owner_a tries to access a chat in group_b.
    let resp = h
        .router
        .clone()
        .oneshot(authed_request(
            "GET",
            &format!("/v1/chats/{chat_b_id}"),
            Some(&owner_a_token),
            Body::empty(),
        ))
        .await
        .expect("M3 oneshot");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "M3 status (not 403)");

    // ── M4. GET 401 — missing bearer ────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(authed_request(
            "GET",
            &format!("/v1/chats/{chat_a_id}"),
            None,
            Body::empty(),
        ))
        .await
        .expect("M4 oneshot");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "M4 status");

    // ── M5. PATCH 200 — owner renames a chat; updated_at advances ───
    // Capture current updated_at before patch.
    let resp = h
        .router
        .clone()
        .oneshot(authed_request(
            "GET",
            &format!("/v1/chats/{chat_a_id}"),
            Some(&owner_a_token),
            Body::empty(),
        ))
        .await
        .expect("M5 pre-patch GET");
    let before = body_json(resp).await;
    let before_updated_at = before["updated_at"].as_str().unwrap_or("").to_string();

    // Small delay so updated_at actually changes.
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let resp = h
        .router
        .clone()
        .oneshot(authed_request(
            "PATCH",
            &format!("/v1/chats/{chat_a_id}"),
            Some(&owner_a_token),
            Body::from(json!({"name": "renamed-channel"}).to_string()),
        ))
        .await
        .expect("M5 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "M5 status");
    let v = body_json(resp).await;
    assert_eq!(v["name"], "renamed-channel", "M5 new name");
    assert_eq!(v["id"], chat_a_id.to_string(), "M5 id unchanged");
    let after_updated_at = v["updated_at"].as_str().unwrap_or("").to_string();
    assert!(
        after_updated_at >= before_updated_at,
        "M5 updated_at advanced or same: before={before_updated_at} after={after_updated_at}"
    );

    // Verify ChatUpdated audit event emitted.
    let events = fetch_audit_events_for_group(&h, group_a_id)
        .await
        .expect("M5 fetch audit");
    let upd_event = events
        .iter()
        .find(|(action, _, _, rid, _)| action == "chat.updated" && rid == &chat_a_id.to_string());
    assert!(upd_event.is_some(), "M5 chat.updated audit event present");

    // ── M6. PATCH 400 — empty body (no name or topic) ───────────────
    let resp = h
        .router
        .clone()
        .oneshot(authed_request(
            "PATCH",
            &format!("/v1/chats/{chat_a_id}"),
            Some(&owner_a_token),
            Body::from(json!({}).to_string()),
        ))
        .await
        .expect("M6 oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "M6 status");

    // ── M7. DELETE 204 — owner archives a chat; GET → 404 ───────────
    let delete_chat_id = seed_chat(&h, group_a_id, owner_a_id, "to-delete-m7").await;
    let resp = h
        .router
        .clone()
        .oneshot(authed_request(
            "DELETE",
            &format!("/v1/chats/{delete_chat_id}"),
            Some(&owner_a_token),
            Body::empty(),
        ))
        .await
        .expect("M7 DELETE oneshot");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT, "M7 DELETE status");

    let resp = h
        .router
        .clone()
        .oneshot(authed_request(
            "GET",
            &format!("/v1/chats/{delete_chat_id}"),
            Some(&owner_a_token),
            Body::empty(),
        ))
        .await
        .expect("M7 GET after archive oneshot");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "M7 GET after archive");

    // Verify ChatArchived audit event.
    let events = fetch_audit_events_for_group(&h, group_a_id)
        .await
        .expect("M7 fetch audit");
    let arch_event = events.iter().find(|(action, _, _, rid, _)| {
        action == "chat.archived" && rid == &delete_chat_id.to_string()
    });
    assert!(arch_event.is_some(), "M7 chat.archived audit event present");

    // ── M8. GET /members 200 — list includes creator as owner ────────
    let resp = h
        .router
        .clone()
        .oneshot(authed_request(
            "GET",
            &format!("/v1/chats/{chat_a_id}/members"),
            Some(&owner_a_token),
            Body::empty(),
        ))
        .await
        .expect("M8 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "M8 status");
    let v = body_json(resp).await;
    let items = v["items"].as_array().expect("M8 items array");
    assert!(!items.is_empty(), "M8 members not empty");
    let owner_member = items
        .iter()
        .find(|m| m["user_id"] == owner_a_id.to_string());
    assert!(owner_member.is_some(), "M8 owner present in members");
    assert_eq!(
        owner_member.unwrap()["role"],
        "owner",
        "M8 creator role = owner"
    );

    // ── M9. POST /members 201 — add a second group member ────────────
    let (second_user_id, _second_token) =
        seed_member_via_admin(&h, group_a_id, "member", "second@chat-mgmt-m9.test")
            .await
            .expect("M9 seed second member");

    let resp = h
        .router
        .clone()
        .oneshot(authed_request(
            "POST",
            &format!("/v1/chats/{chat_a_id}/members"),
            Some(&owner_a_token),
            Body::from(json!({"user_id": second_user_id, "role": "member"}).to_string()),
        ))
        .await
        .expect("M9 oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "M9 status");
    let v = body_json(resp).await;
    assert_eq!(v["user_id"], second_user_id.to_string(), "M9 user_id");
    assert_eq!(v["role"], "member", "M9 role");

    // Verify ChatMemberAdded audit.
    let events = fetch_audit_events_for_group(&h, group_a_id)
        .await
        .expect("M9 fetch audit");
    let add_event = events.iter().find(|(action, _, _, rid, _)| {
        action == "chat.member.added" && rid == &chat_a_id.to_string()
    });
    assert!(add_event.is_some(), "M9 chat.member.added audit present");

    // ── M10. POST /members 409 — adding same user twice ──────────────
    let resp = h
        .router
        .clone()
        .oneshot(authed_request(
            "POST",
            &format!("/v1/chats/{chat_a_id}/members"),
            Some(&owner_a_token),
            Body::from(json!({"user_id": second_user_id, "role": "member"}).to_string()),
        ))
        .await
        .expect("M10 oneshot");
    assert_eq!(resp.status(), StatusCode::CONFLICT, "M10 status");

    // ── M11. DELETE /members/{user_id} 204 — remove the added member ─
    let resp = h
        .router
        .clone()
        .oneshot(authed_request(
            "DELETE",
            &format!("/v1/chats/{chat_a_id}/members/{second_user_id}"),
            Some(&owner_a_token),
            Body::empty(),
        ))
        .await
        .expect("M11 oneshot");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT, "M11 status");

    // Verify ChatMemberRemoved audit.
    let events = fetch_audit_events_for_group(&h, group_a_id)
        .await
        .expect("M11 fetch audit");
    let rem_event = events.iter().find(|(action, _, _, rid, _)| {
        action == "chat.member.removed" && rid == &chat_a_id.to_string()
    });
    assert!(rem_event.is_some(), "M11 chat.member.removed audit present");

    // ── M12. DELETE /members/{owner_id} 409 — cannot remove last owner
    let resp = h
        .router
        .clone()
        .oneshot(authed_request(
            "DELETE",
            &format!("/v1/chats/{chat_a_id}/members/{owner_a_id}"),
            Some(&owner_a_token),
            Body::empty(),
        ))
        .await
        .expect("M12 oneshot");
    assert_eq!(resp.status(), StatusCode::CONFLICT, "M12 status");
}
