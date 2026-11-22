//! `GET /v1/me/threads` — inbox of threads the caller created or replied to.

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

// ─── GET /v1/me/threads ──────────────────────────────────────────────────────

/// Internal DB row returned by the threads participation query.
#[derive(sqlx::FromRow)]
struct ThreadRow {
    thread_id: Uuid,
    chat_id: Uuid,
    group_id: Uuid,
    title: Option<String>,
    root_message_id: Uuid,
    resolved_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    created_by: Uuid,
}

/// Query params for `GET /v1/me/threads`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListMyThreadsQuery {
    /// Group UUID (required; sets RLS context).
    pub group_id: Uuid,
    /// Keyset cursor — thread UUID of the last item on the previous page.
    pub after: Option<Uuid>,
    /// Page size, clamped 1..100; defaults to 20.
    pub limit: Option<i64>,
    /// When `true`, include resolved threads. Defaults to `false`.
    pub include_resolved: Option<bool>,
}

/// One thread in the caller's thread-participation inbox.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyThreadSummary {
    pub thread_id: Uuid,
    pub chat_id: Uuid,
    pub group_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub root_message_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    /// `"creator"` if the caller started this thread; `"participant"` otherwise.
    pub role: String,
}

/// Response body for `GET /v1/me/threads`.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyThreadsResponse {
    pub items: Vec<MyThreadSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Uuid>,
}

