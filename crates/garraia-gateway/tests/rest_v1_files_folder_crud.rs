// Gated so `cargo clippy --all-targets` without `test-helpers` skips this
// file and doesn't try to compile the `common` harness.
#![cfg(feature = "test-helpers")]
//! Integration tests for folder CRUD endpoints
//! (plan 0091, GAR-561).
//!
//! All scenarios bundled into ONE `#[tokio::test]` function — same pattern
//! as `rest_v1_files_patch.rs`. Splitting historically triggered the sqlx
//! runtime-teardown race documented in plan 0016 M3 commit `4f8be37`.
//!
//! Scenarios covered (10 total):
//!
//! F1.  POST 201 — create top-level folder.
//! F2.  POST 201 — create nested folder (valid parent_id).
//! F3.  POST 403 — path group_id ≠ principal group_id.
//! F4.  POST 400 — name too long (> 500 chars).
//! F5.  PATCH 200 — rename live folder.
//! F6.  PATCH 404 — non-existent folder_id.
//! F7.  PATCH 403 — path group_id ≠ principal group_id.
//! F8.  DELETE 204 — soft-delete live folder.
//! F9.  DELETE 404 — already-deleted folder.
//! F10. DELETE 403 — path group_id ≠ principal group_id.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Method, Request, StatusCode};
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

fn folder_req(
    method: Method,
    token: Option<&str>,
    path_group_id: &str,
    folder_id: Option<&str>,
    x_group_id: Option<&str>,
    body: Option<serde_json::Value>,
) -> Request<Body> {
    let uri = match folder_id {
        Some(fid) => format!("/v1/groups/{path_group_id}/folders/{fid}"),
        None => format!("/v1/groups/{path_group_id}/folders"),
    };
    let body_bytes = body
        .map(|v| serde_json::to_vec(&v).expect("json serialize"))
        .unwrap_or_default();
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body_bytes))
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

