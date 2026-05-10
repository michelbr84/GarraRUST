// Gated so `cargo clippy --all-targets` without `test-helpers` skips this
// file and doesn't try to compile the `common` harness.
#![cfg(feature = "test-helpers")]
//! Integration tests for `GET /v1/groups/{group_id}/files/{file_id}/versions`
//! (plan 0095, GAR-569, Fase 3.4 files slice 8).
//!
//! All scenarios bundled into ONE `#[tokio::test]` — same pattern as other
//! files tests to avoid the sqlx runtime-teardown race.
//!
//! Scenarios:
//! VL1. 200 happy path — 2 versions, newest-first, next_cursor=null.
//! VL2. 200 pagination — 3 versions, limit=2 first page, then second page.
//! VL3. 404 non-existent file_id.
//! VL4. 404 soft-deleted file.
//! VL5. 403 path_group_id ≠ principal.group_id.
//! VL6. 403 role lacks FilesRead (child role).
//! VL7. 400 missing X-Group-Id header.
//! VL8. 200 empty list — file exists but zero versions.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use chrono::Utc;
use http_body_util::BodyExt;
use tower::ServiceExt;
use uuid::Uuid;

use common::Harness;
use common::fixtures::{seed_member_via_admin, seed_user_with_group};

fn list_versions_req(
    token: Option<&str>,
    path_group_id: &str,
    file_id: &str,
    x_group_id: Option<&str>,
    query: &str,
) -> Request<Body> {
    let uri = format!("/v1/groups/{path_group_id}/files/{file_id}/versions{query}");
    let mut req = Request::builder()
        .method("GET")
        .uri(&uri)
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

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("failed to collect response body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response body is not JSON")
}

/// Seed a `files` row with explicit version counters. Pass the desired
/// `current_version` / `total_versions` to satisfy schema CHECK (>= 1).
async fn seed_file(
    h: &Harness,
    group_id: Uuid,
    created_by: Uuid,
    name: &str,
    current_version: i32,
    total_versions: i32,
) -> anyhow::Result<Uuid> {
    let file_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO files (id, group_id, folder_id, name, current_version, \
                            total_versions, size_bytes, mime_type, settings, \
                            created_by, created_by_label) \
         VALUES ($1, $2, NULL, $3, $4, $5, 0, 'text/plain', '{}'::jsonb, $6, $7)",
    )
    .bind(file_id)
    .bind(group_id)
    .bind(name)
    .bind(current_version)
    .bind(total_versions)
    .bind(created_by)
    .bind("Test User")
    .execute(&h.admin_pool)
    .await?;
    Ok(file_id)
}

