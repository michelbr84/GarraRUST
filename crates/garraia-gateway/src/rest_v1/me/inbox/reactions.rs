//! `GET /v1/me/reactions` — inbox of messages the caller reacted to.

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

// ─── GET /v1/me/reactions ────────────────────────────────────────────────────

/// Query parameters for `GET /v1/me/reactions`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListReactionsQuery {
    /// Group to scope the reactions inbox. Required — RLS needs `app.current_group_id`.
    pub group_id: Uuid,
    /// Keyset cursor — `message_id` of the last row on the previous page.
    /// Returns rows with an earlier `MAX(reacted_at)` (exclusive). Omit for first page.
    pub after: Option<Uuid>,
    /// Maximum items per page. Clamped to `[1, 100]`; default `20`.
    pub limit: Option<i64>,
}

/// Internal row type for decoding GROUP BY + ARRAY_AGG results.
///
/// `sqlx::FromRow` on a named struct is required here because `Vec<String>` cannot
/// be decoded from a PostgreSQL array column via an anonymous tuple type alias.
#[derive(Debug, sqlx::FromRow)]
struct ReactionGroupRow {
    message_id: Uuid,
    chat_id: Uuid,
    group_id: Uuid,
    sender_user_id: Uuid,
    sender_label: String,
    body_excerpt: String,
    emojis: Vec<String>,
    reacted_at: DateTime<Utc>,
}

/// One message-level reaction summary in the `GET /v1/me/reactions` response.
///
/// `emojis` contains all distinct emoji the caller placed on this message,
/// ordered by first reaction time then lexicographically.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyReactionSummary {
    /// UUID of the message the caller reacted to.
    pub message_id: Uuid,
    /// UUID of the chat containing the message.
    pub chat_id: Uuid,
    /// UUID of the group (denormalized from `message_reactions.group_id`).
    pub group_id: Uuid,
    /// UUID of the user who sent the message.
    pub sender_user_id: Uuid,
    /// Display label of the message sender at send time.
    pub sender_label: String,
    /// First 200 characters of the message body.
    pub body_excerpt: String,
    /// All emoji the caller placed on this message.
    pub emojis: Vec<String>,
    /// `MAX(reacted_at)` — when the caller most recently reacted to this message.
    pub reacted_at: DateTime<Utc>,
}

/// Response body for `GET /v1/me/reactions`.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyReactionsResponse {
    pub items: Vec<MyReactionSummary>,
    /// `message_id` of the last item in this page. Pass as `?after=<uuid>` to
    /// fetch the next (older) page. Absent when the end has been reached.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Uuid>,
}

