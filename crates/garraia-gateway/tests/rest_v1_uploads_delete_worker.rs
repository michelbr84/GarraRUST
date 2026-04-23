//! Integration tests for plan 0047 / GAR-395 slice 3 commit 4.
//!
//! Exercises the three production paths that the earlier slice 3
//! commits introduced (DELETE termination handler, expiration worker
//! `run_expiration_tick`, streaming `put_stream` in `finalize_upload`)
//! end-to-end against a shared pgvector/pg16 testcontainer plus a
//! tempdir-backed `LocalFs` ObjectStore.
//!
//! Layout mirrors `rest_v1_uploads_patch.rs` intentionally — one
//! `#[tokio::test]` orchestrating three `async fn` helper blocks. The
//! single-function shape avoids the sqlx runtime-teardown race that
//! bites when multiple top-level `#[tokio::test]` fns share a
//! `OnceCell<Arc<Harness>>` inside the same binary; helpers keep the
//! sub-sections readable and independently debuggable.
//!
//! Coverage map:
//!
//! | Block | Handler / path                                         |
//! | ----- | ------------------------------------------------------ |
//! | A     | `DELETE /v1/uploads/{id}` — 6 scenarios                |
//! | B     | `uploads_worker::run_expiration_tick` — 3 sub-asserts  |
//! | C     | `finalize_upload` streaming via `put_stream` — 1 e2e   |
//!
//! Docker absence is surfaced via `Harness::get()` (pgvector startup
//! fails fast); every helper below still guards with
//! [`docker_available`] so running on a dev host without Docker yields
//! a clean skip rather than a confusing Harness boot panic.

mod common;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use bytes::Bytes;
use garraia_gateway::rest_v1::uploads::UploadStaging;
use garraia_gateway::server::build_router_for_test_with_storage;
use garraia_gateway::uploads_worker::run_expiration_tick;
use garraia_storage::{LocalFs, ObjectStore};
use tower::ServiceExt;
use uuid::Uuid;

use common::Harness;
use common::fixtures::{fetch_audit_events_for_group, seed_user_with_group};

// ─── Docker gate ────────────────────────────────────────────────────────

