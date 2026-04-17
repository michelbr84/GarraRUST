//! Integration tests for `POST /v1/invites/{token}/accept` (plan 0019).
//!
//! Scenarios:
//!   A1. Happy path — owner creates invite, second user accepts → 200.
//!   A2. Double-accept — same token again → 404 (filtered from pending set).
//!   A3. Expired invite → 410.
//!   A4. Invalid token (no match) → 404.
//!   A5. Already a member of the group → 409.
//!   A6. Missing bearer → 401.
//!
//! Bundled into ONE `#[tokio::test]` to avoid the sqlx runtime-teardown
//! race that split functions trigger (see plan 0016 M3 commit `4f8be37`).

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

use common::Harness;
use common::fixtures::{seed_user_with_group, seed_user_without_group};

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("body is JSON")
}

fn req_with_peer(builder: axum::http::request::Builder, body: Body) -> Request<Body> {
    let mut req = builder.body(body).expect("request builder");
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
            "127.0.0.1:1".parse().unwrap(),
        ));
    req
}

fn post_create_group(bearer: &str, body: serde_json::Value) -> Request<Body> {
    let mut req = req_with_peer(
        Request::builder()
            .method("POST")
            .uri("/v1/groups")
            .header("content-type", "application/json"),
        Body::from(body.to_string()),
    );
    req.headers_mut().insert(
        HeaderName::from_static("authorization"),
        HeaderValue::from_str(&format!("Bearer {bearer}")).unwrap(),
    );
    req
}

fn post_invite_create(bearer: &str, group_id: &str, body: serde_json::Value) -> Request<Body> {
    let mut req = req_with_peer(
        Request::builder()
            .method("POST")
            .uri(format!("/v1/groups/{group_id}/invites"))
            .header("content-type", "application/json"),
        Body::from(body.to_string()),
    );
    req.headers_mut().insert(
        HeaderName::from_static("authorization"),
        HeaderValue::from_str(&format!("Bearer {bearer}")).unwrap(),
    );
    req.headers_mut().insert(
        HeaderName::from_static("x-group-id"),
        HeaderValue::from_str(group_id).unwrap(),
    );
    req
}

fn post_accept(bearer: Option<&str>, token: &str) -> Request<Body> {
    let mut req = req_with_peer(
        Request::builder()
            .method("POST")
            .uri(format!("/v1/invites/{token}/accept")),
        Body::empty(),
    );
    if let Some(bearer) = bearer {
        req.headers_mut().insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_str(&format!("Bearer {bearer}")).unwrap(),
        );
    }
    req
}

