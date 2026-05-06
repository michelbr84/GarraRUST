//! Integration tests for `GET /v1/groups/{group_id}/audit` (plan 0070, GAR-522).
//!
//! All scenarios in ONE `#[tokio::test]` — same pattern as `rest_v1_memory.rs`.
//! Splitting into multiple `#[tokio::test]`s triggers the sqlx runtime-teardown
//! race documented in plan 0016 M3 commit `4f8be37`.
//!
//! Scenarios:
//!   A1. GET 200 — happy path: owner retrieves audit events.
//!   A2. GET 200 — cursor pagination: limit=2 on 3 events, second page via cursor.
//!   A3. GET 200 — action filter: only events matching `?action=` are returned.
//!   A4. GET 404 — cross-group: owner of group A requests group B's audit.
//!   A5. GET 403 — member role: member of own group is denied (ExportGroup = owner only).
//!   A6. GET 401 — no JWT: unauthenticated request rejected.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

use common::Harness;
use common::fixtures::{seed_member_via_admin, seed_user_with_group};

// ─── Helpers ─────────────────────────────────────────────────────────────────

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("failed to collect response body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response body is not JSON")
}

fn get_audit(
    token: Option<&str>,
    group_id: &str,
    cursor: Option<&str>,
    limit: Option<u32>,
    action: Option<&str>,
    resource_type: Option<&str>,
) -> Request<Body> {
    let mut query = String::new();
    let mut sep = '?';
    let mut push_param = |k: &str, v: &str| {
        query.push(sep);
        query.push_str(k);
        query.push('=');
        query.push_str(v);
        sep = '&';
    };
    if let Some(c) = cursor {
        push_param("cursor", c);
    }
    if let Some(l) = limit {
        push_param("limit", &l.to_string());
    }
    if let Some(a) = action {
        push_param("action", a);
    }
    if let Some(r) = resource_type {
        push_param("resource_type", r);
    }

    let mut req = Request::builder()
        .method("GET")
        .uri(format!("/v1/groups/{group_id}/audit{query}"))
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
    req.headers_mut().insert(
        HeaderName::from_static("x-group-id"),
        HeaderValue::from_str(group_id).unwrap(),
    );
    req
}

/// Seed `n` audit events for `group_id` via the superuser pool (bypasses RLS).
/// Events are inserted with `created_at = now() - (n - i) * interval '1 second'`
/// so they arrive oldest-first in time but query newest-first (`ORDER BY created_at
/// DESC`). Returns the vector of inserted IDs in creation order (oldest first).
async fn seed_audit_events(
    h: &Harness,
    group_id: Uuid,
    actor_user_id: Uuid,
    action: &str,
    resource_type: &str,
    n: u32,
) -> Vec<Uuid> {
    let mut ids = Vec::new();
    for i in 0..n {
        let id = Uuid::new_v4();
        let offset_secs = (n - i) as i64;
        sqlx::query(
            "INSERT INTO audit_events \
             (id, group_id, actor_user_id, actor_label, action, resource_type, \
              resource_id, metadata, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now() - ($9 || ' seconds')::interval)",
        )
        .bind(id)
        .bind(group_id)
        .bind(actor_user_id)
        .bind("Test Actor")
        .bind(action)
        .bind(resource_type)
        .bind(id.to_string())
        .bind(json!({"seeded": true}))
        .bind(offset_secs.to_string())
        .execute(&h.admin_pool)
        .await
        .expect("seed_audit_events insert");
        ids.push(id);
    }
    ids
}

