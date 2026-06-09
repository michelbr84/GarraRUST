//! Doc-pages handlers: `POST` and `GET` (list).
//!
//! Plan 0297 / GAR-834 — Docs Tier 2 scaffold.
//!
//! Two endpoints on the `garraia_app` RLS-enforced pool:
//! - `POST /v1/groups/{group_id}/doc-pages` — create a doc page
//! - `GET  /v1/groups/{group_id}/doc-pages` — cursor-paginated list
//!
//! ## Tenant-context protocol
//!
//! `doc_pages` uses FORCE RLS with direct `group_id` isolation (migration 026).
//! Both RLS vars are set via parameterised `set_config` before any SQL.
//!
//! ## App-layer group validation
//!
//! Path `{group_id}` must equal `principal.group_id` — mismatch returns 403.

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

const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 100;

// ─── Private DB row struct ────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct DocPageRow {
    id: Uuid,
    group_id: Uuid,
    parent_page_id: Option<Uuid>,
    title: String,
    icon: Option<String>,
    created_by: Option<Uuid>,
    created_by_label: String,
    archived_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

// ─── DTOs ─────────────────────────────────────────────────────────────────────

/// Request body for `POST /v1/groups/{group_id}/doc-pages`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateDocPageRequest {
    /// Page title. 1–255 characters.
    pub title: String,
    /// Parent page UUID. `null` or absent for a root-level page.
    pub parent_page_id: Option<Uuid>,
    /// Optional emoji or icon identifier.
    pub icon: Option<String>,
}

impl CreateDocPageRequest {
    fn validate(&self) -> Result<(), &'static str> {
        let len = self.title.chars().count();
        if len == 0 {
            return Err("title must not be empty");
        }
        if len > 255 {
            return Err("title exceeds 255 character limit");
        }
        Ok(())
    }
}

