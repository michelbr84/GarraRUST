// Gated so `cargo clippy --all-targets` without `test-helpers` skips this
// file and doesn't try to compile the `common` harness.
#![cfg(feature = "test-helpers")]
//! Integration tests for `GET /v1/files/{file_id}/download` (plan 0093, GAR-564).
//!
//! All scenarios bundled into ONE `#[tokio::test]` — same pattern as other
//! files tests to avoid the sqlx runtime-teardown race.
//!
//! Scenarios:
//! D1. 200 happy path — bytes stored in LocalFs, file_versions row present.
//! D2. 404 soft-deleted file.
//! D3. 404 non-existent file_id.
//! D4. 403 Child role lacks FilesRead.
//! D5. 400 missing X-Group-Id header.
//! D6. 503 object store not configured.
//! D7. 404 cross-group (RLS filters out the file).

mod common;

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use bytes::Bytes;
use chrono::Utc;
use garraia_gateway::rest_v1::uploads::UploadStaging;
use garraia_gateway::server::build_router_for_test_with_storage;
use garraia_storage::{LocalFs, PutOptions};
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

fn download_req(token: &str, file_id: &str, group_id: Option<&str>) -> Request<Body> {
    let mut req = req_with_peer(
        Request::builder()
            .method("GET")
            .uri(format!("/v1/files/{file_id}/download")),
        Body::empty(),
    );
    req.headers_mut().insert(
        HeaderName::from_static("authorization"),
        HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
    );
    if let Some(g) = group_id {
        req.headers_mut().insert(
            HeaderName::from_static("x-group-id"),
            HeaderValue::from_str(g).unwrap(),
        );
    }
    req
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
///
/// Uses placeholder etag / sha256 / hmac values — valid per schema
/// constraints but not representing real content integrity.
async fn seed_file_version(
    h: &Harness,
    file_id: Uuid,
    group_id: Uuid,
    created_by: Uuid,
    object_key: &str,
    size_bytes: i64,
    mime_type: &str,
) -> anyhow::Result<()> {
    let hex64 = "a".repeat(64); // 64 lowercase hex chars — passes CHECK regex
    sqlx::query(
        "INSERT INTO file_versions \
         (file_id, group_id, version, object_key, etag, checksum_sha256, \
          integrity_hmac, size_bytes, mime_type, created_by, created_by_label) \
         VALUES ($1, $2, 1, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(file_id)
    .bind(group_id)
    .bind(object_key)
    .bind("abc123") // short etag — allowed per migration comment
    .bind(&hex64) // checksum_sha256
    .bind(&hex64) // integrity_hmac
    .bind(size_bytes)
    .bind(mime_type)
    .bind(created_by)
    .bind("Test User")
    .execute(&h.admin_pool)
    .await?;
    Ok(())
}

async fn build_storage_router(h: &Harness, tmp: &Path) -> (axum::Router, Arc<LocalFs>) {
    let fs_root = tmp.join("storage");
    let staging_dir = tmp.join("staging");
    std::fs::create_dir_all(&fs_root).unwrap();
    std::fs::create_dir_all(&staging_dir).unwrap();

    let local_fs = Arc::new(LocalFs::new(&fs_root).expect("LocalFs::new"));
    let staging = Arc::new(UploadStaging {
        staging_dir: std::fs::canonicalize(&staging_dir).unwrap(),
        max_patch_bytes: 10 * 1024 * 1024,
        hmac_secret: b"test-secret-32-bytes-minimum-xxx".to_vec(),
    });

    let router = build_router_for_test_with_storage(
        garraia_config::AppConfig::default(),
        h.login_pool.clone(),
        h.signup_pool.clone(),
        h.jwt.clone(),
        Some(h.app_pool.clone()),
        Some(local_fs.clone() as Arc<dyn garraia_storage::ObjectStore>),
        Some(staging.clone()),
    )
    .await;

    (router, local_fs)
}

async fn build_no_storage_router(h: &Harness) -> axum::Router {
    let staging_dir = tempfile::tempdir().expect("tempdir").into_path();
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
async fn v1_files_download_scenarios() {
    if !docker_available() {
        eprintln!("docker not available; skipping v1_files_download_scenarios");
        return;
    }

    let h = Harness::get().await;
    let tmp = tempfile::tempdir().expect("tempdir");
    let (router, local_fs) = build_storage_router(&h, tmp.path()).await;

    let (owner_id, group_id, owner_token) = seed_user_with_group(&h, "owner@0093-download.test")
        .await
        .expect("seed owner+group");
    let gid_str = group_id.to_string();

    // ─── D1. 200 happy path ────────────────────────────────────────────
    let d1_bytes: &[u8] = b"hello download world";
    let d1_key = format!("{group_id}/files/v1/d1.bin");
    local_fs
        .put(&d1_key, Bytes::from_static(d1_bytes), PutOptions::default())
        .await
        .expect("D1 put to LocalFs");

    let d1_id = seed_file(
        &h,
        group_id,
        owner_id,
        "d1.bin",
        d1_bytes.len() as i64,
        "application/octet-stream",
    )
    .await
    .expect("D1 seed file");
    seed_file_version(
        &h,
        d1_id,
        group_id,
        owner_id,
        &d1_key,
        d1_bytes.len() as i64,
        "application/octet-stream",
    )
    .await
    .expect("D1 seed file_version");

    let resp = router
        .clone()
        .oneshot(download_req(
            &owner_token,
            &d1_id.to_string(),
            Some(&gid_str),
        ))
        .await
        .expect("D1 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "D1 status");
    assert_eq!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/octet-stream"),
        "D1 content-type"
    );
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.as_ref(), d1_bytes, "D1 body bytes");

    // ─── D2. 404 soft-deleted file ─────────────────────────────────────
    let d2_id = seed_file(
        &h,
        group_id,
        owner_id,
        "d2.bin",
        10,
        "application/octet-stream",
    )
    .await
    .expect("D2 seed file");
    sqlx::query("UPDATE files SET deleted_at = $1 WHERE id = $2")
        .bind(Utc::now())
        .bind(d2_id)
        .execute(&h.admin_pool)
        .await
        .expect("D2 soft delete");

    let resp = router
        .clone()
        .oneshot(download_req(
            &owner_token,
            &d2_id.to_string(),
            Some(&gid_str),
        ))
        .await
        .expect("D2 oneshot");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "D2 status");

    // ─── D3. 404 non-existent file_id ─────────────────────────────────
    let phantom = Uuid::new_v4();
    let resp = router
        .clone()
        .oneshot(download_req(
            &owner_token,
            &phantom.to_string(),
            Some(&gid_str),
        ))
        .await
        .expect("D3 oneshot");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "D3 status");

    // ─── D4. 403 Child role lacks FilesRead ────────────────────────────
    let (_child_uid, child_token) =
        seed_member_via_admin(&h, group_id, "child", "child@0093-download.test")
            .await
            .expect("D4 seed child");

    let resp = router
        .clone()
        .oneshot(download_req(
            &child_token,
            &d1_id.to_string(),
            Some(&gid_str),
        ))
        .await
        .expect("D4 oneshot");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "D4 status");

    // ─── D5. 400 missing X-Group-Id ───────────────────────────────────
    let resp = router
        .clone()
        .oneshot(download_req(&owner_token, &d1_id.to_string(), None))
        .await
        .expect("D5 oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "D5 status");

    // ─── D6. 503 object store not configured ──────────────────────────
    let d6_key = format!("{group_id}/files/v1/d6.bin");
    let d6_id = seed_file(&h, group_id, owner_id, "d6.bin", 4, "text/plain")
        .await
        .expect("D6 seed file");
    seed_file_version(&h, d6_id, group_id, owner_id, &d6_key, 4, "text/plain")
        .await
        .expect("D6 seed version");

    let no_store_router = build_no_storage_router(&h).await;
    let resp = no_store_router
        .oneshot(download_req(
            &owner_token,
            &d6_id.to_string(),
            Some(&gid_str),
        ))
        .await
        .expect("D6 oneshot");
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE, "D6 status");

    // ─── D7. 404 cross-group (RLS filters out the file) ───────────────
    let (_other_uid, other_group_id, other_token) =
        seed_user_with_group(&h, "other@0093-download.test")
            .await
            .expect("D7 seed other group");

    let resp = router
        .clone()
        .oneshot(download_req(
            &other_token,
            &d1_id.to_string(),
            Some(&other_group_id.to_string()),
        ))
        .await
        .expect("D7 oneshot");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "D7 status");
}
