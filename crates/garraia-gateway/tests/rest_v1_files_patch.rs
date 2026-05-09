//! Integration tests for `PATCH /v1/groups/{group_id}/files/{file_id}`
//! (plan 0089, GAR-557).
//!
//! All scenarios bundled into ONE `#[tokio::test]` function — same pattern
//! as `rest_v1_chats.rs`/`rest_v1_groups.rs`. Splitting into multiple
//! `#[tokio::test]`s historically triggered the sqlx runtime-teardown race
//! documented in plan 0016 M3 commit `4f8be37`.
//!
//! Scenarios covered (7 total):
//!
//! F1. PATCH 200 — owner happy-path rename: asserts response shape, DB
//!     `files.name` updated, `audit_events` row with action=`file.renamed`
//!     and structural-only metadata (`name_len`, `group_id` only).
//! F2. PATCH 404 — soft-deleted file (deleted_at IS NOT NULL).
//! F3. PATCH 404 — non-existent file_id.
//! F4. PATCH 400 — empty name (after trim).
//! F5. PATCH 400 — name exceeding 500 chars.
//! F6. PATCH 400 — name containing `/`.
//! F7. PATCH 403 — path group_id ≠ principal group_id.

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
        .expect("failed to collect response body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response body is not JSON")
}

fn patch_file_req(
    token: Option<&str>,
    path_group_id: &str,
    file_id: &str,
    x_group_id: Option<&str>,
    body: serde_json::Value,
) -> Request<Body> {
    let mut req = Request::builder()
        .method("PATCH")
        .uri(format!("/v1/groups/{path_group_id}/files/{file_id}"))
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

/// Insert a live `files` row directly via the admin pool. Used to seed
/// fixtures for PATCH scenarios. Returns the file id.
///
/// Mirrors the schema in migration 003 — every NOT NULL column is supplied.
async fn seed_file(
    h: &Harness,
    group_id: Uuid,
    created_by: Uuid,
    name: &str,
) -> anyhow::Result<Uuid> {
    let file_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO files (id, group_id, folder_id, name, current_version, \
                            total_versions, size_bytes, mime_type, settings, \
                            created_by, created_by_label) \
         VALUES ($1, $2, NULL, $3, 1, 1, 0, $4, '{}'::jsonb, $5, $6)",
    )
    .bind(file_id)
    .bind(group_id)
    .bind(name)
    .bind("application/octet-stream")
    .bind(created_by)
    .bind("Test User")
    .execute(&h.admin_pool)
    .await?;
    Ok(file_id)
}

/// Soft-delete a file directly via the admin pool. Used by F2.
async fn soft_delete_file(h: &Harness, file_id: Uuid) -> anyhow::Result<()> {
    sqlx::query("UPDATE files SET deleted_at = $1 WHERE id = $2")
        .bind(Utc::now())
        .bind(file_id)
        .execute(&h.admin_pool)
        .await?;
    Ok(())
}