/// Seed a `file_versions` row with the given `version` number.
async fn seed_file_version(
    h: &Harness,
    file_id: Uuid,
    group_id: Uuid,
    created_by: Uuid,
    version: i32,
    size_bytes: i64,
    mime_type: &str,
) -> anyhow::Result<()> {
    let hex64 = "a".repeat(64);
    let object_key = format!("groups/{group_id}/files/{file_id}/v{version}/blob");
    sqlx::query(
        "INSERT INTO file_versions \
         (file_id, group_id, version, object_key, etag, checksum_sha256, \
          integrity_hmac, size_bytes, mime_type, created_by, created_by_label) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(file_id)
    .bind(group_id)
    .bind(version)
    .bind(&object_key)
    .bind("abc123")
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

/// Soft-delete a file.
async fn soft_delete_file(h: &Harness, file_id: Uuid) -> anyhow::Result<()> {
    sqlx::query("UPDATE files SET deleted_at = $1 WHERE id = $2")
        .bind(Utc::now())
        .bind(file_id)
        .execute(&h.admin_pool)
        .await?;
    Ok(())
}

#[tokio::test]
async fn v1_files_list_versions_scenarios() {
    let h = Harness::get().await;

    let (owner_id, group_id, owner_token) =
        seed_user_with_group(&h, "owner@0095-list-versions.test")
            .await
            .expect("seed owner+group");
    let gid_str = group_id.to_string();

    // ─── VL1. 200 happy path — 2 versions, newest-first ──────────────────
    let vl1_id = seed_file(&h, group_id, owner_id, "doc-vl1.txt", 2, 2)
        .await
        .expect("VL1 seed file");
    seed_file_version(&h, vl1_id, group_id, owner_id, 1, 100, "text/plain")
        .await
        .expect("VL1 seed v1");
    seed_file_version(&h, vl1_id, group_id, owner_id, 2, 200, "text/plain")
        .await
        .expect("VL1 seed v2");

    let resp = h
        .router
        .clone()
        .oneshot(list_versions_req(
            Some(&owner_token),
            &gid_str,
            &vl1_id.to_string(),
            Some(&gid_str),
            "",
        ))
        .await
        .expect("VL1 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "VL1 status");
    let body = body_json(resp).await;
    let items = body["items"].as_array().expect("VL1 items array");
    assert_eq!(items.len(), 2, "VL1 item count");
    // Newest first: version 2, then version 1
    assert_eq!(items[0]["version"].as_i64(), Some(2), "VL1 first version");
    assert_eq!(items[1]["version"].as_i64(), Some(1), "VL1 second version");
    assert!(body["next_cursor"].is_null(), "VL1 next_cursor null");
    // object_key must NOT appear in any response field
    assert!(
        items[0].get("object_key").is_none(),
        "VL1 object_key absent"
    );
    assert!(
        items[0].get("integrity_hmac").is_none(),
        "VL1 integrity_hmac absent"
    );

    // ─── VL2. 200 pagination — 3 versions, limit=2 ───────────────────────
    let vl2_id = seed_file(&h, group_id, owner_id, "doc-vl2.txt", 3, 3)
        .await
        .expect("VL2 seed file");
    for v in 1..=3i32 {
        seed_file_version(
            &h,
            vl2_id,
            group_id,
            owner_id,
            v,
            50 * v as i64,
            "text/plain",
        )
        .await
        .expect("VL2 seed version");
    }

    // First page: limit=2 → versions 3 and 2; next_cursor = 2
    let resp = h
        .router
        .clone()
        .oneshot(list_versions_req(
            Some(&owner_token),
            &gid_str,
            &vl2_id.to_string(),
            Some(&gid_str),
            "?limit=2",
        ))
        .await
        .expect("VL2 first-page oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "VL2 first-page status");
    let page1 = body_json(resp).await;
    let p1_items = page1["items"].as_array().expect("VL2 p1 items");
    assert_eq!(p1_items.len(), 2, "VL2 p1 item count");
    assert_eq!(p1_items[0]["version"].as_i64(), Some(3), "VL2 p1 first");
    assert_eq!(p1_items[1]["version"].as_i64(), Some(2), "VL2 p1 second");
    let next_cursor = page1["next_cursor"].as_i64().expect("VL2 next_cursor");
    assert_eq!(next_cursor, 2, "VL2 next_cursor value");

    // Second page: cursor=2 → versions < 2 → version 1; next_cursor = null
    let resp = h
        .router
        .clone()
        .oneshot(list_versions_req(
            Some(&owner_token),
            &gid_str,
            &vl2_id.to_string(),
            Some(&gid_str),
            &format!("?limit=2&cursor={next_cursor}"),
        ))
        .await
        .expect("VL2 second-page oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "VL2 second-page status");
    let page2 = body_json(resp).await;
    let p2_items = page2["items"].as_array().expect("VL2 p2 items");
    assert_eq!(p2_items.len(), 1, "VL2 p2 item count");
    assert_eq!(p2_items[0]["version"].as_i64(), Some(1), "VL2 p2 version");
    assert!(page2["next_cursor"].is_null(), "VL2 p2 next_cursor null");

    // ─── VL3. 404 non-existent file_id ───────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(list_versions_req(
            Some(&owner_token),
            &gid_str,
            &Uuid::new_v4().to_string(),
            Some(&gid_str),
            "",
        ))
        .await
        .expect("VL3 oneshot");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "VL3 status");

    // ─── VL4. 404 soft-deleted file ──────────────────────────────────────
    let vl4_id = seed_file(&h, group_id, owner_id, "dead-vl4.txt", 1, 1)
        .await
        .expect("VL4 seed file");
    seed_file_version(&h, vl4_id, group_id, owner_id, 1, 10, "text/plain")
        .await
        .expect("VL4 seed v1");
    soft_delete_file(&h, vl4_id).await.expect("VL4 soft delete");

    let resp = h
        .router
        .clone()
        .oneshot(list_versions_req(
            Some(&owner_token),
            &gid_str,
            &vl4_id.to_string(),
            Some(&gid_str),
            "",
        ))
        .await
        .expect("VL4 oneshot");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "VL4 status");

    // ─── VL5. 403 path_group_id ≠ principal.group_id ─────────────────────
    let (_other_id, other_group_id, _other_token) =
        seed_user_with_group(&h, "other@0095-list-versions.test")
            .await
            .expect("VL5 seed other group");

    let vl5_id = seed_file(&h, group_id, owner_id, "cross-vl5.txt", 1, 1)
        .await
        .expect("VL5 seed file");
    seed_file_version(&h, vl5_id, group_id, owner_id, 1, 5, "text/plain")
        .await
        .expect("VL5 seed v1");

    // owner_token → principal.group_id = group_id, but path says other_group_id
    let resp = h
        .router
        .clone()
        .oneshot(list_versions_req(
            Some(&owner_token),
            &other_group_id.to_string(),
            &vl5_id.to_string(),
            Some(&other_group_id.to_string()),
            "",
        ))
        .await
        .expect("VL5 oneshot");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "VL5 status");

    // ─── VL6. 403 child role lacks FilesRead ─────────────────────────────
    let (_child_id, child_token) =
        seed_member_via_admin(&h, group_id, "child", "child@0095-list-versions.test")
            .await
            .expect("VL6 seed child");

    let vl6_id = seed_file(&h, group_id, owner_id, "child-vl6.txt", 1, 1)
        .await
        .expect("VL6 seed file");
    seed_file_version(&h, vl6_id, group_id, owner_id, 1, 7, "text/plain")
        .await
        .expect("VL6 seed v1");

    let resp = h
        .router
        .clone()
        .oneshot(list_versions_req(
            Some(&child_token),
            &gid_str,
            &vl6_id.to_string(),
            Some(&gid_str),
            "",
        ))
        .await
        .expect("VL6 oneshot");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "VL6 status");

    // ─── VL7. 400 missing X-Group-Id header ──────────────────────────────
    let vl7_id = seed_file(&h, group_id, owner_id, "noxgid-vl7.txt", 1, 1)
        .await
        .expect("VL7 seed file");
    seed_file_version(&h, vl7_id, group_id, owner_id, 1, 3, "text/plain")
        .await
        .expect("VL7 seed v1");

    let resp = h
        .router
        .clone()
        .oneshot(list_versions_req(
            Some(&owner_token),
            &gid_str,
            &vl7_id.to_string(),
            None, // no X-Group-Id
            "",
        ))
        .await
        .expect("VL7 oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "VL7 status");

    // ─── VL8. 200 empty list — file exists, zero file_versions rows ──────
    // Schema requires current_version >= 1, so seed with 1.
    // No file_versions row is inserted, so the query returns an empty list.
    let vl8_id = seed_file(&h, group_id, owner_id, "empty-vl8.txt", 1, 1)
        .await
        .expect("VL8 seed file");

    let resp = h
        .router
        .clone()
        .oneshot(list_versions_req(
            Some(&owner_token),
            &gid_str,
            &vl8_id.to_string(),
            Some(&gid_str),
            "",
        ))
        .await
        .expect("VL8 oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "VL8 status");
    let body = body_json(resp).await;
    let items = body["items"].as_array().expect("VL8 items array");
    assert_eq!(items.len(), 0, "VL8 empty items");
    assert!(body["next_cursor"].is_null(), "VL8 next_cursor null");
}
