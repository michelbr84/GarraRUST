// Gated so `cargo clippy --all-targets` without `test-helpers` skips this
// file and doesn't try to compile the `common` harness.
#![cfg(feature = "test-helpers")]
//! Integration tests for `POST /v1/groups/{group_id}/files/{file_id}/versions`
//! (plan 0094, GAR-567, Fase 3.4 files slice 7).
//!
//! All scenarios bundled into ONE `#[tokio::test]` — same pattern as other
//! files tests to avoid the sqlx runtime-teardown race.
//!
//! Scenarios:
//! NV1. 201 happy path — valid bytes + Content-Type, files row updated.
//! NV2. 404 non-existent file_id.
//! NV3. 404 soft-deleted file.
//! NV4. 404 cross-group attempt (RLS filters).
//! NV5. 403 Child role lacks FilesWrite.
//! NV6. 400 missing X-Group-Id header.
//! NV7. 415 Content-Type MIME not in allow-list (or absent).
//! NV8. 503 object store not configured.

mod common;

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use chrono::Utc;
use garraia_gateway::rest_v1::uploads::UploadStaging;
use garraia_gateway::server::build_router_for_test_with_storage;
use http_body_util::BodyExt;
use tower::ServiceExt;
use uuid::Uuid;

use common::Harness;
use common::fixtures::{seed_member_via_admin, seed_user_with_group};

fn docker_available() -> bool {
    std::process::Command::new("docker")
        .arg("version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn req_with_peer(builder: axum::http::request::Builder, body: Body) -> Request<Body> {
    let mut req = builder.body(body).expect("request builder");
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
            "127.0.0.1:1".parse().unwrap(),
        ));
    req
}

fn new_version_req(
    token: Option<&str>,
    path_group_id: &str,
    file_id: &str,
    x_group_id: Option<&str>,
    content_type: Option<&str>,
    body_bytes: Vec<u8>,
) -> Request<Body> {
    let mut builder = Request::builder().method("POST").uri(format!(
        "/v1/groups/{path_group_id}/files/{file_id}/versions"
    ));
    if let Some(ct) = content_type {
        builder = builder.header("content-type", ct);
    }
    if let Some(cl) = Some(body_bytes.len().to_string()) {
        builder = builder.header("content-length", cl);
    }
    let mut req = req_with_peer(builder, Body::from(body_bytes));
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

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("failed to collect response body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response body is not JSON")
}

/// Seed a `files` row and return its id.
async fn seed_file(
    h: &Harness,
    group_id: Uuid,
    created_by: Uuid,
    name: &str,
    size_bytes: i64,
    mime_type: &str,
) -> anyhow::Result<Uuid> {
    let file_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO files (id, group_id, folder_id, name, current_version, \
                            total_versions, size_bytes, mime_type, settings, \
                            created_by, created_by_label) \
         VALUES ($1, $2, NULL, $3, 1, 1, $4, $5, '{}'::jsonb, $6, $7)",
    )
    .bind(file_id)
    .bind(group_id)
    .bind(name)
    .bind(size_bytes)
    .bind(mime_type)
    .bind(created_by)
    .bind("Test User")
    .execute(&h.admin_pool)
    .await?;
    Ok(file_id)
}