/// True when the Docker daemon is reachable. Integration tests bail
/// gracefully when Docker is absent (common on Windows dev hosts); CI
/// Linux images run with Docker preinstalled so the gate is a no-op
/// there. Mirrors the check in `rest_v1_uploads_patch.rs`.
fn docker_available() -> bool {
    std::process::Command::new("docker")
        .arg("version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ─── Request builders (mirrors rest_v1_uploads_patch.rs pattern) ────────

/// Inject the `ConnectInfo<SocketAddr>` extension so `tower_governor`'s
/// `PeerIpKeyExtractor` does not bail with `"Unable To Extract Key!"`
/// when the router is exercised via `oneshot`. Every request in this
/// binary must flow through this helper.
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
    token: &str,
    group_id: &str,
    upload_id: Uuid,
    upload_offset: i64,
    body: Bytes,
) -> Request<Body> {
    let mut req = req_with_peer(
        Request::builder()
            .method("PATCH")
            .uri(format!("/v1/uploads/{upload_id}"))
            .header("tus-resumable", "1.0.0")
            .header("content-type", "application/offset+octet-stream")
            .header("upload-offset", upload_offset.to_string()),
        Body::from(body),
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

fn delete_upload_req(
    token: Option<&str>,
    group_id: Option<&str>,
    upload_id: Uuid,
    tus_resumable: Option<&str>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method("DELETE")
        .uri(format!("/v1/uploads/{upload_id}"));
    if let Some(v) = tus_resumable {
        builder = builder.header("tus-resumable", v);
    }
    let mut req = req_with_peer(builder, Body::empty());
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

/// Extract the tus upload id from the `Location` header of a
/// `POST /v1/uploads` 201 response.
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
/// `Harness` pools. Per-invocation tempdir so concurrent integration
/// binaries cannot collide on a shared staging root.
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

/// Small helper: create + fully PATCH a new upload so downstream tests
/// can exercise the `completed` status branch. Returns the upload id.
async fn create_and_complete_upload(
    router: &axum::Router,
    token: &str,
    group_id_str: &str,
    payload: &[u8],
) -> Uuid {
    let resp = router
        .clone()
        .oneshot(post_create_upload(
            token,
            group_id_str,
            payload.len() as i64,
            "text/plain",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let upload_id = upload_id_from_location(&resp);

    let resp = router
        .clone()
        .oneshot(patch_upload_req(
            token,
            group_id_str,
            upload_id,
            0,
            Bytes::copy_from_slice(payload),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "complete PATCH → 204"
    );
    upload_id
}

/// Small helper: create an `in_progress` upload and return its id.
/// Callers decide whether to leave staging empty or write a placeholder
/// via [`write_placeholder_staging`] — the worker scenarios exercise
/// both variants.
async fn create_in_progress_upload(
    router: &axum::Router,
    token: &str,
    group_id_str: &str,
    upload_length: i64,
) -> Uuid {
    let resp = router
        .clone()
        .oneshot(post_create_upload(
            token,
            group_id_str,
            upload_length,
            "text/plain",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    upload_id_from_location(&resp)
}

/// Write a zero-content placeholder staging file so the worker's
/// best-effort cleanup branch increments `staging_removed`. Real
/// content is not necessary — the worker just calls `remove_file`.
async fn write_placeholder_staging(staging: &UploadStaging, upload_id: Uuid) {
    let path = staging.staging_dir.join(format!("{upload_id}.staging"));
    tokio::fs::write(&path, b"placeholder")
        .await
        .expect("write placeholder staging");
}

/// Flip `expires_at` to a point in the past via admin_pool. The tus
/// creation path always writes `now() + 24h`; the worker only sweeps
/// rows with `expires_at < now()`, so this is the test-only handshake
/// that lets us validate the sweep without a 24-hour sleep.
async fn expire_row(h: &Harness, upload_id: Uuid) {
    sqlx::query(
        "UPDATE tus_uploads \
         SET expires_at = now() - interval '1 hour' \
         WHERE id = $1",
    )
    .bind(upload_id)
    .execute(&h.admin_pool)
    .await
    .expect("admin UPDATE expires_at");
}

// ─── Main orchestrator ──────────────────────────────────────────────────

#[tokio::test]
async fn v1_uploads_delete_worker_streaming_scenarios() {
    if !docker_available() {
        eprintln!("docker not available; skipping v1_uploads_delete_worker_streaming_scenarios");
        return;
    }

    let h = Harness::get().await;
    let tmp = tempfile::tempdir().expect("tempdir");
    let (router, local_fs, staging) = build_storage_router(&h, tmp.path()).await;

    run_delete_scenarios(&h, &router).await;
    run_worker_scenarios(&h, &router, &staging).await;
    run_streaming_scenario(&h, &router, local_fs.as_ref()).await;

    // Keep the staging tempdir alive for the whole test — dropping it
    // earlier would remove staging files mid-test and invalidate the
    // worker's cleanup assertions.
    drop(tmp);
}

// ─── Block A — DELETE /v1/uploads/{id} scenarios ────────────────────────

async fn run_delete_scenarios(h: &Arc<Harness>, router: &axum::Router) {
    let (_owner_id, group_id, token) = seed_user_with_group(h, "delete-owner@0047.test")
        .await
        .expect("seed owner");
    let gid_str = group_id.to_string();

    // ─── A1. DELETE happy path ────────────────────────────────────────
    let upload_id = create_in_progress_upload(router, &token, &gid_str, 16).await;

    {
        let resp = router
            .clone()
            .oneshot(delete_upload_req(
                Some(&token),
                Some(&gid_str),
                upload_id,
                Some("1.0.0"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT, "A1 DELETE → 204");
        assert_eq!(
            resp.headers().get("tus-resumable").unwrap(),
            "1.0.0",
            "A1 204 carries Tus-Resumable"
        );
    }

    // Row must now be `aborted`.
    let status_after: (String,) = sqlx::query_as("SELECT status FROM tus_uploads WHERE id = $1")
        .bind(upload_id)
        .fetch_one(&h.admin_pool)
        .await
        .unwrap();
    assert_eq!(status_after.0, "aborted", "A1 status flipped to aborted");

    // Audit row `upload.terminated` emitted inside the same tx.
    let audit = fetch_audit_events_for_group(h, group_id)
        .await
        .expect("fetch audit");
    let terminated: Vec<_> = audit
        .iter()
        .filter(|r| r.0 == "upload.terminated")
        .collect();
    assert_eq!(terminated.len(), 1, "A1 one upload.terminated row");
    let meta = &terminated[0].4;
    assert_eq!(
        terminated[0].3,
        upload_id.to_string(),
        "A1 resource_id = upload_id"
    );
    assert_eq!(terminated[0].2, "tus_uploads");
    // PII-safe: metadata must contain object_key_hash (hex 64 chars) but
    // never the raw key or any filename.
    let hash = meta
        .get("object_key_hash")
        .and_then(|v| v.as_str())
        .unwrap();
    assert_eq!(hash.len(), 64, "A1 object_key_hash is hex-64");
    assert!(
        !meta.to_string().contains("upload.bin"),
        "A1 audit metadata must not contain filename"
    );

    // ─── A2. Idempotent DELETE → second call returns 410 ─────────────
    {
        let resp = router
            .clone()
            .oneshot(delete_upload_req(
                Some(&token),
                Some(&gid_str),
                upload_id,
                Some("1.0.0"),
            ))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::GONE,
            "A2 second DELETE on aborted → 410"
        );
    }

    // ─── A3. DELETE on a completed upload → 410 ──────────────────────
    let completed_id = create_and_complete_upload(router, &token, &gid_str, b"hello-delete").await;
    {
        let resp = router
            .clone()
            .oneshot(delete_upload_req(
                Some(&token),
                Some(&gid_str),
                completed_id,
                Some("1.0.0"),
            ))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::GONE,
            "A3 DELETE on completed → 410"
        );
    }

    // ─── A4. Cross-group DELETE → 404 (never 403) ───────────────────
    let (_other_id, other_group, other_token) = seed_user_with_group(h, "other@0047.test")
        .await
        .expect("seed other");
    let other_gid_str = other_group.to_string();

    let foreign_upload = create_in_progress_upload(router, &token, &gid_str, 8).await;
    {
        let resp = router
            .clone()
            .oneshot(delete_upload_req(
                Some(&other_token),
                Some(&other_gid_str),
                foreign_upload,
                Some("1.0.0"),
            ))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "A4 cross-group DELETE → 404 (ADR 0004 §7 — never 403)"
        );
    }

    // Confirm the foreign upload was NOT mutated by the cross-group attempt.
    let foreign_status: (String,) = sqlx::query_as("SELECT status FROM tus_uploads WHERE id = $1")
        .bind(foreign_upload)
        .fetch_one(&h.admin_pool)
        .await
        .unwrap();
    assert_eq!(
        foreign_status.0, "in_progress",
        "A4 cross-group DELETE must not mutate"
    );

    // ─── A5. DELETE without Tus-Resumable → 412 ──────────────────────
    {
        let resp = router
            .clone()
            .oneshot(delete_upload_req(
                Some(&token),
                Some(&gid_str),
                foreign_upload,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::PRECONDITION_FAILED,
            "A5 missing Tus-Resumable → 412"
        );
        assert_eq!(
            resp.headers().get("tus-version").unwrap(),
            "1.0.0",
            "A5 412 advertises Tus-Version"
        );
    }

    // ─── A6. DELETE unknown UUID → 404 ───────────────────────────────
    {
        let unknown = Uuid::now_v7();
        let resp = router
            .clone()
            .oneshot(delete_upload_req(
                Some(&token),
                Some(&gid_str),
                unknown,
                Some("1.0.0"),
            ))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "A6 unknown upload → 404"
        );
    }
}

// ─── Block B — Expiration worker tick end-to-end ────────────────────────

async fn run_worker_scenarios(
    h: &Arc<Harness>,
    router: &axum::Router,
    staging: &Arc<UploadStaging>,
) {
    let (_user_id, group_id, token) = seed_user_with_group(h, "worker-owner@0047.test")
        .await
        .expect("seed worker owner");
    let gid_str = group_id.to_string();

    // ─── B1. Basic sweep — expired row → status='expired' + audit ────
    let b1_upload = create_in_progress_upload(router, &token, &gid_str, 32).await;
    write_placeholder_staging(staging, b1_upload).await;
    expire_row(h, b1_upload).await;

    let report = run_expiration_tick(h.app_pool.clone(), Some(&staging.staging_dir), 16)
        .await
        .expect("B1 tick ok");
    assert!(
        report.expired_count >= 1,
        "B1 expired_count >= 1 (got {})",
        report.expired_count
    );
    assert!(
        report.staging_removed >= 1,
        "B1 staging_removed >= 1 (got {})",
        report.staging_removed
    );
    assert_eq!(report.audit_failed, 0, "B1 no audit failures");

    // `tus_uploads.status` flipped to `expired`.
    let row: (String,) = sqlx::query_as("SELECT status FROM tus_uploads WHERE id = $1")
        .bind(b1_upload)
        .fetch_one(&h.admin_pool)
        .await
        .unwrap();
    assert_eq!(row.0, "expired", "B1 row transitioned to expired");

    // Audit row `upload.expired` emitted with metadata shape.
    let audit = fetch_audit_events_for_group(h, group_id)
        .await
        .expect("fetch audit after B1");
    let expired_audit: Vec<_> = audit
        .iter()
        .filter(|r| r.0 == "upload.expired" && r.3 == b1_upload.to_string())
        .collect();
    assert_eq!(expired_audit.len(), 1, "B1 one upload.expired row");
    let meta = &expired_audit[0].4;
    assert!(
        meta.get("upload_offset").is_some(),
        "B1 metadata.upload_offset"
    );
    assert!(
        meta.get("upload_length").is_some(),
        "B1 metadata.upload_length"
    );
    assert!(meta.get("age_secs").is_some(), "B1 metadata.age_secs");
    assert_eq!(
        meta.get("object_key_hash")
            .and_then(|v| v.as_str())
            .map(str::len),
        Some(64),
        "B1 metadata.object_key_hash is hex-64"
    );

    // Staging file physically gone.
    let staging_path = staging.staging_dir.join(format!("{b1_upload}.staging"));
    assert!(!staging_path.exists(), "B1 staging file removed by worker");

    // ─── B2. Idempotent sweep — second tick finds nothing ───────────
    // Advisory lock release is async via `AdvisoryLockGuard::Drop` →
    // `tokio::spawn` — give it headroom before acquiring again.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let report2 = run_expiration_tick(h.app_pool.clone(), Some(&staging.staging_dir), 16)
        .await
        .expect("B2 tick ok");
    assert_eq!(
        report2.expired_count, 0,
        "B2 idempotent — no in_progress rows past expires_at"
    );
    assert_eq!(report2.audit_failed, 0);

    // ─── B3. Staging missing is tolerated ──────────────────────────
    let b3_upload = create_in_progress_upload(router, &token, &gid_str, 64).await;
    // NOTE: we deliberately do NOT call write_placeholder_staging here.
    expire_row(h, b3_upload).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let report3 = run_expiration_tick(h.app_pool.clone(), Some(&staging.staging_dir), 16)
        .await
        .expect("B3 tick ok");
    assert!(
        report3.expired_count >= 1,
        "B3 expired_count >= 1 (got {})",
        report3.expired_count
    );
    assert!(
        report3.staging_missing >= 1,
        "B3 staging_missing increments when file absent (got {})",
        report3.staging_missing
    );
    assert_eq!(report3.audit_failed, 0, "B3 no audit failures");
}

// ─── Block C — Streaming put_stream e2e via finalize_upload ─────────────

async fn run_streaming_scenario(h: &Arc<Harness>, router: &axum::Router, local_fs: &LocalFs) {
    let (_user_id, group_id, token) = seed_user_with_group(h, "streaming-owner@0047.test")
        .await
        .expect("seed streaming owner");
    let gid_str = group_id.to_string();

    const MIB: usize = 1024 * 1024;
    const TOTAL: usize = 2 * MIB;

    // Deterministic byte pattern so the round-trip asserts catch
    // off-by-one / truncation bugs in the streaming path.
    let mut payload = Vec::with_capacity(TOTAL);
    for i in 0..TOTAL {
        payload.push((i % 251) as u8);
    }

    let upload_id = create_in_progress_upload(router, &token, &gid_str, TOTAL as i64).await;

    // Chunk 1 — [0 .. MIB)
    {
        let resp = router
            .clone()
            .oneshot(patch_upload_req(
                &token,
                &gid_str,
                upload_id,
                0,
                Bytes::copy_from_slice(&payload[..MIB]),
            ))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NO_CONTENT,
            "C1 chunk1 → 204 (in_progress)"
        );
        assert_eq!(
            resp.headers().get("upload-offset").unwrap(),
            MIB.to_string().as_str()
        );
    }

    // Chunk 2 — [MIB .. 2*MIB) triggers finalize → put_stream.
    {
        let resp = router
            .clone()
            .oneshot(patch_upload_req(
                &token,
                &gid_str,
                upload_id,
                MIB as i64,
                Bytes::copy_from_slice(&payload[MIB..]),
            ))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NO_CONTENT,
            "C1 chunk2 → 204 (completed)"
        );
        assert_eq!(
            resp.headers().get("upload-offset").unwrap(),
            TOTAL.to_string().as_str()
        );
    }

    // ─── Assert status flipped to completed ─────────────────────────
    let tus_row: (String,) = sqlx::query_as("SELECT status FROM tus_uploads WHERE id = $1")
        .bind(upload_id)
        .fetch_one(&h.admin_pool)
        .await
        .unwrap();
    assert_eq!(tus_row.0, "completed", "C1 tus_uploads.status = completed");

    // ─── Assert files / file_versions rows + checksum shape ─────────
    let files_rows: Vec<(Uuid, i64, String)> =
        sqlx::query_as("SELECT id, size_bytes, mime_type FROM files WHERE group_id = $1")
            .bind(group_id)
            .fetch_all(&h.admin_pool)
            .await
            .unwrap();
    assert_eq!(files_rows.len(), 1, "C1 one files row");
    assert_eq!(files_rows[0].1, TOTAL as i64, "C1 files.size_bytes = 2 MiB");
    assert_eq!(files_rows[0].2, "text/plain", "C1 files.mime_type");

    let fv_rows: Vec<(String, String, String, i64)> = sqlx::query_as(
        "SELECT object_key, checksum_sha256, integrity_hmac, size_bytes \
           FROM file_versions WHERE group_id = $1",
    )
    .bind(group_id)
    .fetch_all(&h.admin_pool)
    .await
    .unwrap();
    assert_eq!(fv_rows.len(), 1, "C1 one file_versions row");
    let (object_key, checksum, hmac, fv_size) = &fv_rows[0];
    assert_eq!(
        *fv_size, TOTAL as i64,
        "C1 file_versions.size_bytes = 2 MiB"
    );
    assert_eq!(checksum.len(), 64, "C1 checksum_sha256 is hex-64");
    assert_eq!(hmac.len(), 64, "C1 integrity_hmac is hex-64");

    // ─── Byte-for-byte round-trip via ObjectStore::get ──────────────
    let got = local_fs.get(object_key).await.expect("C1 get object");
    assert_eq!(
        got.bytes.len(),
        TOTAL,
        "C1 LocalFs round-trip length = 2 MiB"
    );
    assert_eq!(
        got.bytes.as_ref(),
        payload.as_slice(),
        "C1 streaming round-trip preserves every byte"
    );

    // ─── Audit `upload.completed` emitted with PII-safe metadata ────
    let audit = fetch_audit_events_for_group(h, group_id)
        .await
        .expect("fetch audit after C1");
    let completed_audit: Vec<_> = audit
        .iter()
        .filter(|r| r.0 == "upload.completed" && r.3 == files_rows[0].0.to_string())
        .collect();
    assert_eq!(completed_audit.len(), 1, "C1 one upload.completed row");
    let meta_str = completed_audit[0].4.to_string();
    assert!(
        !meta_str.contains("upload.bin"),
        "C1 audit metadata must not contain filename"
    );
    assert!(
        !meta_str.contains("text/plain"),
        "C1 audit metadata must not contain mime"
    );
}
