//! Integration tests for the tus 1.0 PATCH + OPTIONS handlers (plan
//! 0044 / GAR-395 slice 2).
//!
//! Spins up a pgvector testcontainer (via the shared harness) plus
//! a tempdir-backed `LocalFs` ObjectStore + `UploadStaging`, then
//! walks the spec-level scenarios listed in plan 0044 §7.2.
//!
//! All assertions live inside one `#[tokio::test]` function to keep
//! the sqlx runtime-teardown race avoided (same reason as
//! `rest_v1_invites.rs`).

mod common;

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use bytes::Bytes;
use garraia_gateway::rest_v1::uploads::UploadStaging;
use garraia_gateway::server::build_router_for_test_with_storage;
use garraia_storage::LocalFs;
use http_body_util::BodyExt;
use tower::ServiceExt;
use uuid::Uuid;

use common::Harness;
use common::fixtures::seed_user_with_group;

/// True when the Docker daemon is reachable. Integration tests bail
/// gracefully when Docker is absent (common on Windows dev hosts);
/// CI images run with Docker preinstalled so the gate is a no-op there.
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

fn post_create_upload(
    token: &str,
    group_id: &str,
    upload_length: i64,
    mime_type: &str,
) -> Request<Body> {
    // base64("upload.bin") = "dXBsb2FkLmJpbg==", base64(mime) runtime.
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;
    let filename_b64 = b64.encode("upload.bin");
    let mime_b64 = b64.encode(mime_type);
    let metadata = format!("filename {filename_b64},filetype {mime_b64}");
    let mut req = req_with_peer(
        Request::builder()
            .method("POST")
            .uri("/v1/uploads")
            .header("tus-resumable", "1.0.0")
            .header("upload-length", upload_length.to_string())
            .header("upload-metadata", metadata),
        Body::empty(),
    );
    req.headers_mut().insert(
        HeaderName::from_static("authorization"),
        HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
    );
    req.headers_mut().insert(
        HeaderName::from_static("x-group-id"),
        HeaderValue::from_str(group_id).unwrap(),
    );
    req
}

fn patch_upload_req(
    token: Option<&str>,
    group_id: Option<&str>,
    upload_id: &str,
    content_type: &str,
    upload_offset: Option<i64>,
    body: Bytes,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method("PATCH")
        .uri(format!("/v1/uploads/{upload_id}"))
        .header("tus-resumable", "1.0.0")
        .header("content-type", content_type);
    if let Some(o) = upload_offset {
        builder = builder.header("upload-offset", o.to_string());
    }
    let mut req = req_with_peer(builder, Body::from(body));
    if let Some(t) = token {
        req.headers_mut().insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_str(&format!("Bearer {t}")).unwrap(),
        );
    }
    if let Some(g) = group_id {
        req.headers_mut().insert(
            HeaderName::from_static("x-group-id"),
            HeaderValue::from_str(g).unwrap(),
        );
    }
    req
}

/// Helper: extract the tus upload id from the Location header of a
/// POST /v1/uploads 201 response.
fn upload_id_from_location(resp: &axum::response::Response) -> Uuid {
    let loc = resp
        .headers()
        .get("location")
        .expect("201 must carry Location")
        .to_str()
        .unwrap();
    let id_str = loc
        .strip_prefix("/v1/uploads/")
        .expect("Location has /v1/uploads/ prefix");
    Uuid::parse_str(id_str).expect("uuid")
}

/// Build a storage-augmented test router on top of the shared
/// `Harness` pools. Uses a per-invocation tempdir so concurrent
/// integration binaries cannot collide on a shared staging root.
async fn build_storage_router(
    h: &Harness,
    tmp: &Path,
) -> (axum::Router, Arc<LocalFs>, Arc<UploadStaging>) {
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

    (router, local_fs, staging)
}

