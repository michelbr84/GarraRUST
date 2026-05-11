//! Integration tests for `GET /v1/groups/{id}/members` and
//! `GET /v1/groups/{id}/invites` (plan 0097, GAR-574).
//!
//! All scenarios are bundled into ONE `#[tokio::test]` to avoid the
//! sqlx runtime-teardown race (see commit `4f8be37` for the post-mortem).
//!
//! Scenarios:
//!   GM1 — GET /members 200 — owner lists members; at least the owner is present.
//!   GM2 — GET /members 400 — missing X-Group-Id header.
//!   GM3 — GET /members 403 — non-member cannot list members of a different group.
//!   GI1 — GET /invites 200 — owner sees pending invites; token_hash absent.
//!   GI2 — GET /invites 403 — plain member gets 403.
//!   GI3 — GET /invites 200 empty — no pending invites returns empty list.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;
use uuid::Uuid;

use common::fixtures::{seed_member_via_admin, seed_user_with_group};
use common::{Harness, harness_get};

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("failed to collect body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response body is not JSON")
}

fn get_members(token: &str, group_id: &str, x_group_id: Option<&str>) -> Request<Body> {
    let mut req = harness_get(&format!("/v1/groups/{group_id}/members"));
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

fn get_invites(
    token: &str,
    group_id: &str,
    x_group_id: Option<&str>,
    cursor: Option<&str>,
) -> Request<Body> {
    let uri = match cursor {
        Some(c) => format!("/v1/groups/{group_id}/invites?cursor={c}"),
        None => format!("/v1/groups/{group_id}/invites"),
    };
    let mut req = harness_get(&uri);
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

/// Insert a pending invite directly via the admin pool (bypasses rate limiting
/// and Argon2 hashing — for test setup only).
async fn seed_invite_via_admin(
    h: &Harness,
    group_id: Uuid,
    invited_email: &str,
    proposed_role: &str,
    created_by: Uuid,
) -> anyhow::Result<Uuid> {
    let invite_id = Uuid::new_v4();
    // token_hash is a placeholder — the actual value is opaque to the list endpoint.
    sqlx::query(
        "INSERT INTO group_invites \
             (id, group_id, invited_email, proposed_role, token_hash, expires_at, created_by) \
         VALUES ($1, $2, $3, $4, 'test-hash-placeholder', now() + interval '7 days', $5)",
    )
    .bind(invite_id)
    .bind(group_id)
    .bind(invited_email)
    .bind(proposed_role)
    .bind(created_by)
    .execute(&h.admin_pool)
    .await?;
    Ok(invite_id)
}

#[tokio::test]
async fn v1_groups_members_invites_scenarios() {
    let h = Harness::get().await;

    // Shared fixture: one group with an owner, one admin member, one plain member.
    let (_owner_id, owner_group_id, owner_token) =
        seed_user_with_group(&h, "gmi-owner@test.example")
            .await
            .expect("GM: seed owner");
    let (admin_id, admin_token) =
        seed_member_via_admin(&h, owner_group_id, "admin", "gmi-admin@test.example")
            .await
            .expect("GM: seed admin");
    let (_member_id, member_token) =
        seed_member_via_admin(&h, owner_group_id, "member", "gmi-member@test.example")
            .await
            .expect("GM: seed member");

    let group_id_str = owner_group_id.to_string();

    // ─ GM1: GET /members 200 — owner lists members ──────────────────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(get_members(
                &owner_token,
                &group_id_str,
                Some(&group_id_str),
            ))
            .await
            .expect("GM1: oneshot");
        assert_eq!(resp.status(), StatusCode::OK, "GM1: should return 200");
        let v = body_json(resp).await;
        assert!(v["items"].is_array(), "GM1: items should be array");
        let items = v["items"].as_array().unwrap();
        // At minimum the owner + admin + member we seeded are present.
        assert!(
            items.len() >= 3,
            "GM1: at least 3 members expected, got {}",
            items.len()
        );
        // Verify response shape: each item has user_id, role, status, joined_at.
        for item in items {
            assert!(item["user_id"].is_string(), "GM1: user_id must be a string");
            assert!(item["role"].is_string(), "GM1: role must be a string");
            assert!(item["status"].is_string(), "GM1: status must be a string");
            assert!(
                item["joined_at"].is_string(),
                "GM1: joined_at must be a string"
            );
        }
        // next_cursor may be null on first page with few members.
        assert!(
            v["next_cursor"].is_null() || v["next_cursor"].is_string(),
            "GM1: next_cursor must be null or string"
        );
        // Verify no token_hash in any item (not applicable for members, sanity check).
        for item in v["items"].as_array().unwrap() {
            assert!(
                item.get("token_hash").is_none(),
                "GM1: token_hash must never appear"
            );
        }
    }

    // ─ GM1b: admin member can also list members ──────────────────────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(get_members(
                &admin_token,
                &group_id_str,
                Some(&group_id_str),
            ))
            .await
            .expect("GM1b: oneshot");
        assert_eq!(resp.status(), StatusCode::OK, "GM1b: admin should 200");
        // Plain member can too.
        let resp = h
            .router
            .clone()
            .oneshot(get_members(
                &member_token,
                &group_id_str,
                Some(&group_id_str),
            ))
            .await
            .expect("GM1b-member: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "GM1b-member: member should 200"
        );
    }

    // ─ GM2: GET /members 400 — missing X-Group-Id ───────────────────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(get_members(&owner_token, &group_id_str, None))
            .await
            .expect("GM2: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "GM2: missing X-Group-Id should 400"
        );
    }

    // ─ GM3: GET /members 403 — non-member (cross-group isolation) ───────────
    {
        // Create a second group; the owner of group 2 tries to read group 1's members.
        let (_g2_owner_id, g2_group_id, g2_token) =
            seed_user_with_group(&h, "gmi-g2owner@test.example")
                .await
                .expect("GM3: seed g2 owner");
        let g2_id_str = g2_group_id.to_string();

        // X-Group-Id = group1, path = group1, but JWT is for a user who is
        // a member of group2 only — the Principal extractor returns 403.
        let resp = h
            .router
            .clone()
            .oneshot(get_members(&g2_token, &group_id_str, Some(&group_id_str)))
            .await
            .expect("GM3: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "GM3: non-member should get 403"
        );

        // Accessing own group still works for g2 owner.
        let resp = h
            .router
            .clone()
            .oneshot(get_members(&g2_token, &g2_id_str, Some(&g2_id_str)))
            .await
            .expect("GM3b: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "GM3b: g2 owner reads g2 = 200"
        );
        let _ = g2_id_str;
    }

    // ─ GI1: GET /invites 200 — owner sees pending invites ───────────────────
    {
        // Seed a pending invite via admin pool.
        let invite_id = seed_invite_via_admin(
            &h,
            owner_group_id,
            "pending@test.example",
            "member",
            admin_id,
        )
        .await
        .expect("GI1: seed invite");

        let resp = h
            .router
            .clone()
            .oneshot(get_invites(
                &owner_token,
                &group_id_str,
                Some(&group_id_str),
                None,
            ))
            .await
            .expect("GI1: oneshot");
        assert_eq!(resp.status(), StatusCode::OK, "GI1: owner should 200");
        let v = body_json(resp).await;
        assert!(v["items"].is_array(), "GI1: items must be array");
        let items = v["items"].as_array().unwrap();
        assert!(!items.is_empty(), "GI1: should have at least one invite");

        // Verify shape: id, invited_email, proposed_role, expires_at, created_by, created_at present.
        let found = items
            .iter()
            .find(|i| i["id"].as_str() == Some(&invite_id.to_string()));
        assert!(found.is_some(), "GI1: seeded invite must appear in list");
        let inv = found.unwrap();
        assert_eq!(inv["invited_email"], "pending@test.example", "GI1: email");
        assert_eq!(inv["proposed_role"], "member", "GI1: role");
        assert!(
            inv["expires_at"].is_string(),
            "GI1: expires_at must be string"
        );
        assert!(
            inv["created_by"].is_string(),
            "GI1: created_by must be string"
        );
        assert!(
            inv["created_at"].is_string(),
            "GI1: created_at must be string"
        );

        // token_hash must NEVER appear.
        for item in items {
            assert!(
                item.get("token_hash").is_none(),
                "GI1: token_hash must never appear in invite list response"
            );
        }
    }

    // ─ GI2: GET /invites 403 — plain member gets 403 ────────────────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(get_invites(
                &member_token,
                &group_id_str,
                Some(&group_id_str),
                None,
            ))
            .await
            .expect("GI2: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "GI2: plain member should 403 on invites (PII gate)"
        );
    }

    // ─ GI3: GET /invites 200 empty — group with no pending invites ──────────
    {
        // Create a fresh group with no invites.
        let (_g3_owner_id, g3_group_id, g3_token) =
            seed_user_with_group(&h, "gmi-g3owner@test.example")
                .await
                .expect("GI3: seed g3 owner");
        let g3_id_str = g3_group_id.to_string();

        let resp = h
            .router
            .clone()
            .oneshot(get_invites(&g3_token, &g3_id_str, Some(&g3_id_str), None))
            .await
            .expect("GI3: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "GI3: empty invites should 200"
        );
        let v = body_json(resp).await;
        assert_eq!(
            v["items"].as_array().map(|a| a.len()).unwrap_or(1),
            0,
            "GI3: empty group should have 0 invites"
        );
        assert!(
            v["next_cursor"].is_null(),
            "GI3: no next_cursor on empty list"
        );
    }
}
