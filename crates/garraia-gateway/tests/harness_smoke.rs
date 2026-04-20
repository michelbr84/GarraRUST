//! Smoke test for the gateway integration harness (plan 0016 M2-T4).
//!
//! Validates the whole stack end-to-end over a real testcontainer
//! pgvector/pg16 + migrations 001..012 + typed pools + prebuilt
//! Router, by doing one `GET /v1/openapi.json` oneshot call and
//! asserting a 200 with a parseable OpenAPI body.
//!
//! Plan 0021 adds a schema-level assertion here — the
//! `group_members_single_owner_idx` predicate must include
//! `status = 'active'` (migration 012). Keeping the assertion in the
//! smoke test file (rather than a new schema_smoke.rs) avoids
//! standing up a second harness just for one query.

mod common;

use axum::http::StatusCode;
use http_body_util::BodyExt;
use tower::ServiceExt;

use common::{Harness, harness_get};

#[tokio::test]
async fn harness_boots_and_router_responds_to_openapi_json() {
    let h = Harness::get().await;

    let resp = h
        .router
        .clone()
        .oneshot(harness_get("/v1/openapi.json"))
        .await
        .expect("oneshot should succeed");

    let status = resp.status();
    let body = resp
        .into_body()
        .collect()
        .await
        .expect("failed to collect openapi.json body bytes")
        .to_bytes();
    if status != StatusCode::OK {
        let body_str = String::from_utf8_lossy(&body);
        panic!("openapi.json expected 200, got {status}. body: {body_str}");
    }
    let v: serde_json::Value = serde_json::from_slice(&body).expect("openapi body is JSON");
    assert_eq!(
        v["info"]["title"], "GarraIA REST /v1",
        "unexpected OpenAPI info.title — router wiring regression?"
    );
    assert_eq!(v["info"]["version"], "0.1.0");
    // Proves `GET /v1/me` is wired under full state (not 503 stub).
    assert!(
        v["paths"]["/v1/me"]["get"].is_object(),
        "/v1/me must be listed in the OpenAPI paths"
    );
}

/// Plan 0021 T1 — schema assertion: `group_members_single_owner_idx`
/// predicate must filter `status = 'active'` after migration 012.
///
/// Before migration 012 the predicate was `WHERE role = 'owner'`
/// alone, which meant soft-deleted owner rows still occupied the
/// single-owner slot. Migration 012 amends the predicate so the
/// DB constraint matches the app-layer last-owner invariant.
///
/// Uses `admin_pool` because `pg_indexes` is a catalog view — visible
/// regardless of RLS, but requires read access that `garraia_app`
/// may not have on `pg_indexes` rows for non-public schemas. The
/// harness's admin pool is the consistent choice for schema
/// introspection (same pattern as fixture setup).
#[tokio::test]
async fn migration_012_single_owner_idx_predicate_filters_active() {
    let h = Harness::get().await;

    let (indexdef,): (String,) = sqlx::query_as(
        "SELECT indexdef FROM pg_indexes \
         WHERE indexname = 'group_members_single_owner_idx'",
    )
    .fetch_one(&h.admin_pool)
    .await
    .expect("group_members_single_owner_idx must exist after migration 012");

    assert!(
        indexdef.contains("status = 'active'"),
        "plan 0021 migration 012 must amend the predicate to include \
         `AND status = 'active'`; got: {indexdef}"
    );
    assert!(
        indexdef.contains("role = 'owner'"),
        "predicate must still include `role = 'owner'`; got: {indexdef}"
    );
}
