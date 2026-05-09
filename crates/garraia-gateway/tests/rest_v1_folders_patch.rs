// Gated so `cargo clippy --all-targets` without `test-helpers` skips this
// file and doesn't try to compile the `common` harness (which references
// `JwtIssuer::new_for_test` and `issue_access_for_test`, both gated by
// `garraia-auth/test-support`). Same pattern as `rest_v1_files_get_single.rs`.
#![cfg(feature = "test-helpers")]
//! Integration tests for `PATCH /v1/groups/{group_id}/folders/{folder_id}`
//! (plan 0091, GAR-561, Fase 3.4 files slice 4).
//!
//! All scenarios bundled into ONE `#[tokio::test]` function — same pattern
//! as `rest_v1_files_patch.rs`/`rest_v1_chats.rs`/`rest_v1_groups.rs`.
//! Splitting into multiple `#[tokio::test]`s historically triggered the
//! sqlx runtime-teardown race documented in plan 0016 M3 commit `4f8be37`.
//!
//! Scenarios covered (8 total):
//!
//! F1. PATCH 200 — owner happy-path rename: asserts response shape, DB
//!     `folders.name` updated, `audit_events` row with action=`folder.renamed`
//!     and PII-safe metadata (`folder_id`, `group_id`, `name_len` only —
//!     never `name`).
//! F2. PATCH 404 — soft-deleted folder (`deleted_at IS NOT NULL`).
//! F3. PATCH 404 — non-existent folder_id.
//! F4. PATCH 400 — empty name (after trim).
//! F5. PATCH 400 — name exceeding 200 chars (folders boundary, not 500).
//! F6. PATCH 400 — name containing `/`.
//! F7. PATCH 403 — path group_id ≠ principal group_id.
//! F8. PATCH 409 — name collides with sibling under same parent
//!     (`folders_unique_name_per_parent_idx` UNIQUE; new for slice 4 —
//!     plan 0089 file rename did not need this branch).

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