#[tokio::test]
async fn v1_invites_accept_scenarios() {
    let h = Harness::get().await;

    // Seed: owner (will create a fresh group via API and issue invites).
    let (_owner_seed_id, _owner_seed_group, owner_token) =
        seed_user_with_group(&h, "owner@0019.test")
            .await
            .expect("seed owner");

    // Create a fresh group via the API so this test owns the whole
    // lifecycle of the tenant it operates on.
    let group_id: uuid::Uuid = {
        let resp = h
            .router
            .clone()
            .oneshot(post_create_group(
                &owner_token,
                json!({"name": "Accept Test Group", "type": "team"}),
            ))
            .await
            .expect("create group");
        assert_eq!(resp.status(), StatusCode::CREATED);
        body_json(resp).await["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap()
    };
    let gid = group_id.to_string();

    // Create invite for a fresh email. Owner has MembersManage.
    let invite_token: String = {
        let resp = h
            .router
            .clone()
            .oneshot(post_invite_create(
                &owner_token,
                &gid,
                json!({"email": "joiner@0019.test", "role": "member"}),
            ))
            .await
            .expect("create invite");
        assert_eq!(resp.status(), StatusCode::CREATED, "invite created");
        body_json(resp).await["token"].as_str().unwrap().to_string()
    };

    // Seed: the user who will accept the invite.
    let (joiner_id, _joiner_seed_group, joiner_token) =
        seed_user_with_group(&h, "joiner@0019.test")
            .await
            .expect("seed joiner");

    // ─── A1: happy path — accept invite → 200 ───────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(post_accept(Some(&joiner_token), &invite_token))
            .await
            .expect("A1: oneshot");
        assert_eq!(resp.status(), StatusCode::OK, "A1: accept invite");
        let v = body_json(resp).await;
        assert_eq!(v["group_id"], gid);
        assert_eq!(v["role"], "member");
        assert!(v["invite_id"].is_string());

        // Verify group_members row via admin_pool (fixture-only path,
        // assertions run outside RLS — sanctioned by plan 0016 M3
        // test boundary note).
        let (role,): (String,) = sqlx::query_as(
            "SELECT role::text FROM group_members \
             WHERE group_id = $1 AND user_id = $2 AND status = 'active'",
        )
        .bind(group_id)
        .bind(joiner_id)
        .fetch_one(&h.admin_pool)
        .await
        .expect("A1: member row must exist");
        assert_eq!(role, "member");
    }

    // ─── A2: double-accept → 404 (filtered from pending) ────
    {
        let resp = h
            .router
            .clone()
            .oneshot(post_accept(Some(&joiner_token), &invite_token))
            .await
            .expect("A2: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "A2: already-accepted invite no longer in pending set → 404"
        );
    }

    // ─── A3: expired invite → 410 ───────────────────────────
    {
        // Create a new invite, then force-expire it.
        let expired_token: String = {
            let resp = h
                .router
                .clone()
                .oneshot(post_invite_create(
                    &owner_token,
                    &gid,
                    json!({"email": "expired@0019.test", "role": "guest"}),
                ))
                .await
                .expect("A3: create invite");
            assert_eq!(resp.status(), StatusCode::CREATED);
            let v = body_json(resp).await;
            let invite_id: uuid::Uuid = v["id"].as_str().unwrap().parse().unwrap();

            sqlx::query(
                "UPDATE group_invites SET expires_at = now() - interval '1 hour' \
                 WHERE id = $1",
            )
            .bind(invite_id)
            .execute(&h.admin_pool)
            .await
            .expect("A3: force-expire");

            v["token"].as_str().unwrap().to_string()
        };

        let (_expired_user_id, expired_user_token) =
            seed_user_without_group(&h, "expired-acceptor@0019.test")
                .await
                .expect("A3: seed acceptor");

        let resp = h
            .router
            .clone()
            .oneshot(post_accept(Some(&expired_user_token), &expired_token))
            .await
            .expect("A3: oneshot");
        assert_eq!(resp.status(), StatusCode::GONE, "A3: expired invite → 410");
    }

    // ─── A4: invalid token (no hash match) → 404 ────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(post_accept(Some(&joiner_token), "totally-bogus-token"))
            .await
            .expect("A4: oneshot");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "A4: bad token → 404");
    }

    // ─── A5: already a member → 409 ─────────────────────────
    {
        // Fresh email so create_invite doesn't 409 on duplicate
        // pending invite (migration 011 partial unique index).
        let dupe_token: String = {
            let resp = h
                .router
                .clone()
                .oneshot(post_invite_create(
                    &owner_token,
                    &gid,
                    json!({"email": "dupe-joiner@0019.test", "role": "admin"}),
                ))
                .await
                .expect("A5: create invite");
            assert_eq!(resp.status(), StatusCode::CREATED);
            body_json(resp).await["token"].as_str().unwrap().to_string()
        };

        // Joiner (already member from A1) tries to accept a different
        // invite for the same group. Handler UPDATE succeeds, INSERT
        // on group_members hits PK collision (23505) → 409.
        let resp = h
            .router
            .clone()
            .oneshot(post_accept(Some(&joiner_token), &dupe_token))
            .await
            .expect("A5: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::CONFLICT,
            "A5: already a member → 409"
        );
        let v = body_json(resp).await;
        assert!(
            v["detail"].as_str().unwrap().contains("already a member"),
            "A5: detail mentions membership"
        );

        // Invariant: the dupe invite must remain PENDING — the 409
        // failure rolled back the UPDATE as well. Plan 0019 §5b note.
        let accepted_at: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
            "SELECT accepted_at FROM group_invites \
             WHERE group_id = $1 AND invited_email = 'dupe-joiner@0019.test' \
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(group_id)
        .fetch_one(&h.admin_pool)
        .await
        .expect("A5: dupe invite row present");
        assert!(
            accepted_at.is_none(),
            "A5: dupe invite must stay pending after 409 rollback"
        );
    }

    // ─── A6: missing bearer → 401 ───────────────────────────
    {
        // Create another pending invite so the token is valid but
        // auth is missing — guarantees the 401 comes from the
        // Principal extractor, not from a 404 race.
        let bearerless_token: String = {
            let resp = h
                .router
                .clone()
                .oneshot(post_invite_create(
                    &owner_token,
                    &gid,
                    json!({"email": "nobearer@0019.test", "role": "guest"}),
                ))
                .await
                .expect("A6: create invite");
            assert_eq!(resp.status(), StatusCode::CREATED);
            body_json(resp).await["token"].as_str().unwrap().to_string()
        };

        let resp = h
            .router
            .clone()
            .oneshot(post_accept(None, &bearerless_token))
            .await
            .expect("A6: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "A6: missing bearer → 401"
        );
    }
}
