//! `GET /v1/me/doc-pages` — inbox of doc pages authored by the caller.

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

// ─── GET /v1/me/doc-pages ────────────────────────────────────────────────────

/// Query parameters for `GET /v1/me/doc-pages`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListMyDocPagesQuery {
    /// Group to scope the inbox. Required — RLS needs `app.current_group_id`.
    pub group_id: Uuid,
    /// Keyset cursor — `id` of the last doc page on the previous page.
    /// Returns pages created earlier than this one (exclusive). Omit for first page.
    pub after: Option<Uuid>,
    /// Page size. Default 20, max 100. Values > 100 are clamped to 100.
    pub limit: Option<i64>,
    /// When `true`, archived pages (`archived_at IS NOT NULL`) are included.
    /// Default `false` — archived pages are excluded.
    pub include_archived: Option<bool>,
}

/// One doc page entry in the `GET /v1/me/doc-pages` response.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyDocPageSummary {
    /// UUID of the doc page.
    pub id: Uuid,
    /// UUID of the group the page belongs to.
    pub group_id: Uuid,
    /// UUID of the parent page. `null` for root-level pages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_page_id: Option<Uuid>,
    /// Page title.
    pub title: String,
    /// Optional emoji or icon identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// UTC timestamp when the page was soft-deleted (archived). `null` if active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<DateTime<Utc>>,
    /// UTC timestamp when the page was created.
    pub created_at: DateTime<Utc>,
}

/// Response body for `GET /v1/me/doc-pages`.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyDocPagesResponse {
    pub items: Vec<MyDocPageSummary>,
    /// `id` of the last item in this page. Pass as `?after=<uuid>` to fetch the
    /// next (older) page. Absent when the end has been reached.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Uuid>,
}