// ─── Test ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn audit_api_scenarios() {
    let h = Harness::get().await;

    // ── Seed: owner user + group ──────────────────────────────────────────────
    let (owner_id, group_id, owner_token) = seed_user_with_group(&h, "audit-owner@test.local")
        .await
        .unwrap();

    // ── Seed: member in same group ────────────────────────────────────────────
    let (_member_id, member_token) =
        seed_member_via_admin(&h, group_id, "member", "audit-member@test.local")
            .await
            .unwrap();

    // ── Seed: a completely separate group (for cross-group test) ──────────────
    let (_other_id, other_group_id, other_token) =
        seed_user_with_group(&h, "audit-other@test.local")
            .await
            .unwrap();

    // ── Seed audit events for the owner's group ───────────────────────────────
    // Seed 3 events with action "tasks.created" for pagination/filter tests.
    let event_ids = seed_audit_events(&h, group_id, owner_id, "tasks.created", "tasks", 3).await;

    // Seed 1 event with a different action for the action-filter test.
    seed_audit_events(&h, group_id, owner_id, "member.joined", "group_members", 1).await;

    // ─── A1: happy path — owner retrieves audit events ────────────────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(get_audit(
                Some(&owner_token),
                &group_id.to_string(),
                None,
                None,
                None,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "A1: expected 200");
        let body = body_json(resp).await;
        let items = body["items"].as_array().unwrap();
        // At least the 4 events we seeded.
        assert!(
            items.len() >= 4,
            "A1: expected at least 4 items, got {}",
            items.len()
        );
        // Each item has the expected fields.
        let first = &items[0];
        assert!(first["id"].is_string(), "A1: item missing id");
        assert!(first["action"].is_string(), "A1: item missing action");
        assert!(
            first["resource_type"].is_string(),
            "A1: item missing resource_type"
        );
        assert!(
            first["created_at"].is_string(),
            "A1: item missing created_at"
        );
    }

    // ─── A2: cursor pagination ────────────────────────────────────────────────
    // Seed 3 "tasks.created" events (already done above). Query with limit=2.
    {
        let resp = h
            .router
            .clone()
            .oneshot(get_audit(
                Some(&owner_token),
                &group_id.to_string(),
                None,
                Some(2),
                Some("tasks.created"),
                Some("tasks"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "A2 page1: expected 200");
        let body = body_json(resp).await;
        let items = body["items"].as_array().unwrap();
        assert_eq!(items.len(), 2, "A2: first page should have 2 items");
        let next_cursor = body["next_cursor"]
            .as_str()
            .expect("A2: next_cursor must be set");

        // Second page via cursor.
        let resp2 = h
            .router
            .clone()
            .oneshot(get_audit(
                Some(&owner_token),
                &group_id.to_string(),
                Some(next_cursor),
                Some(2),
                Some("tasks.created"),
                Some("tasks"),
            ))
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::OK, "A2 page2: expected 200");
        let body2 = body_json(resp2).await;
        let items2 = body2["items"].as_array().unwrap();
        assert_eq!(
            items2.len(),
            1,
            "A2: second page should have 1 item (the oldest)"
        );
        // The oldest event_id (index 0 = oldest) should appear on page 2.
        let oldest_id = event_ids[0].to_string();
        assert_eq!(
            items2[0]["id"].as_str().unwrap(),
            oldest_id,
            "A2: second page item should be the oldest seeded event"
        );
        // No more pages.
        assert!(
            body2["next_cursor"].is_null(),
            "A2: second page should have no next_cursor"
        );
    }

    // ─── A3: action filter ────────────────────────────────────────────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(get_audit(
                Some(&owner_token),
                &group_id.to_string(),
                None,
                None,
                Some("member.joined"),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "A3: expected 200");
        let body = body_json(resp).await;
        let items = body["items"].as_array().unwrap();
        assert!(
            !items.is_empty(),
            "A3: expected at least 1 member.joined event"
        );
        for item in items {
            assert_eq!(
                item["action"].as_str().unwrap(),
                "member.joined",
                "A3: all items must match the action filter"
            );
        }
    }

    // ─── A4: cross-group — owner of groupA requests groupB's audit → 404 ─────
    {
        let resp = h
            .router
            .clone()
            .oneshot(get_audit(
                Some(&other_token),
                &group_id.to_string(), // other user requests group_id (not their group)
                None,
                None,
                None,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "A4: expected 404 for cross-group"
        );
        let _ = other_group_id; // used to ensure seed ran
    }

    // ─── A5: member role — 403 ───────────────────────────────────────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(get_audit(
                Some(&member_token),
                &group_id.to_string(),
                None,
                None,
                None,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "A5: expected 403 for member"
        );
    }

    // ─── A6: no JWT — 401 ────────────────────────────────────────────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(get_audit(
                None,
                &group_id.to_string(),
                None,
                None,
                None,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "A6: expected 401 for no JWT"
        );
    }
}
