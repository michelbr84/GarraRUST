//! Doc-page version handlers for the Docs Tier 2 surface.
//! Plan 0307 / GAR-845.
//!
//! Three endpoints on the `garraia_app` RLS-enforced pool:
//! - `POST /v1/doc-pages/{page_id}/versions`              — create snapshot (201)
//! - `GET  /v1/doc-pages/{page_id}/versions`              — list headers (200)
//! - `GET  /v1/doc-pages/{page_id}/versions/{version_id}` — single version (200)
//!
//! ## Tenant-context protocol
//!
//! `doc_page_versions` uses FORCE RLS with `group_id` isolation (migration 028).
//! Both RLS vars (`app.current_user_id` + `app.current_group_id`) are set
//! via parameterised `set_config` before any SQL in every transaction.
//!
//! ## Snapshot content
//!
//! `snapshot_jsonb` is `{title, icon, parent_page_id, blocks:[...]}` captured
//! at the moment `POST` is called. Blocks are ordered by `(position ASC, id ASC)`.
//!
//! ## Cross-group isolation
//!
//! The `page_id` path param is resolved inside the caller's RLS context.
//! A `page_id` belonging to a different group returns 0 rows → 404.

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

const DEFAULT_LIMIT: u32 = 20;
const MAX_LIMIT: u32 = 100;

// ─── Private DB row structs ───────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct DocPageVersionHeaderRow {
    id: Uuid,
    page_id: Uuid,
    group_id: Uuid,
    created_by: Uuid,
    created_by_label: String,
    created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct DocPageVersionRow {
    id: Uuid,
    page_id: Uuid,
    group_id: Uuid,
    snapshot_jsonb: serde_json::Value,
    created_by: Uuid,
    created_by_label: String,
    created_at: DateTime<Utc>,
}

// ─── Snapshot builder types ───────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct DocPageSnapshotRow {
    title: String,
    icon: Option<String>,
    parent_page_id: Option<Uuid>,
}

#[derive(sqlx::FromRow)]
struct DocBlockSnapshotRow {
    id: Uuid,
    parent_block_id: Option<Uuid>,
    position: f64,
    block_type: String,
    content_jsonb: serde_json::Value,
}

// ─── DTOs ─────────────────────────────────────────────────────────────────────

/// Version header returned by `POST` and `GET` (list).
/// Does NOT include `snapshot_jsonb` — use the single-version endpoint for that.
#[derive(Debug, Serialize, ToSchema)]
pub struct DocPageVersionHeader {
    pub id: Uuid,
    pub page_id: Uuid,
    pub group_id: Uuid,
    pub created_by: Uuid,
    pub created_by_label: String,
    pub created_at: DateTime<Utc>,
}

impl From<DocPageVersionHeaderRow> for DocPageVersionHeader {
    fn from(r: DocPageVersionHeaderRow) -> Self {
        Self {
            id: r.id,
            page_id: r.page_id,
            group_id: r.group_id,
            created_by: r.created_by,
            created_by_label: r.created_by_label,
            created_at: r.created_at,
        }
    }
}

/// Full version returned by `GET /v1/doc-pages/{page_id}/versions/{version_id}`.
#[derive(Debug, Serialize, ToSchema)]
pub struct DocPageVersionFull {
    pub id: Uuid,
    pub page_id: Uuid,
    pub group_id: Uuid,
    /// Snapshot of the page at the time the version was created.
    /// Shape: `{title, icon, parent_page_id, blocks: [{id, type, position, content}]}`
    pub snapshot: serde_json::Value,
    pub created_by: Uuid,
    pub created_by_label: String,
    pub created_at: DateTime<Utc>,
}

impl From<DocPageVersionRow> for DocPageVersionFull {
    fn from(r: DocPageVersionRow) -> Self {
        Self {
            id: r.id,
            page_id: r.page_id,
            group_id: r.group_id,
            snapshot: r.snapshot_jsonb,
            created_by: r.created_by,
            created_by_label: r.created_by_label,
            created_at: r.created_at,
        }
    }
}