/// Insert a live `folders` row directly via admin pool. Returns the folder id.
async fn seed_folder(
    h: &Harness,
    group_id: Uuid,
    created_by: Uuid,
    name: &str,
    parent_id: Option<Uuid>,
) -> anyhow::Result<Uuid> {
    let folder_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO folders (id, group_id, parent_id, name, created_by, created_by_label) \
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

/// Soft-delete a folder via admin pool.
async fn soft_delete_folder(h: &Harness, folder_id: Uuid) -> anyhow::Result<()> {
    sqlx::query("UPDATE folders SET deleted_at = now() WHERE id = $1")
        .bind(folder_id)
        .execute(&h.admin_pool)
        .await?;
    Ok(())
}

#[tokio::test]
async fn v1_folders_crud_scenarios() {
    let h = Harness::get().await;

    let (owner_id, group_id, owner_token) = seed_user_with_group(&h, "owner@folders-slice4.test")
        .await
        .expect("seed owner+group");

    let group_id_str = group_id.to_string();
    let other_group_id = Uuid::new_v4().to_string();

    // ── F1. POST 201 — create top-level folder ────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(folder_req(
            Method::POST,
            Some(&owner_token),
            &group_id_str,
            None,
            Some(&group_id_str),
            Some(serde_json::json!({ "name": "Documents" })),
        ))
        .await
        .expect("F1 request");
    assert_eq!(resp.status(), StatusCode::CREATED, "F1 expected 201");
    let body = body_json(resp).await;
    assert_eq!(body["name"], "Documents", "F1 name");
    assert!(body["id"].is_string(), "F1 id present");
    assert_eq!(body["group_id"], group_id_str, "F1 group_id");
    assert!(body["parent_id"].is_null(), "F1 parent_id null for root");
    let f1_id = body["id"].as_str().unwrap().to_owned();

    // ── F2. POST 201 — create nested folder (valid parent_id) ─────────────
    let resp = h
        .router
        .clone()
        .oneshot(folder_req(
            Method::POST,
            Some(&owner_token),
            &group_id_str,
            None,
            Some(&group_id_str),
            Some(serde_json::json!({ "name": "Q1", "parent_id": f1_id })),
        ))
        .await
        .expect("F2 request");
    assert_eq!(resp.status(), StatusCode::CREATED, "F2 expected 201");
    let body = body_json(resp).await;
    assert_eq!(body["name"], "Q1", "F2 name");
    assert_eq!(body["parent_id"], f1_id, "F2 parent_id");

    // ── F3. POST 403 — path group_id ≠ principal group_id ────────────────
    let resp = h
        .router
        .clone()
        .oneshot(folder_req(
            Method::POST,
            Some(&owner_token),
            &other_group_id,
            None,
            Some(&group_id_str),
            Some(serde_json::json!({ "name": "Forbidden" })),
        ))
        .await
        .expect("F3 request");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "F3 expected 403");

    // ── F4. POST 400 — name too long (> 500 chars) ────────────────────────
    let long_name = "x".repeat(501);
    let resp = h
        .router
        .clone()
        .oneshot(folder_req(
            Method::POST,
            Some(&owner_token),
            &group_id_str,
            None,
            Some(&group_id_str),
            Some(serde_json::json!({ "name": long_name })),
        ))
        .await
        .expect("F4 request");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "F4 expected 400");

    // ── F5. PATCH 200 — rename live folder ────────────────────────────────
    let rename_target = seed_folder(&h, group_id, owner_id, "Old Name", None)
        .await
        .expect("F5 seed folder");
    let resp = h
        .router
        .clone()
        .oneshot(folder_req(
            Method::PATCH,
            Some(&owner_token),
            &group_id_str,
            Some(&rename_target.to_string()),
            Some(&group_id_str),
            Some(serde_json::json!({ "name": "New Name" })),
        ))
        .await
        .expect("F5 request");
    assert_eq!(resp.status(), StatusCode::OK, "F5 expected 200");
    let body = body_json(resp).await;
    assert_eq!(body["name"], "New Name", "F5 name updated");
    assert_eq!(body["id"], rename_target.to_string(), "F5 id matches");

    // ── F6. PATCH 404 — non-existent folder_id ────────────────────────────
    let ghost_id = Uuid::new_v4().to_string();
    let resp = h
        .router
        .clone()
        .oneshot(folder_req(
            Method::PATCH,
            Some(&owner_token),
            &group_id_str,
            Some(&ghost_id),
            Some(&group_id_str),
            Some(serde_json::json!({ "name": "Ghost" })),
        ))
        .await
        .expect("F6 request");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "F6 expected 404");

    // ── F7. PATCH 403 — path group_id ≠ principal group_id ───────────────
    let resp = h
        .router
        .clone()
        .oneshot(folder_req(
            Method::PATCH,
            Some(&owner_token),
            &other_group_id,
            Some(&rename_target.to_string()),
            Some(&group_id_str),
            Some(serde_json::json!({ "name": "Forbidden" })),
        ))
        .await
        .expect("F7 request");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "F7 expected 403");

    // ── F8. DELETE 204 — soft-delete live folder ──────────────────────────
    let delete_target = seed_folder(&h, group_id, owner_id, "To Delete", None)
        .await
        .expect("F8 seed folder");
    let resp = h
        .router
        .clone()
        .oneshot(folder_req(
            Method::DELETE,
            Some(&owner_token),
            &group_id_str,
            Some(&delete_target.to_string()),
            Some(&group_id_str),
            None,
        ))
        .await
        .expect("F8 request");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT, "F8 expected 204");

    // ── F9. DELETE 404 — already-deleted folder ───────────────────────────
    let already_deleted = seed_folder(&h, group_id, owner_id, "Already Gone", None)
        .await
        .expect("F9 seed folder");
    soft_delete_folder(&h, already_deleted)
        .await
        .expect("F9 soft delete");
    let resp = h
        .router
        .clone()
        .oneshot(folder_req(
            Method::DELETE,
            Some(&owner_token),
            &group_id_str,
            Some(&already_deleted.to_string()),
            Some(&group_id_str),
            None,
        ))
        .await
        .expect("F9 request");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "F9 expected 404");

    // ── F10. DELETE 403 — path group_id ≠ principal group_id ─────────────
    let resp = h
        .router
        .clone()
        .oneshot(folder_req(
            Method::DELETE,
            Some(&owner_token),
            &other_group_id,
            Some(&delete_target.to_string()),
            Some(&group_id_str),
            None,
        ))
        .await
        .expect("F10 request");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "F10 expected 403");
}
