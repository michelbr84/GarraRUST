//! `/v1/me` — authenticated caller identity + self-service profile update.
//!
//! `GET /v1/me` returns identity info from the `Principal` extractor only —
//! no SQL. `PATCH /v1/me` updates `display_name` on the `users` table via
//! the `garraia_app` AppPool. `users` is NOT FORCE-RLS group-scoped, so no
//! `SET LOCAL` context is needed; isolation is via `WHERE id = $1` with the
//! JWT-authenticated `principal.user_id`.
//!
//! `GET /v1/me/mentions` — cursor-paginated inbox of @mentions received by
//! the caller in a given group (plan 0237 / GAR-755).

use axum::Json;
use axum::extract::{Query, State};
use chrono::{DateTime, Utc};
use garraia_auth::Principal;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::RestV1FullState;
use super::problem::RestError;

// ─── GET /v1/me ──────────────────────────────────────────────────────────────

/// Response body for `GET /v1/me`.
#[derive(Debug, Serialize, ToSchema)]
pub struct MeResponse {
    /// UUID of the authenticated user (from the JWT `sub` claim).
    pub user_id: Uuid,
    /// Active group UUID if the caller supplied `X-Group-Id` and is an
    /// active member; absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<Uuid>,
    /// Group role string (e.g. `"owner"`, `"admin"`, `"member"`).
    /// Absent when `group_id` is absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[utoipa::path(
    get,
    path = "/v1/me",
    responses(
        (status = 200, description = "Authenticated identity", body = MeResponse),
        (status = 401, description = "Missing or invalid JWT", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member of X-Group-Id", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn get_me(principal: Principal) -> Result<Json<MeResponse>, RestError> {
    Ok(Json(MeResponse {
        user_id: principal.user_id,
        group_id: principal.group_id,
        role: principal.role.map(|r| r.as_str().to_string()),
    }))
}

// ─── PATCH /v1/me ────────────────────────────────────────────────────────────

/// Request body for `PATCH /v1/me`.
///
/// All fields are optional. An empty body `{}` is a valid no-op that returns
/// the current profile. Unknown fields are rejected with 422 (deny_unknown_fields).
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchMeRequest {
    /// Updated display name. 1–128 characters when provided.
    pub display_name: Option<String>,
}

impl PatchMeRequest {
    fn validate(&self) -> Result<(), &'static str> {
        if let Some(dn) = &self.display_name {
            let len = dn.chars().count();
            if len == 0 {
                return Err("display_name must not be empty");
            }
            if len > 128 {
                return Err("display_name exceeds 128 character limit");
            }
        }
        Ok(())
    }
}

/// Response body for `PATCH /v1/me`.
#[derive(Debug, Serialize, ToSchema)]
pub struct PatchMeResponse {
    /// UUID of the authenticated user.
    pub user_id: Uuid,
    /// Current email address (read-only, returned for client sync).
    pub email: String,
    /// Current display name after the update.
    pub display_name: String,
    /// Account creation timestamp (UTC).
    pub created_at: DateTime<Utc>,
    /// Last update timestamp (UTC).
    pub updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct UserRow {
    id: Uuid,
    email: String,
    display_name: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[utoipa::path(
    patch,
    path = "/v1/me",
    request_body = PatchMeRequest,
    responses(
        (status = 200, description = "Profile updated.", body = PatchMeResponse),
        (status = 400, description = "Validation error.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 422, description = "Unknown field or malformed body.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn patch_me(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Json(body): Json<PatchMeRequest>,
) -> Result<Json<PatchMeResponse>, RestError> {
    body.validate()
        .map_err(|msg| RestError::BadRequest(msg.into()))?;

    let pool = state.app_pool.pool_for_handlers();

    let row: UserRow = if body.display_name.is_none() {
        // No-op path: return current user data without issuing an UPDATE.
        sqlx::query_as(
            "SELECT id, email, display_name, created_at, updated_at \
             FROM users WHERE id = $1",
        )
        .bind(principal.user_id)
        .fetch_one(pool)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    } else {
        sqlx::query_as(
            "UPDATE users \
             SET display_name = COALESCE($2, display_name), updated_at = now() \
             WHERE id = $1 \
             RETURNING id, email, display_name, created_at, updated_at",
        )
        .bind(principal.user_id)
        .bind(body.display_name.as_deref())
        .fetch_one(pool)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    };

    Ok(Json(PatchMeResponse {
        user_id: row.id,
        email: row.email,
        display_name: row.display_name,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }))
}

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
        (status = 400, description = "Validation error.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member of the specified group.", body = super::problem::ProblemDetails),
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

    #[test]
    fn me_response_serializes_without_group_when_absent() {
        let body = MeResponse {
            user_id: Uuid::nil(),
            group_id: None,
            role: None,
        };
        let v = serde_json::to_value(&body).unwrap();
        assert_eq!(v["user_id"], "00000000-0000-0000-0000-000000000000");
        assert!(
            v.get("group_id").is_none(),
            "absent group_id must be skipped"
        );
        assert!(v.get("role").is_none());
    }

    #[test]
    fn me_response_serializes_with_group_when_present() {
        let gid = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let body = MeResponse {
            user_id: Uuid::nil(),
            group_id: Some(gid),
            role: Some("owner".into()),
        };
        let v = serde_json::to_value(&body).unwrap();
        assert_eq!(v["group_id"], "11111111-1111-1111-1111-111111111111");
        assert_eq!(v["role"], "owner");
    }

    #[test]
    fn patch_me_request_validates_name_too_long() {
        let req = PatchMeRequest {
            display_name: Some("x".repeat(129)),
        };
        assert!(
            req.validate().is_err(),
            "129-char name must fail validation"
        );
    }

    #[test]
    fn patch_me_request_validates_empty_name() {
        let req = PatchMeRequest {
            display_name: Some(String::new()),
        };
        assert!(req.validate().is_err(), "empty name must fail validation");
    }

    #[test]
    fn patch_me_request_allows_none() {
        let req = PatchMeRequest { display_name: None };
        assert!(req.validate().is_ok(), "None display_name is valid (no-op)");
    }

    #[test]
    fn patch_me_request_rejects_unknown_fields() {
        let json = r#"{"display_name": "Alice", "status": "deleted"}"#;
        let result: Result<PatchMeRequest, _> = serde_json::from_str(json);
        assert!(result.is_err(), "unknown field 'status' must be rejected");
    }

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
