//! Integration tests for `PATCH /v1/groups/{group_id}/files/{file_id}` (plan 0089, GAR-557).
//!
//! All scenarios bundled into ONE `#[tokio::test]` — same pattern as other
//! rest_v1_* tests (sqlx runtime-teardown race documented in plan 0016 M3).
//!
//! Scenarios:
//!   L1.  PATCH 200 — happy path: valid name returns updated FileSummary.
//!   L2.  PATCH 200 — name with leading/trailing whitespace is trimmed.
//!   L3.  PATCH 400 — empty name (after trim) → 400.
//!   L4.  PATCH 400 — name too long (501 chars) → 400.
//!   L5.  PATCH 400 — name with '/' → 400.
//!   L6.  PATCH 403 — path group_id ≠ principal group_id → 403.
//!   L7.  PATCH 404 — file belongs to a different group (cross-tenant) → 404.
//!   L8.  PATCH 404 — soft-deleted file → 404.
//!   L9.  PATCH 401 — missing bearer token → 401.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

use common::Harness;
use common::fixtures::seed_user_with_group;

// ─── Helpers ─────────────────────────────────────────────────────────────────

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("body is not JSON")
}

fn patch_req(
    uri: &str,
    token: Option<&str>,
    x_group_id: Option<&str>,
    body: serde_json::Value,
) -> Request<Body> {
    let mut r = Request::builder()
        .method("PATCH")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request builder");
    r.extensions_mut()
        .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
            "127.0.0.1:1".parse().unwrap(),
        ));
    if let Some(t) = token {
        r.headers_mut().insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_str(&format!("Bearer {t}")).unwrap(),
        );
    }
    if let Some(g) = x_group_id {
        r.headers_mut().insert(
            HeaderName::from_static("x-group-id"),
            HeaderValue::from_str(g).unwrap(),
        );
    }
    r
}

/// Insert a minimal file row directly via the admin (superuser) pool.
/// Returns the generated file UUID.
async fn seed_file(
    h: &Harness,
    group_id: Uuid,
    user_id: Uuid,
    name: &str,
    deleted: bool,
) -> Uuid {
    let file_id: (Uuid,) = sqlx::query_as(
        "INSERT INTO files \
         (group_id, name, size_bytes, mime_type, created_by, created_by_label, \
          current_version, total_versions, deleted_at) \
         VALUES ($1, $2, 0, 'text/plain', $3, 'Test User', 1, 1, \
                 CASE WHEN $4 THEN now() ELSE NULL END) \
         RETURNING id",
    )
    .bind(group_id)
    .bind(name)
    .bind(user_id)
    .bind(deleted)
    .fetch_one(&h.admin_pool)
    .await
    .expect("seed_file INSERT");
    file_id.0
}

