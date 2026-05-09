// Gated so `cargo clippy --all-targets` without `test-helpers` skips this
// file and doesn't try to compile the `common` harness (which references
// `JwtIssuer::new_for_test` and `issue_access_for_test`, both gated by
// `garraia-auth/test-support`). Same pattern as `rest_v1_folders_patch.rs`.
#![cfg(feature = "test-helpers")]
//! Integration tests for:
//!   `POST   /v1/groups/{group_id}/folders`              (plan 0092, GAR-562)
//!   `DELETE /v1/groups/{group_id}/folders/{folder_id}`  (plan 0092, GAR-562)
//!
//! All scenarios bundled into ONE `#[tokio::test]` function — same pattern
//! as `rest_v1_folders_patch.rs`/`rest_v1_files_patch.rs`. Splitting into
//! multiple `#[tokio::test]`s historically triggered the sqlx
//! runtime-teardown race documented in plan 0016 M3 commit `4f8be37`.
//!
//! POST scenarios (C1–C7):
//! C1. POST 201 — root folder (no parent_id): asserts response shape, DB row,
//!     `audit_events` row with `folder.created` + PII-safe metadata.
//! C2. POST 201 — child folder (valid parent_id): asserts `parent_id` in body.
//! C3. POST 400 — empty name.
//! C4. POST 400 — name >200 chars.
//! C5. POST 400 — name with `/`.
//! C6. POST 400 — unknown parent_id (not in group).
//! C7. POST 403 — path group_id ≠ principal group_id.
//! C8. POST 409 — sibling name collision under same parent.
//!
//! DELETE scenarios (D1–D6):
//! D1. DELETE 204 — live folder: DB shows deleted_at set, audit row emitted.
//! D2. DELETE 204 — already soft-deleted (idempotent, NO audit re-emitted).
//! D3. DELETE 404 — non-existent folder_id.
//! D4. DELETE 404 — cross-group folder_id.
//! D5. DELETE 403 — path group_id ≠ principal group_id.
//! D6. DELETE 403 — Member role (has FilesWrite, lacks FilesDelete): proves
//!     the canonical authz delta vs PR #248 — Members can rename/create
//!     folders but cannot delete them. Uses `seed_member_via_admin`.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use chrono::Utc;
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

fn post_folder_req(
    token: Option<&str>,
    path_group_id: &str,
    x_group_id: Option<&str>,
    body: serde_json::Value,
) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri(format!("/v1/groups/{path_group_id}/folders"))
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

