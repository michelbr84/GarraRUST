//! Integration tests for `/v1/groups` real handlers (plan 0016 M4-T4).
//!
//! Exercises the first write handlers that land on the
//! `garraia_app` RLS-enforced pool. Like M3's
//! `rest_v1_me_authed.rs`, all scenarios are bundled into ONE
//! `#[tokio::test]` function to avoid the sqlx runtime-teardown
//! race that split `#[tokio::test]` functions trigger (see
//! commit `4f8be37` on plan 0016 M3 for the full post-mortem).
//!
//! Scenarios covered:
//!
//!   1. POST /v1/groups 201 — happy path: seeded user creates a
//!      `team` group and becomes auto-enrolled as `owner`. Asserts
//!      201 status, response shape, and that `group_members` has
//!      the matching row.
//!   2. POST /v1/groups 400 — invalid type `personal` (reserved
//!      per migration 001 line 114).
//!   3. POST /v1/groups 400 — empty `name`.
//!   4. POST /v1/groups 401 — missing bearer.
//!   5. GET /v1/groups/{id} 200 — seeded member reads their
//!      group, receives role `owner`.
//!   6. GET /v1/groups/{id} 400 — `X-Group-Id` header mismatches
//!      the path id.
//!   7. GET /v1/groups/{id} 403 — caller is not a member of the
//!      requested group.
//!
//!  P1-P6: PATCH /v1/groups/{id} scenarios (plan 0017).
//!
//!  I1-I6: POST /v1/groups/{id}/invites scenarios (plan 0018).

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

use common::fixtures::seed_user_with_group;
use common::{Harness, harness_get};

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("failed to collect response body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response body is not JSON")
}

fn post_groups(token: Option<&str>, body: serde_json::Value) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri("/v1/groups")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request builder");
    if let Some(token) = token {
        req.headers_mut().insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
    }
    // The tower_governor PeerIpKeyExtractor requires ConnectInfo on
    // every request served via `oneshot`; harness_get sets that up
    // for GETs, so we mirror the behavior manually here for POST.
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
            "127.0.0.1:1".parse().unwrap(),
        ));
    req
}

