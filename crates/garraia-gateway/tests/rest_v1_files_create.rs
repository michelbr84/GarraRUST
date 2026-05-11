// Gated so `cargo clippy --all-targets` without `test-helpers` skips this
// file and doesn't try to compile the `common` harness.
#![cfg(feature = "test-helpers")]
//! Integration tests for `POST /v1/groups/{group_id}/files`
//! (plan 0099, GAR-577, Fase 3.4 files slice 9).
//!
//! All scenarios bundled into ONE `#[tokio::test]` — same pattern as other
//! files tests to avoid the sqlx runtime-teardown race.
//!
//! Scenarios:
//! FC1. 201 happy path — file row + file_versions v1 created.
//! FC2. 400 missing X-File-Name header.
//! FC3. 400 invalid X-File-Name (empty string).
//! FC4. 400 bad X-Folder-Id (folder not in this group).
//! FC5. 403 cross-group path_group_id mismatch.
//! FC6. 415 MIME type not in allow-list.
//! FC7. 503 object store not configured.
//! FC8. 403 Child role lacks FilesWrite.
//! FC9. audit event `file.created` present in audit_events table.
//! FC10. 201 happy path with X-Folder-Id (file inside a folder).

mod common;

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
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

fn create_file_req(
    token: Option<&str>,
    path_group_id: &str,
    x_group_id: Option<&str>,
    x_file_name: Option<&str>,
    x_folder_id: Option<&str>,
    content_type: Option<&str>,
    body_bytes: Vec<u8>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri(format!("/v1/groups/{path_group_id}/files"));
    if let Some(ct) = content_type {
        builder = builder.header("content-type", ct);
    }
    builder = builder.header("content-length", body_bytes.len().to_string());
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
    if let Some(n) = x_file_name {
        req.headers_mut().insert(
            HeaderName::from_static("x-file-name"),
            HeaderValue::from_str(n).unwrap(),
        );
    }
    if let Some(fid) = x_folder_id {
        req.headers_mut().insert(
            HeaderName::from_static("x-folder-id"),
            HeaderValue::from_str(fid).unwrap(),
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

/// Seed a `folders` row and return its id.
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

#[tokio::test]
async fn v1_files_create_scenarios() {
    if !docker_available() {
        eprintln!("docker not available; skipping v1_files_create_scenarios");
        return;
    }

    let h = Harness::get().await;
    let tmp = tempfile::tempdir().expect("tempdir");
    let router = build_storage_router(&h, tmp.path()).await;

    let (owner_id, group_id, owner_token) = seed_user_with_group(&h, "owner@0099-create-file.test")
        .await
        .expect("seed owner+group");
    let gid_str = group_id.to_string();

    // ─── FC1. 201 happy path ──────────────────────────────────────────────────
    let fc1_payload: &[u8] = b"hello from FC1";
    let resp = router
        .clone()
        .oneshot(create_file_req(
            Some(&owner_token),
            &gid_str,
            Some(&gid_str),
            Some("readme.txt"),
            None,
            Some("text/plain"),
            fc1_payload.to_vec(),
        ))
        .await
        .expect("FC1 oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "FC1 status");
    let body = body_json(resp).await;
    let fc1_file_id: Uuid = body["file_id"]
        .as_str()
        .unwrap()
        .parse()
        .expect("FC1 file_id parse");
    assert_eq!(body["name"], "readme.txt", "FC1 name");
    assert_eq!(body["group_id"], gid_str, "FC1 group_id");
    assert_eq!(body["version"].as_i64(), Some(1), "FC1 version");
    assert_eq!(
        body["size_bytes"].as_i64(),
        Some(fc1_payload.len() as i64),
        "FC1 size_bytes"
    );
    assert_eq!(body["mime_type"], "text/plain", "FC1 mime_type");
    assert!(body["folder_id"].is_null(), "FC1 folder_id null");

    // FC1 — DB: files row created.
    let (cur_ver, tot_ver, sz, mime): (i32, i32, i64, String) = sqlx::query_as(
        "SELECT current_version, total_versions, size_bytes, mime_type \
         FROM files WHERE id = $1",
    )
    .bind(fc1_file_id)
    .fetch_one(&h.admin_pool)
    .await
    .expect("FC1 fetch files row");
    assert_eq!(cur_ver, 1, "FC1 db current_version");
    assert_eq!(tot_ver, 1, "FC1 db total_versions");
    assert_eq!(sz, fc1_payload.len() as i64, "FC1 db size_bytes");
    assert_eq!(mime, "text/plain", "FC1 db mime_type");

    // FC1 — DB: file_versions v1 row created.
    let (fv_ver,): (i32,) =
        sqlx::query_as("SELECT version FROM file_versions WHERE file_id = $1 AND version = 1")
            .bind(fc1_file_id)
            .fetch_one(&h.admin_pool)
            .await
            .expect("FC1 fetch file_versions v1");
    assert_eq!(fv_ver, 1, "FC1 db file_versions version");

    // FC1 — file is readable via GET.
    let get_resp = router
        .clone()
        .oneshot(req_with_peer(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/groups/{gid_str}/files/{fc1_file_id}"))
                .header("authorization", format!("Bearer {owner_token}"))
                .header("x-group-id", &gid_str),
            Body::empty(),
        ))
        .await
        .expect("FC1 GET oneshot");
    assert_eq!(get_resp.status(), StatusCode::OK, "FC1 GET status");

    // ─── FC2. 400 missing X-File-Name ────────────────────────────────────────
    let resp = router
        .clone()
        .oneshot(create_file_req(
            Some(&owner_token),
            &gid_str,
            Some(&gid_str),
            None, // no X-File-Name
            None,
            Some("text/plain"),
            b"data".to_vec(),
        ))
        .await
        .expect("FC2 oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "FC2 status");

    // ─── FC3. 400 invalid X-File-Name (empty string) ─────────────────────────
    let resp = router
        .clone()
        .oneshot(create_file_req(
            Some(&owner_token),
            &gid_str,
            Some(&gid_str),
            Some(""), // empty name
            None,
            Some("text/plain"),
            b"data".to_vec(),
        ))
        .await
        .expect("FC3 oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "FC3 status");

    // ─── FC4. 400 bad X-Folder-Id (not in this group) ────────────────────────
    // Seed a folder in a *different* group then reference it here.
    let (_other_id, other_group_id, _other_token) =
        seed_user_with_group(&h, "other@0099-create-file.test")
            .await
            .expect("FC4 seed other group");
    let other_folder_id = seed_folder(&h, other_group_id, _other_id, "foreign-folder")
        .await
        .expect("FC4 seed folder in other group");

    let resp = router
        .clone()
        .oneshot(create_file_req(
            Some(&owner_token),
            &gid_str,
            Some(&gid_str),
            Some("test.txt"),
            Some(&other_folder_id.to_string()),
            Some("text/plain"),
            b"data".to_vec(),
        ))
        .await
        .expect("FC4 oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "FC4 status");

    // ─── FC5. 403 cross-group path mismatch ──────────────────────────────────
    // owner_token is for group_id, but path has other_group_id → 403.
    let resp = router
        .clone()
        .oneshot(create_file_req(
            Some(&owner_token),
            &other_group_id.to_string(),
            Some(&other_group_id.to_string()),
            Some("test.txt"),
            None,
            Some("text/plain"),
            b"data".to_vec(),
        ))
        .await
        .expect("FC5 oneshot");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "FC5 status");

    // ─── FC6. 415 MIME not in allow-list ─────────────────────────────────────
    let resp = router
        .clone()
        .oneshot(create_file_req(
            Some(&owner_token),
            &gid_str,
            Some(&gid_str),
            Some("evil.bin"),
            None,
            Some("application/x-evil"),
            b"data".to_vec(),
        ))
        .await
        .expect("FC6 oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "FC6 status"
    );

    // ─── FC7. 503 object store not configured ────────────────────────────────
    let no_store_router = build_no_storage_router(&h).await;
    let resp = no_store_router
        .oneshot(create_file_req(
            Some(&owner_token),
            &gid_str,
            Some(&gid_str),
            Some("test.txt"),
            None,
            Some("text/plain"),
            b"data".to_vec(),
        ))
        .await
        .expect("FC7 oneshot");
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE, "FC7 status");

    // ─── FC8. 403 Child role lacks FilesWrite ─────────────────────────────────
    let child_email = "child@0099-create-file.test";
    let (_child_id, child_token) = seed_member_via_admin(&h, group_id, "child", child_email)
        .await
        .expect("FC8 seed child");

    let resp = router
        .clone()
        .oneshot(create_file_req(
            Some(&child_token),
            &gid_str,
            Some(&gid_str),
            Some("child-file.txt"),
            None,
            Some("text/plain"),
            b"data".to_vec(),
        ))
        .await
        .expect("FC8 oneshot");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "FC8 status");

    // ─── FC9. audit event `file.created` present ─────────────────────────────
    // fc1_file_id was committed in FC1 — check audit row.
    let (action,): (String,) = sqlx::query_as(
        "SELECT action FROM audit_events \
         WHERE resource_type = 'files' AND resource_id = $1 \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(fc1_file_id.to_string())
    .fetch_one(&h.admin_pool)
    .await
    .expect("FC9 fetch audit event");
    assert_eq!(action, "file.created", "FC9 audit action");

    // ─── FC10. 201 happy path with X-Folder-Id ───────────────────────────────
    let folder_id = seed_folder(&h, group_id, owner_id, "docs-folder")
        .await
        .expect("FC10 seed folder");

    let fc10_payload: &[u8] = b"inside a folder";
    let resp = router
        .clone()
        .oneshot(create_file_req(
            Some(&owner_token),
            &gid_str,
            Some(&gid_str),
            Some("nested.txt"),
            Some(&folder_id.to_string()),
            Some("text/plain"),
            fc10_payload.to_vec(),
        ))
        .await
        .expect("FC10 oneshot");
    assert_eq!(resp.status(), StatusCode::CREATED, "FC10 status");
    let body = body_json(resp).await;
    assert_eq!(body["folder_id"], folder_id.to_string(), "FC10 folder_id");
    assert_eq!(body["name"], "nested.txt", "FC10 name");
    assert_eq!(body["version"].as_i64(), Some(1), "FC10 version");
}
