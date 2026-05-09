// Gated so `cargo clippy --all-targets` without `test-helpers` skips this
// file and doesn't try to compile the `common` harness.
#![cfg(feature = "test-helpers")]
//! Integration tests for `GET /v1/groups/{group_id}/files/{file_id}`
//! and `GET /v1/groups/{group_id}/folders/{folder_id}`
//! (plan 0090, GAR-559).
//!
//! All scenarios bundled into ONE `#[tokio::test]` function — same pattern
//! as `rest_v1_files_patch.rs`. Splitting historically triggered the sqlx
//! runtime-teardown race documented in plan 0016 M3 commit `4f8be37`.
//!
//! Scenarios covered (7 total):
//!
//! G1. GET 200 — live file happy path: asserts all FileSummary fields.
//! G2. GET 404 — soft-deleted file.
//! G3. GET 404 — non-existent file_id.
//! G4. GET 403 — path group_id ≠ principal group_id.
//! G5. GET 200 — live folder happy path: asserts all FolderSummary fields.
//! G6. GET 404 — soft-deleted folder.
//! G7. GET 404 — non-existent folder_id.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use chrono::Utc;
use http_body_util::BodyExt;
use tower::ServiceExt;
use uuid::Uuid;

use common::Harness;
use common::fixtures::seed_user_with_group;

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("failed to collect response body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response body is not JSON")
}

fn get_req(
    token: Option<&str>,
    path_group_id: &str,
    resource: &str,
    resource_id: &str,
    x_group_id: Option<&str>,
) -> Request<Body> {
    let mut req = Request::builder()
        .method("GET")
        .uri(format!(
            "/v1/groups/{path_group_id}/{resource}/{resource_id}"
        ))
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

/// Insert a live `files` row. Returns the file id.
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
         VALUES ($1, $2, NULL, $3, 1, 1, 1024, $4, '{}'::jsonb, $5, $6)",
    )
    .bind(file_id)
    .bind(group_id)
    .bind(name)
    .bind("application/pdf")
    .bind(created_by)
    .bind("Test User")
    .execute(&h.admin_pool)
    .await?;
    Ok(file_id)
}

/// Soft-delete a file.
async fn soft_delete_file(h: &Harness, file_id: Uuid) -> anyhow::Result<()> {
    sqlx::query("UPDATE files SET deleted_at = $1 WHERE id = $2")
        .bind(Utc::now())
        .bind(file_id)
        .execute(&h.admin_pool)
        .await?;
    Ok(())
}

/// Insert a live `folders` row. Returns the folder id.
async fn seed_folder(
    h: &Harness,
    group_id: Uuid,
    created_by: Uuid,
    name: &str,
) -> anyhow::Result<Uuid> {
    let folder_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO folders (id, group_id, parent_id, name, created_by, created_by_label) \
         VALUES ($1, $2, NULL, $3, $4, $5)",
    )
    .bind(folder_id)
    .bind(group_id)
    .bind(name)
    .bind(created_by)
    .bind("Test User")
    .execute(&h.admin_pool)
    .await?;
    Ok(folder_id)
}

/// Soft-delete a folder.
async fn soft_delete_folder(h: &Harness, folder_id: Uuid) -> anyhow::Result<()> {
    sqlx::query("UPDATE folders SET deleted_at = $1 WHERE id = $2")
        .bind(Utc::now())
        .bind(folder_id)
        .execute(&h.admin_pool)
        .await?;
    Ok(())
}

#[tokio::test]
async fn v1_files_get_single_scenarios() {
    let h = Harness::get().await;

    let (owner_id, group_id, owner_token) = seed_user_with_group(&h, "owner@files-slice3.test")
        .await
        .expect("seed owner+group");

    let group_id_str = group_id.to_string();

    // ── G1. GET 200 — live file happy path ─────────────────────────────
    let g1_id = seed_file(&h, group_id, owner_id, "report.pdf")
        .await
        .expect("G1 seed file");

    let resp = h
        .router
        .clone()
        .oneshot(get_req(
            Some(&owner_token),
            &group_id_str,
            "files",
            &g1_id.to_string(),
            Some(&group_id_str),
        ))
        .await
        .expect("G1 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "G1 status");
    let v = body_json(resp).await;
    assert_eq!(v["id"], g1_id.to_string(), "G1 id");
    assert_eq!(v["name"], "report.pdf", "G1 name");
    assert_eq!(v["mime_type"], "application/pdf", "G1 mime_type");
    assert_eq!(v["size_bytes"], 1024, "G1 size_bytes");
    assert_eq!(v["current_version"], 1, "G1 current_version");
    assert!(v["folder_id"].is_null(), "G1 folder_id null");

    // ── G2. GET 404 — soft-deleted file ────────────────────────────────
    let g2_id = seed_file(&h, group_id, owner_id, "deleted.pdf")
        .await
        .expect("G2 seed");
    soft_delete_file(&h, g2_id).await.expect("G2 soft-delete");

    let resp = h
        .router
        .clone()
        .oneshot(get_req(
            Some(&owner_token),
            &group_id_str,
            "files",
            &g2_id.to_string(),
            Some(&group_id_str),
        ))
        .await
        .expect("G2 oneshot");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "G2 status");

    // ── G3. GET 404 — non-existent file_id ─────────────────────────────
    let phantom = Uuid::new_v4();
    let resp = h
        .router
        .clone()
        .oneshot(get_req(
            Some(&owner_token),
            &group_id_str,
            "files",
            &phantom.to_string(),
            Some(&group_id_str),
        ))
        .await
        .expect("G3 oneshot");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "G3 status");

    // ── G4. GET 403 — path group_id ≠ principal group_id ───────────────
    let other_group = Uuid::new_v4();
    let resp = h
        .router
        .clone()
        .oneshot(get_req(
            Some(&owner_token),
            &other_group.to_string(),
            "files",
            &g1_id.to_string(),
            Some(&group_id_str),
        ))
        .await
        .expect("G4 oneshot");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "G4 status");

    // ── G5. GET 200 — live folder happy path ───────────────────────────
    let g5_id = seed_folder(&h, group_id, owner_id, "Documents")
        .await
        .expect("G5 seed folder");

    let resp = h
        .router
        .clone()
        .oneshot(get_req(
            Some(&owner_token),
            &group_id_str,
            "folders",
            &g5_id.to_string(),
            Some(&group_id_str),
        ))
        .await
        .expect("G5 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "G5 status");
    let v = body_json(resp).await;
    assert_eq!(v["id"], g5_id.to_string(), "G5 id");
    assert_eq!(v["name"], "Documents", "G5 name");
    assert!(v["parent_id"].is_null(), "G5 parent_id null");

    // ── G6. GET 404 — soft-deleted folder ──────────────────────────────
    let g6_id = seed_folder(&h, group_id, owner_id, "Archived")
        .await
        .expect("G6 seed");
    soft_delete_folder(&h, g6_id).await.expect("G6 soft-delete");

    let resp = h
        .router
        .clone()
        .oneshot(get_req(
            Some(&owner_token),
            &group_id_str,
            "folders",
            &g6_id.to_string(),
            Some(&group_id_str),
        ))
        .await
        .expect("G6 oneshot");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "G6 status");

    // ── G7. GET 404 — non-existent folder_id ───────────────────────────
    let phantom_folder = Uuid::new_v4();
    let resp = h
        .router
        .clone()
        .oneshot(get_req(
            Some(&owner_token),
            &group_id_str,
            "folders",
            &phantom_folder.to_string(),
            Some(&group_id_str),
        ))
        .await
        .expect("G7 oneshot");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "G7 status");
}