#[tokio::test]
async fn v1_files_patch_scenarios() {
    let h = Harness::get().await;

    // Seed owner + group A — used by F1..F6.
    let (owner_id, group_id, owner_token) = seed_user_with_group(&h, "owner@files-slice2.test")
        .await
        .expect("seed owner+group A");

    let group_id_str = group_id.to_string();

    // ── F1. PATCH 200 happy path ────────────────────────────────────
    let live_id = seed_file(&h, group_id, owner_id, "old-name.pdf")
        .await
        .expect("F1 seed file");

    let resp = h
        .router
        .clone()
        .oneshot(patch_file_req(
            Some(&owner_token),
            &group_id_str,
            &live_id.to_string(),
            Some(&group_id_str),
            json!({ "name": "renamed.pdf" }),
        ))
        .await
        .expect("F1 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "F1 status");
    let v = body_json(resp).await;
    assert_eq!(v["id"], live_id.to_string(), "F1 id");
    assert_eq!(v["name"], "renamed.pdf", "F1 name");

    // F1 — verify DB row was actually updated.
    let (db_name,): (String,) = sqlx::query_as("SELECT name FROM files WHERE id = $1")
        .bind(live_id)
        .fetch_one(&h.admin_pool)
        .await
        .expect("F1 fetch db name");
    assert_eq!(db_name, "renamed.pdf", "F1 db name");

    // F1 — audit row asserted (structural metadata only).
    let events = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("F1 fetch audit");
    let rename_event = events
        .iter()
        .find(|(action, _, _, rid, _)| action == "file.renamed" && rid == &live_id.to_string())
        .expect("F1 file.renamed audit row");
    let (_, actor, resource_type, resource_id, metadata) = rename_event;
    assert_eq!(actor.as_ref(), Some(&owner_id), "F1 audit actor");
    assert_eq!(resource_type, "files", "F1 audit resource_type");
    assert_eq!(resource_id, &live_id.to_string(), "F1 audit resource_id");
    // PII safety: only structural fields. `name` MUST NOT appear.
    assert!(
        metadata.get("name").is_none(),
        "F1 audit MUST NOT carry raw file name"
    );
    assert_eq!(
        metadata["name_len"].as_u64(),
        Some("renamed.pdf".chars().count() as u64),
        "F1 audit name_len"
    );
    assert_eq!(metadata["group_id"], group_id_str, "F1 audit group_id");

    // ── F2. PATCH 404 — soft-deleted file ───────────────────────────
    let dead_id = seed_file(&h, group_id, owner_id, "to-be-deleted.txt")
        .await
        .expect("F2 seed file");
    soft_delete_file(&h, dead_id).await.expect("F2 soft-delete");

    let resp = h
        .router
        .clone()
        .oneshot(patch_file_req(
            Some(&owner_token),
            &group_id_str,
            &dead_id.to_string(),
            Some(&group_id_str),
            json!({ "name": "ghost.txt" }),
        ))
        .await
        .expect("F2 oneshot");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "F2 status");

    // F2 — DB confirms name was NOT changed.
    let (after_name,): (String,) = sqlx::query_as("SELECT name FROM files WHERE id = $1")
        .bind(dead_id)
        .fetch_one(&h.admin_pool)
        .await
        .expect("F2 fetch db name");
    assert_eq!(after_name, "to-be-deleted.txt", "F2 name preserved");

    // ── F3. PATCH 404 — non-existent file_id ────────────────────────
    let phantom = Uuid::new_v4();
    let resp = h
        .router
        .clone()
        .oneshot(patch_file_req(
            Some(&owner_token),
            &group_id_str,
            &phantom.to_string(),
            Some(&group_id_str),
            json!({ "name": "ghost.pdf" }),
        ))
        .await
        .expect("F3 oneshot");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "F3 status");

    // ── F4. PATCH 400 — empty name after trim ───────────────────────
    let f4_id = seed_file(&h, group_id, owner_id, "f4.pdf")
        .await
        .expect("F4 seed");
    let resp = h
        .router
        .clone()
        .oneshot(patch_file_req(
            Some(&owner_token),
            &group_id_str,
            &f4_id.to_string(),
            Some(&group_id_str),
            json!({ "name": "   " }),
        ))
        .await
        .expect("F4 oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "F4 status");

    // ── F5. PATCH 400 — name >500 chars ─────────────────────────────
    let f5_id = seed_file(&h, group_id, owner_id, "f5.pdf")
        .await
        .expect("F5 seed");
    let too_long: String = "a".repeat(501);
    let resp = h
        .router
        .clone()
        .oneshot(patch_file_req(
            Some(&owner_token),
            &group_id_str,
            &f5_id.to_string(),
            Some(&group_id_str),
            json!({ "name": too_long }),
        ))
        .await
        .expect("F5 oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "F5 status");

    // ── F6. PATCH 400 — name with '/' ───────────────────────────────
    let f6_id = seed_file(&h, group_id, owner_id, "f6.pdf")
        .await
        .expect("F6 seed");
    let resp = h
        .router
        .clone()
        .oneshot(patch_file_req(
            Some(&owner_token),
            &group_id_str,
            &f6_id.to_string(),
            Some(&group_id_str),
            json!({ "name": "etc/passwd" }),
        ))
        .await
        .expect("F6 oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "F6 status");

    // ── F7. PATCH 403 — path group_id ≠ principal group_id ─────────
    // Owner A's principal carries group_id = A (set via X-Group-Id header
    // matching principal). Path uses some other group_id — handler rejects
    // with 403 via `check_group_match`.
    let other_group = Uuid::new_v4();
    let f7_id = seed_file(&h, group_id, owner_id, "f7.pdf")
        .await
        .expect("F7 seed");
    let resp = h
        .router
        .clone()
        .oneshot(patch_file_req(
            Some(&owner_token),
            &other_group.to_string(),
            &f7_id.to_string(),
            Some(&group_id_str), // X-Group-Id matches principal (A) → no 400
            json!({ "name": "trying-cross-group.pdf" }),
        ))
        .await
        .expect("F7 oneshot");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "F7 status");
}
