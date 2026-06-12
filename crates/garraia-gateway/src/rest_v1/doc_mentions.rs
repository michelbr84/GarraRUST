//! Doc-page mention handlers for the Docs Tier 2 surface.
//! Plan 0318 / GAR-858.
//!
//! Three endpoints on the `garraia_app` RLS-enforced pool:
//! - `POST   /v1/doc-pages/{page_id}/mentions`            — add user mention (201 / idempotent 200)
//! - `GET    /v1/doc-pages/{page_id}/mentions`            — list mentions, cursor-paginated
//! - `DELETE /v1/doc-pages/{page_id}/mentions/{user_id}` — remove mention (204 idempotent)
//!
//! ## Tenant-context protocol
//!
//! `doc_page_mentions` uses FORCE RLS with direct `group_id` isolation (migration 029).
//! Both RLS vars (`app.current_user_id` + `app.current_group_id`) are set
//! via parameterised `set_config` before any SQL in every transaction.
//!
//! ## Cross-group isolation
//!
//! The `page_id` path param is looked up against `doc_pages` inside the
//! caller's RLS context. A `page_id` belonging to a different group returns
//! 0 rows → 404, preventing cross-group information disclosure.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use garraia_auth::{Action, Principal, WorkspaceAuditAction, audit_workspace_event, can};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::RestV1FullState;
use super::problem::RestError;

// ─── Constants ───────────────────────────────────────────────────────────────

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 100;

// ─── Helper ──────────────────────────────────────────────────────────────────

fn require_group_id(principal: &Principal) -> Result<Uuid, RestError> {
    principal
        .group_id
        .ok_or_else(|| RestError::BadRequest("X-Group-Id header is required".into()))
}

// ─── Shared types ────────────────────────────────────────────────────────────

/// A single user mention in a doc page.
#[derive(Debug, Serialize, ToSchema)]
pub struct DocPageMentionSummary {
    /// UUID of the doc page.
    pub page_id: Uuid,
    /// UUID of the mentioned user.
    pub mentioned_user_id: Uuid,
    /// UUID of the group (denormalized).
    pub group_id: Uuid,
    /// UTC timestamp when the mention was created.
    pub created_at: DateTime<Utc>,
}

/// Response body for `GET /v1/doc-pages/{page_id}/mentions`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ListDocPageMentionsResponse {
    pub items: Vec<DocPageMentionSummary>,
    /// `mentioned_user_id` of the last item. Pass as `?after=<uuid>` for the next page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Uuid>,
}

// ─── POST /v1/doc-pages/{page_id}/mentions ────────────────────────────────────

/// Request body for `POST /v1/doc-pages/{page_id}/mentions`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AddDocPageMentionRequest {
    /// UUID of the user to mention.
    pub mentioned_user_id: Uuid,
}