// ─── Test ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn files_patch_scenarios() {
    let h = Harness::get().await;

    // Two isolated groups.
    let (user_id, group_id, token) =
        seed_user_with_group(&h, "owner@files-patch.test")
            .await
            .expect("seed owner");
    let (_user2_id, group2_id, _token2) =
        seed_user_with_group(&h, "owner2@files-patch.test")
            .await
            .expect("seed owner2");

    // Seed files for group 1.
    let file_id = seed_file(&h, group_id, user_id, "original.txt", false).await;
    let deleted_file_id = seed_file(&h, group_id, user_id, "deleted.txt", true).await;

    // Seed a file in group 2 to test cross-tenant isolation.
    let group2_file_id = seed_file(&h, group2_id, _user2_id, "group2.txt", false).await;

    let gid = group_id.to_string();
    let g2id = group2_id.to_string();

    // ── L1: 200 — happy path ──────────────────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(patch_req(
            &format!("/v1/groups/{gid}/files/{file_id}"),
            Some(&token),
            Some(&gid),
            json!({ "name": "renamed.pdf" }),
        ))
        .await
        .expect("oneshot L1");
    assert_eq!(resp.status(), StatusCode::OK, "L1 status");
    let body = body_json(resp).await;
    assert_eq!(body["name"], "renamed.pdf", "L1 name updated");
    assert_eq!(body["id"], file_id.to_string(), "L1 id unchanged");
    assert!(body["updated_at"].as_str().is_some(), "L1 updated_at present");

    // ── L2: 200 — name with whitespace is trimmed ─────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(patch_req(
            &format!("/v1/groups/{gid}/files/{file_id}"),
            Some(&token),
            Some(&gid),
            json!({ "name": "  trimmed name.txt  " }),
        ))
        .await
        .expect("oneshot L2");
    assert_eq!(resp.status(), StatusCode::OK, "L2 status");
    let body = body_json(resp).await;
    assert_eq!(body["name"], "trimmed name.txt", "L2 name trimmed");

    // ── L3: 400 — empty name after trim ──────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(patch_req(
            &format!("/v1/groups/{gid}/files/{file_id}"),
            Some(&token),
            Some(&gid),
            json!({ "name": "   " }),
        ))
        .await
        .expect("oneshot L3");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "L3 empty name → 400");

    // ── L4: 400 — name too long (501 chars) ──────────────────────────────
    let long_name: String = "a".repeat(501);
    let resp = h
        .router
        .clone()
        .oneshot(patch_req(
            &format!("/v1/groups/{gid}/files/{file_id}"),
            Some(&token),
            Some(&gid),
            json!({ "name": long_name }),
        ))
        .await
        .expect("oneshot L4");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "L4 too long → 400");

    // ── L5: 400 — name contains '/' ──────────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(patch_req(
            &format!("/v1/groups/{gid}/files/{file_id}"),
            Some(&token),
            Some(&gid),
            json!({ "name": "dir/traversal.txt" }),
        ))
        .await
        .expect("oneshot L5");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "L5 slash → 400");

    // ── L6: 403 — path group_id ≠ principal group_id ─────────────────────
    // token is for group1 but path uses group2
    let resp = h
        .router
        .clone()
        .oneshot(patch_req(
            &format!("/v1/groups/{g2id}/files/{file_id}"),
            Some(&token),
            Some(&gid),
            json!({ "name": "should-fail.txt" }),
        ))
        .await
        .expect("oneshot L6");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "L6 group mismatch → 403");

    // ── L7: 404 — cross-tenant file (group2 file, group1 token) ──────────
    let resp = h
        .router
        .clone()
        .oneshot(patch_req(
            &format!("/v1/groups/{gid}/files/{group2_file_id}"),
            Some(&token),
            Some(&gid),
            json!({ "name": "steal.txt" }),
        ))
        .await
        .expect("oneshot L7");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "L7 cross-tenant → 404");

    // ── L8: 404 — soft-deleted file ───────────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(patch_req(
            &format!("/v1/groups/{gid}/files/{deleted_file_id}"),
            Some(&token),
            Some(&gid),
            json!({ "name": "restore-via-rename.txt" }),
        ))
        .await
        .expect("oneshot L8");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "L8 deleted file → 404");

    // ── L9: 401 — missing bearer token ───────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(patch_req(
            &format!("/v1/groups/{gid}/files/{file_id}"),
            None,
            Some(&gid),
            json!({ "name": "no-auth.txt" }),
        ))
        .await
        .expect("oneshot L9");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "L9 no token → 401");

    // ── Verify audit event was written for L1 ────────────────────────────
    let audit: Vec<(String,)> = sqlx::query_as(
        "SELECT action FROM audit_events \
         WHERE resource_type = 'files' AND resource_id = $1 AND action = 'file.renamed' \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(file_id.to_string())
    .fetch_all(&h.admin_pool)
    .await
    .expect("audit query");
    assert_eq!(audit.len(), 1, "audit event for file.renamed written");
    assert_eq!(audit[0].0, "file.renamed");
}
