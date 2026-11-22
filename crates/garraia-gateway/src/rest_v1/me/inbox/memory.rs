//! `GET /v1/me/memory` — inbox of the caller's personal memory items.

// Request-path (#1569): panic aqui e controlavel por quem manda a requisicao.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use axum::Json;
use axum::extract::{Query, State};
use chrono::{DateTime, Utc};
use garraia_auth::Principal;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::rest_v1::RestV1FullState;
use crate::rest_v1::problem::RestError;

// ─── GET /v1/me/memory ───────────────────────────────────────────────────────

/// Query parameters for `GET /v1/me/memory`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListMyMemoryQuery {
    /// Keyset cursor — `id` of the last item on the previous page.
    /// Returns items created earlier than this one (exclusive). Omit for first page.
    pub after: Option<Uuid>,
    /// Maximum items per page. Clamped to `[1, 100]`; default `50`.
    pub limit: Option<i64>,
    /// Optional kind filter. Accepted: `fact`, `preference`, `note`,
    /// `reminder`, `rule`, `profile`. Unknown values are rejected with 400.
    pub kind: Option<String>,
}

impl ListMyMemoryQuery {
    fn validate_kind(k: &str) -> bool {
        matches!(
            k,
            "fact" | "preference" | "note" | "reminder" | "rule" | "profile"
        )
    }
}

/// One memory entry in the `GET /v1/me/memory` response.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyMemorySummary {
    pub id: Uuid,
    /// Semantic kind: `fact`, `preference`, `note`, `reminder`, `rule`, or `profile`.
    pub kind: String,
    /// First 200 characters of the memory content (preview only — avoids bulk PII exposure).
    pub content_preview: String,
    /// UTC timestamp when this item was pinned. `None` if not pinned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned_at: Option<DateTime<Utc>>,
    /// UTC expiry timestamp. `None` if this item never expires.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Response body for `GET /v1/me/memory`.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyMemoryResponse {
    pub items: Vec<MyMemorySummary>,
    /// `id` of the last item in this page. Pass as `?after=<uuid>` to fetch the
    /// next (older) page. `None` when the end has been reached.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Uuid>,
}

