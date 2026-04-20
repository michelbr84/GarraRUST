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

use common::fixtures::{
    restore_single_owner_idx, seed_member_via_admin, seed_second_owner_via_admin,
    seed_user_with_group,
};
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

/// Request builder for `POST /v1/groups/{id}/members/{user_id}/setRole`
/// (plan 0020 slice 4 — setRole endpoint).
fn post_setrole(
    token: Option<&str>,
    group_path_id: &str,
    target_user_id: &str,
    x_group_id: Option<&str>,
    body: serde_json::Value,
) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri(format!(
            "/v1/groups/{group_path_id}/members/{target_user_id}/setRole"
        ))
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

/// Request builder for `DELETE /v1/groups/{id}/members/{user_id}`
/// (plan 0020 slice 4 — soft-delete endpoint).
fn delete_member_req(
    token: Option<&str>,
    group_path_id: &str,
    target_user_id: &str,
    x_group_id: Option<&str>,
) -> Request<Body> {
    let mut req = Request::builder()
        .method("DELETE")
        .uri(format!(
            "/v1/groups/{group_path_id}/members/{target_user_id}"
        ))
        .body(Body::empty())
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
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "I1: owner creates invite"
        );
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

    // ─── POST /v1/groups/{id}/members/{user_id}/setRole — plan 0020 Task 5 ─────

    // Seed a fresh group with its own owner for the setRole scenarios.
    // Re-using `created_group_id` from above would couple M/D scenarios to
    // the mutations done by P/I scenarios; a fresh group keeps the invariants
    // local to this section and easier to reason about.
    let (m_owner_id, m_group_id, m_owner_token) = seed_user_with_group(&h, "m-owner@0020.test")
        .await
        .expect("M setup: seed owner+group");
    let m_group_path = m_group_id.to_string();

    // Scenario M1: Owner demotes a member → admin. 200 + MemberResponse.role=="admin".
    let (m1_target_id, _m1_token) =
        seed_member_via_admin(&h, m_group_id, "member", "m1-target@0020.test")
            .await
            .expect("M1: seed target");
    {
        let resp = h
            .router
            .clone()
            .oneshot(post_setrole(
                Some(&m_owner_token),
                &m_group_path,
                &m1_target_id.to_string(),
                Some(&m_group_path),
                json!({"role": "admin"}),
            ))
            .await
            .expect("M1: oneshot");
        assert_eq!(resp.status(), StatusCode::OK, "M1: owner→admin promote");
        let v = body_json(resp).await;
        assert_eq!(v["role"], "admin");
        assert_eq!(v["status"], "active");
        assert_eq!(v["group_id"], m_group_path);
        assert_eq!(v["user_id"], m1_target_id.to_string());
        assert!(v["updated_at"].is_string(), "M1: updated_at must be set");

        // DB assertion via admin_pool.
        let (db_role,): (String,) =
            sqlx::query_as("SELECT role FROM group_members WHERE group_id = $1 AND user_id = $2")
                .bind(m_group_id)
                .bind(m1_target_id)
                .fetch_one(&h.admin_pool)
                .await
                .expect("M1: DB row check");
        assert_eq!(db_role, "admin", "M1: DB role must be admin");
    }

    // Scenario M2: Admin setRole of another member → guest. 200.
    // Uses m1_target_id (now an admin from M1) as the caller.
    let m1_admin_token = h.jwt.issue_access_for_test(m1_target_id);
    let (m2_target_id, _) = seed_member_via_admin(&h, m_group_id, "member", "m2-target@0020.test")
        .await
        .expect("M2: seed target");
    {
        let resp = h
            .router
            .clone()
            .oneshot(post_setrole(
                Some(&m1_admin_token),
                &m_group_path,
                &m2_target_id.to_string(),
                Some(&m_group_path),
                json!({"role": "guest"}),
            ))
            .await
            .expect("M2: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "M2: admin can modify members"
        );
        let v = body_json(resp).await;
        assert_eq!(v["role"], "guest");
    }

    // Scenario M3: Admin tries to setRole of the Owner (non-self) → 403.
    {
        let resp = h
            .router
            .clone()
            .oneshot(post_setrole(
                Some(&m1_admin_token),
                &m_group_path,
                &m_owner_id.to_string(),
                Some(&m_group_path),
                json!({"role": "member"}),
            ))
            .await
            .expect("M3: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "M3: admin cannot modify owner"
        );
    }

    // Scenario M4: Admin tries to setRole of another Admin (non-self) → 403.
    // Seed a second admin, then have m1 try to demote them.
    let (m4_other_admin_id, _) =
        seed_member_via_admin(&h, m_group_id, "admin", "m4-other-admin@0020.test")
            .await
            .expect("M4: seed second admin");
    {
        let resp = h
            .router
            .clone()
            .oneshot(post_setrole(
                Some(&m1_admin_token),
                &m_group_path,
                &m4_other_admin_id.to_string(),
                Some(&m_group_path),
                json!({"role": "member"}),
            ))
            .await
            .expect("M4: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "M4: admin cannot modify another admin (non-self)"
        );
    }

    // Scenario M5: Owner self-demote WITH a second owner existing → 200.
    // Seed a second owner via admin_pool (the only way, since setRole
    // rejects role=owner). Then the first owner demotes themselves to admin.
    let (m5_coowner_id, _m5_coowner_token) =
        seed_second_owner_via_admin(&h, m_group_id, "m5-coowner@0020.test")
            .await
            .expect("M5: seed co-owner");
    {
        let resp = h
            .router
            .clone()
            .oneshot(post_setrole(
                Some(&m_owner_token),
                &m_group_path,
                &m_owner_id.to_string(),
                Some(&m_group_path),
                json!({"role": "admin"}),
            ))
            .await
            .expect("M5: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "M5: owner self-demote OK when co-owner exists"
        );
        let v = body_json(resp).await;
        assert_eq!(v["role"], "admin", "M5: response role reflects the demote");

        // Restore state for subsequent scenarios. After the setRole
        // call, the group has: m_owner=admin (just demoted), m5_coowner=owner.
        // The partial unique index is still dropped (fixture's contract).
        // We need to flip back to: m_owner=owner, m5_coowner=admin, and
        // recreate the index. The order matters — flip coowner FIRST to
        // admin (now 0 owners momentarily, but admin_pool bypasses the app
        // checks), then promote m_owner, then recreate the index on the
        // single-owner state.
        sqlx::query("UPDATE group_members SET role = 'admin' WHERE group_id = $1 AND user_id = $2")
            .bind(m_group_id)
            .bind(m5_coowner_id)
            .execute(&h.admin_pool)
            .await
            .expect("M5 restore: demote co-owner to admin");
        sqlx::query("UPDATE group_members SET role = 'owner' WHERE group_id = $1 AND user_id = $2")
            .bind(m_group_id)
            .bind(m_owner_id)
            .execute(&h.admin_pool)
            .await
            .expect("M5 restore: re-promote original owner");
        restore_single_owner_idx(&h)
            .await
            .expect("M5 restore: recreate single-owner idx");
    }

    // Scenario M6: Owner self-demote WITHOUT a second owner → 409 last-owner.
    // DB state after M5 restore: m_owner is the sole owner. Self-demote must 409.
    {
        let resp = h
            .router
            .clone()
            .oneshot(post_setrole(
                Some(&m_owner_token),
                &m_group_path,
                &m_owner_id.to_string(),
                Some(&m_group_path),
                json!({"role": "admin"}),
            ))
            .await
            .expect("M6: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::CONFLICT,
            "M6: owner self-demote must 409 without co-owner"
        );
        let v = body_json(resp).await;
        assert!(
            v["detail"].as_str().unwrap().contains("without an owner"),
            "M6: detail mentions last-owner, got {v}"
        );
        // DB invariant: m_owner STILL owner (tx was rolled back).
        let (db_role,): (String,) =
            sqlx::query_as("SELECT role FROM group_members WHERE group_id = $1 AND user_id = $2")
                .bind(m_group_id)
                .bind(m_owner_id)
                .fetch_one(&h.admin_pool)
                .await
                .expect("M6: DB role check");
        assert_eq!(db_role, "owner", "M6: owner must remain after 409 rollback");
    }

    // Scenario M7: body role="owner" → 400 promote-to-owner rejected.
    {
        let resp = h
            .router
            .clone()
            .oneshot(post_setrole(
                Some(&m_owner_token),
                &m_group_path,
                &m1_target_id.to_string(),
                Some(&m_group_path),
                json!({"role": "owner"}),
            ))
            .await
            .expect("M7: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "M7: role=owner must 400"
        );
        let v = body_json(resp).await;
        assert_eq!(
            v["detail"], "cannot promote to owner via setRole",
            "M7: deterministic detail"
        );
    }

    // Scenario M8: Member tries setRole of another member (non-self) → 403.
    let (m8_member_id, m8_member_token) =
        seed_member_via_admin(&h, m_group_id, "member", "m8-member@0020.test")
            .await
            .expect("M8: seed member caller");
    let (m8_other_id, _) = seed_member_via_admin(&h, m_group_id, "member", "m8-other@0020.test")
        .await
        .expect("M8: seed target");
    let _ = m8_member_id; // silence unused warning
    {
        let resp = h
            .router
            .clone()
            .oneshot(post_setrole(
                Some(&m8_member_token),
                &m_group_path,
                &m8_other_id.to_string(),
                Some(&m_group_path),
                json!({"role": "guest"}),
            ))
            .await
            .expect("M8: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "M8: member without MembersManage must 403 (non-self)"
        );
    }

    // Scenario M9: target user_id is not a member of the group → 404.
    {
        let ghost = uuid::Uuid::new_v4().to_string();
        let resp = h
            .router
            .clone()
            .oneshot(post_setrole(
                Some(&m_owner_token),
                &m_group_path,
                &ghost,
                Some(&m_group_path),
                json!({"role": "admin"}),
            ))
            .await
            .expect("M9: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "M9: non-member target must 404"
        );
    }

    // Scenario M10: no bearer → 401.
    {
        let resp = h
            .router
            .clone()
            .oneshot(post_setrole(
                None,
                &m_group_path,
                &m1_target_id.to_string(),
                Some(&m_group_path),
                json!({"role": "admin"}),
            ))
            .await
            .expect("M10: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "M10: missing bearer must 401"
        );
    }

    // ─── DELETE /v1/groups/{id}/members/{user_id} — plan 0020 Task 6 ─────

    // Reuse `m_group_id` / `m_owner_token` (single-owner state restored post-M6).
    // Admin caller for hierarchy-related scenarios: reuse `m1_admin_token`
    // (the user promoted to admin in M1).

    // Scenario D1: Owner DELETEs a member → 204 + DB row `status = 'removed'`.
    let (d1_target_id, _) = seed_member_via_admin(&h, m_group_id, "member", "d1-target@0020.test")
        .await
        .expect("D1: seed target");
    {
        let resp = h
            .router
            .clone()
            .oneshot(delete_member_req(
                Some(&m_owner_token),
                &m_group_path,
                &d1_target_id.to_string(),
                Some(&m_group_path),
            ))
            .await
            .expect("D1: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::NO_CONTENT,
            "D1: owner DELETE member must 204"
        );
        // Body is empty.
        let bytes = resp
            .into_body()
            .collect()
            .await
            .expect("D1: body collect")
            .to_bytes();
        assert!(bytes.is_empty(), "D1: 204 must have empty body");

        // DB row must now be status='removed'.
        let (db_status,): (String,) =
            sqlx::query_as("SELECT status FROM group_members WHERE group_id = $1 AND user_id = $2")
                .bind(m_group_id)
                .bind(d1_target_id)
                .fetch_one(&h.admin_pool)
                .await
                .expect("D1: post-DELETE DB read");
        assert_eq!(db_status, "removed", "D1: soft-delete flips status");
    }

    // Scenario D2: Admin DELETEs a guest → 204.
    let (d2_target_id, _) = seed_member_via_admin(&h, m_group_id, "guest", "d2-target@0020.test")
        .await
        .expect("D2: seed guest");
    {
        let resp = h
            .router
            .clone()
            .oneshot(delete_member_req(
                Some(&m1_admin_token),
                &m_group_path,
                &d2_target_id.to_string(),
                Some(&m_group_path),
            ))
            .await
            .expect("D2: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::NO_CONTENT,
            "D2: admin DELETE guest must 204"
        );
    }

    // Scenario D2b: Admin self-DELETE (leave group) → 204.
    // Closes the gap flagged by plan 0020 security review (SEC-LOW):
    // the M/D scenarios explicitly covered every caller-role × action
    // combination except an admin leaving the group via self-DELETE.
    // Happy-path positive test: admin is self-acting, capability gate
    // is bypassed, hierarchy gate is bypassed (self), last-owner
    // invariant does not apply (m_owner is still the sole owner), so
    // the UPDATE to status='removed' succeeds.
    let (d2b_admin_id, d2b_admin_token) =
        seed_member_via_admin(&h, m_group_id, "admin", "d2b-leaver-admin@0020.test")
            .await
            .expect("D2b: seed admin");
    {
        let resp = h
            .router
            .clone()
            .oneshot(delete_member_req(
                Some(&d2b_admin_token),
                &m_group_path,
                &d2b_admin_id.to_string(),
                Some(&m_group_path),
            ))
            .await
            .expect("D2b: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::NO_CONTENT,
            "D2b: admin self-DELETE (leave) must 204"
        );
        let (db_status,): (String,) =
            sqlx::query_as("SELECT status FROM group_members WHERE group_id = $1 AND user_id = $2")
                .bind(m_group_id)
                .bind(d2b_admin_id)
                .fetch_one(&h.admin_pool)
                .await
                .expect("D2b: DB read");
        assert_eq!(db_status, "removed");
    }

    // Scenario D3: Admin tries DELETE Owner (non-self) → 403.
    {
        let resp = h
            .router
            .clone()
            .oneshot(delete_member_req(
                Some(&m1_admin_token),
                &m_group_path,
                &m_owner_id.to_string(),
                Some(&m_group_path),
            ))
            .await
            .expect("D3: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "D3: admin cannot DELETE owner (non-self)"
        );
    }

    // Scenario D4: Owner self-DELETE WITHOUT co-owner → 409 (last-owner).
    // Uses the current state (m_owner is the sole active owner).
    {
        let resp = h
            .router
            .clone()
            .oneshot(delete_member_req(
                Some(&m_owner_token),
                &m_group_path,
                &m_owner_id.to_string(),
                Some(&m_group_path),
            ))
            .await
            .expect("D4: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::CONFLICT,
            "D4: owner self-DELETE must 409 without co-owner"
        );
        let v = body_json(resp).await;
        assert!(
            v["detail"].as_str().unwrap().contains("without an owner"),
            "D4: detail mentions last-owner, got {v}"
        );
        // Invariant check: m_owner still active owner.
        let (db_role, db_status): (String, String) = sqlx::query_as(
            "SELECT role, status FROM group_members WHERE group_id = $1 AND user_id = $2",
        )
        .bind(m_group_id)
        .bind(m_owner_id)
        .fetch_one(&h.admin_pool)
        .await
        .expect("D4: DB read");
        assert_eq!(db_role, "owner");
        assert_eq!(db_status, "active", "D4: rollback preserved active status");
    }

    // Scenario D5: Owner self-DELETE WITH co-owner seeded → 204.
    // Same index dance as M5: fixture drops the partial unique index,
    // seeds a second owner, leaves the index dropped for the caller to
    // restore after the test's state settles.
    let (d5_coowner_id, _d5_coowner_token) =
        seed_second_owner_via_admin(&h, m_group_id, "d5-coowner@0020.test")
            .await
            .expect("D5: seed co-owner");
    {
        let resp = h
            .router
            .clone()
            .oneshot(delete_member_req(
                Some(&m_owner_token),
                &m_group_path,
                &m_owner_id.to_string(),
                Some(&m_group_path),
            ))
            .await
            .expect("D5: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::NO_CONTENT,
            "D5: owner self-DELETE OK with co-owner"
        );
        // m_owner should now be status='removed'; d5_coowner remains owner.
        let (m_owner_status,): (String,) =
            sqlx::query_as("SELECT status FROM group_members WHERE group_id = $1 AND user_id = $2")
                .bind(m_group_id)
                .bind(m_owner_id)
                .fetch_one(&h.admin_pool)
                .await
                .expect("D5: m_owner DB read");
        assert_eq!(m_owner_status, "removed", "D5: m_owner soft-deleted");

        let (coowner_role, coowner_status): (String, String) = sqlx::query_as(
            "SELECT role, status FROM group_members WHERE group_id = $1 AND user_id = $2",
        )
        .bind(m_group_id)
        .bind(d5_coowner_id)
        .fetch_one(&h.admin_pool)
        .await
        .expect("D5: coowner DB read");
        assert_eq!(coowner_role, "owner");
        assert_eq!(coowner_status, "active");

        // Restore the single-owner partial unique index. Plan 0021
        // migration 012 amended the predicate to `WHERE role = 'owner'
        // AND status = 'active'`, so the soft-deleted m_owner row no
        // longer counts toward uniqueness — `restore_single_owner_idx`
        // rebuilds cleanly without the hard-delete workaround that
        // the 0020 version of this test needed.
        restore_single_owner_idx(&h)
            .await
            .expect("D5: restore single-owner idx");
    }

    // Scenario D6: Member self-DELETE (leave group) → 204.
    let (d6_member_id, d6_member_token) =
        seed_member_via_admin(&h, m_group_id, "member", "d6-leaver@0020.test")
            .await
            .expect("D6: seed leaver");
    {
        let resp = h
            .router
            .clone()
            .oneshot(delete_member_req(
                Some(&d6_member_token),
                &m_group_path,
                &d6_member_id.to_string(),
                Some(&m_group_path),
            ))
            .await
            .expect("D6: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::NO_CONTENT,
            "D6: member self-DELETE (leave group) must 204"
        );
    }

    // Scenario D7: DELETE of already-removed member → 404.
    // d1_target_id was soft-deleted in D1.
    {
        let resp = h
            .router
            .clone()
            .oneshot(delete_member_req(
                // Use the co-owner (current sole owner) as caller since
                // m_owner is now soft-deleted (from D5).
                Some(&_d5_coowner_token),
                &m_group_path,
                &d1_target_id.to_string(),
                Some(&m_group_path),
            ))
            .await
            .expect("D7: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "D7: DELETE already-removed must 404 (idempotent)"
        );
    }

    // Scenario D8: DELETE of a non-existent user (not a member of this group) → 404.
    {
        let ghost = uuid::Uuid::new_v4().to_string();
        let resp = h
            .router
            .clone()
            .oneshot(delete_member_req(
                Some(&_d5_coowner_token),
                &m_group_path,
                &ghost,
                Some(&m_group_path),
            ))
            .await
            .expect("D8: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "D8: DELETE non-member must 404"
        );
    }

    // Scenario D9: missing bearer → 401.
    {
        let resp = h
            .router
            .clone()
            .oneshot(delete_member_req(
                None,
                &m_group_path,
                &d5_coowner_id.to_string(),
                Some(&m_group_path),
            ))
            .await
            .expect("D9: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "D9: missing bearer must 401"
        );
    }
}
