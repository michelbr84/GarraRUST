#![cfg(feature = "test-helpers")]
//! Integration tests for:
//!   `POST   /v1/groups/{group_id}/folders`
//!   `DELETE /v1/groups/{group_id}/folders/{folder_id}`
//! (plan 0092, GAR-562, Fase 3.4 files slice 5).
//!
//! All scenarios in ONE `#[tokio::test]` — same pattern as the other files
//! integration suites (`rest_v1_folders_patch.rs`, `rest_v1_files_patch.rs`).
//!
//! Scenarios covered (10 total):
//!
//! C1. POST 201 — create root folder (parent_id absent).
//! C2. POST 201 — create nested folder under a live parent; parent_id echoed.
//! C3. POST 403 — path group_id ≠ principal group_id.
//! C4. POST 400 — name > 200 chars (folder name boundary).
//! C5. POST 404 — parent_id points to a soft-deleted folder.
//!
//! D1. DELETE 204 — soft-delete a live folder; deleted_at set in DB.
//! D2. DELETE 404 — folder already soft-deleted.
//! D3. DELETE 404 — non-existent folder_id.
//! D4. DELETE 403 — path group_id ≠ principal group_id.
//! D5. DELETE 204 — verify audit row `folder.deleted` is present after success.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use chrono::Utc;
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

use common::Harness;
use common::fixtures::{fetch_audit_events_for_group, seed_user_with_group};

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("body is JSON")
}

fn build_req(
    method: &str,
    uri: &str,
    token: Option<&str>,
    x_group_id: Option<&str>,
    body: Option<serde_json::Value>,
) -> Request<Body> {
    let raw_body = body.map(|v| v.to_string()).unwrap_or_default();
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(raw_body))
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

/// Seed a folder directly via the admin pool.
async fn seed_folder(
    h: &Harness,
    group_id: Uuid,
    parent_id: Option<Uuid>,
    created_by: Uuid,
    name: &str,
) -> anyhow::Result<Uuid> {
    let folder_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO folders (id, group_id, parent_id, name, created_by, created_by_label) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(folder_id)
    .bind(group_id)
    .bind(parent_id)
    .bind(name)
    .bind(created_by)
    .bind("Test User")
    .execute(&h.admin_pool)
    .await?;
    Ok(folder_id)
}

/// Soft-delete a folder directly via the admin pool.
async fn soft_delete_folder(h: &Harness, folder_id: Uuid) -> anyhow::Result<()> {
    sqlx::query("UPDATE folders SET deleted_at = $1 WHERE id = $2")
        .bind(Utc::now())
        .bind(folder_id)
        .execute(&h.admin_pool)
        .await?;
    Ok(())
}