/// `GET /v1/me/doc-pages` — inbox of doc pages authored by the authenticated caller.
///
/// Returns up to `limit` (default 20, max 100) doc pages in `group_id` where
/// `doc_pages.created_by = caller_user_id`, ordered by
/// `(created_at DESC, id DESC)`.
///
/// Cursor-based pagination via `?after=<last_page_id>`. Optional
/// `?include_archived=true` includes pages with `archived_at IS NOT NULL`
/// (archived pages are excluded by default).
///
/// FORCE RLS is enforced via `SET LOCAL app.current_user_id` and
/// `SET LOCAL app.current_group_id` — rows from other groups are invisible.
/// A cross-group `after=` cursor returns 0 rows (fail-closed, no info leak).
#[utoipa::path(
    get,
    path = "/v1/me/doc-pages",
    params(ListMyDocPagesQuery),
    responses(
        (status = 200, description = "List of authored doc pages, newest first.", body = MyDocPagesResponse),
        (status = 401, description = "Missing or invalid JWT.", body = crate::rest_v1::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_my_doc_pages(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Query(params): Query<ListMyDocPagesQuery>,
) -> Result<Json<MyDocPagesResponse>, RestError> {
    let limit = params.limit.map(|l| l.min(100)).unwrap_or(20).max(1);
    let group_id = params.group_id;
    let include_archived = params.include_archived.unwrap_or(false);

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    sqlx::query("SELECT set_config('app.current_user_id', $1, true)")
        .bind(principal.user_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    sqlx::query("SELECT set_config('app.current_group_id', $1, true)")
        .bind(group_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // Columns: id, group_id, parent_page_id, title, icon, archived_at, created_at
    type DocPageInboxRow = (
        Uuid,
        Uuid,
        Option<Uuid>,
        String,
        Option<String>,
        Option<DateTime<Utc>>,
        DateTime<Utc>,
    );

    let rows: Vec<DocPageInboxRow> = if let Some(after_id) = params.after {
        // Cursor subquery: if after_id is not found or belongs to a different
        // group, the subquery returns NULL → comparison is always false → 0 rows
        // (fail-closed, no info leak for cross-group cursor attacks).
        sqlx::query_as(
            "SELECT id, group_id, parent_page_id, title, icon, archived_at, created_at \
             FROM doc_pages \
             WHERE group_id = $1 \
               AND created_by = $2 \
               AND ($5 OR archived_at IS NULL) \
               AND (created_at, id) < ( \
                   SELECT created_at, id FROM doc_pages \
                   WHERE id = $3 AND group_id = $1 \
               ) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $4",
        )
        .bind(group_id)
        .bind(principal.user_id)
        .bind(after_id)
        .bind(limit)
        .bind(include_archived)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    } else {
        sqlx::query_as(
            "SELECT id, group_id, parent_page_id, title, icon, archived_at, created_at \
             FROM doc_pages \
             WHERE group_id = $1 \
               AND created_by = $2 \
               AND ($3 OR archived_at IS NULL) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $4",
        )
        .bind(group_id)
        .bind(principal.user_id)
        .bind(include_archived)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
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
            |(id, group_id, parent_page_id, title, icon, archived_at, created_at)| {
                MyDocPageSummary {
                    id,
                    group_id,
                    parent_page_id,
                    icon,
                    title,
                    archived_at,
                    created_at,
                }
            },
        )
        .collect();

    Ok(Json(MyDocPagesResponse { items, next_cursor }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── GET /v1/me/doc-pages tests ──────────────────────────────────────────

    #[test]
    fn my_doc_pages_response_empty_no_cursor() {
        let resp = MyDocPagesResponse {
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
    fn my_doc_pages_response_with_cursor() {
        let cursor = Uuid::parse_str("cccccccc-0000-0000-0000-000000000001").unwrap();
        let resp = MyDocPagesResponse {
            items: vec![],
            next_cursor: Some(cursor),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["next_cursor"], "cccccccc-0000-0000-0000-000000000001");
    }

    #[test]
    fn my_doc_page_summary_serializes_all_fields() {
        use chrono::TimeZone;
        let ts = Utc.with_ymd_and_hms(2026, 6, 12, 10, 0, 0).unwrap();
        let archived_ts = Utc.with_ymd_and_hms(2026, 6, 13, 8, 0, 0).unwrap();
        let s = MyDocPageSummary {
            id: Uuid::nil(),
            group_id: Uuid::nil(),
            parent_page_id: Some(Uuid::nil()),
            title: "Meeting notes".to_string(),
            icon: Some("📝".to_string()),
            archived_at: Some(archived_ts),
            created_at: ts,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert!(v.get("id").is_some());
        assert!(v.get("group_id").is_some());
        assert!(v.get("parent_page_id").is_some());
        assert_eq!(v["title"], "Meeting notes");
        assert_eq!(v["icon"], "📝");
        assert!(v.get("archived_at").is_some());
        assert!(v.get("created_at").is_some());
    }

    #[test]
    fn my_doc_page_summary_omits_optional_fields_when_none() {
        use chrono::TimeZone;
        let ts = Utc.with_ymd_and_hms(2026, 6, 12, 10, 0, 0).unwrap();
        let s = MyDocPageSummary {
            id: Uuid::nil(),
            group_id: Uuid::nil(),
            parent_page_id: None,
            title: "Root page".to_string(),
            icon: None,
            archived_at: None,
            created_at: ts,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert!(
            v.get("parent_page_id").is_none(),
            "parent_page_id must be absent when None"
        );
        assert!(v.get("icon").is_none(), "icon must be absent when None");
        assert!(
            v.get("archived_at").is_none(),
            "archived_at must be absent when None"
        );
    }

    #[test]
    fn list_my_doc_pages_limit_clamp() {
        let over: i64 = Some(200i64).map(|l| l.min(100)).unwrap_or(20).max(1);
        let under: i64 = Some(0i64).map(|l| l.min(100)).unwrap_or(20).max(1);
        let default: i64 = None::<i64>.map(|l| l.min(100)).unwrap_or(20).max(1);
        assert_eq!(over, 100, "over-limit must be clamped to 100");
        assert_eq!(under, 1, "zero must be clamped to 1");
        assert_eq!(default, 20, "no limit defaults to 20");
    }

    #[test]
    fn list_my_doc_pages_query_defaults() {
        let q = ListMyDocPagesQuery {
            group_id: Uuid::nil(),
            after: None,
            limit: None,
            include_archived: None,
        };
        assert!(q.after.is_none());
        assert!(q.limit.is_none());
        assert!(
            !q.include_archived.unwrap_or(false),
            "default include_archived is false"
        );
    }
}