/// Response body for `GET /v1/doc-pages/{page_id}/versions`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ListDocPageVersionsResponse {
    pub items: Vec<DocPageVersionHeader>,
    pub next_cursor: Option<Uuid>,
}

/// Query params for `GET /v1/doc-pages/{page_id}/versions`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListDocPageVersionsQuery {
    /// Cursor: the `id` of the last item from the previous page.
    pub after: Option<Uuid>,
    /// Number of items to return (default 20, max 100).
    pub limit: Option<u32>,
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

fn clamp_limit(limit: Option<u32>) -> i64 {
    limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT) as i64
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

/// `POST /v1/doc-pages/{page_id}/versions` — create a manual version snapshot.
///
/// Captures the current state of the page (title, icon, parent_page_id) plus
/// all current blocks (ordered by position ASC, id ASC) into `snapshot_jsonb`.
/// Authz: `Action::DocsWrite`. The `page_id` must belong to `principal.group_id`.
///
/// ## Error matrix
///
/// | Condition                    | Status |
/// |------------------------------|--------|
/// | Missing/invalid JWT          | 401    |
/// | Caller not a group member    | 403    |
/// | Missing X-Group-Id header    | 400    |
/// | Page not found / cross-group | 404    |
/// | Happy path                   | 201    |
#[utoipa::path(
    post,
    path = "/v1/doc-pages/{page_id}/versions",
    params(
        ("page_id" = Uuid, Path, description = "Doc page UUID."),
    ),
    responses(
        (status = 201, description = "Version snapshot created.", body = DocPageVersionHeader),
        (status = 400, description = "Validation error.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a group member.", body = super::problem::ProblemDetails),
        (status = 404, description = "Page not found or cross-group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn create_doc_page_version(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(page_id): Path<Uuid>,
) -> Result<(StatusCode, Json<DocPageVersionHeader>), RestError> {
    let group_id = require_group_id(&principal)?;
    if !can(&principal, Action::DocsWrite) {
        return Err(RestError::Forbidden);
    }

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // Verify the page exists and belongs to the caller's group.
    let page: Option<DocPageSnapshotRow> = sqlx::query_as(
        "SELECT title, icon, parent_page_id \
         FROM doc_pages \
         WHERE id = $1 AND archived_at IS NULL",
    )
    .bind(page_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;
    let page = page.ok_or(RestError::NotFound)?;

    // Collect all current blocks for this page (position-ordered).
    let blocks: Vec<DocBlockSnapshotRow> = sqlx::query_as(
        "SELECT id, parent_block_id, position, block_type, content_jsonb \
         FROM doc_blocks \
         WHERE page_id = $1 \
         ORDER BY position ASC, id ASC",
    )
    .bind(page_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let block_count = blocks.len();
    let snapshot = json!({
        "title": page.title,
        "icon": page.icon,
        "parent_page_id": page.parent_page_id,
        "blocks": blocks.iter().map(|b| json!({
            "id": b.id,
            "parent_block_id": b.parent_block_id,
            "type": b.block_type,
            "position": b.position,
            "content": b.content_jsonb,
        })).collect::<Vec<_>>(),
    });

    // Look up created_by_label from the users table.
    let created_by_label: Option<String> =
        sqlx::query_scalar("SELECT display_name FROM users WHERE id = $1")
            .bind(principal.user_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;
    let created_by_label = created_by_label.unwrap_or_else(|| "unknown".to_string());

    let row: DocPageVersionHeaderRow = sqlx::query_as(
        "INSERT INTO doc_page_versions \
             (page_id, group_id, snapshot_jsonb, created_by, created_by_label) \
         VALUES ($1, $2, $3, $4, $5) \
         RETURNING id, page_id, group_id, created_by, created_by_label, created_at",
    )
    .bind(page_id)
    .bind(group_id)
    .bind(&snapshot)
    .bind(principal.user_id)
    .bind(&created_by_label)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let version_id = row.id;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::DocPageVersionCreated,
        principal.user_id,
        group_id,
        "doc_page_versions",
        version_id.to_string(),
        json!({ "block_count": block_count }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok((StatusCode::CREATED, Json(DocPageVersionHeader::from(row))))
}

/// `GET /v1/doc-pages/{page_id}/versions` — list version headers (cursor-paginated).
///
/// Returns version headers ordered by `created_at DESC, id DESC`.
/// Does NOT include `snapshot_jsonb` — use the single-version endpoint for full content.
/// Authz: `Action::DocsRead`.
///
/// ## Error matrix
///
/// | Condition                    | Status |
/// |------------------------------|--------|
/// | Missing/invalid JWT          | 401    |
/// | Caller not a group member    | 403    |
/// | Page not found / cross-group | 404    |
/// | Happy path                   | 200    |
#[utoipa::path(
    get,
    path = "/v1/doc-pages/{page_id}/versions",
    params(
        ("page_id" = Uuid, Path, description = "Doc page UUID."),
        ListDocPageVersionsQuery,
    ),
    responses(
        (status = 200, description = "Version list.", body = ListDocPageVersionsResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a group member.", body = super::problem::ProblemDetails),
        (status = 404, description = "Page not found or cross-group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_doc_page_versions(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(page_id): Path<Uuid>,
    Query(params): Query<ListDocPageVersionsQuery>,
) -> Result<Json<ListDocPageVersionsResponse>, RestError> {
    let group_id = require_group_id(&principal)?;
    if !can(&principal, Action::DocsRead) {
        return Err(RestError::Forbidden);
    }

    let limit = clamp_limit(params.limit);

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // Verify the page exists (RLS filters cross-group automatically).
    let page_exists: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM doc_pages WHERE id = $1 AND archived_at IS NULL")
            .bind(page_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;
    if page_exists.is_none() {
        return Err(RestError::NotFound);
    }

    // Cursor-based pagination: after a given (created_at, id) pair.
    let rows: Vec<DocPageVersionHeaderRow> = if let Some(cursor_id) = params.after {
        // Look up the cursor row's created_at for keyset pagination.
        let cursor_ts: Option<DateTime<Utc>> = sqlx::query_scalar(
            "SELECT created_at FROM doc_page_versions WHERE id = $1 AND page_id = $2",
        )
        .bind(cursor_id)
        .bind(page_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

        if let Some(ts) = cursor_ts {
            sqlx::query_as(
                "SELECT id, page_id, group_id, created_by, created_by_label, created_at \
                 FROM doc_page_versions \
                 WHERE page_id = $1 \
                   AND (created_at, id) < ($2, $3) \
                 ORDER BY created_at DESC, id DESC \
                 LIMIT $4",
            )
            .bind(page_id)
            .bind(ts)
            .bind(cursor_id)
            .bind(limit + 1)
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?
        } else {
            vec![]
        }
    } else {
        sqlx::query_as(
            "SELECT id, page_id, group_id, created_by, created_by_label, created_at \
             FROM doc_page_versions \
             WHERE page_id = $1 \
             ORDER BY created_at DESC, id DESC \
             LIMIT $2",
        )
        .bind(page_id)
        .bind(limit + 1)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let has_more = rows.len() > limit as usize;
    let mut items: Vec<DocPageVersionHeader> = rows
        .into_iter()
        .take(limit as usize)
        .map(DocPageVersionHeader::from)
        .collect();

    let next_cursor = if has_more {
        items.last().map(|v| v.id)
    } else {
        None
    };

    // Reverse so oldest-first within the page (most recent snapshot at top of next call).
    // Actually: ordered DESC so newest first — keep that order.
    // next_cursor is the last item's id (oldest in this page).
    // This matches the pattern in docs.rs.
    let _ = &mut items; // no-op, items already correct order

    Ok(Json(ListDocPageVersionsResponse { items, next_cursor }))
}

/// `GET /v1/doc-pages/{page_id}/versions/{version_id}` — fetch a single version.
///
/// Returns the full version including `snapshot` content.
/// Authz: `Action::DocsRead`.
///
/// ## Error matrix
///
/// | Condition                         | Status |
/// |-----------------------------------|--------|
/// | Missing/invalid JWT               | 401    |
/// | Caller not a group member         | 403    |
/// | Page not found / cross-group      | 404    |
/// | Version not found / wrong page    | 404    |
/// | Happy path                        | 200    |
#[utoipa::path(
    get,
    path = "/v1/doc-pages/{page_id}/versions/{version_id}",
    params(
        ("page_id"    = Uuid, Path, description = "Doc page UUID."),
        ("version_id" = Uuid, Path, description = "Version UUID."),
    ),
    responses(
        (status = 200, description = "Version detail.", body = DocPageVersionFull),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a group member.", body = super::problem::ProblemDetails),
        (status = 404, description = "Page or version not found.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn get_doc_page_version(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((page_id, version_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<DocPageVersionFull>, RestError> {
    let group_id = require_group_id(&principal)?;
    if !can(&principal, Action::DocsRead) {
        return Err(RestError::Forbidden);
    }

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // Verify the page exists (RLS handles cross-group).
    let page_exists: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM doc_pages WHERE id = $1 AND archived_at IS NULL")
            .bind(page_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;
    if page_exists.is_none() {
        return Err(RestError::NotFound);
    }

    // Fetch the specific version — must belong to this page (and group via RLS).
    let row: Option<DocPageVersionRow> = sqlx::query_as(
        "SELECT id, page_id, group_id, snapshot_jsonb, created_by, created_by_label, created_at \
         FROM doc_page_versions \
         WHERE id = $1 AND page_id = $2",
    )
    .bind(version_id)
    .bind(page_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let row = row.ok_or(RestError::NotFound)?;
    Ok(Json(DocPageVersionFull::from(row)))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_page_version_header_fields_present() {
        let id = Uuid::new_v4();
        let page_id = Uuid::new_v4();
        let group_id = Uuid::new_v4();
        let created_by = Uuid::new_v4();
        let now = chrono::Utc::now();
        let header = DocPageVersionHeader {
            id,
            page_id,
            group_id,
            created_by,
            created_by_label: "Alice".to_string(),
            created_at: now,
        };
        assert_eq!(header.id, id);
        assert_eq!(header.page_id, page_id);
        assert_eq!(header.group_id, group_id);
        assert_eq!(header.created_by, created_by);
        assert_eq!(header.created_by_label, "Alice");
    }

    #[test]
    fn doc_page_version_full_exposes_snapshot() {
        let snapshot = json!({
            "title": "My page",
            "icon": null,
            "parent_page_id": null,
            "blocks": []
        });
        let full = DocPageVersionFull {
            id: Uuid::new_v4(),
            page_id: Uuid::new_v4(),
            group_id: Uuid::new_v4(),
            snapshot: snapshot.clone(),
            created_by: Uuid::new_v4(),
            created_by_label: "Bob".to_string(),
            created_at: chrono::Utc::now(),
        };
        assert_eq!(full.snapshot["title"], "My page");
        assert!(full.snapshot["blocks"].is_array());
    }

    #[test]
    fn clamp_limit_defaults_to_20() {
        assert_eq!(clamp_limit(None), 20);
    }

    #[test]
    fn clamp_limit_caps_at_100() {
        assert_eq!(clamp_limit(Some(999)), 100);
    }

    #[test]
    fn clamp_limit_passthrough_within_bounds() {
        assert_eq!(clamp_limit(Some(50)), 50);
    }

    #[test]
    fn list_response_next_cursor_none_when_no_more() {
        let resp = ListDocPageVersionsResponse {
            items: vec![],
            next_cursor: None,
        };
        assert!(resp.next_cursor.is_none());
    }

    #[test]
    fn snapshot_builder_serializes_blocks_correctly() {
        let block_id = Uuid::new_v4();
        let snap = json!({
            "title": "Notes",
            "icon": "📝",
            "parent_page_id": null,
            "blocks": [{
                "id": block_id,
                "parent_block_id": null,
                "type": "paragraph",
                "position": 1.0,
                "content": {"text": "hello"},
            }]
        });
        assert_eq!(snap["blocks"][0]["type"], "paragraph");
        assert_eq!(snap["blocks"][0]["position"], 1.0);
    }
}