/// `GET /v1/me/reactions` — inbox of messages the authenticated caller has reacted to.
///
/// Returns up to `limit` (default 20, max 100) messages in `group_id` on which
/// the caller has at least one emoji reaction, ordered by
/// `(MAX(reacted_at) DESC, message_id DESC)`. Each item contains the full list
/// of emoji the caller placed on that message (`ARRAY_AGG` per-message group).
///
/// Cursor-based pagination via `?after=<last_message_id>`.
///
/// RLS is enforced via `SET LOCAL app.current_user_id` and
/// `SET LOCAL app.current_group_id` — rows from other groups are invisible.
#[utoipa::path(
    get,
    path = "/v1/me/reactions",
    params(ListReactionsQuery),
    responses(
        (status = 200, description = "Emoji reactions grouped by message, newest first.", body = MyReactionsResponse),
        (status = 401, description = "Missing or invalid JWT.", body = crate::rest_v1::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member of the specified group.", body = crate::rest_v1::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_my_reactions(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Query(params): Query<ListReactionsQuery>,
) -> Result<Json<MyReactionsResponse>, RestError> {
    let limit = params.limit.map(|l| l.min(100)).unwrap_or(20).max(1);
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

    let rows: Vec<ReactionGroupRow> = if let Some(after_id) = params.after {
        // HAVING cursor: if after_id is deleted or from a different group the
        // inner SELECT returns no rows → MAX returns NULL → comparison is false
        // → empty safe result (same fail-closed pattern as list_my_mentions).
        sqlx::query_as(
            "SELECT mr.message_id, m.chat_id, mr.group_id, \
                    m.sender_user_id, m.sender_label, \
                    LEFT(m.body, 200) AS body_excerpt, \
                    ARRAY_AGG(mr.emoji ORDER BY mr.reacted_at, mr.emoji) AS emojis, \
                    MAX(mr.reacted_at) AS reacted_at \
             FROM message_reactions mr \
             JOIN messages m ON mr.message_id = m.id \
             WHERE mr.user_id = $1 AND mr.group_id = $2 \
             GROUP BY mr.message_id, m.chat_id, mr.group_id, \
                      m.sender_user_id, m.sender_label, m.body \
             HAVING (MAX(mr.reacted_at), mr.message_id) < ( \
                 SELECT MAX(reacted_at), $3::uuid \
                 FROM message_reactions \
                 WHERE message_id = $3 AND user_id = $1 \
             ) \
             ORDER BY MAX(mr.reacted_at) DESC, mr.message_id DESC \
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
            "SELECT mr.message_id, m.chat_id, mr.group_id, \
                    m.sender_user_id, m.sender_label, \
                    LEFT(m.body, 200) AS body_excerpt, \
                    ARRAY_AGG(mr.emoji ORDER BY mr.reacted_at, mr.emoji) AS emojis, \
                    MAX(mr.reacted_at) AS reacted_at \
             FROM message_reactions mr \
             JOIN messages m ON mr.message_id = m.id \
             WHERE mr.user_id = $1 AND mr.group_id = $2 \
             GROUP BY mr.message_id, m.chat_id, mr.group_id, \
                      m.sender_user_id, m.sender_label, m.body \
             ORDER BY MAX(mr.reacted_at) DESC, mr.message_id DESC \
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
        rows.last().map(|r| r.message_id)
    } else {
        None
    };

    let items = rows
        .into_iter()
        .map(|r| MyReactionSummary {
            message_id: r.message_id,
            chat_id: r.chat_id,
            group_id: r.group_id,
            sender_user_id: r.sender_user_id,
            sender_label: r.sender_label,
            body_excerpt: r.body_excerpt,
            emojis: r.emojis,
            reacted_at: r.reacted_at,
        })
        .collect();

    Ok(Json(MyReactionsResponse { items, next_cursor }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── GET /v1/me/reactions ─────────────────────────────────────────────────

    #[test]
    fn my_reactions_response_empty_no_cursor() {
        let resp = MyReactionsResponse {
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
    fn my_reactions_response_with_cursor() {
        let cursor = Uuid::parse_str("eeeeeeee-0000-0000-0000-000000000005").unwrap();
        let resp = MyReactionsResponse {
            items: vec![],
            next_cursor: Some(cursor),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["next_cursor"], "eeeeeeee-0000-0000-0000-000000000005");
    }

    #[test]
    fn my_reaction_summary_serializes_all_fields() {
        let message_id = Uuid::parse_str("11111111-0000-0000-0000-000000000001").unwrap();
        let chat_id = Uuid::parse_str("22222222-0000-0000-0000-000000000002").unwrap();
        let group_id = Uuid::parse_str("33333333-0000-0000-0000-000000000003").unwrap();
        let sender_user_id = Uuid::parse_str("44444444-0000-0000-0000-000000000004").unwrap();
        let ts = chrono::DateTime::from_timestamp(1_000_000, 0).unwrap();
        let summary = MyReactionSummary {
            message_id,
            chat_id,
            group_id,
            sender_user_id,
            sender_label: "Alice".into(),
            body_excerpt: "Hello world".into(),
            emojis: vec!["👍".into(), "❤️".into()],
            reacted_at: ts,
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert_eq!(v["message_id"], "11111111-0000-0000-0000-000000000001");
        assert_eq!(v["chat_id"], "22222222-0000-0000-0000-000000000002");
        assert_eq!(v["emojis"][0], "👍");
        assert_eq!(v["emojis"][1], "❤️");
        assert!(v.get("reacted_at").is_some());
    }

    #[test]
    fn list_my_reactions_limit_clamp() {
        let over: i64 = Some(200i64).map(|l| l.min(100)).unwrap_or(20).max(1);
        let under: i64 = Some(0i64).map(|l| l.min(100)).unwrap_or(20).max(1);
        let default: i64 = None::<i64>.map(|l| l.min(100)).unwrap_or(20).max(1);
        assert_eq!(over, 100, "over-limit must be clamped to 100");
        assert_eq!(under, 1, "zero must be clamped to 1");
        assert_eq!(default, 20, "no limit defaults to 20");
    }

    #[test]
    fn list_my_reactions_query_defaults() {
        let q = ListReactionsQuery {
            group_id: Uuid::nil(),
            after: None,
            limit: None,
        };
        assert!(q.after.is_none());
        assert!(q.limit.is_none());
    }
}
