//! Integration tests for `GET /v1/groups` — list user's groups (plan 0105, GAR-580).
//!
//! All scenarios are bundled into ONE `#[tokio::test]` to avoid the
//! sqlx runtime-teardown race (see commit `4f8be37` for the post-mortem).
//!
//! Scenarios:
//!   GL1 — 200 — owner of one group sees it in the list.
//!   GL2 — 401 — missing JWT returns 401.
//!   GL3 — 200 empty — user with no memberships returns empty list.
//!   GL4 — 200 paginated — user in 3 groups, limit=2 returns cursor; page 2 correct.
//!   GL5 — 200 role filter — ?role=owner filters to only owned groups.
//!   GL6 — cross-group — user A cannot see user B's groups.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;
use uuid::Uuid;

use common::fixtures::seed_user_with_group;
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

fn get_groups(token: Option<&str>, query: &str) -> Request<Body> {
    let uri = if query.is_empty() {
        "/v1/groups".to_string()
    } else {
        format!("/v1/groups?{query}")
    };
    let mut req = harness_get(&uri);
    if let Some(t) = token {
        req.headers_mut().insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_str(&format!("Bearer {t}")).unwrap(),
        );
    }
    req
}

/// Seed a user who belongs to multiple groups.
///
/// Returns `(user_id, [group_id_1, group_id_2, group_id_3], token)`.
/// Groups are inserted in UUID order to match the keyset cursor ordering.
async fn seed_user_with_multiple_groups(
    h: &Harness,
    email: &str,
) -> anyhow::Result<(Uuid, Vec<Uuid>, String)> {
    let user_id = Uuid::new_v4();
    // Generate fixed group IDs so we can predict sort order (ASC by group_id UUID).
    let mut group_ids: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
    group_ids.sort(); // ensure deterministic keyset order

    let mut tx = h.admin_pool.begin().await?;

    sqlx::query(
        "INSERT INTO users (id, email, display_name, status) VALUES ($1, $2, $3, 'active')",
    )
    .bind(user_id)
    .bind(email)
    .bind(format!("Multi {email}"))
    .execute(&mut *tx)
    .await?;

    for (i, &gid) in group_ids.iter().enumerate() {
        sqlx::query("INSERT INTO groups (id, name, type, created_by) VALUES ($1, $2, 'team', $3)")
            .bind(gid)
            .bind(format!("Group {i} for {email}"))
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        sqlx::query(
            "INSERT INTO group_members (group_id, user_id, role, status) \
             VALUES ($1, $2, 'owner', 'active')",
        )
        .bind(gid)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    let token = h.jwt.issue_access_for_test(user_id);
    Ok((user_id, group_ids, token))
}

/// Seed a plain user with no group memberships.
async fn seed_user_no_groups(h: &Harness, email: &str) -> anyhow::Result<(Uuid, String)> {
    let user_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (id, email, display_name, status) VALUES ($1, $2, $3, 'active')",
    )
    .bind(user_id)
    .bind(email)
    .bind(format!("NoGroup {email}"))
    .execute(&h.admin_pool)
    .await?;
    let token = h.jwt.issue_access_for_test(user_id);
    Ok((user_id, token))
}

