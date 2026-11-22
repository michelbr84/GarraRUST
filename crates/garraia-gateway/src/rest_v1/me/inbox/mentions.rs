//! `GET /v1/me/mentions` — inbox of @mentions received in a group.

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

// ─── GET /v1/me/mentions ─────────────────────────────────────────────────────

/// Query parameters for `GET /v1/me/mentions`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListMentionsQuery {
    /// Group to scope the mention inbox. Required — the caller must be a member.
    pub group_id: Uuid,
    /// Keyset cursor — `message_id` of the last mention received. Returns
    /// mentions older than this one (exclusive). Omit for the first page.
    pub after: Option<Uuid>,
    /// Page size. Default 50, max 100. Values > 100 are clamped to 100.
    pub limit: Option<i64>,
}

/// A single @mention received by the caller.
#[derive(Debug, Serialize, ToSchema)]
pub struct MentionSummary {
    /// UUID of the message in which the caller was mentioned.
    pub message_id: Uuid,
    /// UUID of the chat that contains the message.
    pub chat_id: Uuid,
    /// UUID of the group (denormalized for convenience).
    pub group_id: Uuid,
    /// UUID of the user who sent the message.
    pub sender_user_id: Uuid,
    /// Display name of the sender at send time.
    pub sender_label: String,
    /// First 200 characters of the message body.
    pub body_excerpt: String,
    /// UTC timestamp when the mention row was created.
    pub created_at: DateTime<Utc>,
}

/// Response body for `GET /v1/me/mentions`.
#[derive(Debug, Serialize, ToSchema)]
pub struct MentionsListResponse {
    pub items: Vec<MentionSummary>,
    /// `message_id` of the last item in this page. Pass as `?after=<uuid>` to
    /// fetch the next (older) page. `None` when the end has been reached.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Uuid>,
}

/// `GET /v1/me/mentions` — inbox of @mentions received by the authenticated caller.
///
/// Returns up to `limit` (default 50, max 100) mentions in `group_id`,
/// ordered by `(mm.created_at DESC, mm.message_id DESC)`.
/// Cursor-based pagination via `?after=<last_message_id>`.
///
/// RLS is enforced via `SET LOCAL app.current_user_id` and
/// `SET LOCAL app.current_group_id` — rows from other groups are invisible.
#[utoipa::path(
    get,
    path = "/v1/me/mentions",
    params(ListMentionsQuery),
    responses(
        (status = 200, description = "List of @mentions, newest first.", body = MentionsListResponse),
        (status = 400, description = "Validation error.", body = crate::rest_v1::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = crate::rest_v1::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member of the specified group.", body = crate::rest_v1::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_my_mentions(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Query(params): Query<ListMentionsQuery>,
) -> Result<Json<MentionsListResponse>, RestError> {
    // Validate group membership — the Principal extractor enforces this only
    // when X-Group-Id is present; here we require group_id as a query param.
    // We rely on FORCE RLS + SET LOCAL to enforce isolation — no explicit
    // membership check is needed because RLS will return 0 rows for any
    // group the caller does not belong to (correct behavior: empty inbox).

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

    type MentionRow = (Uuid, Uuid, Uuid, Uuid, String, String, DateTime<Utc>);

    let rows: Vec<MentionRow> = if let Some(after_id) = params.after {
        // Cursor subquery: if the after message_id is not found (deleted or
        // wrong group), the subquery returns NULL → comparison is always false
        // → empty safe result (same pattern as list_messages).
        sqlx::query_as(
            "SELECT mm.message_id, m.chat_id, mm.group_id, \
                    m.sender_user_id, m.sender_label, \
                    LEFT(m.body, 200) AS body_excerpt, \
                    mm.created_at \
             FROM message_mentions mm \
             JOIN messages m ON mm.message_id = m.id \
             WHERE mm.mentioned_user_id = $1 \
               AND mm.group_id = $2 \
               AND (mm.created_at, mm.message_id) < ( \
                   SELECT created_at, message_id \
                   FROM message_mentions \
                   WHERE message_id = $3 AND mentioned_user_id = $1 \
               ) \
             ORDER BY mm.created_at DESC, mm.message_id DESC \
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
            "SELECT mm.message_id, m.chat_id, mm.group_id, \
                    m.sender_user_id, m.sender_label, \
                    LEFT(m.body, 200) AS body_excerpt, \
                    mm.created_at \
             FROM message_mentions mm \
             JOIN messages m ON mm.message_id = m.id \
             WHERE mm.mentioned_user_id = $1 \
               AND mm.group_id = $2 \
             ORDER BY mm.created_at DESC, mm.message_id DESC \
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
        rows.last().map(|(message_id, ..)| *message_id)
    } else {
        None
    };

    let items = rows
        .into_iter()
        .map(
            |(
                message_id,
                chat_id,
                group_id,
                sender_user_id,
                sender_label,
                body_excerpt,
                created_at,
            )| {
                MentionSummary {
                    message_id,
                    chat_id,
                    group_id,
                    sender_user_id,
                    sender_label,
                    body_excerpt,
                    created_at,
                }
            },
        )
        .collect();

    Ok(Json(MentionsListResponse { items, next_cursor }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── MentionsListResponse serialization ─────────────────────────────────

    #[test]
    fn mentions_list_response_empty_no_cursor() {
        let resp = MentionsListResponse {
            items: vec![],
            next_cursor: None,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 0);
        assert!(
            v.get("next_cursor").is_none(),
            "absent cursor must be skipped"
        );
    }

    #[test]
    fn mentions_list_response_with_cursor() {
        let cursor = Uuid::parse_str("aaaaaaaa-0000-0000-0000-000000000001").unwrap();
        let resp = MentionsListResponse {
            items: vec![],
            next_cursor: Some(cursor),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["next_cursor"], "aaaaaaaa-0000-0000-0000-000000000001");
    }

    #[test]
    fn mention_summary_serializes_all_fields() {
        let msg_id = Uuid::nil();
        let chat_id = Uuid::parse_str("11111111-0000-0000-0000-000000000001").unwrap();
        let group_id = Uuid::parse_str("22222222-0000-0000-0000-000000000001").unwrap();
        let sender = Uuid::parse_str("33333333-0000-0000-0000-000000000001").unwrap();
        let summary = MentionSummary {
            message_id: msg_id,
            chat_id,
            group_id,
            sender_user_id: sender,
            sender_label: "Alice".into(),
            body_excerpt: "Hello @Bob".into(),
            created_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert_eq!(v["message_id"], "00000000-0000-0000-0000-000000000000");
        assert_eq!(v["sender_label"], "Alice");
        assert_eq!(v["body_excerpt"], "Hello @Bob");
    }

    #[test]
    fn list_mentions_query_defaults() {
        let q = ListMentionsQuery {
            group_id: Uuid::nil(),
            after: None,
            limit: None,
        };
        assert!(q.after.is_none());
        assert!(q.limit.is_none());
    }
}