/// List threads where the authenticated caller is a creator or participant.
///
/// Returns threads in a given group where `created_by = caller` OR the caller
/// has posted at least one non-deleted reply (`messages.thread_id = thread.id
/// AND messages.sender_user_id = caller`).  Keyset cursor on
/// `(mt.created_at DESC, mt.id DESC)`.  FORCE RLS is enforced via
/// parameterised `SET LOCAL` for both `app.current_user_id` and
/// `app.current_group_id`.
#[utoipa::path(
    get,
    path = "/v1/me/threads",
    params(ListMyThreadsQuery),
    responses(
        (status = 200, description = "Thread participation inbox", body = MyThreadsResponse),
        (status = 400, description = "Missing or invalid parameters"),
        (status = 401, description = "Unauthorized"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_my_threads(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Query(params): Query<ListMyThreadsQuery>,
) -> Result<Json<MyThreadsResponse>, RestError> {
    let limit = params.limit.map(|l| l.min(100)).unwrap_or(20).max(1);
    let group_id = params.group_id;
    let include_resolved = params.include_resolved.unwrap_or(false);

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

    // 4-branch static SQL: cursor × include_resolved.
    // The cursor sub-select is gated by FORCE RLS, so a cross-group or deleted
    // after_id returns NULL → row-value comparison yields NULL → 0 rows (fail-closed).
    let rows: Vec<ThreadRow> = match (params.after, include_resolved) {
        (None, false) => sqlx::query_as(
            "SELECT mt.id AS thread_id, mt.chat_id, c.group_id, mt.title, \
                    mt.root_message_id, mt.resolved_at, mt.created_at, mt.created_by \
             FROM message_threads mt \
             JOIN chats c ON c.id = mt.chat_id \
             WHERE c.group_id = $1 \
               AND mt.resolved_at IS NULL \
               AND (mt.created_by = $2 OR EXISTS ( \
                   SELECT 1 FROM messages m \
                   WHERE m.thread_id = mt.id \
                     AND m.sender_user_id = $2 \
                     AND m.deleted_at IS NULL \
               )) \
             ORDER BY mt.created_at DESC, mt.id DESC \
             LIMIT $3",
        )
        .bind(group_id)
        .bind(principal.user_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        (Some(after_id), false) => sqlx::query_as(
            "SELECT mt.id AS thread_id, mt.chat_id, c.group_id, mt.title, \
                    mt.root_message_id, mt.resolved_at, mt.created_at, mt.created_by \
             FROM message_threads mt \
             JOIN chats c ON c.id = mt.chat_id \
             WHERE c.group_id = $1 \
               AND mt.resolved_at IS NULL \
               AND (mt.created_by = $2 OR EXISTS ( \
                   SELECT 1 FROM messages m \
                   WHERE m.thread_id = mt.id \
                     AND m.sender_user_id = $2 \
                     AND m.deleted_at IS NULL \
               )) \
               AND (mt.created_at, mt.id) < ( \
                   SELECT created_at, id FROM message_threads WHERE id = $4 \
               ) \
             ORDER BY mt.created_at DESC, mt.id DESC \
             LIMIT $3",
        )
        .bind(group_id)
        .bind(principal.user_id)
        .bind(limit)
        .bind(after_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        (None, true) => sqlx::query_as(
            "SELECT mt.id AS thread_id, mt.chat_id, c.group_id, mt.title, \
                    mt.root_message_id, mt.resolved_at, mt.created_at, mt.created_by \
             FROM message_threads mt \
             JOIN chats c ON c.id = mt.chat_id \
             WHERE c.group_id = $1 \
               AND (mt.created_by = $2 OR EXISTS ( \
                   SELECT 1 FROM messages m \
                   WHERE m.thread_id = mt.id \
                     AND m.sender_user_id = $2 \
                     AND m.deleted_at IS NULL \
               )) \
             ORDER BY mt.created_at DESC, mt.id DESC \
             LIMIT $3",
        )
        .bind(group_id)
        .bind(principal.user_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        (Some(after_id), true) => sqlx::query_as(
            "SELECT mt.id AS thread_id, mt.chat_id, c.group_id, mt.title, \
                    mt.root_message_id, mt.resolved_at, mt.created_at, mt.created_by \
             FROM message_threads mt \
             JOIN chats c ON c.id = mt.chat_id \
             WHERE c.group_id = $1 \
               AND (mt.created_by = $2 OR EXISTS ( \
                   SELECT 1 FROM messages m \
                   WHERE m.thread_id = mt.id \
                     AND m.sender_user_id = $2 \
                     AND m.deleted_at IS NULL \
               )) \
               AND (mt.created_at, mt.id) < ( \
                   SELECT created_at, id FROM message_threads WHERE id = $4 \
               ) \
             ORDER BY mt.created_at DESC, mt.id DESC \
             LIMIT $3",
        )
        .bind(group_id)
        .bind(principal.user_id)
        .bind(limit)
        .bind(after_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let next_cursor = if rows.len() as i64 == limit {
        rows.last().map(|r| r.thread_id)
    } else {
        None
    };

    let user_id = principal.user_id;
    let items = rows
        .into_iter()
        .map(|r| MyThreadSummary {
            thread_id: r.thread_id,
            chat_id: r.chat_id,
            group_id: r.group_id,
            title: r.title,
            root_message_id: r.root_message_id,
            resolved_at: r.resolved_at,
            created_at: r.created_at,
            role: if r.created_by == user_id {
                "creator".into()
            } else {
                "participant".into()
            },
        })
        .collect();

    Ok(Json(MyThreadsResponse { items, next_cursor }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── GET /v1/me/threads tests ──────────────────────────────────────────

    fn make_thread_summary(role: &str) -> MyThreadSummary {
        MyThreadSummary {
            thread_id: Uuid::nil(),
            chat_id: Uuid::nil(),
            group_id: Uuid::nil(),
            title: None,
            root_message_id: Uuid::nil(),
            resolved_at: None,
            created_at: DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
            role: role.into(),
        }
    }

    #[test]
    fn my_threads_response_empty_no_cursor() {
        let resp = MyThreadsResponse {
            items: vec![],
            next_cursor: None,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert!(v["items"].as_array().unwrap().is_empty());
        assert!(
            v.get("next_cursor").is_none(),
            "next_cursor must be absent when None"
        );
    }

    #[test]
    fn my_threads_response_with_cursor() {
        let cursor = Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
        let resp = MyThreadsResponse {
            items: vec![make_thread_summary("creator")],
            next_cursor: Some(cursor),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 1);
        assert_eq!(v["next_cursor"], "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");
    }

    #[test]
    fn my_thread_summary_serializes_creator_role() {
        let s = make_thread_summary("creator");
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["role"], "creator");
    }

    #[test]
    fn my_thread_summary_serializes_participant_role() {
        let s = make_thread_summary("participant");
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["role"], "participant");
    }

    #[test]
    fn my_thread_summary_title_and_resolved_omitted_when_none() {
        let s = make_thread_summary("creator");
        let v = serde_json::to_value(&s).unwrap();
        assert!(v.get("title").is_none(), "title must be absent when None");
        assert!(
            v.get("resolved_at").is_none(),
            "resolved_at must be absent when None"
        );
    }

    #[test]
    fn my_thread_summary_title_and_resolved_present_when_some() {
        let now = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap();
        let s = MyThreadSummary {
            thread_id: Uuid::nil(),
            chat_id: Uuid::nil(),
            group_id: Uuid::nil(),
            title: Some("My thread".into()),
            root_message_id: Uuid::nil(),
            resolved_at: Some(now),
            created_at: now,
            role: "creator".into(),
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["title"], "My thread");
        assert!(v["resolved_at"].is_string());
    }

    #[test]
    fn list_my_threads_limit_clamp() {
        let over: i64 = Some(200i64).map(|l| l.min(100)).unwrap_or(20).max(1);
        let under: i64 = Some(0i64).map(|l| l.min(100)).unwrap_or(20).max(1);
        let default: i64 = None::<i64>.map(|l| l.min(100)).unwrap_or(20).max(1);
        assert_eq!(over, 100, "over-limit must be clamped to 100");
        assert_eq!(under, 1, "zero must be clamped to 1");
        assert_eq!(default, 20, "no limit defaults to 20");
    }

    #[test]
    fn list_my_threads_query_defaults() {
        let q = ListMyThreadsQuery {
            group_id: Uuid::nil(),
            after: None,
            limit: None,
            include_resolved: None,
        };
        assert!(q.after.is_none());
        assert!(q.limit.is_none());
        assert!(q.include_resolved.is_none());
        assert!(
            !q.include_resolved.unwrap_or(false),
            "default include_resolved is false"
        );
    }
}