fn delete_folder_req(
    token: Option<&str>,
    path_group_id: &str,
    folder_id: &str,
    x_group_id: Option<&str>,
) -> Request<Body> {
    let mut req = Request::builder()
        .method("DELETE")
        .uri(format!("/v1/groups/{path_group_id}/folders/{folder_id}"))
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

/// Insert a live `folders` row directly via the admin pool (bypassing RLS).
/// Every NOT NULL column is supplied; `parent_id` is optional.
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

    // Seed owner + group A — used by C1..C8 + D1..D5.
    let (owner_id, group_id, owner_token) = seed_user_with_group(&h, "owner@folders-slice5.test")
        .await
        .expect("seed owner+group A");

    let group_id_str = group_id.to_string();

    // ── C1. POST 201 — root folder (no parent_id) ───────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(post_folder_req(
            Some(&owner_token),
            &group_id_str,
            Some(&group_id_str),
            json!({ "name": "my-root-folder" }),
        ))
        .await
        .expect("C1 oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "C1 status");
    let v = body_json(resp).await;
    assert!(v["id"].is_string(), "C1 id is string: {v}");
    assert_eq!(v["name"], "my-root-folder", "C1 name");
    assert!(v["parent_id"].is_null(), "C1 parent_id null");
    let c1_folder_id: Uuid = v["id"].as_str().unwrap().parse().expect("C1 uuid");

    // C1 — verify DB row was actually inserted.
    let (db_name,): (String,) = sqlx::query_as("SELECT name FROM folders WHERE id = $1")
        .bind(c1_folder_id)
        .fetch_one(&h.admin_pool)
        .await
        .expect("C1 fetch db name");
    assert_eq!(db_name, "my-root-folder", "C1 db name");

    // C1 — audit row (PII-safe metadata only).
    let events = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("C1 fetch audit");
    let create_event = events
        .iter()
        .find(|(action, _, _, rid, _)| {
            action == "folder.created" && rid == &c1_folder_id.to_string()
        })
        .expect("C1 folder.created audit row");
    let (_, actor, resource_type, resource_id, metadata) = create_event;
    assert_eq!(actor.as_ref(), Some(&owner_id), "C1 audit actor");
    assert_eq!(resource_type, "folders", "C1 audit resource_type");
    assert_eq!(
        resource_id,
        &c1_folder_id.to_string(),
        "C1 audit resource_id"
    );
    assert!(
        metadata.get("name").is_none(),
        "C1 audit MUST NOT carry raw folder name"
    );
    assert_eq!(
        metadata["name_len"].as_u64(),
        Some("my-root-folder".chars().count() as u64),
        "C1 audit name_len"
    );
    assert_eq!(metadata["group_id"], group_id_str, "C1 audit group_id");
    assert_eq!(
        metadata["has_parent"].as_bool(),
        Some(false),
        "C1 audit has_parent false"
    );

    // ── C2. POST 201 — child folder (valid parent_id) ──────────────────
    let resp = h
        .router
        .clone()
        .oneshot(post_folder_req(
            Some(&owner_token),
            &group_id_str,
            Some(&group_id_str),
            json!({ "name": "child-folder", "parent_id": c1_folder_id }),
        ))
        .await
        .expect("C2 oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "C2 status");
    let v = body_json(resp).await;
    assert_eq!(v["name"], "child-folder", "C2 name");
    assert_eq!(
        v["parent_id"].as_str(),
        Some(c1_folder_id.to_string().as_str()),
        "C2 parent_id"
    );
    let c2_folder_id: Uuid = v["id"].as_str().unwrap().parse().expect("C2 uuid");

    // C2 — audit has_parent = true.
    let events2 = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("C2 fetch audit");
    let c2_event = events2
        .iter()
        .find(|(action, _, _, rid, _)| {
            action == "folder.created" && rid == &c2_folder_id.to_string()
        })
        .expect("C2 folder.created audit row");
    assert_eq!(
        c2_event.4["has_parent"].as_bool(),
        Some(true),
        "C2 audit has_parent true"
    );

    // ── C3. POST 400 — empty name ───────────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(post_folder_req(
            Some(&owner_token),
            &group_id_str,
            Some(&group_id_str),
            json!({ "name": "   " }),
        ))
        .await
        .expect("C3 oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "C3 status");

    // ── C4. POST 400 — name >200 chars ─────────────────────────────────
    let too_long: String = "b".repeat(201);
    let resp = h
        .router
        .clone()
        .oneshot(post_folder_req(
            Some(&owner_token),
            &group_id_str,
            Some(&group_id_str),
            json!({ "name": too_long }),
        ))
        .await
        .expect("C4 oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "C4 status");

    // ── C5. POST 400 — name with '/' ────────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(post_folder_req(
            Some(&owner_token),
            &group_id_str,
            Some(&group_id_str),
            json!({ "name": "etc/passwd" }),
        ))
        .await
        .expect("C5 oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "C5 status");

    // ── C6. POST 400 — unknown parent_id ───────────────────────────────
    let ghost_parent = Uuid::new_v4();
    let resp = h
        .router
        .clone()
        .oneshot(post_folder_req(
            Some(&owner_token),
            &group_id_str,
            Some(&group_id_str),
            json!({ "name": "orphan", "parent_id": ghost_parent }),
        ))
        .await
        .expect("C6 oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "C6 status");

    // ── C7. POST 403 — path group_id ≠ principal group_id ──────────────
    let other_group = Uuid::new_v4();
    let resp = h
        .router
        .clone()
        .oneshot(post_folder_req(
            Some(&owner_token),
            &other_group.to_string(),
            Some(&group_id_str), // X-Group-Id matches principal → no 400
            json!({ "name": "cross-group-attempt" }),
        ))
        .await
        .expect("C7 oneshot");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "C7 status");

    // ── C8. POST 409 — sibling name collision ───────────────────────────
    // Create a root-level folder called "c8-sibling", then try to POST another
    // folder with the same name under the same parent (root).
    let resp = h
        .router
        .clone()
        .oneshot(post_folder_req(
            Some(&owner_token),
            &group_id_str,
            Some(&group_id_str),
            json!({ "name": "c8-collision" }),
        ))
        .await
        .expect("C8 first POST");
    assert_eq!(resp.status(), StatusCode::CREATED, "C8 first create");

    // Second POST with identical name → 409.
    let resp = h
        .router
        .clone()
        .oneshot(post_folder_req(
            Some(&owner_token),
            &group_id_str,
            Some(&group_id_str),
            json!({ "name": "c8-collision" }),
        ))
        .await
        .expect("C8 duplicate POST");
    assert_eq!(resp.status(), StatusCode::CONFLICT, "C8 status");
    // Body MUST NOT echo the user-controlled name (PII safety).
    let v = body_json(resp).await;
    let detail = v["detail"].as_str().unwrap_or_default();
    assert!(
        !detail.contains("c8-collision"),
        "C8 conflict detail MUST NOT echo the name: {detail}"
    );

    // ── D1. DELETE 204 — live folder, audit row emitted ─────────────────
    let d1_id = seed_folder(&h, group_id, None, owner_id, "d1-live")
        .await
        .expect("D1 seed");

    let resp = h
        .router
        .clone()
        .oneshot(delete_folder_req(
            Some(&owner_token),
            &group_id_str,
            &d1_id.to_string(),
            Some(&group_id_str),
        ))
        .await
        .expect("D1 oneshot");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT, "D1 status");

    // D1 — verify DB row has deleted_at set.
    let (deleted_at,): (Option<chrono::DateTime<Utc>>,) =
        sqlx::query_as("SELECT deleted_at FROM folders WHERE id = $1")
            .bind(d1_id)
            .fetch_one(&h.admin_pool)
            .await
            .expect("D1 fetch deleted_at");
    assert!(deleted_at.is_some(), "D1 deleted_at is set");

    // D1 — audit row emitted.
    let events_d1 = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("D1 fetch audit");
    let delete_event = events_d1
        .iter()
        .find(|(action, _, _, rid, _)| action == "folder.deleted" && rid == &d1_id.to_string())
        .expect("D1 folder.deleted audit row");
    let (_, actor_d1, resource_type_d1, _, metadata_d1) = delete_event;
    assert_eq!(actor_d1.as_ref(), Some(&owner_id), "D1 audit actor");
    assert_eq!(resource_type_d1, "folders", "D1 audit resource_type");
    assert!(
        metadata_d1.get("name").is_none(),
        "D1 audit MUST NOT carry raw folder name"
    );
    assert_eq!(
        metadata_d1["name_len"].as_u64(),
        Some("d1-live".chars().count() as u64),
        "D1 audit name_len"
    );

    // ── D2. DELETE 204 — already soft-deleted (idempotent, no new audit) ─
    let d2_id = seed_folder(&h, group_id, None, owner_id, "d2-predead")
        .await
        .expect("D2 seed");
    soft_delete_folder(&h, d2_id).await.expect("D2 pre-delete");

    let events_before = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("D2 events before");
    let audit_count_before = events_before
        .iter()
        .filter(|(a, _, _, rid, _)| a == "folder.deleted" && rid == &d2_id.to_string())
        .count();

    let resp = h
        .router
        .clone()
        .oneshot(delete_folder_req(
            Some(&owner_token),
            &group_id_str,
            &d2_id.to_string(),
            Some(&group_id_str),
        ))
        .await
        .expect("D2 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "D2 status (idempotent)"
    );

    // D2 — no NEW audit row was emitted.
    let events_after = fetch_audit_events_for_group(&h, group_id)
        .await
        .expect("D2 events after");
    let audit_count_after = events_after
        .iter()
        .filter(|(a, _, _, rid, _)| a == "folder.deleted" && rid == &d2_id.to_string())
        .count();
    assert_eq!(
        audit_count_before, audit_count_after,
        "D2 no new audit row on idempotent delete"
    );

    // ── D3. DELETE 404 — non-existent folder_id ─────────────────────────
    let phantom = Uuid::new_v4();
    let resp = h
        .router
        .clone()
        .oneshot(delete_folder_req(
            Some(&owner_token),
            &group_id_str,
            &phantom.to_string(),
            Some(&group_id_str),
        ))
        .await
        .expect("D3 oneshot");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "D3 status");

    // ── D4. DELETE 404 — cross-group folder (belongs to group B) ────────
    // Seed a second user+group B. Insert a folder in group B. Principal A
    // cannot see it because the SELECT filters on group_id = A's group_id.
    let (owner_b_id, group_b_id, _owner_b_token) =
        seed_user_with_group(&h, "owner-b@folders-slice5.test")
            .await
            .expect("D4 seed group B");
    let d4_folder = seed_folder(&h, group_b_id, None, owner_b_id, "group-b-folder")
        .await
        .expect("D4 seed folder in group B");

    // Principal A's path uses group A's ID, folder ID belongs to group B.
    let resp = h
        .router
        .clone()
        .oneshot(delete_folder_req(
            Some(&owner_token),
            &group_id_str,          // path = group A
            &d4_folder.to_string(), // folder is in group B
            Some(&group_id_str),
        ))
        .await
        .expect("D4 oneshot");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "D4 status");

    // ── D5. DELETE 403 — path group_id ≠ principal group_id ────────────
    let d5_id = seed_folder(&h, group_id, None, owner_id, "d5")
        .await
        .expect("D5 seed");
    let resp = h
        .router
        .clone()
        .oneshot(delete_folder_req(
            Some(&owner_token),
            &other_group.to_string(), // path = some other group
            &d5_id.to_string(),
            Some(&group_id_str), // X-Group-Id matches principal (A)
        ))
        .await
        .expect("D5 oneshot");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "D5 status");

    // ── D6. DELETE 403 — Member lacks FilesDelete (canonical authz) ─────
    // Member role has FilesWrite (can create/rename folders via POST/PATCH)
    // but NOT FilesDelete (which is Owner/Admin only, per `can()` matrix
    // mirroring migration 002 lines 78-111). This test guards against a
    // regression where folder DELETE would inherit FilesWrite instead of
    // matching the canonical `delete_file` precedent from plan 0088.
    let (_member_id, member_token) =
        seed_member_via_admin(&h, group_id, "member", "member@folders-slice5.test")
            .await
            .expect("D6 seed member");
    let d6_id = seed_folder(&h, group_id, None, owner_id, "d6")
        .await
        .expect("D6 seed folder");
    let resp = h
        .router
        .clone()
        .oneshot(delete_folder_req(
            Some(&member_token),
            &group_id_str, // path matches principal's group
            &d6_id.to_string(),
            Some(&group_id_str),
        ))
        .await
        .expect("D6 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "D6 status — Member must NOT be able to delete folders (FilesDelete is Owner/Admin)"
    );

    // D6 — DB confirms folder was NOT soft-deleted by the rejected request.
    let (deleted_at,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT deleted_at FROM folders WHERE id = $1")
            .bind(d6_id)
            .fetch_one(&h.admin_pool)
            .await
            .expect("D6 fetch deleted_at");
    assert!(
        deleted_at.is_none(),
        "D6 folder MUST remain live after Member's rejected DELETE"
    );
}