fn patch_folder_req(
    token: Option<&str>,
    path_group_id: &str,
    folder_id: &str,
    x_group_id: Option<&str>,
    body: serde_json::Value,
) -> Request<Body> {
    let mut req = Request::builder()
        .method("PATCH")
        .uri(format!("/v1/groups/{path_group_id}/folders/{folder_id}"))
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

/// Insert one live `folders` row directly via the admin pool. Mirrors the
/// schema in migration 003 — every NOT NULL column is supplied. `parent_id`
/// is optional (`None` → root-level folder).
async fn seed_folder(
    h: &Harness,
    group_id: Uuid,
    parent_id: Option<Uuid>,
    created_by: Uuid,
    name: &str,
) -> anyhow::Result<Uuid> {
    let folder_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO folders (id, group_id, parent_id, name, \
                              created_by, created_by_label) \
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

/// Soft-delete a folder directly via the admin pool. Used by F2.
async fn soft_delete_folder(h: &Harness, folder_id: Uuid) -> anyhow::Result<()> {
    sqlx::query("UPDATE folders SET deleted_at = $1 WHERE id = $2")
        .bind(Utc::now())
        .bind(folder_id)
        .execute(&h.admin_pool)
        .await?;
    Ok(())
}

#[tokio::test]
async fn v1_folders_patch_scenarios() {
    let h = Harness::get().await;

    // Seed owner + group A — used by all F1..F8.
    let (owner_id, group_id, owner_token) = seed_user_with_group(&h, "owner@folders-slice4.test")
        .await
        .expect("seed owner+group A");

    let group_id_str = group_id.to_string();

    // ── F1. PATCH 200 happy path ────────────────────────────────────
    let live_id = seed_folder(&h, group_id, None, owner_id, "old-name")
        .await
        .expect("F1 seed folder");

    let resp = h
        .router
        .clone()
        .oneshot(patch_folder_req(
            Some(&owner_token),
            &group_id_str,
            &live_id.to_string(),
            Some(&group_id_str),
            json!({ "name": "renamed-folder" }),
        ))
        .await
        .expect("F1 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "F1 status");
    let v = body_json(resp).await;
    assert_eq!(v["id"], live_id.to_string(), "F1 id");
    assert_eq!(v["name"], "renamed-folder", "F1 name");
    assert!(v["parent_id"].is_null(), "F1 parent_id null for root");

    // F1 — verify DB row was actually updated.
    let (db_name,): (String,) = sqlx::query_as("SELECT name FROM folders WHERE id = $1")
        .bind(live_id)
        .fetch_one(&h.admin_pool)
        .await
        .expect("F1 fetch db name");
    assert_eq!(db_name, "renamed-folder", "F1 db name");

    // F1 — audit row asserted (PII-safe metadata only).
    let events = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("F1 fetch audit");
    let rename_event = events
        .iter()
        .find(|(action, _, _, rid, _)| action == "folder.renamed" && rid == &live_id.to_string())
        .expect("F1 folder.renamed audit row");
    let (_, actor, resource_type, resource_id, metadata) = rename_event;
    assert_eq!(actor.as_ref(), Some(&owner_id), "F1 audit actor");
    assert_eq!(resource_type, "folders", "F1 audit resource_type");
    assert_eq!(resource_id, &live_id.to_string(), "F1 audit resource_id");
    // PII safety: only structural fields. `name` MUST NOT appear.
    assert!(
        metadata.get("name").is_none(),
        "F1 audit MUST NOT carry raw folder name"
    );
    assert_eq!(
        metadata["name_len"].as_u64(),
        Some("renamed-folder".chars().count() as u64),
        "F1 audit name_len"
    );
    assert_eq!(metadata["group_id"], group_id_str, "F1 audit group_id");
    assert_eq!(
        metadata["folder_id"],
        live_id.to_string(),
        "F1 audit folder_id"
    );

    // ── F2. PATCH 404 — soft-deleted folder ─────────────────────────
    let dead_id = seed_folder(&h, group_id, None, owner_id, "to-be-deleted")
        .await
        .expect("F2 seed folder");
    soft_delete_folder(&h, dead_id)
        .await
        .expect("F2 soft-delete");

    let resp = h
        .router
        .clone()
        .oneshot(patch_folder_req(
            Some(&owner_token),
            &group_id_str,
            &dead_id.to_string(),
            Some(&group_id_str),
            json!({ "name": "ghost-folder" }),
        ))
        .await
        .expect("F2 oneshot");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "F2 status");

    // F2 — DB confirms name was NOT changed.
    let (after_name,): (String,) = sqlx::query_as("SELECT name FROM folders WHERE id = $1")
        .bind(dead_id)
        .fetch_one(&h.admin_pool)
        .await
        .expect("F2 fetch db name");
    assert_eq!(after_name, "to-be-deleted", "F2 name preserved");

    // ── F3. PATCH 404 — non-existent folder_id ──────────────────────
    let phantom = Uuid::new_v4();
    let resp = h
        .router
        .clone()
        .oneshot(patch_folder_req(
            Some(&owner_token),
            &group_id_str,
            &phantom.to_string(),
            Some(&group_id_str),
            json!({ "name": "ghost" }),
        ))
        .await
        .expect("F3 oneshot");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "F3 status");

    // ── F4. PATCH 400 — empty name after trim ───────────────────────
    let f4_id = seed_folder(&h, group_id, None, owner_id, "f4")
        .await
        .expect("F4 seed");
    let resp = h
        .router
        .clone()
        .oneshot(patch_folder_req(
            Some(&owner_token),
            &group_id_str,
            &f4_id.to_string(),
            Some(&group_id_str),
            json!({ "name": "   " }),
        ))
        .await
        .expect("F4 oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "F4 status");

    // ── F5. PATCH 400 — name >200 chars ─────────────────────────────
    let f5_id = seed_folder(&h, group_id, None, owner_id, "f5")
        .await
        .expect("F5 seed");
    let too_long: String = "a".repeat(201);
    let resp = h
        .router
        .clone()
        .oneshot(patch_folder_req(
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
    let f6_id = seed_folder(&h, group_id, None, owner_id, "f6")
        .await
        .expect("F6 seed");
    let resp = h
        .router
        .clone()
        .oneshot(patch_folder_req(
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
    // X-Group-Id matches principal (A) so we get past require_group_id
    // and check_group_match catches the path mismatch.
    let other_group = Uuid::new_v4();
    let f7_id = seed_folder(&h, group_id, None, owner_id, "f7")
        .await
        .expect("F7 seed");
    let resp = h
        .router
        .clone()
        .oneshot(patch_folder_req(
            Some(&owner_token),
            &other_group.to_string(),
            &f7_id.to_string(),
            Some(&group_id_str),
            json!({ "name": "trying-cross-group" }),
        ))
        .await
        .expect("F7 oneshot");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "F7 status");

    // ── F8. PATCH 409 — sibling unique-name collision ───────────────
    // Seed two folders under the SAME parent (root). Try to rename
    // f8_target to "f8-sibling" — the index
    // `folders_unique_name_per_parent_idx UNIQUE (group_id,
    // COALESCE(parent_id, nil_uuid), name) WHERE deleted_at IS NULL`
    // raises 23505, which the handler maps to 409 Conflict.
    let _f8_sibling = seed_folder(&h, group_id, None, owner_id, "f8-sibling")
        .await
        .expect("F8 seed sibling");
    let f8_target = seed_folder(&h, group_id, None, owner_id, "f8-target")
        .await
        .expect("F8 seed target");
    let resp = h
        .router
        .clone()
        .oneshot(patch_folder_req(
            Some(&owner_token),
            &group_id_str,
            &f8_target.to_string(),
            Some(&group_id_str),
            json!({ "name": "f8-sibling" }),
        ))
        .await
        .expect("F8 oneshot");
    assert_eq!(resp.status(), StatusCode::CONFLICT, "F8 status");

    // F8 — DB confirms target name was NOT changed (rolled back).
    let (after_name,): (String,) = sqlx::query_as("SELECT name FROM folders WHERE id = $1")
        .bind(f8_target)
        .fetch_one(&h.admin_pool)
        .await
        .expect("F8 fetch db name");
    assert_eq!(after_name, "f8-target", "F8 name preserved");

    // F8 — body MUST NOT echo the conflicting name (PII safety).
    // (Handler returns a static string; this is a guardrail in case
    // someone later edits the message to include the user-controlled name.)
    let v = body_json(resp).await;
    let detail = v["detail"].as_str().unwrap_or_default();
    assert!(
        !detail.contains("f8-sibling"),
        "F8 conflict detail MUST NOT echo the conflicting name: {detail}"
    );
}