/// Full doc page representation returned by `POST`.
#[derive(Debug, Serialize, ToSchema)]
pub struct DocPageResponse {
    pub id: Uuid,
    pub group_id: Uuid,
    pub parent_page_id: Option<Uuid>,
    pub title: String,
    pub icon: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_by_label: String,
    pub archived_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<DocPageRow> for DocPageResponse {
    fn from(r: DocPageRow) -> Self {
        Self {
            id: r.id,
            group_id: r.group_id,
            parent_page_id: r.parent_page_id,
            title: r.title,
            icon: r.icon,
            created_by: r.created_by,
            created_by_label: r.created_by_label,
            archived_at: r.archived_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// Compact doc page item used in `GET /v1/groups/{group_id}/doc-pages`.
#[derive(Debug, Serialize, ToSchema)]
pub struct DocPageSummary {
    pub id: Uuid,
    pub group_id: Uuid,
    pub parent_page_id: Option<Uuid>,
    pub title: String,
    pub icon: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_by_label: String,
    pub archived_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<DocPageRow> for DocPageSummary {
    fn from(r: DocPageRow) -> Self {
        Self {
            id: r.id,
            group_id: r.group_id,
            parent_page_id: r.parent_page_id,
            title: r.title,
            icon: r.icon,
            created_by: r.created_by,
            created_by_label: r.created_by_label,
            archived_at: r.archived_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// Response body for `GET /v1/groups/{group_id}/doc-pages`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ListDocPagesResponse {
    pub items: Vec<DocPageSummary>,
    /// Cursor for the next page. `None` when the end of the list is reached.
    pub next_cursor: Option<Uuid>,
}

/// Query parameters for `GET /v1/groups/{group_id}/doc-pages`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListDocPagesQuery {
    /// Keyset cursor — UUID of the last item received. Omit for the first page.
    pub cursor: Option<Uuid>,
    /// Page size. Default 50, max 100.
    pub limit: Option<u32>,
    /// Filter to direct children of this page. Omit for all root-level pages.
    pub parent_page_id: Option<Uuid>,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

async fn set_rls_context(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
    group_id: Uuid,
) -> Result<(), RestError> {
    sqlx::query("SELECT set_config('app.current_user_id', $1, true)")
        .bind(user_id.to_string())
        .execute(&mut **tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    sqlx::query("SELECT set_config('app.current_group_id', $1, true)")
        .bind(group_id.to_string())
        .execute(&mut **tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    Ok(())
}

fn require_group_id(principal: &Principal) -> Result<Uuid, RestError> {
    principal
        .group_id
        .ok_or_else(|| RestError::BadRequest("X-Group-Id header is required".into()))
}

fn check_group_match(path_group_id: Uuid, principal_group_id: Uuid) -> Result<(), RestError> {
    if path_group_id != principal_group_id {
        Err(RestError::Forbidden)
    } else {
        Ok(())
    }
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

/// `POST /v1/groups/{group_id}/doc-pages` — create a doc page.
///
/// Authz: `Action::DocsWrite`. Path `group_id` must equal `principal.group_id`.
///
/// ## Error matrix
///
/// | Condition                          | Status |
/// |------------------------------------|--------|
/// | Missing/invalid JWT                | 401    |
/// | Non-member of group                | 403    |
/// | Path group_id ≠ principal group_id | 403    |
/// | Validation failure                 | 400    |
/// | Parent page not in group           | 404    |
/// | Happy path                         | 201    |
#[utoipa::path(
    post,
    path = "/v1/groups/{group_id}/doc-pages",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
    ),
    request_body = CreateDocPageRequest,
    responses(
        (status = 201, description = "Doc page created.", body = DocPageResponse),
        (status = 400, description = "Validation error.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::problem::ProblemDetails),
        (status = 404, description = "Parent page not found in group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn create_doc_page(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(path_group_id): Path<Uuid>,
    Json(body): Json<CreateDocPageRequest>,
) -> Result<(StatusCode, Json<DocPageResponse>), RestError> {
    let group_id = require_group_id(&principal)?;
    check_group_match(path_group_id, group_id)?;
    if !can(&principal, Action::DocsWrite) {
        return Err(RestError::Forbidden);
    }
    body.validate()
        .map_err(|msg| RestError::BadRequest(msg.into()))?;

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // Validate parent_page_id if provided.
    if let Some(parent_id) = body.parent_page_id {
        let parent_exists: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM doc_pages WHERE id = $1 AND archived_at IS NULL")
                .bind(parent_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| RestError::Internal(e.into()))?;

        if parent_exists.is_none() {
            return Err(RestError::NotFound);
        }
    }

    let (created_by_label,): (String,) =
        sqlx::query_as("SELECT display_name FROM users WHERE id = $1")
            .bind(principal.user_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

    let title_trimmed = body.title.trim().to_string();

    let row: DocPageRow = sqlx::query_as(
        "INSERT INTO doc_pages \
             (group_id, parent_page_id, title, icon, created_by, created_by_label) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         RETURNING id, group_id, parent_page_id, title, icon, \
                   created_by, created_by_label, archived_at, created_at, updated_at",
    )
    .bind(group_id)
    .bind(body.parent_page_id)
    .bind(&title_trimmed)
    .bind(&body.icon)
    .bind(principal.user_id)
    .bind(&created_by_label)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let page_id = row.id;
    let title_len = title_trimmed.chars().count();

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::DocPageCreated,
        principal.user_id,
        group_id,
        "doc_pages",
        page_id.to_string(),
        json!({ "title_len": title_len }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok((StatusCode::CREATED, Json(DocPageResponse::from(row))))
}

/// `GET /v1/groups/{group_id}/doc-pages` — list doc pages (cursor-paginated).
///
/// Returns non-archived doc pages for the caller's group, newest first.
/// Optional `?parent_page_id=` filter for tree traversal.
/// Authz: `Action::DocsRead`. Path `group_id` must equal `principal.group_id`.
///
/// ## Error matrix
///
/// | Condition                          | Status |
/// |------------------------------------|--------|
/// | Missing/invalid JWT                | 401    |
/// | Non-member of group                | 403    |
/// | Path group_id ≠ principal group_id | 403    |
/// | Happy path                         | 200    |
#[utoipa::path(
    get,
    path = "/v1/groups/{group_id}/doc-pages",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ListDocPagesQuery,
    ),
    responses(
        (status = 200, description = "Doc pages.", body = ListDocPagesResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_doc_pages(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(path_group_id): Path<Uuid>,
    Query(params): Query<ListDocPagesQuery>,
) -> Result<Json<ListDocPagesResponse>, RestError> {
    let group_id = require_group_id(&principal)?;
    check_group_match(path_group_id, group_id)?;
    if !can(&principal, Action::DocsRead) {
        return Err(RestError::Forbidden);
    }

    let effective_limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let fetch_limit = i64::from(effective_limit + 1);

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    let rows: Vec<DocPageRow> = match (params.cursor, params.parent_page_id) {
        (Some(cursor_id), Some(parent_id)) => sqlx::query_as(
            "SELECT id, group_id, parent_page_id, title, icon, \
                    created_by, created_by_label, archived_at, created_at, updated_at \
             FROM doc_pages \
             WHERE group_id = $1 \
               AND archived_at IS NULL \
               AND parent_page_id IS NOT DISTINCT FROM $2::uuid \
               AND (created_at, id) < ( \
                   SELECT created_at, id FROM doc_pages WHERE id = $3 \
               ) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $4",
        )
        .bind(group_id)
        .bind(parent_id)
        .bind(cursor_id)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        (Some(cursor_id), None) => sqlx::query_as(
            "SELECT id, group_id, parent_page_id, title, icon, \
                    created_by, created_by_label, archived_at, created_at, updated_at \
             FROM doc_pages \
             WHERE group_id = $1 \
               AND archived_at IS NULL \
               AND (created_at, id) < ( \
                   SELECT created_at, id FROM doc_pages WHERE id = $2 \
               ) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $3",
        )
        .bind(group_id)
        .bind(cursor_id)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        (None, Some(parent_id)) => sqlx::query_as(
            "SELECT id, group_id, parent_page_id, title, icon, \
                    created_by, created_by_label, archived_at, created_at, updated_at \
             FROM doc_pages \
             WHERE group_id = $1 \
               AND archived_at IS NULL \
               AND parent_page_id IS NOT DISTINCT FROM $2::uuid \
             ORDER BY created_at DESC, id DESC \
             LIMIT $3",
        )
        .bind(group_id)
        .bind(parent_id)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        (None, None) => sqlx::query_as(
            "SELECT id, group_id, parent_page_id, title, icon, \
                    created_by, created_by_label, archived_at, created_at, updated_at \
             FROM doc_pages \
             WHERE group_id = $1 \
               AND archived_at IS NULL \
             ORDER BY created_at DESC, id DESC \
             LIMIT $2",
        )
        .bind(group_id)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let has_more = rows.len() as u32 > effective_limit;
    let items: Vec<DocPageSummary> = rows
        .into_iter()
        .take(effective_limit as usize)
        .map(DocPageSummary::from)
        .collect();
    let next_cursor = if has_more {
        items.last().map(|it| it.id)
    } else {
        None
    };

    Ok(Json(ListDocPagesResponse { items, next_cursor }))
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn make_row(id: Uuid, group_id: Uuid, title: &str) -> DocPageRow {
        DocPageRow {
            id,
            group_id,
            parent_page_id: None,
            title: title.to_string(),
            icon: None,
            created_by: Some(Uuid::nil()),
            created_by_label: "Alice".to_string(),
            archived_at: None,
            created_at: Utc.with_ymd_and_hms(2026, 6, 9, 12, 0, 0).unwrap(),
            updated_at: Utc.with_ymd_and_hms(2026, 6, 9, 12, 0, 0).unwrap(),
        }
    }

    #[test]
    fn doc_page_response_serializes_all_fields() {
        let id = Uuid::new_v4();
        let group_id = Uuid::new_v4();
        let resp = DocPageResponse::from(make_row(id, group_id, "Getting Started"));
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["id"], id.to_string());
        assert_eq!(v["group_id"], group_id.to_string());
        assert_eq!(v["title"], "Getting Started");
        assert!(v["parent_page_id"].is_null());
        assert!(v["icon"].is_null());
        assert!(v["archived_at"].is_null());
        assert_eq!(v["created_by"], Uuid::nil().to_string());
        assert_eq!(v["created_by_label"], "Alice");
    }

    #[test]
    fn doc_page_summary_mirrors_response_fields() {
        let id = Uuid::new_v4();
        let group_id = Uuid::new_v4();
        let summary = DocPageSummary::from(make_row(id, group_id, "Intro"));
        let v = serde_json::to_value(&summary).unwrap();
        assert_eq!(v["id"], id.to_string());
        assert_eq!(v["title"], "Intro");
    }

    #[test]
    fn nil_created_by_serializes_as_null() {
        let mut row = make_row(Uuid::new_v4(), Uuid::new_v4(), "Page");
        row.created_by = None;
        let resp = DocPageResponse::from(row);
        let v = serde_json::to_value(&resp).unwrap();
        assert!(v["created_by"].is_null());
    }

    #[test]
    fn icon_roundtrip() {
        let mut row = make_row(Uuid::new_v4(), Uuid::new_v4(), "Page");
        row.icon = Some("📝".to_string());
        let resp = DocPageResponse::from(row);
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["icon"], "📝");
    }

    #[test]
    fn list_doc_pages_response_no_next_cursor_when_exact_page() {
        let items: Vec<DocPageSummary> = (0..3)
            .map(|_| DocPageSummary::from(make_row(Uuid::new_v4(), Uuid::new_v4(), "P")))
            .collect();
        let resp = ListDocPagesResponse {
            items,
            next_cursor: None,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 3);
        assert!(v["next_cursor"].is_null());
    }

    #[test]
    fn create_request_validation_rejects_empty_title() {
        let req = CreateDocPageRequest {
            title: "".to_string(),
            parent_page_id: None,
            icon: None,
        };
        assert_eq!(req.validate(), Err("title must not be empty"));
    }
}