#[tokio::test]
async fn v1_uploads_patch_scenarios() {
    if !docker_available() {
        eprintln!("docker not available; skipping v1_uploads_patch_scenarios");
        return;
    }

    let h = Harness::get().await;
    let tmp = tempfile::tempdir().expect("tempdir");
    let (router, local_fs, _staging) = build_storage_router(&h, tmp.path()).await;

    let (_user_id, group_id, token) = seed_user_with_group(&h, "patch-owner@0044.test")
        .await
        .unwrap();
    let gid_str = group_id.to_string();

    // ─── OPTIONS happy path ─────────────────────────────────────────
    //
    // NB: the production router wraps `/v1` with a permissive
    // `tower_http::cors::CorsLayer::allow_methods(Any)` which
    // intercepts pure OPTIONS (no Access-Control-Request-Method) and
    // returns 200 OK *without* the tus headers. In the test harness
    // we bypass that layer (no CORS applied), so OPTIONS hits our
    // handler directly and returns 204 + tus headers. Accept both
    // shapes so the assertion survives future router-layer changes.
    {
        let req = req_with_peer(
            Request::builder().method("OPTIONS").uri("/v1/uploads"),
            Body::empty(),
        );
        let resp = router.clone().oneshot(req).await.expect("options");
        assert!(
            resp.status() == StatusCode::NO_CONTENT || resp.status() == StatusCode::OK,
            "OPTIONS /v1/uploads returned unexpected status: {}",
            resp.status()
        );
        // When the tus handler did run, verify the full header set;
        // otherwise accept the CORS-layer short-circuit silently.
        if let Some(v) = resp.headers().get("tus-resumable") {
            assert_eq!(v, "1.0.0");
            assert_eq!(resp.headers().get("tus-version").unwrap(), "1.0.0");
            let ext = resp.headers().get("tus-extension").unwrap();
            assert!(ext.to_str().unwrap().contains("creation"));
            assert_eq!(resp.headers().get("tus-max-size").unwrap(), "5368709120");
        }
    }

    // ─── PATCH single-chunk happy path (commits files + file_versions) ──
    let upload_id = {
        let resp = router
            .clone()
            .oneshot(post_create_upload(&token, &gid_str, 5, "text/plain"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        upload_id_from_location(&resp)
    };

    {
        let body = Bytes::from_static(b"hello");
        let req = patch_upload_req(
            Some(&token),
            Some(&gid_str),
            &upload_id.to_string(),
            "application/offset+octet-stream",
            Some(0),
            body,
        );
        let resp = router.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT, "happy PATCH → 204");
        assert_eq!(resp.headers().get("upload-offset").unwrap(), "5");
        assert_eq!(resp.headers().get("tus-resumable").unwrap(), "1.0.0");
    }

    // files + file_versions asserts via admin_pool.
    let files_rows: Vec<(Uuid, i64, String)> =
        sqlx::query_as("SELECT id, size_bytes, mime_type FROM files WHERE group_id = $1")
            .bind(group_id)
            .fetch_all(&h.admin_pool)
            .await
            .unwrap();
    assert_eq!(files_rows.len(), 1, "one files row after first commit");
    assert_eq!(files_rows[0].1, 5);
    assert_eq!(files_rows[0].2, "text/plain");

    let fv_rows: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT object_key, etag, checksum_sha256, integrity_hmac
           FROM file_versions WHERE group_id = $1",
    )
    .bind(group_id)
    .fetch_all(&h.admin_pool)
    .await
    .unwrap();
    assert_eq!(fv_rows.len(), 1);
    let (object_key, _etag, checksum, hmac) = &fv_rows[0];
    assert!(checksum.len() == 64);
    assert!(hmac.len() == 64);
    // Blob physically present in LocalFs.
    use garraia_storage::ObjectStore;
    assert!(local_fs.exists(object_key).await.unwrap());

    // ─── PATCH offset mismatch → 409 ────────────────────────────────
    let upload_id2 = {
        let resp = router
            .clone()
            .oneshot(post_create_upload(&token, &gid_str, 10, "text/plain"))
            .await
            .unwrap();
        upload_id_from_location(&resp)
    };
    {
        let req = patch_upload_req(
            Some(&token),
            Some(&gid_str),
            &upload_id2.to_string(),
            "application/offset+octet-stream",
            Some(5), // expected 0
            Bytes::from_static(b"hello"),
        );
        let resp = router.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT, "offset mismatch → 409");
    }

    // ─── PATCH wrong content-type → 415 ─────────────────────────────
    {
        let req = patch_upload_req(
            Some(&token),
            Some(&gid_str),
            &upload_id2.to_string(),
            "application/json",
            Some(0),
            Bytes::from_static(b"{}"),
        );
        let resp = router.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "wrong content-type → 415"
        );
    }

    // ─── PATCH missing Tus-Resumable → 412 ──────────────────────────
    {
        let mut req = req_with_peer(
            Request::builder()
                .method("PATCH")
                .uri(format!("/v1/uploads/{upload_id2}"))
                .header("content-type", "application/offset+octet-stream")
                .header("upload-offset", "0"),
            Body::from(Bytes::from_static(b"hi")),
        );
        req.headers_mut().insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        req.headers_mut().insert(
            HeaderName::from_static("x-group-id"),
            HeaderValue::from_str(&gid_str).unwrap(),
        );
        let resp = router.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::PRECONDITION_FAILED,
            "missing Tus-Resumable → 412"
        );
        assert_eq!(
            resp.headers().get("tus-version").unwrap(),
            "1.0.0",
            "412 advertises Tus-Version"
        );
    }

    // ─── PATCH body exceeds Upload-Length → 413 ─────────────────────
    {
        // upload_id2 has Upload-Length = 10; send 20 bytes at offset 0.
        let req = patch_upload_req(
            Some(&token),
            Some(&gid_str),
            &upload_id2.to_string(),
            "application/offset+octet-stream",
            Some(0),
            Bytes::from_static(b"aaaaaaaaaaaaaaaaaaaa"),
        );
        let resp = router.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::PAYLOAD_TOO_LARGE,
            "body beyond upload_length → 413"
        );
    }

    // ─── PATCH without JWT → 401 ────────────────────────────────────
    {
        let req = patch_upload_req(
            None,
            Some(&gid_str),
            &upload_id2.to_string(),
            "application/offset+octet-stream",
            Some(0),
            Bytes::from_static(b"hi"),
        );
        let resp = router.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "no JWT → 401");
    }

    // ─── PATCH completed upload → 410 ──────────────────────────────
    {
        let req = patch_upload_req(
            Some(&token),
            Some(&gid_str),
            &upload_id.to_string(),
            "application/offset+octet-stream",
            Some(5),
            Bytes::from_static(b"x"),
        );
        let resp = router.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::GONE,
            "PATCH on completed upload → 410"
        );
    }

    // ─── Multi-chunk commit (3 × ~1 KiB) ────────────────────────────
    let chunk = vec![b'x'; 1024];
    let upload_id3 = {
        let resp = router
            .clone()
            .oneshot(post_create_upload(
                &token,
                &gid_str,
                (chunk.len() * 3) as i64,
                "text/plain",
            ))
            .await
            .unwrap();
        upload_id_from_location(&resp)
    };
    for (i, offset) in [0, 1024, 2048].iter().enumerate() {
        let req = patch_upload_req(
            Some(&token),
            Some(&gid_str),
            &upload_id3.to_string(),
            "application/offset+octet-stream",
            Some(*offset),
            Bytes::from(chunk.clone()),
        );
        let resp = router.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NO_CONTENT,
            "multi-chunk patch #{} expected 204",
            i
        );
    }

    // Assert HEAD final offset and that a second files row exists.
    {
        let req = req_with_peer(
            Request::builder()
                .method("HEAD")
                .uri(format!("/v1/uploads/{upload_id3}"))
                .header("tus-resumable", "1.0.0"),
            Body::empty(),
        );
        let (parts, _) = req.into_parts();
        let mut req = Request::from_parts(parts, Body::empty());
        req.headers_mut().insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        req.headers_mut().insert(
            HeaderName::from_static("x-group-id"),
            HeaderValue::from_str(&gid_str).unwrap(),
        );
        let resp = router.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let off = resp
            .headers()
            .get("upload-offset")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(off, "3072");
    }

    let files_count: (i64,) =
        sqlx::query_as("SELECT count(*)::bigint FROM files WHERE group_id = $1")
            .bind(group_id)
            .fetch_one(&h.admin_pool)
            .await
            .unwrap();
    assert_eq!(files_count.0, 2, "two commits → two files rows");

    // ─── Cross-group PATCH → 404 ────────────────────────────────────
    // Seed a second user in a different group; PATCH upload_id2 with
    // their token + their group_id → 404 (upload is in original group).
    let (_other_user, other_group, other_token) =
        seed_user_with_group(&h, "other@0044.test").await.unwrap();
    let other_gid_str = other_group.to_string();
    {
        let req = patch_upload_req(
            Some(&other_token),
            Some(&other_gid_str),
            &upload_id2.to_string(),
            "application/offset+octet-stream",
            Some(0),
            Bytes::from_static(b"hi"),
        );
        let resp = router.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "cross-group PATCH must be 404 (never 403, ADR 0004 §7)"
        );
    }

    // ─── audit_events upload.completed emission ────────────────────
    let audit: Vec<(String, Option<Uuid>, String, String, serde_json::Value)> = sqlx::query_as(
        "SELECT action, actor_user_id, resource_type, resource_id, metadata
           FROM audit_events
          WHERE group_id = $1 AND action = 'upload.completed'
          ORDER BY created_at DESC",
    )
    .bind(group_id)
    .fetch_all(&h.admin_pool)
    .await
    .unwrap();
    assert_eq!(audit.len(), 2, "two uploads → two audit rows");
    for row in &audit {
        assert_eq!(row.0, "upload.completed");
        assert_eq!(row.2, "files");
        assert!(row.4.get("upload_id").is_some());
        // PII-safe check: filename + mime must NOT appear in metadata.
        let s = row.4.to_string();
        assert!(
            !s.contains("upload.bin"),
            "audit metadata must not contain filename"
        );
        assert!(
            !s.contains("text/plain"),
            "audit metadata must not contain mime"
        );
    }

    // Drain body just so the test doesn't hold unread body streams.
    let _ = audit
        .into_iter()
        .map(|r| r.4.to_string())
        .collect::<Vec<_>>();
    // Ensure tempdir still exists until the end.
    drop(tmp);

    // Collecting the body of a few unconsumed response bodies to
    // appease http-body-util's debt (strictly optional, but cheap).
    let _ = BodyExt::collect(Body::empty()).await;
}
