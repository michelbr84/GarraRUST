//! Integration tests for `GET /v1/groups/{group_id}/task-lists/{list_id}` (plan 0110 T4, GAR-599).
//!
//! All scenarios bundled into ONE `#[tokio::test]` — same pattern as
//! `rest_v1_tasks.rs`. Splitting triggers the sqlx runtime-teardown race.
//!
//! Scenarios (5):
//!   TL-GET-1. Owner 200 — happy path, all response fields present.
//!   TL-GET-2. Member 200 — second user is a member, also gets 200.
//!   TL-GET-3. Cross-group 404 — list belongs to another group.
//!   TL-GET-4. Archived task list 404 — archived_at IS NOT NULL → 404.
//!   TL-GET-5. Missing X-Group-Id → 400.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

use common::Harness;
use common::fixtures::{seed_member_via_admin, seed_user_with_group};

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("collect response body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response body is not JSON")
}

fn auth_req(
    method: &str,
    uri: &str,
    token: Option<&str>,
    x_group_id: Option<&str>,
    body: Option<serde_json::Value>,
) -> Request<Body> {
    let body_bytes = match body {
        Some(v) => Body::from(v.to_string()),
        None => Body::empty(),
    };
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(body_bytes)
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

fn post_task_list(
    token: Option<&str>,
    x_group_id: Option<&str>,
    path_group_id: &str,
    body: serde_json::Value,
) -> Request<Body> {
    auth_req(
        "POST",
        &format!("/v1/groups/{path_group_id}/task-lists"),
        token,
        x_group_id,
        Some(body),
    )
}

fn get_task_list(
    token: Option<&str>,
    x_group_id: Option<&str>,
    path_group_id: &str,
    list_id: &str,
) -> Request<Body> {
    auth_req(
        "GET",
        &format!("/v1/groups/{path_group_id}/task-lists/{list_id}"),
        token,
        x_group_id,
        None,
    )
}

#[cfg(feature = "test-helpers")]
#[tokio::test]
async fn rest_v1_task_list_get_scenarios() {
    let h = Harness::get().await;

    // Seed two independent groups for cross-tenant isolation.
    let (_owner_id, group_id, owner_token) = seed_user_with_group(&h, "alice@task-list-get.test")
        .await
        .expect("seed alice+group1");
    let (_, group2_id, owner2_token) = seed_user_with_group(&h, "bob@task-list-get.test")
        .await
        .expect("seed bob+group2");

    let gid = group_id.to_string();
    let g2id = group2_id.to_string();

    // ── Seed: create a task list in group1 ──────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(post_task_list(
            Some(&owner_token),
            Some(&gid),
            &gid,
            json!({ "name": "Sprint Backlog", "type": "list", "description": "Initial backlog" }),
        ))
        .await
        .expect("seed: create task list");
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "seed: task list created"
    );
    let created = body_json(resp).await;
    let list_id = created["id"].as_str().expect("seed: list id").to_string();

    // ── TL-GET-1: owner 200 — all fields correct ─────────────────────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(get_task_list(
                Some(&owner_token),
                Some(&gid),
                &gid,
                &list_id,
            ))
            .await
            .expect("TL-GET-1 oneshot");
        assert_eq!(resp.status(), StatusCode::OK, "TL-GET-1 status");
        let v = body_json(resp).await;
        assert_eq!(v["id"], list_id, "TL-GET-1 id");
        assert_eq!(v["group_id"], gid, "TL-GET-1 group_id");
        assert_eq!(v["name"], "Sprint Backlog", "TL-GET-1 name");
        assert_eq!(v["type"], "list", "TL-GET-1 type");
        assert_eq!(v["description"], "Initial backlog", "TL-GET-1 description");
        assert!(v.get("created_at").is_some(), "TL-GET-1 created_at present");
        assert!(v.get("updated_at").is_some(), "TL-GET-1 updated_at present");
        assert!(
            v["archived_at"].is_null() || v.get("archived_at").is_none(),
            "TL-GET-1 archived_at null/absent"
        );
    }

    // ── TL-GET-2: member 200 ─────────────────────────────────────────────────
    {
        let (_member_id, member_token) =
            seed_member_via_admin(&h, group_id, "member@task-list-get.test", "member")
                .await
                .expect("TL-GET-2 seed member");
        let resp = h
            .router
            .clone()
            .oneshot(get_task_list(
                Some(&member_token),
                Some(&gid),
                &gid,
                &list_id,
            ))
            .await
            .expect("TL-GET-2 oneshot");
        assert_eq!(resp.status(), StatusCode::OK, "TL-GET-2 member gets 200");
        let v = body_json(resp).await;
        assert_eq!(v["id"], list_id, "TL-GET-2 id matches");
    }

    // ── TL-GET-3: cross-group 404 — list_id from group1 requested via group2 ─
    {
        let resp = h
            .router
            .clone()
            .oneshot(get_task_list(
                Some(&owner2_token),
                Some(&g2id),
                &g2id,
                &list_id,
            ))
            .await
            .expect("TL-GET-3 oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "TL-GET-3 cross-group must return 404"
        );
    }

    // ── TL-GET-4: archived task list 404 ────────────────────────────────────
    {
        // Archive the list via DELETE (archive endpoint).
        let del_resp = h
            .router
            .clone()
            .oneshot(auth_req(
                "DELETE",
                &format!("/v1/groups/{gid}/task-lists/{list_id}"),
                Some(&owner_token),
                Some(&gid),
                None,
            ))
            .await
            .expect("TL-GET-4 archive list");
        assert_eq!(
            del_resp.status(),
            StatusCode::NO_CONTENT,
            "TL-GET-4 archive step"
        );

        let resp = h
            .router
            .clone()
            .oneshot(get_task_list(
                Some(&owner_token),
                Some(&gid),
                &gid,
                &list_id,
            ))
            .await
            .expect("TL-GET-4 oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "TL-GET-4 archived list must return 404"
        );
    }

    // ── TL-GET-5: missing X-Group-Id → 400 ───────────────────────────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(get_task_list(
                Some(&owner_token),
                None, // no X-Group-Id
                &gid,
                &list_id,
            ))
            .await
            .expect("TL-GET-5 oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "TL-GET-5 missing X-Group-Id must return 400"
        );
    }
}