#[tokio::test]
async fn v1_folders_post_delete_scenarios() {
    let h = Harness::get().await;

    // Seed owner + group A — reused across all scenarios.
    let (owner_id, group_id, owner_token) = seed_user_with_group(&h, "owner@folders-slice5.test")
        .await
        .expect("seed owner+group A");

    let g = group_id.to_string();
    let other_group = Uuid::new_v4().to_string();

    // ── C1. POST 201 — root folder ──────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(build_req(
            "POST",
            &format!("/v1/groups/{g}/folders"),
            Some(&owner_token),
            Some(&g),
            Some(json!({ "name": "my-root-folder" })),
        ))
        .await
        .expect("C1 oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "C1 status");
    let v = body_json(resp).await;
    assert_eq!(v["name"], "my-root-folder", "C1 name");
    assert!(v["parent_id"].is_null(), "C1 parent_id null");
    let c1_folder_id = v["id"].as_str().expect("C1 id").to_string();

    // C1 — verify DB row.
    let (db_name,): (String,) = sqlx::query_as("SELECT name FROM folders WHERE id = $1")
        .bind(Uuid::parse_str(&c1_folder_id).unwrap())
        .fetch_one(&h.admin_pool)
        .await
        .expect("C1 fetch db");
    assert_eq!(db_name, "my-root-folder", "C1 db name");

    // C1 — audit row with PII-safe metadata.
    let events = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("C1 fetch audit");
    let create_event = events
        .iter()
        .find(|(action, _, _, rid, _)| action == "folder.created" && rid == &c1_folder_id)
        .expect("C1 folder.created audit row");
    let (_, actor, resource_type, _, metadata) = create_event;
    assert_eq!(actor.as_ref(), Some(&owner_id), "C1 audit actor");
    assert_eq!(resource_type, "folders", "C1 audit resource_type");
    assert!(
        metadata.get("name").is_none(),
        "C1 audit MUST NOT carry raw folder name"
    );
    assert!(
        metadata.get("name_len").is_some(),
        "C1 audit must carry name_len"
    );

    // ── C2. POST 201 — nested folder ───────────────────────────────
    let parent_id = seed_folder(&h, group_id, None, owner_id, "parent-folder-c2")
        .await
        .expect("C2 seed parent");

    let resp = h
        .router
        .clone()
        .oneshot(build_req(
            "POST",
            &format!("/v1/groups/{g}/folders"),
            Some(&owner_token),
            Some(&g),
            Some(json!({ "name": "nested-folder", "parent_id": parent_id })),
        ))
        .await
        .expect("C2 oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "C2 status");
    let v = body_json(resp).await;
    assert_eq!(v["name"], "nested-folder", "C2 name");
    assert_eq!(
        v["parent_id"].as_str().expect("C2 parent_id str"),
        parent_id.to_string(),
        "C2 parent_id echoed"
    );

    // ── C3. POST 403 — group mismatch ──────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(build_req(
            "POST",
            &format!("/v1/groups/{other_group}/folders"),
            Some(&owner_token),
            Some(&other_group),
            Some(json!({ "name": "should-fail" })),
        ))
        .await
        .expect("C3 oneshot");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "C3 status");

    // ── C4. POST 400 — name too long (> 200 chars) ─────────────────
    let long_name = "a".repeat(201);
    let resp = h
        .router
        .clone()
        .oneshot(build_req(
            "POST",
            &format!("/v1/groups/{g}/folders"),
            Some(&owner_token),
            Some(&g),
            Some(json!({ "name": long_name })),
        ))
        .await
        .expect("C4 oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "C4 status");

    // ── C5. POST 404 — parent_id is soft-deleted ───────────────────
    let deleted_parent = seed_folder(&h, group_id, None, owner_id, "deleted-parent-c5")
        .await
        .expect("C5 seed parent");
    soft_delete_folder(&h, deleted_parent)
        .await
        .expect("C5 soft-delete parent");

    let resp = h
        .router
        .clone()
        .oneshot(build_req(
            "POST",
            &format!("/v1/groups/{g}/folders"),
            Some(&owner_token),
            Some(&g),
            Some(json!({ "name": "nested-under-deleted", "parent_id": deleted_parent })),
        ))
        .await
        .expect("C5 oneshot");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "C5 status");

    // ── D1. DELETE 204 — soft-delete a live folder ─────────────────
    let d1_folder = seed_folder(&h, group_id, None, owner_id, "d1-live-folder")
        .await
        .expect("D1 seed folder");

    let resp = h
        .router
        .clone()
        .oneshot(build_req(
            "DELETE",
            &format!("/v1/groups/{g}/folders/{d1_folder}"),
            Some(&owner_token),
            Some(&g),
            None,
        ))
        .await
        .expect("D1 oneshot");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT, "D1 status");

    // D1 — verify deleted_at is set in DB.
    let (deleted_at,): (Option<chrono::DateTime<Utc>>,) =
        sqlx::query_as("SELECT deleted_at FROM folders WHERE id = $1")
            .bind(d1_folder)
            .fetch_one(&h.admin_pool)
            .await
            .expect("D1 fetch db");
    assert!(deleted_at.is_some(), "D1 deleted_at set in DB");

    // ── D2. DELETE 404 — already soft-deleted ──────────────────────
    let d2_folder = seed_folder(&h, group_id, None, owner_id, "d2-already-deleted")
        .await
        .expect("D2 seed folder");
    soft_delete_folder(&h, d2_folder)
        .await
        .expect("D2 soft-delete setup");

    let resp = h
        .router
        .clone()
        .oneshot(build_req(
            "DELETE",
            &format!("/v1/groups/{g}/folders/{d2_folder}"),
            Some(&owner_token),
            Some(&g),
            None,
        ))
        .await
        .expect("D2 oneshot");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "D2 status");

    // ── D3. DELETE 404 — non-existent folder ───────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(build_req(
            "DELETE",
            &format!("/v1/groups/{g}/folders/{}", Uuid::new_v4()),
            Some(&owner_token),
            Some(&g),
            None,
        ))
        .await
        .expect("D3 oneshot");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "D3 status");

    // ── D4. DELETE 403 — group mismatch ────────────────────────────
    let d4_folder = seed_folder(&h, group_id, None, owner_id, "d4-group-mismatch")
        .await
        .expect("D4 seed folder");

    let resp = h
        .router
        .clone()
        .oneshot(build_req(
            "DELETE",
            &format!("/v1/groups/{other_group}/folders/{d4_folder}"),
            Some(&owner_token),
            Some(&other_group),
            None,
        ))
        .await
        .expect("D4 oneshot");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "D4 status");

    // ── D5. DELETE 204 + audit ─────────────────────────────────────
    let d5_folder = seed_folder(&h, group_id, None, owner_id, "d5-audit-check")
        .await
        .expect("D5 seed folder");

    let resp = h
        .router
        .clone()
        .oneshot(build_req(
            "DELETE",
            &format!("/v1/groups/{g}/folders/{d5_folder}"),
            Some(&owner_token),
            Some(&g),
            None,
        ))
        .await
        .expect("D5 oneshot");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT, "D5 status");

    let events = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("D5 fetch audit");
    let delete_event = events
        .iter()
        .find(|(action, _, _, rid, _)| action == "folder.deleted" && rid == &d5_folder.to_string())
        .expect("D5 folder.deleted audit row");
    let (_, actor, resource_type, _, metadata) = delete_event;
    assert_eq!(actor.as_ref(), Some(&owner_id), "D5 audit actor");
    assert_eq!(resource_type, "folders", "D5 audit resource_type");
    assert!(
        metadata.get("name").is_none(),
        "D5 audit MUST NOT carry raw folder name"
    );
}
