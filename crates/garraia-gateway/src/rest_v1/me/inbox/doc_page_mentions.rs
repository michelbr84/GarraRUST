//! `GET /v1/me/doc-page-mentions` — inbox of doc page @mentions received.

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

// ─── GET /v1/me/doc-page-mentions ────────────────────────────────────────────

/// Query parameters for `GET /v1/me/doc-page-mentions`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListMyDocPageMentionsQuery {
    /// Group to scope the mention inbox. Required — caller must be a member.
    pub group_id: Uuid,
    /// Keyset cursor — `page_id` of the last item. Returns mentions with
    /// `created_at` older than that row (exclusive). Omit for the first page.
    pub after: Option<Uuid>,
    /// Page size. Default 50, max 100.
    pub limit: Option<i64>,
}

/// A single doc page mention in the caller's inbox.
#[derive(Debug, Serialize, ToSchema)]
pub struct DocPageMentionInboxSummary {
    /// UUID of the doc page in which the caller was mentioned.
    pub page_id: Uuid,
    /// UUID of the group (denormalized).
    pub group_id: Uuid,
    /// Title of the doc page at the time of the query.
    pub page_title: String,
    /// UTC timestamp when the mention was created.
    pub created_at: DateTime<Utc>,
}

/// Response body for `GET /v1/me/doc-page-mentions`.
#[derive(Debug, Serialize, ToSchema)]
pub struct DocPageMentionsInboxResponse {
    pub items: Vec<DocPageMentionInboxSummary>,
    /// `page_id` of the last item. Pass as `?after=<uuid>` for the next page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Uuid>,
}

/// `GET /v1/me/doc-page-mentions` — inbox of doc page @mentions received by the caller.
///
/// Returns up to `limit` (default 50, max 100) mentions in `group_id`,
/// ordered by `(dpm.created_at DESC, dpm.page_id DESC)`.
/// Cursor-based pagination via `?after=<last_page_id>`.
#[utoipa::path(
    get,
    path = "/v1/me/doc-page-mentions",
    params(ListMyDocPageMentionsQuery),
    responses(
        (status = 200, description = "List of doc page @mentions, newest first.", body = DocPageMentionsInboxResponse),
        (status = 400, description = "Validation error.", body = crate::rest_v1::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = crate::rest_v1::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_my_doc_page_mentions(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Query(params): Query<ListMyDocPageMentionsQuery>,
) -> Result<Json<DocPageMentionsInboxResponse>, RestError> {
    let limit = params.limit.map(|l| l.min(100)).unwrap_or(50).max(1);
    let group_id = params.group_id;

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

    // (page_id, group_id, page_title, created_at)
    type InboxRow = (Uuid, Uuid, String, DateTime<Utc>);

    let rows: Vec<InboxRow> = if let Some(after_id) = params.after {
        sqlx::query_as(
            "SELECT dpm.page_id, dpm.group_id, \
                    COALESCE(dp.title, '') AS page_title, \
                    dpm.created_at \
             FROM doc_page_mentions dpm \
             LEFT JOIN doc_pages dp ON dp.id = dpm.page_id \
             WHERE dpm.mentioned_user_id = $1 \
               AND dpm.group_id = $2 \
               AND (dpm.created_at, dpm.page_id) < ( \
                   SELECT created_at, page_id \
                   FROM doc_page_mentions \
                   WHERE page_id = $3 AND mentioned_user_id = $1 \
               ) \
             ORDER BY dpm.created_at DESC, dpm.page_id DESC \
             LIMIT $4",
        )
        .bind(principal.user_id)
        .bind(group_id)
        .bind(after_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    } else {
        sqlx::query_as(
            "SELECT dpm.page_id, dpm.group_id, \
                    COALESCE(dp.title, '') AS page_title, \
                    dpm.created_at \
             FROM doc_page_mentions dpm \
             LEFT JOIN doc_pages dp ON dp.id = dpm.page_id \
             WHERE dpm.mentioned_user_id = $1 \
               AND dpm.group_id = $2 \
             ORDER BY dpm.created_at DESC, dpm.page_id DESC \
             LIMIT $3",
        )
        .bind(principal.user_id)
        .bind(group_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let next_cursor = if rows.len() as i64 == limit {
        rows.last().map(|(pid, _, _, _)| *pid)
    } else {
        None
    };

    let items = rows
        .into_iter()
        .map(
            |(page_id, group_id, page_title, created_at)| DocPageMentionInboxSummary {
                page_id,
                group_id,
                page_title,
                created_at,
            },
        )
        .collect();

    Ok(Json(DocPageMentionsInboxResponse { items, next_cursor }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── GET /v1/me/doc-page-mentions tests ──────────────────────────────────

    #[test]
    fn doc_page_mention_inbox_summary_serializes_all_fields() {
        use chrono::TimeZone;
        let s = DocPageMentionInboxSummary {
            page_id: Uuid::nil(),
            group_id: Uuid::nil(),
            page_title: "Design notes".to_string(),
            created_at: Utc.with_ymd_and_hms(2026, 6, 12, 0, 0, 0).unwrap(),
        };
        let v = serde_json::to_value(&s).unwrap();
        assert!(v.get("page_id").is_some());
        assert!(v.get("group_id").is_some());
        assert!(v.get("page_title").is_some());
        assert!(v.get("created_at").is_some());
    }

    #[test]
    fn doc_page_mention_inbox_summary_created_at_utc_z() {
        use chrono::TimeZone;
        let s = DocPageMentionInboxSummary {
            page_id: Uuid::nil(),
            group_id: Uuid::nil(),
            page_title: "Notes".to_string(),
            created_at: Utc.with_ymd_and_hms(2026, 6, 12, 0, 0, 0).unwrap(),
        };
        let v = serde_json::to_value(&s).unwrap();
        let ts = v["created_at"].as_str().unwrap();
        assert!(ts.ends_with('Z'), "expected UTC Z suffix, got: {ts}");
    }

    #[test]
    fn doc_page_mention_inbox_response_no_next_cursor_omitted() {
        let resp = DocPageMentionsInboxResponse {
            items: vec![],
            next_cursor: None,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert!(v.get("next_cursor").is_none());
    }

    #[test]
    fn doc_page_mention_inbox_response_cursor_present() {
        let uid = Uuid::new_v4();
        let resp = DocPageMentionsInboxResponse {
            items: vec![],
            next_cursor: Some(uid),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["next_cursor"].as_str().unwrap(), uid.to_string());
    }

    #[test]
    fn doc_page_mention_inbox_limit_clamp() {
        let over: i64 = Some(200i64).map(|l| l.min(100)).unwrap_or(50).max(1);
        let under: i64 = Some(0i64).map(|l| l.min(100)).unwrap_or(50).max(1);
        let default: i64 = None::<i64>.map(|l| l.min(100)).unwrap_or(50).max(1);
        assert_eq!(over, 100);
        assert_eq!(under, 1);
        assert_eq!(default, 50);
    }

    #[test]
    fn doc_page_mention_inbox_query_defaults() {
        let q = ListMyDocPageMentionsQuery {
            group_id: Uuid::nil(),
            after: None,
            limit: None,
        };
        assert!(q.after.is_none());
        assert!(q.limit.is_none());
    }
}
