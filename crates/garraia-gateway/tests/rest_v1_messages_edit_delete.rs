//! Integration tests for `PATCH /v1/messages/{id}` and
//! `DELETE /v1/messages/{id}` (plan 0107, GAR-592).
//!
//! All scenarios are bundled into ONE `#[tokio::test]` function to
//! avoid the sqlx runtime-teardown race documented in plan 0016 M3
//! commit `4f8be37`.
//!
//! Edit scenarios (10):
//!   ME1. PATCH 200 — sender edits own message; body + edited_at updated.
//!   ME2. PATCH 404 — non-existent message_id.
//!   ME3. PATCH 404 — message sent by another user (sender check).
//!   ME4. PATCH 404 — message already soft-deleted.
//!   ME5. PATCH 400 — empty body rejected.
//!
//! Delete scenarios (5):
//!   MD1. DELETE 204 — sender deletes own message; subsequent PATCH → 404.
//!   MD2. DELETE 404 — already-deleted message (idempotent guard).
//!   MD3. DELETE 404 — member tries to delete another user's message.
//!   MD4. DELETE 204 — admin deletes another user's message (admin override).
//!   MD5. DELETE 404 — cross-group: group-B message invisible to group-A caller.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

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

fn patch_message(
    token: Option<&str>,
    message_id: &str,
    x_group_id: Option<&str>,
    body: serde_json::Value,
) -> Request<Body> {
    let mut req = Request::builder()
        .method("PATCH")
        .uri(format!("/v1/messages/{message_id}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
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

fn delete_message(
    token: Option<&str>,
    message_id: &str,
    x_group_id: Option<&str>,
) -> Request<Body> {
    let mut req = Request::builder()
        .method("DELETE")
        .uri(format!("/v1/messages/{message_id}"))
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

/// Create a chat via `POST /v1/groups/{group_id}/chats`, return chat_id.
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
        .expect("create_chat oneshot");
    let b = body_json(resp).await;
    b["id"].as_str().unwrap().to_string()
}

/// Send one message to a chat, return the message_id.
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
        .expect("send_message oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "seed message should 201"
    );
    let b = body_json(resp).await;
    b["id"].as_str().unwrap().to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn rest_v1_messages_edit_delete_scenarios() {
    let h = Harness::get().await;

    // Primary actor: owner of group1.
    let (owner_id, group1_id, owner_token) = seed_user_with_group(&h, "owner@msg-edit-delete.test")
        .await
        .expect("seed owner+group1");
    let group1_str = group1_id.to_string();

    let chat_id = create_chat(&h, &owner_token, &group1_str, "edit-delete-general").await;

    // Second actor in group1 — Member (tier 50, cannot delete others' messages).
    let (member_id, member_token) =
        seed_member_via_admin(&h, group1_id, "member", "member@msg-edit-delete.test")
            .await
            .expect("seed member");

    // Third actor in group1 — Admin (tier 80, can delete others' messages).
    let (_admin_id, admin_token) =
        seed_member_via_admin(&h, group1_id, "admin", "admin@msg-edit-delete.test")
            .await
            .expect("seed admin");

    // Independent group2 + owner for cross-group isolation tests.
    let (_, group2_id, owner2_token) = seed_user_with_group(&h, "owner2@msg-edit-delete.test")
        .await
        .expect("seed owner2+group2");
    let group2_str = group2_id.to_string();
    let chat2_id = create_chat(&h, &owner2_token, &group2_str, "edit-delete-g2").await;

    // Pre-seed messages used across multiple scenarios.
    // msg_owner: sent by owner — used in ME1, ME3, ME4, MD1, MD2.
    let msg_owner = send_message(&h, &owner_token, &chat_id, &group1_str, "Original body").await;
    // msg_member: sent by member — used in ME3, MD3, MD4.
    let msg_member = send_message(&h, &member_token, &chat_id, &group1_str, "Member message").await;
    // msg_in_g2: sent by owner2 in group2 — used in MD5.
    let msg_in_g2 = send_message(&h, &owner2_token, &chat2_id, &group2_str, "Group2 message").await;

    // ── ME1. PATCH 200 — sender edits own message ────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(patch_message(
            Some(&owner_token),
            &msg_owner,
            Some(&group1_str),
            json!({"body": "Updated body"}),
        ))
        .await
        .expect("ME1 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "ME1 status");
    let me1_body = body_json(resp).await;
    assert_eq!(me1_body["body"], "Updated body", "ME1 body updated");
    assert_eq!(me1_body["id"], msg_owner, "ME1 id matches");
    assert_eq!(me1_body["group_id"], group1_str, "ME1 group_id matches");
    assert!(
        !me1_body["edited_at"].is_null(),
        "ME1 edited_at must be present"
    );

    // ME1 — audit row: structural only, no body content.
    let events = fetch_audit_events_for_group(&h, group1_id)
        .await
        .expect("ME1 fetch audit");
    let edit_event = events
        .iter()
        .find(|e| e.0 == "message.edited" && e.3 == msg_owner)
        .expect("ME1 message.edited audit row missing");
    let (_, actor, resource_type, _, metadata) = edit_event;
    assert_eq!(actor, &Some(owner_id), "ME1 audit actor");
    assert_eq!(resource_type, "messages", "ME1 audit resource_type");
    assert_eq!(
        metadata["body_len"],
        "Updated body".chars().count() as i64,
        "ME1 audit body_len"
    );
    assert!(
        metadata.get("body").is_none(),
        "ME1 audit MUST NOT carry body content"
    );

    // ── ME2. PATCH 404 — non-existent message_id ──────────────────────
    let nonexistent = uuid::Uuid::new_v4().to_string();
    let resp = h
        .router
        .clone()
        .oneshot(patch_message(
            Some(&owner_token),
            &nonexistent,
            Some(&group1_str),
            json!({"body": "Any body"}),
        ))
        .await
        .expect("ME2 oneshot");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "ME2 status");

    // ── ME3. PATCH 404 — edit another user's message (sender check) ──────
    // owner tries to edit msg_member (which was sent by member).
    let resp = h
        .router
        .clone()
        .oneshot(patch_message(
            Some(&owner_token),
            &msg_member,
            Some(&group1_str),
            json!({"body": "Owner trying to edit member's message"}),
        ))
        .await
        .expect("ME3 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "ME3 status (wrong sender → 404)"
    );

    // ── ME4. PATCH 404 — already-deleted message ─────────────────────
    // Soft-delete msg_member first via its sender, then try to edit it.
    let del_resp = h
        .router
        .clone()
        .oneshot(delete_message(
            Some(&member_token),
            &msg_member,
            Some(&group1_str),
        ))
        .await
        .expect("ME4 pre-delete");
    assert_eq!(
        del_resp.status(),
        StatusCode::NO_CONTENT,
        "ME4 pre-delete should 204"
    );

    let resp = h
        .router
        .clone()
        .oneshot(patch_message(
            Some(&member_token),
            &msg_member,
            Some(&group1_str),
            json!({"body": "Edit after delete"}),
        ))
        .await
        .expect("ME4 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "ME4 status (deleted → 404)"
    );

    // ── ME5. PATCH 400 — empty body rejected ────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(patch_message(
            Some(&owner_token),
            &msg_owner,
            Some(&group1_str),
            json!({"body": "   "}),
        ))
        .await
        .expect("ME5 oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "ME5 status");

    // ── MD1. DELETE 204 — sender deletes own message ────────────────────
    // Seed a fresh message to use for MD1 (msg_owner was edited, still live).
    let msg_for_md1 = send_message(
        &h,
        &owner_token,
        &chat_id,
        &group1_str,
        "To be deleted by owner",
    )
    .await;

    let resp = h
        .router
        .clone()
        .oneshot(delete_message(
            Some(&owner_token),
            &msg_for_md1,
            Some(&group1_str),
        ))
        .await
        .expect("MD1 oneshot");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT, "MD1 status");

    // MD1 — verify deleted_at set in DB.
    let (deleted_at,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT deleted_at FROM messages WHERE id = $1")
            .bind(uuid::Uuid::parse_str(&msg_for_md1).unwrap())
            .fetch_one(&h.admin_pool)
            .await
            .expect("MD1 DB check");
    assert!(deleted_at.is_some(), "MD1 deleted_at must be set");

    // MD1 — audit row.
    let events = fetch_audit_events_for_group(&h, group1_id)
        .await
        .expect("MD1 fetch audit");
    let del_event = events
        .iter()
        .find(|e| e.0 == "message.deleted" && e.3 == msg_for_md1)
        .expect("MD1 message.deleted audit row missing");
    let (_, _, _, _, md1_meta) = del_event;
    assert_eq!(
        md1_meta["admin_override"], false,
        "MD1 admin_override must be false"
    );

    // ── MD2. DELETE 404 — already-deleted message (idempotent guard) ─────
    let resp = h
        .router
        .clone()
        .oneshot(delete_message(
            Some(&owner_token),
            &msg_for_md1,
            Some(&group1_str),
        ))
        .await
        .expect("MD2 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "MD2 status (already deleted → 404)"
    );

    // ── MD3. DELETE 404 — member cannot delete another user's message ─────
    // Send a fresh message by owner; member tries to delete it.
    let msg_for_md3 =
        send_message(&h, &owner_token, &chat_id, &group1_str, "Owner msg for MD3").await;

    let resp = h
        .router
        .clone()
        .oneshot(delete_message(
            Some(&member_token),
            &msg_for_md3,
            Some(&group1_str),
        ))
        .await
        .expect("MD3 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "MD3 status (member, wrong sender → 404)"
    );

    // Confirm message is still live.
    let (still_alive,): (bool,) =
        sqlx::query_as("SELECT deleted_at IS NULL FROM messages WHERE id = $1")
            .bind(uuid::Uuid::parse_str(&msg_for_md3).unwrap())
            .fetch_one(&h.admin_pool)
            .await
            .expect("MD3 alive check");
    assert!(still_alive, "MD3 message must not be deleted");

    // ── MD4. DELETE 204 — admin override deletes another user's message ───
    // Use msg_for_md3 (owner's message, still live); admin deletes it.
    let resp = h
        .router
        .clone()
        .oneshot(delete_message(
            Some(&admin_token),
            &msg_for_md3,
            Some(&group1_str),
        ))
        .await
        .expect("MD4 oneshot");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT, "MD4 status");

    // MD4 — audit row: admin_override = true.
    let events = fetch_audit_events_for_group(&h, group1_id)
        .await
        .expect("MD4 fetch audit");
    let md4_event = events
        .iter()
        .find(|e| e.0 == "message.deleted" && e.3 == msg_for_md3)
        .expect("MD4 message.deleted audit row missing");
    let (_, _, _, _, md4_meta) = md4_event;
    assert_eq!(
        md4_meta["admin_override"], true,
        "MD4 admin_override must be true"
    );

    // ── MD5. DELETE 404 — cross-group isolation ──────────────────────────
    // owner (group1) tries to delete msg_in_g2 (group2) using group1 context.
    let resp = h
        .router
        .clone()
        .oneshot(delete_message(
            Some(&owner_token),
            &msg_in_g2,
            Some(&group1_str),
        ))
        .await
        .expect("MD5 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "MD5 status (cross-group → 404)"
    );

    // Confirm group2's message is still live.
    let (g2_alive,): (bool,) =
        sqlx::query_as("SELECT deleted_at IS NULL FROM messages WHERE id = $1")
            .bind(uuid::Uuid::parse_str(&msg_in_g2).unwrap())
            .fetch_one(&h.admin_pool)
            .await
            .expect("MD5 alive check");
    assert!(g2_alive, "MD5 group2 message must not be deleted");

    // Suppress unused variable warnings for IDs only referenced in
    // cross-scenario setup assertions above.
    let _ = owner_id;
    let _ = member_id;
}