/// `POST /v1/doc-pages/{page_id}/mentions` — add a user @mention to a doc page.
///
/// Idempotent: if the mention already exists, returns 200 with the existing row.
/// Returns 201 on first creation.
///
/// Authz: `DocsWrite` required.
#[utoipa::path(
    post,
    path = "/v1/doc-pages/{page_id}/mentions",
    params(
        ("page_id" = Uuid, Path, description = "Doc page UUID"),
    ),
    request_body = AddDocPageMentionRequest,
    responses(
        (status = 201, description = "Mention added.", body = DocPageMentionSummary),
        (status = 200, description = "Mention already exists (idempotent).", body = DocPageMentionSummary),
        (status = 400, description = "Validation error.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller lacks DocsWrite permission.", body = super::problem::ProblemDetails),
        (status = 404, description = "Doc page not found or not in caller's group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn add_doc_page_mention(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(page_id): Path<Uuid>,
    Json(body): Json<AddDocPageMentionRequest>,
) -> Result<(StatusCode, Json<DocPageMentionSummary>), RestError> {
    if !can(&principal, Action::DocsWrite) {
        return Err(RestError::Forbidden);
    }
    let group_id = require_group_id(&principal)?;

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

    // Verify page belongs to caller's group (RLS filters cross-group → 0 rows → 404).
    let page_exists: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM doc_pages WHERE id = $1 AND archived_at IS NULL")
            .bind(page_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

    if page_exists.is_none() {
        return Err(RestError::NotFound);
    }

    // Check for existing mention — idempotent return.
    type MentionRow = (Uuid, Uuid, Uuid, DateTime<Utc>);
    let existing: Option<MentionRow> = sqlx::query_as(
        "SELECT page_id, mentioned_user_id, group_id, created_at \
         FROM doc_page_mentions \
         WHERE page_id = $1 AND mentioned_user_id = $2",
    )
    .bind(page_id)
    .bind(body.mentioned_user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if let Some((pid, uid, gid, created_at)) = existing {
        tx.commit()
            .await
            .map_err(|e| RestError::Internal(e.into()))?;
        return Ok((
            StatusCode::OK,
            Json(DocPageMentionSummary {
                page_id: pid,
                mentioned_user_id: uid,
                group_id: gid,
                created_at,
            }),
        ));
    }

    let (pid, uid, gid, created_at): MentionRow = sqlx::query_as(
        "INSERT INTO doc_page_mentions (page_id, mentioned_user_id, group_id) \
         VALUES ($1, $2, $3) \
         RETURNING page_id, mentioned_user_id, group_id, created_at",
    )
    .bind(page_id)
    .bind(body.mentioned_user_id)
    .bind(group_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::DocPageMentionAdded,
        principal.user_id,
        group_id,
        "doc_page_mentions",
        page_id.to_string(),
        json!({ "mentioned_user_id": uid }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok((
        StatusCode::CREATED,
        Json(DocPageMentionSummary {
            page_id: pid,
            mentioned_user_id: uid,
            group_id: gid,
            created_at,
        }),
    ))
}

// ─── GET /v1/doc-pages/{page_id}/mentions ────────────────────────────────────

/// Query parameters for `GET /v1/doc-pages/{page_id}/mentions`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListDocPageMentionsQuery {
    /// Keyset cursor — `mentioned_user_id` of the last item. Returns items created
    /// after this one (exclusive). Omit for the first page.
    pub after: Option<Uuid>,
    /// Page size. Default 50, max 100.
    pub limit: Option<i64>,
}

/// `GET /v1/doc-pages/{page_id}/mentions` — list user mentions in a doc page.
///
/// Cursor-paginated, ordered by `(created_at ASC, mentioned_user_id ASC)`.
/// Authz: `DocsRead` required.
#[utoipa::path(
    get,
    path = "/v1/doc-pages/{page_id}/mentions",
    params(
        ("page_id" = Uuid, Path, description = "Doc page UUID"),
        ListDocPageMentionsQuery,
    ),
    responses(
        (status = 200, description = "List of mentions.", body = ListDocPageMentionsResponse),
        (status = 400, description = "Validation error.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller lacks DocsRead permission.", body = super::problem::ProblemDetails),
        (status = 404, description = "Doc page not found.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_doc_page_mentions(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(page_id): Path<Uuid>,
    Query(params): Query<ListDocPageMentionsQuery>,
) -> Result<Json<ListDocPageMentionsResponse>, RestError> {
    if !can(&principal, Action::DocsRead) {
        return Err(RestError::Forbidden);
    }
    let group_id = require_group_id(&principal)?;

    let limit = params
        .limit
        .map(|l| l.min(MAX_LIMIT))
        .unwrap_or(DEFAULT_LIMIT)
        .max(1);

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

    // Verify page exists in caller's group.
    let exists: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM doc_pages WHERE id = $1 AND archived_at IS NULL")
            .bind(page_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

    if exists.is_none() {
        return Err(RestError::NotFound);
    }

    type MentionRow = (Uuid, Uuid, Uuid, DateTime<Utc>);

    let rows: Vec<MentionRow> = if let Some(after_id) = params.after {
        sqlx::query_as(
            "SELECT page_id, mentioned_user_id, group_id, created_at \
             FROM doc_page_mentions \
             WHERE page_id = $1 \
               AND (created_at, mentioned_user_id) > ( \
                   SELECT created_at, mentioned_user_id \
                   FROM doc_page_mentions \
                   WHERE page_id = $1 AND mentioned_user_id = $3 \
               ) \
             ORDER BY created_at ASC, mentioned_user_id ASC \
             LIMIT $2",
        )
        .bind(page_id)
        .bind(limit)
        .bind(after_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    } else {
        sqlx::query_as(
            "SELECT page_id, mentioned_user_id, group_id, created_at \
             FROM doc_page_mentions \
             WHERE page_id = $1 \
             ORDER BY created_at ASC, mentioned_user_id ASC \
             LIMIT $2",
        )
        .bind(page_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let next_cursor = if rows.len() as i64 == limit {
        rows.last().map(|(_, uid, _, _)| *uid)
    } else {
        None
    };

    let items = rows
        .into_iter()
        .map(|(pid, uid, gid, created_at)| DocPageMentionSummary {
            page_id: pid,
            mentioned_user_id: uid,
            group_id: gid,
            created_at,
        })
        .collect();

    Ok(Json(ListDocPageMentionsResponse { items, next_cursor }))
}

// ─── DELETE /v1/doc-pages/{page_id}/mentions/{user_id} ───────────────────────

/// `DELETE /v1/doc-pages/{page_id}/mentions/{user_id}` — remove a user mention from a doc page.
///
/// Idempotent: returns 204 even if the mention does not exist.
/// Authz: `DocsWrite` required.
#[utoipa::path(
    delete,
    path = "/v1/doc-pages/{page_id}/mentions/{user_id}",
    params(
        ("page_id" = Uuid, Path, description = "Doc page UUID"),
        ("user_id" = Uuid, Path, description = "UUID of the mentioned user to remove"),
    ),
    responses(
        (status = 204, description = "Mention removed (or did not exist)."),
        (status = 400, description = "Validation error.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller lacks DocsWrite permission.", body = super::problem::ProblemDetails),
        (status = 404, description = "Doc page not found.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn delete_doc_page_mention(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((page_id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, RestError> {
    if !can(&principal, Action::DocsWrite) {
        return Err(RestError::Forbidden);
    }
    let group_id = require_group_id(&principal)?;

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

    // Verify page exists in caller's group (RLS filters cross-group → 0 rows → 404).
    let exists: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM doc_pages WHERE id = $1 AND archived_at IS NULL")
            .bind(page_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

    if exists.is_none() {
        return Err(RestError::NotFound);
    }

    // Delete idempotently — 0 rows affected is not an error.
    sqlx::query("DELETE FROM doc_page_mentions WHERE page_id = $1 AND mentioned_user_id = $2")
        .bind(page_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::NO_CONTENT)
}

// ─── Unit tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sample_mention() -> DocPageMentionSummary {
        DocPageMentionSummary {
            page_id: Uuid::nil(),
            mentioned_user_id: Uuid::nil(),
            group_id: Uuid::nil(),
            created_at: Utc.with_ymd_and_hms(2026, 6, 12, 0, 0, 0).unwrap(),
        }
    }

    #[test]
    fn mention_summary_serializes_all_fields() {
        let m = sample_mention();
        let v = serde_json::to_value(&m).unwrap();
        assert!(v.get("page_id").is_some());
        assert!(v.get("mentioned_user_id").is_some());
        assert!(v.get("group_id").is_some());
        assert!(v.get("created_at").is_some());
    }

    #[test]
    fn mention_summary_created_at_is_utc_z() {
        let m = sample_mention();
        let v = serde_json::to_value(&m).unwrap();
        let ts = v["created_at"].as_str().unwrap();
        assert!(ts.ends_with('Z'), "expected UTC Z suffix, got: {ts}");
    }

    #[test]
    fn mention_summary_nil_uuid_round_trips() {
        let m = sample_mention();
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(
            v["page_id"].as_str().unwrap(),
            "00000000-0000-0000-0000-000000000000"
        );
    }

    #[test]
    fn list_response_no_next_cursor_omitted() {
        let resp = ListDocPageMentionsResponse {
            items: vec![],
            next_cursor: None,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert!(
            v.get("next_cursor").is_none(),
            "next_cursor should be omitted when None"
        );
    }

    #[test]
    fn list_response_with_cursor_present() {
        let uid = Uuid::new_v4();
        let resp = ListDocPageMentionsResponse {
            items: vec![],
            next_cursor: Some(uid),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["next_cursor"].as_str().unwrap(), uid.to_string());
    }

    #[test]
    fn add_request_deserializes() {
        let raw = format!(r#"{{"mentioned_user_id": "{}"}}"#, Uuid::nil());
        let req: AddDocPageMentionRequest = serde_json::from_str(&raw).unwrap();
        assert_eq!(req.mentioned_user_id, Uuid::nil());
    }
}