#[tokio::test]
async fn test_list_groups_scenarios() {
    let h = Harness::get().await;
    let router = h.router.clone();

    // ── Setup ────────────────────────────────────────────────────────────────

    // GL1 + GL5: single-group user
    let (_u1, g1_id, tok1) = seed_user_with_group(&h, "gl1-owner@example.com")
        .await
        .expect("GL1 fixture");

    // GL3: user with no groups
    let (_u3, tok3) = seed_user_no_groups(&h, "gl3-nogroups@example.com")
        .await
        .expect("GL3 fixture");

    // GL4: user in 3 groups (sorted by group_id ASC)
    let (_u4, g4_ids, tok4) = seed_user_with_multiple_groups(&h, "gl4-multi@example.com")
        .await
        .expect("GL4 fixture");

    // GL6: separate user to test cross-group isolation
    let (_u6, _g6_id, tok6) = seed_user_with_group(&h, "gl6-other@example.com")
        .await
        .expect("GL6 fixture");

    // ── GL1: 200 — owner sees their group ────────────────────────────────────
    {
        let resp = router
            .clone()
            .oneshot(get_groups(Some(&tok1), ""))
            .await
            .expect("GL1 request");
        assert_eq!(resp.status(), StatusCode::OK, "GL1: expected 200");
        let body = body_json(resp).await;
        let items = body["items"].as_array().expect("GL1: items must be array");
        assert!(!items.is_empty(), "GL1: items must not be empty");
        let found = items.iter().any(|it| {
            it["id"]
                .as_str()
                .map(|s| s == g1_id.to_string())
                .unwrap_or(false)
        });
        assert!(found, "GL1: group {g1_id} must appear in items");
    }

    // ── GL2: 401 — no JWT ────────────────────────────────────────────────────
    {
        let resp = router
            .clone()
            .oneshot(get_groups(None, ""))
            .await
            .expect("GL2 request");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "GL2: expected 401");
    }

    // ── GL3: 200 empty — user with no groups ─────────────────────────────────
    {
        let resp = router
            .clone()
            .oneshot(get_groups(Some(&tok3), ""))
            .await
            .expect("GL3 request");
        assert_eq!(resp.status(), StatusCode::OK, "GL3: expected 200");
        let body = body_json(resp).await;
        let items = body["items"].as_array().expect("GL3: items must be array");
        assert!(
            items.is_empty(),
            "GL3: items must be empty for user with no groups"
        );
        assert!(
            body["next_cursor"].is_null(),
            "GL3: next_cursor must be null when no items"
        );
    }

    // ── GL4: pagination — limit=2 returns cursor, page 2 has remainder ───────
    {
        // Page 1: limit=2
        let resp1 = router
            .clone()
            .oneshot(get_groups(Some(&tok4), "limit=2"))
            .await
            .expect("GL4 page1 request");
        assert_eq!(resp1.status(), StatusCode::OK, "GL4 page1: expected 200");
        let body1 = body_json(resp1).await;
        let items1 = body1["items"].as_array().expect("GL4 p1: items array");
        assert_eq!(items1.len(), 2, "GL4 page1: must have exactly 2 items");
        let next_cursor = body1["next_cursor"]
            .as_str()
            .expect("GL4 page1: must have next_cursor");

        // Page 2: use cursor
        let resp2 = router
            .clone()
            .oneshot(get_groups(
                Some(&tok4),
                &format!("limit=2&cursor={next_cursor}"),
            ))
            .await
            .expect("GL4 page2 request");
        assert_eq!(resp2.status(), StatusCode::OK, "GL4 page2: expected 200");
        let body2 = body_json(resp2).await;
        let items2 = body2["items"].as_array().expect("GL4 p2: items array");
        assert_eq!(items2.len(), 1, "GL4 page2: must have 1 remaining item");
        assert!(
            body2["next_cursor"].is_null(),
            "GL4 page2: next_cursor must be null on last page"
        );

        // All 3 groups must appear across both pages, in group_id ASC order.
        let page1_ids: Vec<&str> = items1.iter().map(|it| it["id"].as_str().unwrap()).collect();
        let page2_ids: Vec<&str> = items2.iter().map(|it| it["id"].as_str().unwrap()).collect();
        let all_ids: Vec<&str> = [page1_ids, page2_ids].concat();
        let expected_ids: Vec<String> = g4_ids.iter().map(|u| u.to_string()).collect();
        assert_eq!(
            all_ids,
            expected_ids.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
            "GL4: pages must cover all 3 groups in group_id ASC order"
        );
    }

    // ── GL5: role filter — ?role=owner ───────────────────────────────────────
    {
        let resp = router
            .clone()
            .oneshot(get_groups(Some(&tok1), "role=owner"))
            .await
            .expect("GL5 request");
        assert_eq!(resp.status(), StatusCode::OK, "GL5: expected 200");
        let body = body_json(resp).await;
        let items = body["items"].as_array().expect("GL5: items array");
        let found = items.iter().any(|it| {
            it["id"]
                .as_str()
                .map(|s| s == g1_id.to_string())
                .unwrap_or(false)
        });
        assert!(found, "GL5: owner group must appear with role=owner filter");
        // All returned items must carry role=owner.
        for it in items {
            assert_eq!(
                it["role"].as_str().unwrap_or(""),
                "owner",
                "GL5: role filter must return only owner rows"
            );
        }
    }

    // ── GL6: cross-group isolation — tok6 sees only their groups ─────────────
    {
        let resp = router
            .clone()
            .oneshot(get_groups(Some(&tok6), ""))
            .await
            .expect("GL6 request");
        assert_eq!(resp.status(), StatusCode::OK, "GL6: expected 200");
        let body = body_json(resp).await;
        let items = body["items"].as_array().expect("GL6: items array");
        // tok1's group must NOT appear in tok6's results.
        let contains_g1 = items.iter().any(|it| {
            it["id"]
                .as_str()
                .map(|s| s == g1_id.to_string())
                .unwrap_or(false)
        });
        assert!(
            !contains_g1,
            "GL6: user B must not see user A's group {g1_id}"
        );
    }
}