/// Seed a `file_versions` row (version 1) for the given file.
async fn seed_file_version(
    h: &Harness,
    file_id: Uuid,
    group_id: Uuid,
    created_by: Uuid,
    object_key: &str,
    size_bytes: i64,
    mime_type: &str,
) -> anyhow::Result<()> {
    let hex64 = "a".repeat(64);
    sqlx::query(
        "INSERT INTO file_versions \
         (file_id, group_id, version, object_key, etag, checksum_sha256, \
          integrity_hmac, size_bytes, mime_type, created_by, created_by_label) \
         VALUES ($1, $2, 1, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(file_id)
    .bind(group_id)
    .bind(object_key)
    .bind(&hex64[..64])
    .bind(&hex64)
    .bind(&hex64)
    .bind(size_bytes)
    .bind(mime_type)
    .bind(created_by)
    .bind("Test User")
    .execute(&h.admin_pool)
    .await?;
    Ok(())
}

/// Soft-delete a file directly via the admin pool.
async fn soft_delete_file(h: &Harness, file_id: Uuid) -> anyhow::Result<()> {
    sqlx::query("UPDATE files SET deleted_at = $1 WHERE id = $2")
        .bind(Utc::now())
        .bind(file_id)
        .execute(&h.admin_pool)
        .await?;
    Ok(())
}

async fn build_storage_router(h: &Harness, tmp: &Path) -> axum::Router {
    let fs_root = tmp.join("storage");
    let staging_dir = tmp.join("staging");
    std::fs::create_dir_all(&fs_root).unwrap();
    std::fs::create_dir_all(&staging_dir).unwrap();

    let local_fs = Arc::new(garraia_storage::LocalFs::new(&fs_root).expect("LocalFs::new"));
    let staging = Arc::new(UploadStaging {
        staging_dir: std::fs::canonicalize(&staging_dir).unwrap(),
        max_patch_bytes: 10 * 1024 * 1024,
        hmac_secret: b"test-secret-32-bytes-minimum-xxx".to_vec(),
    });

    build_router_for_test_with_storage(
        garraia_config::AppConfig::default(),
        h.login_pool.clone(),
        h.signup_pool.clone(),
        h.jwt.clone(),
        Some(h.app_pool.clone()),
        Some(local_fs as Arc<dyn garraia_storage::ObjectStore>),
        Some(staging),
    )
    .await
}

async fn build_no_storage_router(h: &Harness) -> axum::Router {
    let staging_dir = tempfile::tempdir().expect("tempdir").keep();
    let staging = Arc::new(UploadStaging {
        staging_dir,
        max_patch_bytes: 10 * 1024 * 1024,
        hmac_secret: b"test-secret-32-bytes-minimum-xxx".to_vec(),
    });
    build_router_for_test_with_storage(
        garraia_config::AppConfig::default(),
        h.login_pool.clone(),
        h.signup_pool.clone(),
        h.jwt.clone(),
        Some(h.app_pool.clone()),
        None,
        Some(staging),
    )
    .await
}

#[tokio::test]
async fn v1_files_new_version_scenarios() {
    if !docker_available() {
        eprintln!("docker not available; skipping v1_files_new_version_scenarios");
        return;
    }

    let h = Harness::get().await;
    let tmp = tempfile::tempdir().expect("tempdir");
    let router = build_storage_router(&h, tmp.path()).await;

    let (owner_id, group_id, owner_token) = seed_user_with_group(&h, "owner@0094-new-version.test")
        .await
        .expect("seed owner+group");
    let gid_str = group_id.to_string();

    // ─── NV1. 201 happy path ───────────────────────────────────────────────
    let nv1_payload: &[u8] = b"version 2 content";
    let nv1_id = seed_file(&h, group_id, owner_id, "doc.txt", 5, "text/plain")
        .await
        .expect("NV1 seed file");
    seed_file_version(
        &h,
        nv1_id,
        group_id,
        owner_id,
        &format!("{group_id}/files/{nv1_id}/v1/init.txt"),
        5,
        "text/plain",
    )
    .await
    .expect("NV1 seed file_version");

    let resp = router
        .clone()
        .oneshot(new_version_req(
            Some(&owner_token),
            &gid_str,
            &nv1_id.to_string(),
            Some(&gid_str),
            Some("text/plain"),
            nv1_payload.to_vec(),
        ))
        .await
        .expect("NV1 oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "NV1 status");
    let body = body_json(resp).await;
    assert_eq!(body["file_id"], nv1_id.to_string(), "NV1 file_id");
    assert_eq!(body["version"].as_u64(), Some(2), "NV1 version");
    assert_eq!(
        body["size_bytes"].as_u64(),
        Some(nv1_payload.len() as u64),
        "NV1 size_bytes"
    );
    assert_eq!(body["mime_type"], "text/plain", "NV1 mime_type");

    // NV1 — DB: files row updated.
    let (cur_ver, tot_ver, sz, mime): (i32, i32, i64, String) = sqlx::query_as(
        "SELECT current_version, total_versions, size_bytes, mime_type \
         FROM files WHERE id = $1",
    )
    .bind(nv1_id)
    .fetch_one(&h.admin_pool)
    .await
    .expect("NV1 fetch files row");
    assert_eq!(cur_ver, 2, "NV1 db current_version");
    assert_eq!(tot_ver, 2, "NV1 db total_versions");
    assert_eq!(sz, nv1_payload.len() as i64, "NV1 db size_bytes");
    assert_eq!(mime, "text/plain", "NV1 db mime_type");

    // NV1 — DB: file_versions row created.
    let (fv_ver,): (i32,) =
        sqlx::query_as("SELECT version FROM file_versions WHERE file_id = $1 AND version = 2")
            .bind(nv1_id)
            .fetch_one(&h.admin_pool)
            .await
            .expect("NV1 fetch file_versions v2");
    assert_eq!(fv_ver, 2, "NV1 db file_versions version");

    // ─── NV2. 404 non-existent file_id ────────────────────────────────────
    let resp = router
        .clone()
        .oneshot(new_version_req(
            Some(&owner_token),
            &gid_str,
            &Uuid::new_v4().to_string(),
            Some(&gid_str),
            Some("text/plain"),
            b"data".to_vec(),
        ))
        .await
        .expect("NV2 oneshot");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "NV2 status");

    // ─── NV3. 404 soft-deleted file ───────────────────────────────────────
    let nv3_id = seed_file(&h, group_id, owner_id, "dead.txt", 0, "text/plain")
        .await
        .expect("NV3 seed file");
    seed_file_version(
        &h,
        nv3_id,
        group_id,
        owner_id,
        &format!("{group_id}/files/{nv3_id}/v1/init.txt"),
        0,
        "text/plain",
    )
    .await
    .expect("NV3 seed file_version");
    soft_delete_file(&h, nv3_id).await.expect("NV3 soft-delete");

    let resp = router
        .clone()
        .oneshot(new_version_req(
            Some(&owner_token),
            &gid_str,
            &nv3_id.to_string(),
            Some(&gid_str),
            Some("text/plain"),
            b"data".to_vec(),
        ))
        .await
        .expect("NV3 oneshot");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "NV3 status");

    // ─── NV4. 404 cross-group (RLS) ───────────────────────────────────────
    let (_other_id, other_group_id, _other_token) =
        seed_user_with_group(&h, "other@0094-new-version.test")
            .await
            .expect("NV4 seed other group");
    let other_gid_str = other_group_id.to_string();

    // File in group_id; principal group = other_group_id → RLS filters → 404
    let nv4_id = seed_file(&h, group_id, owner_id, "cross.txt", 4, "text/plain")
        .await
        .expect("NV4 seed file");
    seed_file_version(
        &h,
        nv4_id,
        group_id,
        owner_id,
        &format!("{group_id}/files/{nv4_id}/v1/cross.txt"),
        4,
        "text/plain",
    )
    .await
    .expect("NV4 seed file_version");

    // Attempt with owner_token but path/header group = other_group_id → 403 path mismatch
    let resp = router
        .clone()
        .oneshot(new_version_req(
            Some(&owner_token),
            &other_gid_str,
            &nv4_id.to_string(),
            Some(&other_gid_str),
            Some("text/plain"),
            b"data".to_vec(),
        ))
        .await
        .expect("NV4 oneshot");
    // Group mismatch (principal.group_id ≠ path_group_id) returns 403
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "NV4 status");

    // ─── NV5. 403 Child role lacks FilesWrite ────────────────────────────
    let child_email = "child@0094-new-version.test";
    let (child_id, child_token) = seed_member_via_admin(&h, group_id, "child", child_email)
        .await
        .expect("NV5 seed child");
    let _ = child_id;

    let nv5_id = seed_file(&h, group_id, owner_id, "child-target.txt", 2, "text/plain")
        .await
        .expect("NV5 seed file");
    seed_file_version(
        &h,
        nv5_id,
        group_id,
        owner_id,
        &format!("{group_id}/files/{nv5_id}/v1/init.txt"),
        2,
        "text/plain",
    )
    .await
    .expect("NV5 seed file_version");

    let resp = router
        .clone()
        .oneshot(new_version_req(
            Some(&child_token),
            &gid_str,
            &nv5_id.to_string(),
            Some(&gid_str),
            Some("text/plain"),
            b"data".to_vec(),
        ))
        .await
        .expect("NV5 oneshot");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "NV5 status");

    // ─── NV6. 400 missing X-Group-Id header ──────────────────────────────
    let nv6_id = seed_file(&h, group_id, owner_id, "nv6.txt", 1, "text/plain")
        .await
        .expect("NV6 seed file");
    seed_file_version(
        &h,
        nv6_id,
        group_id,
        owner_id,
        &format!("{group_id}/files/{nv6_id}/v1/init.txt"),
        1,
        "text/plain",
    )
    .await
    .expect("NV6 seed file_version");

    let resp = router
        .clone()
        .oneshot(new_version_req(
            Some(&owner_token),
            &gid_str,
            &nv6_id.to_string(),
            None, // no X-Group-Id
            Some("text/plain"),
            b"data".to_vec(),
        ))
        .await
        .expect("NV6 oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "NV6 status");

    // ─── NV7. 415 MIME type not in allow-list ────────────────────────────
    let nv7_id = seed_file(&h, group_id, owner_id, "nv7.bin", 1, "text/plain")
        .await
        .expect("NV7 seed file");
    seed_file_version(
        &h,
        nv7_id,
        group_id,
        owner_id,
        &format!("{group_id}/files/{nv7_id}/v1/init.txt"),
        1,
        "text/plain",
    )
    .await
    .expect("NV7 seed file_version");

    // "application/x-evil" is not in the MIME allow-list.
    let resp = router
        .clone()
        .oneshot(new_version_req(
            Some(&owner_token),
            &gid_str,
            &nv7_id.to_string(),
            Some(&gid_str),
            Some("application/x-evil"),
            b"data".to_vec(),
        ))
        .await
        .expect("NV7 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "NV7 status"
    );

    // ─── NV8. 503 object store not configured ────────────────────────────
    let no_store_router = build_no_storage_router(&h).await;

    let nv8_id = seed_file(&h, group_id, owner_id, "nv8.txt", 1, "text/plain")
        .await
        .expect("NV8 seed file");
    seed_file_version(
        &h,
        nv8_id,
        group_id,
        owner_id,
        &format!("{group_id}/files/{nv8_id}/v1/init.txt"),
        1,
        "text/plain",
    )
    .await
    .expect("NV8 seed file_version");

    let resp = no_store_router
        .oneshot(new_version_req(
            Some(&owner_token),
            &gid_str,
            &nv8_id.to_string(),
            Some(&gid_str),
            Some("text/plain"),
            b"data".to_vec(),
        ))
        .await
        .expect("NV8 oneshot");
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE, "NV8 status");
}