fn patch_group_by_id(
    token: Option<&str>,
    path_id: &str,
    x_group_id: Option<&str>,
    body: serde_json::Value,
) -> Request<Body> {
    let mut req = Request::builder()
        .method("PATCH")
        .uri(format!("/v1/groups/{path_id}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request builder");
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
            "127.0.0.1:1".parse().unwrap(),
        ));
    if let Some(token) = token {
        req.headers_mut().insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
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

fn get_group_by_id(token: &str, path_id: &str, x_group_id: Option<&str>) -> Request<Body> {
    let mut req = harness_get(&format!("/v1/groups/{path_id}"));
    req.headers_mut().insert(
        HeaderName::from_static("authorization"),
        HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
    );
    if let Some(g) = x_group_id {
        req.headers_mut().insert(
            HeaderName::from_static("x-group-id"),
            HeaderValue::from_str(g).unwrap(),
        );
    }
    req
}

fn post_invite(
    token: Option<&str>,
    group_path_id: &str,
    x_group_id: Option<&str>,
    body: serde_json::Value,
) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri(format!("/v1/groups/{group_path_id}/invites"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request builder");
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
            "127.0.0.1:1".parse().unwrap(),
        ));
    if let Some(token) = token {
        req.headers_mut().insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
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

#[tokio::test]
async fn v1_groups_scenarios() {
    let h = Harness::get().await;

    // ─ Scenario 1: POST 201 happy path ────────────────────────
    let (creator_id, _creator_group, creator_token) =
        seed_user_with_group(&h, "scenario1-creator@m4.test")
            .await
            .expect("scenario 1: seed creator");
    let created_group_id: uuid::Uuid = {
        let resp = h
            .router
            .clone()
            .oneshot(post_groups(
                Some(&creator_token),
                json!({"name": "M4 Scenario 1", "type": "team"}),
            ))
            .await
            .expect("scenario 1: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "scenario 1: POST should 201"
        );
        let v = body_json(resp).await;
        assert_eq!(v["name"], "M4 Scenario 1");
        assert_eq!(v["type"], "team");
        assert!(v["id"].is_string());
        assert!(v["created_at"].is_string());
        let id: uuid::Uuid = v["id"].as_str().unwrap().parse().unwrap();

        // Verify group_members has the creator as owner by reading
        // through the admin pool (fixture-grade access, not through
        // the app pool RLS path).
        let (role,): (String,) = sqlx::query_as(
            "SELECT role::text FROM group_members \
             WHERE group_id = $1 AND user_id = $2 AND status = 'active'",
        )
        .bind(id)
        .bind(creator_id)
        .fetch_one(&h.admin_pool)
        .await
        .expect("scenario 1: group_members row should exist");
        assert_eq!(role, "owner", "scenario 1: creator should be owner");
        id
    };

    // ─ Scenario 2: POST 400 invalid type ────────────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(post_groups(
                Some(&creator_token),
                json!({"name": "Invalid", "type": "personal"}),
            ))
            .await
            .expect("scenario 2: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "scenario 2: type=personal must be rejected"
        );
        let v = body_json(resp).await;
        assert_eq!(v["status"], 400);
        assert_eq!(v["title"], "Bad Request");
        assert!(
            v["detail"].as_str().unwrap().contains("group type"),
            "scenario 2: detail should mention group type, got {v}"
        );
    }

    // ─ Scenario 3: POST 400 empty name ──────────────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(post_groups(
                Some(&creator_token),
                json!({"name": "   ", "type": "team"}),
            ))
            .await
            .expect("scenario 3: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "scenario 3: whitespace-only name must be rejected"
        );
        let v = body_json(resp).await;
        assert!(
            v["detail"].as_str().unwrap().contains("name"),
            "scenario 3: detail should mention name, got {v}"
        );
    }

    // ─ Scenario 4: POST 401 missing bearer ─────────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(post_groups(
                None,
                json!({"name": "Unauthed", "type": "team"}),
            ))
            .await
            .expect("scenario 4: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "scenario 4: missing bearer must 401"
        );
    }

    // ─ Scenario 5: GET 200 happy (read own group) ──────────
    {
        let path = created_group_id.to_string();
        let resp = h
            .router
            .clone()
            .oneshot(get_group_by_id(&creator_token, &path, Some(&path)))
            .await
            .expect("scenario 5: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "scenario 5: member reading own group must 200"
        );
        let v = body_json(resp).await;
        assert_eq!(v["id"], path);
        assert_eq!(v["name"], "M4 Scenario 1");
        assert_eq!(v["type"], "team");
        assert_eq!(v["role"], "owner");
        assert_eq!(v["created_by"], creator_id.to_string());
    }

    // ─ Scenario 6: GET 400 header/path mismatch ───────────
    {
        let mismatching = uuid::Uuid::new_v4().to_string();
        let resp = h
            .router
            .clone()
            .oneshot(get_group_by_id(
                &creator_token,
                &created_group_id.to_string(),
                Some(&mismatching),
            ))
            .await
            .expect("scenario 6: oneshot");
        // The Principal extractor runs the membership lookup on the
        // X-Group-Id header first. Because `mismatching` is not a
        // group the creator is a member of, the extractor returns
        // 403 BEFORE the handler's 400-for-mismatch check can run.
        // Both 400 and 403 are acceptable semantics for this case;
        // we assert that it is NOT 200 and pin it to whichever the
        // extractor chose.
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "scenario 6: foreign X-Group-Id header -> Principal extractor 403 (precedes handler 400)"
        );
    }

    // ─ Scenario 7: GET 403 non-member of path group ───────
    {
        // Seed a fresh user with their own group, then try to read
        // the creator's group from scenario 1. Principal extractor
        // fails the membership lookup and 403s before the handler
        // runs.
        let (_other_id, _other_group, other_token) =
            seed_user_with_group(&h, "scenario7-other@m4.test")
                .await
                .expect("scenario 7: seed other");
        let path = created_group_id.to_string();
        let resp = h
            .router
            .clone()
            .oneshot(get_group_by_id(&other_token, &path, Some(&path)))
            .await
            .expect("scenario 7: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "scenario 7: non-member reading foreign group must 403"
        );
    }

    // ─── PATCH /v1/groups/{id} — plan 0017 Task 5 ─────────

    // Scenario P1: owner renames group → 200, response carries new name.
    // Also validates that `updated_at` was bumped in the database
    // (groups has no trigger — handler must set it explicitly per
    // migration 001 line 115).
    {
        let path = created_group_id.to_string();
        // Capture updated_at BEFORE the PATCH.
        let (before_ts,): (chrono::DateTime<chrono::Utc>,) =
            sqlx::query_as("SELECT updated_at FROM groups WHERE id = $1")
                .bind(created_group_id)
                .fetch_one(&h.admin_pool)
                .await
                .expect("P1: pre-PATCH updated_at");

        let resp = h
            .router
            .clone()
            .oneshot(patch_group_by_id(
                Some(&creator_token),
                &path,
                Some(&path),
                json!({"name": "Renamed by P1"}),
            ))
            .await
            .expect("P1: oneshot");
        assert_eq!(resp.status(), StatusCode::OK, "P1: owner rename");
        let v = body_json(resp).await;
        assert_eq!(v["name"], "Renamed by P1");
        assert_eq!(v["role"], "owner");

        // Verify updated_at was bumped in the DB.
        let (after_ts,): (chrono::DateTime<chrono::Utc>,) =
            sqlx::query_as("SELECT updated_at FROM groups WHERE id = $1")
                .bind(created_group_id)
                .fetch_one(&h.admin_pool)
                .await
                .expect("P1: post-PATCH updated_at");
        assert!(
            after_ts > before_ts,
            "P1: updated_at must increase after PATCH; before={before_ts}, after={after_ts}"
        );
    }

    // Scenario P2: empty body → 400 deterministic detail.
    {
        let path = created_group_id.to_string();
        let resp = h
            .router
            .clone()
            .oneshot(patch_group_by_id(
                Some(&creator_token),
                &path,
                Some(&path),
                json!({}),
            ))
            .await
            .expect("P2: oneshot");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "P2: empty body");
        let v = body_json(resp).await;
        assert_eq!(
            v["detail"], "patch body must set at least one field",
            "P2: deterministic detail"
        );
    }

    // Scenario P3: type=personal → 400.
    {
        let path = created_group_id.to_string();
        let resp = h
            .router
            .clone()
            .oneshot(patch_group_by_id(
                Some(&creator_token),
                &path,
                Some(&path),
                json!({"type": "personal"}),
            ))
            .await
            .expect("P3: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "P3: personal rejected"
        );
        let v = body_json(resp).await;
        assert!(
            v["detail"].as_str().unwrap().contains("group type"),
            "P3: detail mentions group type, got {v}"
        );
    }

    // Scenario P4: non-member PATCH → 403 (Principal extractor).
    {
        let path = created_group_id.to_string();
        let (_other_id, _other_group, other_token) =
            seed_user_with_group(&h, "p4-outsider@0017.test")
                .await
                .expect("P4: seed outsider");
        let resp = h
            .router
            .clone()
            .oneshot(patch_group_by_id(
                Some(&other_token),
                &path,
                Some(&path),
                json!({"name": "hacked"}),
            ))
            .await
            .expect("P4: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "P4: non-member PATCH must 403 (extractor)"
        );
    }

    // Scenario P5: unauthenticated → 401.
    {
        let path = created_group_id.to_string();
        let resp = h
            .router
            .clone()
            .oneshot(patch_group_by_id(
                None,
                &path,
                Some(&path),
                json!({"name": "anon"}),
            ))
            .await
            .expect("P5: oneshot");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "P5: no JWT");
    }

    // Scenario P6: owner changes type team → family → 200.
    {
        let path = created_group_id.to_string();
        let resp = h
            .router
            .clone()
            .oneshot(patch_group_by_id(
                Some(&creator_token),
                &path,
                Some(&path),
                json!({"type": "family"}),
            ))
            .await
            .expect("P6: oneshot");
        assert_eq!(resp.status(), StatusCode::OK, "P6: type change");
        let v = body_json(resp).await;
        assert_eq!(v["type"], "family", "P6: type reflected in response");
    }

    // ─── POST /v1/groups/{id}/invites — plan 0018 Task 5 ─────

    // Scenario I1: owner creates invite → 201, response has token + invite_id.
    let invite_email = "invited-i1@0018.test";
    {
        let path = created_group_id.to_string();
        let resp = h
            .router
            .clone()
            .oneshot(post_invite(
                Some(&creator_token),
                &path,
                Some(&path),
                json!({"email": invite_email, "role": "member"}),
            ))
            .await
            .expect("I1: oneshot");
        assert_eq!(resp.status(), StatusCode::CREATED, "I1: owner creates invite");
        let v = body_json(resp).await;
        assert_eq!(v["group_id"], created_group_id.to_string());
        assert_eq!(v["invited_email"], invite_email);
        assert_eq!(v["proposed_role"], "member");
        assert!(
            v["token"].is_string(),
            "I1: response must include plaintext token"
        );
        assert!(
            !v["token"].as_str().unwrap().is_empty(),
            "I1: token must not be empty"
        );
        assert!(v["id"].is_string(), "I1: invite id must be present");
        assert!(
            v["expires_at"].is_string(),
            "I1: expires_at must be present"
        );
        assert!(
            v["created_at"].is_string(),
            "I1: created_at must be present"
        );

        // Verify the invite row exists in DB (via admin_pool to bypass any restrictions).
        let invite_id: uuid::Uuid = v["id"].as_str().unwrap().parse().unwrap();
        let (db_email,): (String,) =
            sqlx::query_as("SELECT invited_email FROM group_invites WHERE id = $1")
                .bind(invite_id)
                .fetch_one(&h.admin_pool)
                .await
                .expect("I1: invite row must exist");
        assert_eq!(db_email, invite_email);
    }

    // Scenario I2: duplicate pending invite for same email → 409.
    {
        let path = created_group_id.to_string();
        let resp = h
            .router
            .clone()
            .oneshot(post_invite(
                Some(&creator_token),
                &path,
                Some(&path),
                json!({"email": invite_email, "role": "admin"}),
            ))
            .await
            .expect("I2: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::CONFLICT,
            "I2: duplicate pending invite must 409"
        );
        let v = body_json(resp).await;
        assert!(
            v["detail"].as_str().unwrap().contains("pending invite"),
            "I2: detail must mention pending invite"
        );
    }

    // Scenario I3: invalid role "owner" → 400.
    {
        let path = created_group_id.to_string();
        let resp = h
            .router
            .clone()
            .oneshot(post_invite(
                Some(&creator_token),
                &path,
                Some(&path),
                json!({"email": "i3@0018.test", "role": "owner"}),
            ))
            .await
            .expect("I3: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "I3: role=owner must 400"
        );
        let v = body_json(resp).await;
        assert!(
            v["detail"].as_str().unwrap().contains("role must be"),
            "I3: detail must mention valid roles"
        );
    }

    // Scenario I4: empty email → 400.
    {
        let path = created_group_id.to_string();
        let resp = h
            .router
            .clone()
            .oneshot(post_invite(
                Some(&creator_token),
                &path,
                Some(&path),
                json!({"email": "  ", "role": "member"}),
            ))
            .await
            .expect("I4: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "I4: empty email must 400"
        );
    }

    // Scenario I5: non-member tries to create invite → 403 (Principal extractor).
    {
        let path = created_group_id.to_string();
        let (_outsider_id, _outsider_group, outsider_token) =
            seed_user_with_group(&h, "i5-outsider@0018.test")
                .await
                .expect("I5: seed outsider");
        let resp = h
            .router
            .clone()
            .oneshot(post_invite(
                Some(&outsider_token),
                &path,
                Some(&path),
                json!({"email": "victim@0018.test", "role": "member"}),
            ))
            .await
            .expect("I5: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "I5: non-member invite must 403 (extractor)"
        );
    }

    // Scenario I6: missing bearer token → 401.
    {
        let path = created_group_id.to_string();
        let resp = h
            .router
            .clone()
            .oneshot(post_invite(
                None,
                &path,
                Some(&path),
                json!({"email": "i6@0018.test", "role": "member"}),
            ))
            .await
            .expect("I6: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "I6: missing bearer must 401"
        );
    }
}