/// `GET /v1/me/memory` — inbox of the caller's personal memory items.
///
/// Returns up to `limit` (default 50, max 100) non-deleted, non-expired personal
/// memory items (scope_type='user') created by the caller, ordered by
/// `(created_at DESC, id DESC)`.
///
/// Cursor-based pagination via `?after=<last_item_id>`. Optional
/// `?kind=<fact|preference|note|reminder|rule|profile>` filter narrows results.
///
/// RLS is enforced via `SET LOCAL app.current_user_id` (branch 2 of the
/// `memory_items_group_or_self` dual policy in migration 007). `app.current_group_id`
/// is also set to satisfy the FORCE RLS protocol even though personal memories
/// have `group_id IS NULL` and do not match branch 1.
#[utoipa::path(
    get,
    path = "/v1/me/memory",
    params(ListMyMemoryQuery),
    responses(
        (status = 200, description = "List of personal memory items, newest first.", body = MyMemoryResponse),
        (status = 400, description = "Validation error (invalid kind value).", body = crate::rest_v1::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = crate::rest_v1::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_my_memory(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Query(params): Query<ListMyMemoryQuery>,
) -> Result<Json<MyMemoryResponse>, RestError> {
    if let Some(ref k) = params.kind
        && !ListMyMemoryQuery::validate_kind(k)
    {
        return Err(RestError::BadRequest(
            "kind must be one of: fact, preference, note, reminder, rule, profile".into(),
        ));
    }

    let limit = params.limit.map(|l| l.min(100)).unwrap_or(50).max(1);

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // SET LOCAL app.current_user_id — required by branch 2 of the dual RLS policy
    // (memory_items_group_or_self: group_id IS NULL AND created_by = app.current_user_id).
    sqlx::query("SELECT set_config('app.current_user_id', $1, true)")
        .bind(principal.user_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // SET LOCAL app.current_group_id — FORCE RLS protocol requires both GUCs to be set.
    // Personal memories have group_id IS NULL so branch 1 (group match) never fires;
    // using nil UUID is safe and avoids an extra param on this caller-only endpoint.
    sqlx::query("SELECT set_config('app.current_group_id', $1, true)")
        .bind(Uuid::nil().to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // Columns: id, kind, content_preview (LEFT 200), pinned_at, ttl_expires_at, created_at
    type MemRow = (
        Uuid,
        String,
        String,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
        DateTime<Utc>,
    );

    let rows: Vec<MemRow> = match (params.after, params.kind.as_deref()) {
        (None, None) => sqlx::query_as(
            "SELECT id, kind, LEFT(content, 200), pinned_at, ttl_expires_at, created_at \
             FROM memory_items \
             WHERE created_by = $1 \
               AND scope_type = 'user' \
               AND deleted_at IS NULL \
               AND (ttl_expires_at IS NULL OR ttl_expires_at > now()) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $2",
        )
        .bind(principal.user_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        (None, Some(kind)) => sqlx::query_as(
            "SELECT id, kind, LEFT(content, 200), pinned_at, ttl_expires_at, created_at \
             FROM memory_items \
             WHERE created_by = $1 \
               AND scope_type = 'user' \
               AND kind = $2 \
               AND deleted_at IS NULL \
               AND (ttl_expires_at IS NULL OR ttl_expires_at > now()) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $3",
        )
        .bind(principal.user_id)
        .bind(kind)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        (Some(after_id), None) => sqlx::query_as(
            "SELECT id, kind, LEFT(content, 200), pinned_at, ttl_expires_at, created_at \
             FROM memory_items \
             WHERE created_by = $1 \
               AND scope_type = 'user' \
               AND deleted_at IS NULL \
               AND (ttl_expires_at IS NULL OR ttl_expires_at > now()) \
               AND (created_at, id) < ( \
                   SELECT m2.created_at, m2.id FROM memory_items m2 \
                   WHERE m2.id = $2 \
                     AND m2.created_by = $1 \
                     AND m2.scope_type = 'user' \
               ) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $3",
        )
        .bind(principal.user_id)
        .bind(after_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        (Some(after_id), Some(kind)) => sqlx::query_as(
            "SELECT id, kind, LEFT(content, 200), pinned_at, ttl_expires_at, created_at \
             FROM memory_items \
             WHERE created_by = $1 \
               AND scope_type = 'user' \
               AND kind = $2 \
               AND deleted_at IS NULL \
               AND (ttl_expires_at IS NULL OR ttl_expires_at > now()) \
               AND (created_at, id) < ( \
                   SELECT m2.created_at, m2.id FROM memory_items m2 \
                   WHERE m2.id = $3 \
                     AND m2.created_by = $1 \
                     AND m2.scope_type = 'user' \
               ) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $4",
        )
        .bind(principal.user_id)
        .bind(kind)
        .bind(after_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let next_cursor = if rows.len() as i64 == limit {
        rows.last().map(|(id, ..)| *id)
    } else {
        None
    };

    let items = rows
        .into_iter()
        .map(
            |(id, kind, content_preview, pinned_at, ttl_expires_at, created_at)| MyMemorySummary {
                id,
                kind,
                content_preview,
                pinned_at,
                ttl_expires_at,
                created_at,
            },
        )
        .collect();

    Ok(Json(MyMemoryResponse { items, next_cursor }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── MyMemoryResponse / MyMemorySummary serialization ─────────────────────

    #[test]
    fn my_memory_response_empty_no_cursor() {
        let resp = MyMemoryResponse {
            items: vec![],
            next_cursor: None,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 0);
        assert!(
            v.get("next_cursor").is_none(),
            "absent cursor must be omitted"
        );
    }

    #[test]
    fn my_memory_response_with_cursor() {
        let cursor = Uuid::parse_str("ffffffff-0000-0000-0000-000000000007").unwrap();
        let resp = MyMemoryResponse {
            items: vec![],
            next_cursor: Some(cursor),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["next_cursor"], "ffffffff-0000-0000-0000-000000000007");
    }

    #[test]
    fn my_memory_summary_serializes_all_fields() {
        let id = Uuid::parse_str("aaaaaaaa-0000-0000-0000-000000000001").unwrap();
        let pinned = chrono::DateTime::from_timestamp(500_000, 0).unwrap();
        let expires = chrono::DateTime::from_timestamp(9_999_999, 0).unwrap();
        let summary = MyMemorySummary {
            id,
            kind: "fact".into(),
            content_preview: "Alice prefers dark mode".into(),
            pinned_at: Some(pinned),
            ttl_expires_at: Some(expires),
            created_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert_eq!(v["id"], "aaaaaaaa-0000-0000-0000-000000000001");
        assert_eq!(v["kind"], "fact");
        assert_eq!(v["content_preview"], "Alice prefers dark mode");
        assert!(
            v.get("pinned_at").is_some(),
            "present pinned_at must be serialized"
        );
        assert!(
            v.get("ttl_expires_at").is_some(),
            "present ttl_expires_at must be serialized"
        );
    }

    #[test]
    fn my_memory_summary_omits_optional_fields_when_absent() {
        let summary = MyMemorySummary {
            id: Uuid::nil(),
            kind: "preference".into(),
            content_preview: "Prefers concise replies".into(),
            pinned_at: None,
            ttl_expires_at: None,
            created_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert!(
            v.get("pinned_at").is_none(),
            "absent pinned_at must be omitted"
        );
        assert!(
            v.get("ttl_expires_at").is_none(),
            "absent ttl_expires_at must be omitted"
        );
    }

    #[test]
    fn list_my_memory_query_kind_valid_values() {
        for k in &["fact", "preference", "note", "reminder", "rule", "profile"] {
            assert!(
                ListMyMemoryQuery::validate_kind(k),
                "expected '{k}' to be valid"
            );
        }
    }

    #[test]
    fn list_my_memory_query_kind_invalid_values() {
        assert!(
            !ListMyMemoryQuery::validate_kind("unknown"),
            "expected 'unknown' to be invalid"
        );
        assert!(
            !ListMyMemoryQuery::validate_kind(""),
            "expected empty string to be invalid"
        );
        assert!(
            !ListMyMemoryQuery::validate_kind("Fact"),
            "expected 'Fact' (capitalized) to be invalid"
        );
    }

    #[test]
    fn list_my_memory_query_defaults() {
        let q = ListMyMemoryQuery {
            after: None,
            limit: None,
            kind: None,
        };
        assert!(q.after.is_none());
        assert!(q.limit.is_none());
        assert!(q.kind.is_none());
    }

    #[test]
    fn list_my_memory_limit_clamp() {
        let over: i64 = Some(200i64).map(|l| l.min(100)).unwrap_or(50).max(1);
        let under: i64 = Some(0i64).map(|l| l.min(100)).unwrap_or(50).max(1);
        let default: i64 = None::<i64>.map(|l| l.min(100)).unwrap_or(50).max(1);
        assert_eq!(over, 100, "over-limit must be clamped to 100");
        assert_eq!(under, 1, "zero must be clamped to 1");
        assert_eq!(default, 50, "no limit defaults to 50");
    }
}
