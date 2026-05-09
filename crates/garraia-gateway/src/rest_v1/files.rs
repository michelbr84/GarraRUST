//! `/v1/groups/{group_id}/files`, `/v1/groups/{group_id}/folders`,
//! and `DELETE /v1/files/{file_id}` handlers
//! (plan 0088, GAR-555, Fase 3.4 files slice 1).
//!
//! Three endpoints on the `garraia_app` RLS-enforced pool:
//! - `GET /v1/groups/{group_id}/files?folder_id=&cursor=&limit=` — cursor-paginated list
//! - `GET /v1/groups/{group_id}/folders?parent_id=&cursor=&limit=` — cursor-paginated list
//! - `DELETE /v1/files/{file_id}` — idempotent soft-delete
//!
//! ## Tenant-context protocol
//!
//! `files` and `folders` use FORCE RLS via `app.current_group_id` (migration 003).
//! Both RLS vars set via parameterized `set_config` (plan 0056 pattern).
//!
//! ## Cross-group protection
//!
//! For group-path endpoints: `path_group_id` must equal `principal.group_id` → 403.
//! For `DELETE /v1/files/{file_id}`: no group_id in path; RLS filters silently → 404.

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

// ─── Constants ────────────────────────────────────────────────────────────────

const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 100;

// ─── Private row types ────────────────────────────────────────────────────────

/// Row for file list queries.
#[derive(sqlx::FromRow)]
struct FileRow {
    id: Uuid,
    name: String,
    mime_type: String,
    size_bytes: i64,
    current_version: i32,
    total_versions: i32,
    folder_id: Option<Uuid>,
    created_by: Option<Uuid>,
    created_by_label: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// Row for folder list queries.
#[derive(sqlx::FromRow)]
struct FolderRow {
    id: Uuid,
    name: String,
    parent_id: Option<Uuid>,
    created_by: Option<Uuid>,
    created_by_label: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

// ─── DTOs ─────────────────────────────────────────────────────────────────────

/// File metadata returned in list responses.
#[derive(Debug, Serialize, ToSchema)]
pub struct FileSummary {
    pub id: Uuid,
    pub name: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub current_version: i32,
    pub total_versions: i32,
    /// `null` for root-level files (no folder).
    pub folder_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
    pub created_by_label: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<FileRow> for FileSummary {
    fn from(r: FileRow) -> Self {
        Self {
            id: r.id,
            name: r.name,
            mime_type: r.mime_type,
            size_bytes: r.size_bytes,
            current_version: r.current_version,
            total_versions: r.total_versions,
            folder_id: r.folder_id,
            created_by: r.created_by,
            created_by_label: r.created_by_label,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// Folder metadata returned in list responses.
#[derive(Debug, Serialize, ToSchema)]
pub struct FolderSummary {
    pub id: Uuid,
    pub name: String,
    /// `null` for root folders.
    pub parent_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
    pub created_by_label: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<FolderRow> for FolderSummary {
    fn from(r: FolderRow) -> Self {
        Self {
            id: r.id,
            name: r.name,
            parent_id: r.parent_id,
            created_by: r.created_by,
            created_by_label: r.created_by_label,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// Response body for `GET /v1/groups/{group_id}/files`.
#[derive(Debug, Serialize, ToSchema)]
pub struct FileListResponse {
    pub items: Vec<FileSummary>,
    /// Opaque cursor for the next page. `null` when the list is exhausted.
    pub next_cursor: Option<Uuid>,
}

/// Response body for `GET /v1/groups/{group_id}/folders`.
#[derive(Debug, Serialize, ToSchema)]
pub struct FolderListResponse {
    pub items: Vec<FolderSummary>,
    /// Opaque cursor for the next page. `null` when the list is exhausted.
    pub next_cursor: Option<Uuid>,
}

/// Query parameters for `GET /v1/groups/{group_id}/files`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListFilesQuery {
    /// Filter to a specific folder UUID. Omit to list all files in the group.
    pub folder_id: Option<Uuid>,
    /// Cursor UUID (last seen file id). Omit for the first page.
    pub cursor: Option<Uuid>,
    /// Page size. Default 50, max 100.
    pub limit: Option<u32>,
}

/// Query parameters for `GET /v1/groups/{group_id}/folders`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListFoldersQuery {
    /// Parent folder UUID. Omit (or `null`) to list root-level folders.
    pub parent_id: Option<Uuid>,
    /// Cursor UUID (last seen folder id). Omit for the first page.
    pub cursor: Option<Uuid>,
    /// Page size. Default 50, max 100.
    pub limit: Option<u32>,
}

/// Body for `PATCH /v1/groups/{group_id}/files/{file_id}` (plan 0089, GAR-557).
///
/// Only `name` is mutable in slice 2. Extra keys are silently ignored
/// per `serde::Deserialize` defaults — we only act on the fields we
/// declare here. Future slices that allow folder moves or settings
/// patches will add fields and be gated by their own validations.
#[derive(Debug, Deserialize, ToSchema)]
pub struct PatchFileRequest {
    /// New file name. Trimmed before validation. Length must be 1..=500
    /// chars after trim, must not contain `/` or NUL byte.
    pub name: String,
}

/// Validation error message family. Plain strings — never embed
/// the offending value (it is user-controlled and may include PII).
const ERR_NAME_EMPTY: &str = "name must not be empty after trim";
const ERR_NAME_TOO_LONG: &str = "name exceeds 500 characters";
const ERR_NAME_HAS_SLASH: &str = "name must not contain '/'";
const ERR_NAME_HAS_NUL: &str = "name must not contain NUL byte";

/// Validate a candidate name. Returns the trimmed name on success or
/// a user-safe error string on failure.
fn validate_file_name(raw: &str) -> Result<String, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ERR_NAME_EMPTY);
    }
    if trimmed.chars().count() > 500 {
        return Err(ERR_NAME_TOO_LONG);
    }
    if trimmed.contains('/') {
        return Err(ERR_NAME_HAS_SLASH);
    }
    if trimmed.contains('\0') {
        return Err(ERR_NAME_HAS_NUL);
    }
    Ok(trimmed.to_string())
}

// ─── Helper ───────────────────────────────────────────────────────────────────

/// Set RLS GUCs for the transaction (plan 0056 pattern).
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

/// Extract and validate `principal.group_id`. Returns 400 if missing.
fn require_group_id(principal: &Principal) -> Result<Uuid, RestError> {
    principal
        .group_id
        .ok_or_else(|| RestError::BadRequest("X-Group-Id header is required".into()))
}

/// Verify path group_id matches principal's group. Returns 403 on mismatch.
fn check_group_match(path_group_id: Uuid, principal_group_id: Uuid) -> Result<(), RestError> {
    if path_group_id != principal_group_id {
        return Err(RestError::Forbidden);
    }
    Ok(())
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

/// `GET /v1/groups/{group_id}/files` — cursor-paginated file listing.
///
/// When `folder_id` is supplied, only files directly inside that folder are
/// returned. When omitted, all non-deleted files in the group are returned.
/// Ordering: `(created_at DESC, id DESC)`.
///
/// Authz: `Action::FilesRead`.
///
/// ## Error matrix
///
/// | Condition                          | Status |
/// |------------------------------------|--------|
/// | Missing/invalid JWT                | 401    |
/// | Not a group member                 | 403    |
/// | Path group_id ≠ principal group_id | 403    |
/// | Happy path                         | 200    |
#[utoipa::path(
    get,
    path = "/v1/groups/{group_id}/files",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ListFilesQuery,
    ),
    responses(
        (status = 200, description = "File list.", body = FileListResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_files(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(path_group_id): Path<Uuid>,
    Query(params): Query<ListFilesQuery>,
) -> Result<Json<FileListResponse>, RestError> {
    let group_id = require_group_id(&principal)?;
    check_group_match(path_group_id, group_id)?;
    if !can(&principal, Action::FilesRead) {
        return Err(RestError::Forbidden);
    }

    let effective_limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let fetch_limit = (effective_limit + 1) as i64;

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    let rows: Vec<FileRow> = match params.cursor {
        Some(cursor_id) => sqlx::query_as(
            "SELECT f.id, f.name, f.mime_type, f.size_bytes, \
                         f.current_version, f.total_versions, f.folder_id, \
                         f.created_by, f.created_by_label, f.created_at, f.updated_at \
                  FROM files f \
                  WHERE f.deleted_at IS NULL \
                    AND ($1::uuid IS NULL OR f.folder_id = $1::uuid) \
                    AND (f.created_at, f.id) < ( \
                        SELECT created_at, id FROM files WHERE id = $2 \
                    ) \
                  ORDER BY f.created_at DESC, f.id DESC \
                  LIMIT $3",
        )
        .bind(params.folder_id)
        .bind(cursor_id)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
        None => sqlx::query_as(
            "SELECT f.id, f.name, f.mime_type, f.size_bytes, \
                         f.current_version, f.total_versions, f.folder_id, \
                         f.created_by, f.created_by_label, f.created_at, f.updated_at \
                  FROM files f \
                  WHERE f.deleted_at IS NULL \
                    AND ($1::uuid IS NULL OR f.folder_id = $1::uuid) \
                  ORDER BY f.created_at DESC, f.id DESC \
                  LIMIT $2",
        )
        .bind(params.folder_id)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let has_next = rows.len() > effective_limit as usize;
    let mut items: Vec<FileSummary> = rows
        .into_iter()
        .take(effective_limit as usize)
        .map(FileSummary::from)
        .collect();
    let next_cursor = if has_next {
        items.last().map(|f| f.id)
    } else {
        None
    };
    if has_next {
        items.pop();
    }

    Ok(Json(FileListResponse { items, next_cursor }))
}

/// `GET /v1/groups/{group_id}/folders` — cursor-paginated folder listing.
///
/// When `parent_id` is supplied, only direct children of that folder are
/// returned. When omitted, only root-level folders (`parent_id IS NULL`)
/// are returned. Ordering: `(created_at DESC, id DESC)`.
///
/// Authz: `Action::FilesRead`.
///
/// ## Error matrix
///
/// | Condition                          | Status |
/// |------------------------------------|--------|
/// | Missing/invalid JWT                | 401    |
/// | Not a group member                 | 403    |
/// | Path group_id ≠ principal group_id | 403    |
/// | Happy path                         | 200    |
#[utoipa::path(
    get,
    path = "/v1/groups/{group_id}/folders",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ListFoldersQuery,
    ),
    responses(
        (status = 200, description = "Folder list.", body = FolderListResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or group mismatch.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_folders(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(path_group_id): Path<Uuid>,
    Query(params): Query<ListFoldersQuery>,
) -> Result<Json<FolderListResponse>, RestError> {
    let group_id = require_group_id(&principal)?;
    check_group_match(path_group_id, group_id)?;
    if !can(&principal, Action::FilesRead) {
        return Err(RestError::Forbidden);
    }

    let effective_limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let fetch_limit = (effective_limit + 1) as i64;

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    let rows: Vec<FolderRow> = match params.cursor {
        Some(cursor_id) => sqlx::query_as(
            "SELECT f.id, f.name, f.parent_id, \
                         f.created_by, f.created_by_label, f.created_at, f.updated_at \
                  FROM folders f \
                  WHERE f.deleted_at IS NULL \
                    AND f.parent_id IS NOT DISTINCT FROM $1::uuid \
                    AND (f.created_at, f.id) < ( \
                        SELECT created_at, id FROM folders WHERE id = $2 \
                    ) \
                  ORDER BY f.created_at DESC, f.id DESC \
                  LIMIT $3",
        )
        .bind(params.parent_id)
        .bind(cursor_id)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
        None => sqlx::query_as(
            "SELECT f.id, f.name, f.parent_id, \
                         f.created_by, f.created_by_label, f.created_at, f.updated_at \
                  FROM folders f \
                  WHERE f.deleted_at IS NULL \
                    AND f.parent_id IS NOT DISTINCT FROM $1::uuid \
                  ORDER BY f.created_at DESC, f.id DESC \
                  LIMIT $2",
        )
        .bind(params.parent_id)
        .bind(fetch_limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let has_next = rows.len() > effective_limit as usize;
    let mut items: Vec<FolderSummary> = rows
        .into_iter()
        .take(effective_limit as usize)
        .map(FolderSummary::from)
        .collect();
    let next_cursor = if has_next {
        items.last().map(|f| f.id)
    } else {
        None
    };
    if has_next {
        items.pop();
    }

    Ok(Json(FolderListResponse { items, next_cursor }))
}

/// `DELETE /v1/files/{file_id}` — idempotent soft-delete.
///
/// Sets `deleted_at = now()` on the file. Idempotent: if the file is already
/// soft-deleted, returns 204 without emitting an audit event. Cross-group
/// access is rejected by RLS (the file is invisible) → 404.
///
/// Authz: `Action::FilesDelete`.
///
/// ## Error matrix
///
/// | Condition                           | Status |
/// |-------------------------------------|--------|
/// | Missing/invalid JWT                 | 401    |
/// | Not a group member                  | 403    |
/// | Insufficient role (< Member)        | 403    |
/// | File not found / cross-tenant       | 404    |
/// | File already soft-deleted           | 204    |
/// | Happy path                          | 204    |
#[utoipa::path(
    delete,
    path = "/v1/files/{file_id}",
    params(
        ("file_id" = Uuid, Path, description = "File UUID."),
    ),
    responses(
        (status = 204, description = "File deleted (or already deleted)."),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member or lacks FilesDelete.", body = super::problem::ProblemDetails),
        (status = 404, description = "File not found or cross-tenant.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn delete_file(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(file_id): Path<Uuid>,
) -> Result<StatusCode, RestError> {
    let group_id = require_group_id(&principal)?;
    if !can(&principal, Action::FilesDelete) {
        return Err(RestError::Forbidden);
    }

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    // Fetch the file regardless of deleted_at to distinguish:
    //   - not found / cross-group (0 rows) → 404
    //   - already deleted (deleted_at IS NOT NULL) → 204 idempotent, no audit
    //   - live file (deleted_at IS NULL) → UPDATE + audit + 204
    let existing: Option<(Option<DateTime<Utc>>, String)> =
        sqlx::query_as("SELECT deleted_at, name FROM files WHERE id = $1 AND group_id = $2")
            .bind(file_id)
            .bind(group_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

    let (deleted_at, name) = match existing {
        Some(row) => row,
        None => return Err(RestError::NotFound),
    };

    if deleted_at.is_some() {
        // Already deleted — idempotent 204 without audit.
        tx.commit()
            .await
            .map_err(|e| RestError::Internal(e.into()))?;
        return Ok(StatusCode::NO_CONTENT);
    }

    let name_len = name.chars().count();

    sqlx::query("UPDATE files SET deleted_at = now() WHERE id = $1 AND deleted_at IS NULL")
        .bind(file_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::FileDeleted,
        principal.user_id,
        group_id,
        "files",
        file_id.to_string(),
        json!({ "name_len": name_len, "group_id": group_id }),
    )
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::NO_CONTENT)
}

/// `PATCH /v1/groups/{group_id}/files/{file_id}` — rename a file
/// (plan 0089, GAR-557, Fase 3.4 files slice 2).
///
/// Only the `name` field may change. Returns the updated `FileSummary`
/// (200) on success.
///
/// Authz: `Action::FilesWrite`.
///
/// ## Error matrix
///
/// | Condition                                    | Status |
/// |----------------------------------------------|--------|
/// | Missing/invalid JWT                          | 401    |
/// | Path group_id ≠ principal group_id           | 403    |
/// | Caller lacks `FilesWrite`                    | 403    |
/// | Body name empty/too long/has '/' or NUL      | 400    |
/// | File not found, soft-deleted, or cross-group | 404    |
/// | Happy path                                   | 200    |
#[utoipa::path(
    patch,
    path = "/v1/groups/{group_id}/files/{file_id}",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID."),
        ("file_id" = Uuid, Path, description = "File UUID."),
    ),
    request_body = PatchFileRequest,
    responses(
        (status = 200, description = "File renamed.", body = FileSummary),
        (status = 400, description = "Invalid name.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller lacks FilesWrite or group mismatch.", body = super::problem::ProblemDetails),
        (status = 404, description = "File not found, soft-deleted, or cross-group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn patch_file(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((path_group_id, file_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PatchFileRequest>,
) -> Result<Json<FileSummary>, RestError> {
    let group_id = require_group_id(&principal)?;
    check_group_match(path_group_id, group_id)?;
    if !can(&principal, Action::FilesWrite) {
        return Err(RestError::Forbidden);
    }

    let new_name =
        validate_file_name(&body.name).map_err(|msg| RestError::BadRequest(msg.into()))?;
    let name_len = new_name.chars().count();

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;
    set_rls_context(&mut tx, principal.user_id, group_id).await?;

    let row: Option<FileRow> = sqlx::query_as(
        "UPDATE files \
         SET name = $1, updated_at = now() \
         WHERE id = $2 AND group_id = $3 AND deleted_at IS NULL \
         RETURNING id, name, mime_type, size_bytes, current_version, \
                   total_versions, folder_id, created_by, created_by_label, \
                   created_at, updated_at",
    )
    .bind(&new_name)
    .bind(file_id)
    .bind(group_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let row = row.ok_or(RestError::NotFound)?;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::FileRenamed,
        principal.user_id,
        group_id,
        "files",
        file_id.to_string(),
        json!({ "name_len": name_len, "group_id": group_id }),
    )
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(Json(FileSummary::from(row)))
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_summary_from_row_preserves_fields() {
        let now = Utc::now();
        let file_id = Uuid::new_v4();
        let folder_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let row = FileRow {
            id: file_id,
            name: "report.pdf".to_string(),
            mime_type: "application/pdf".to_string(),
            size_bytes: 12345,
            current_version: 2,
            total_versions: 3,
            folder_id: Some(folder_id),
            created_by: Some(user_id),
            created_by_label: "Alice".to_string(),
            created_at: now,
            updated_at: now,
        };
        let summary = FileSummary::from(row);
        assert_eq!(summary.id, file_id);
        assert_eq!(summary.name, "report.pdf");
        assert_eq!(summary.mime_type, "application/pdf");
        assert_eq!(summary.size_bytes, 12345);
        assert_eq!(summary.current_version, 2);
        assert_eq!(summary.total_versions, 3);
        assert_eq!(summary.folder_id, Some(folder_id));
        assert_eq!(summary.created_by_label, "Alice");
    }

    #[test]
    fn folder_summary_from_row_preserves_fields() {
        let now = Utc::now();
        let folder_id = Uuid::new_v4();
        let parent_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let row = FolderRow {
            id: folder_id,
            name: "Documents".to_string(),
            parent_id: Some(parent_id),
            created_by: Some(user_id),
            created_by_label: "Bob".to_string(),
            created_at: now,
            updated_at: now,
        };
        let summary = FolderSummary::from(row);
        assert_eq!(summary.id, folder_id);
        assert_eq!(summary.name, "Documents");
        assert_eq!(summary.parent_id, Some(parent_id));
        assert_eq!(summary.created_by_label, "Bob");
    }

    #[test]
    fn file_summary_null_folder_id() {
        let now = Utc::now();
        let row = FileRow {
            id: Uuid::new_v4(),
            name: "root_file.txt".to_string(),
            mime_type: "text/plain".to_string(),
            size_bytes: 0,
            current_version: 1,
            total_versions: 1,
            folder_id: None,
            created_by: None,
            created_by_label: "System".to_string(),
            created_at: now,
            updated_at: now,
        };
        let summary = FileSummary::from(row);
        assert!(summary.folder_id.is_none());
        assert!(summary.created_by.is_none());
    }

    #[test]
    fn folder_summary_root_has_null_parent() {
        let now = Utc::now();
        let row = FolderRow {
            id: Uuid::new_v4(),
            name: "Root".to_string(),
            parent_id: None,
            created_by: None,
            created_by_label: "System".to_string(),
            created_at: now,
            updated_at: now,
        };
        let summary = FolderSummary::from(row);
        assert!(summary.parent_id.is_none());
    }

    #[test]
    fn file_list_response_empty() {
        let resp = FileListResponse {
            items: vec![],
            next_cursor: None,
        };
        assert!(resp.items.is_empty());
        assert!(resp.next_cursor.is_none());
    }

    #[test]
    fn folder_list_response_empty() {
        let resp = FolderListResponse {
            items: vec![],
            next_cursor: None,
        };
        assert!(resp.items.is_empty());
        assert!(resp.next_cursor.is_none());
    }

    #[test]
    fn list_files_query_defaults() {
        let q = ListFilesQuery {
            folder_id: None,
            cursor: None,
            limit: None,
        };
        assert!(q.folder_id.is_none());
        assert!(q.cursor.is_none());
        assert!(q.limit.is_none());
    }

    #[test]
    fn list_folders_query_defaults() {
        let q = ListFoldersQuery {
            parent_id: None,
            cursor: None,
            limit: None,
        };
        assert!(q.parent_id.is_none());
        assert!(q.cursor.is_none());
        assert!(q.limit.is_none());
    }

    #[test]
    fn pagination_has_next_detection() {
        let limit: u32 = 2;
        // Simulate 3 rows returned for fetch_limit = limit + 1 = 3.
        let now = Utc::now();
        let make_row = |n: u8| FileRow {
            id: Uuid::new_v4(),
            name: format!("file{n}.txt"),
            mime_type: "text/plain".to_string(),
            size_bytes: 0,
            current_version: 1,
            total_versions: 1,
            folder_id: None,
            created_by: None,
            created_by_label: "x".to_string(),
            created_at: now,
            updated_at: now,
        };
        let rows: Vec<FileRow> = (0..3).map(make_row).collect();
        let has_next = rows.len() > limit as usize;
        assert!(has_next, "3 rows > limit 2 → has_next");
        let truncated: Vec<FileSummary> = rows
            .into_iter()
            .take(limit as usize)
            .map(FileSummary::from)
            .collect();
        assert_eq!(truncated.len(), 2);
    }

    #[test]
    fn pagination_no_next_when_at_limit() {
        let limit: u32 = 2;
        // Exactly 2 rows → no next page.
        let now = Utc::now();
        let make_row = |n: u8| FileRow {
            id: Uuid::new_v4(),
            name: format!("file{n}.txt"),
            mime_type: "text/plain".to_string(),
            size_bytes: 0,
            current_version: 1,
            total_versions: 1,
            folder_id: None,
            created_by: None,
            created_by_label: "x".to_string(),
            created_at: now,
            updated_at: now,
        };
        let rows: Vec<FileRow> = (0..2).map(make_row).collect();
        let has_next = rows.len() > limit as usize;
        assert!(!has_next, "2 rows == limit 2 → no next");
    }

    #[test]
    fn require_group_id_returns_err_when_none() {
        let principal = Principal {
            user_id: Uuid::new_v4(),
            group_id: None,
            role: None,
        };
        let result = require_group_id(&principal);
        assert!(result.is_err());
    }

    #[test]
    fn require_group_id_returns_ok_when_present() {
        let gid = Uuid::new_v4();
        let principal = Principal {
            user_id: Uuid::new_v4(),
            group_id: Some(gid),
            role: None,
        };
        let result = require_group_id(&principal);
        assert_eq!(result.unwrap(), gid);
    }

    #[test]
    fn check_group_match_ok_when_equal() {
        let id = Uuid::new_v4();
        assert!(check_group_match(id, id).is_ok());
    }

    #[test]
    fn check_group_match_err_when_different() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        assert!(check_group_match(a, b).is_err());
    }

    // ─── PATCH validation helpers (plan 0089, GAR-557) ────────────────

    #[test]
    fn validate_file_name_happy_path_trims() {
        let out = validate_file_name("  report.pdf  ").expect("trim+accept");
        assert_eq!(out, "report.pdf");
    }

    #[test]
    fn validate_file_name_rejects_empty_after_trim() {
        let err = validate_file_name("   ").expect_err("empty");
        assert_eq!(err, ERR_NAME_EMPTY);
    }

    #[test]
    fn validate_file_name_rejects_zero_length() {
        let err = validate_file_name("").expect_err("empty literal");
        assert_eq!(err, ERR_NAME_EMPTY);
    }

    #[test]
    fn validate_file_name_accepts_500_chars() {
        // Boundary: exactly 500 chars passes.
        let name: String = "a".repeat(500);
        let out = validate_file_name(&name).expect("500 ok");
        assert_eq!(out.chars().count(), 500);
    }

    #[test]
    fn validate_file_name_rejects_501_chars() {
        let name: String = "a".repeat(501);
        let err = validate_file_name(&name).expect_err("too long");
        assert_eq!(err, ERR_NAME_TOO_LONG);
    }

    #[test]
    fn validate_file_name_rejects_slash() {
        let err = validate_file_name("dir/file.txt").expect_err("slash");
        assert_eq!(err, ERR_NAME_HAS_SLASH);
    }

    #[test]
    fn validate_file_name_rejects_nul_byte() {
        let err = validate_file_name("foo\0bar").expect_err("nul");
        assert_eq!(err, ERR_NAME_HAS_NUL);
    }

    #[test]
    fn validate_file_name_accepts_unicode() {
        // Multi-byte UTF-8 chars should count as chars, not bytes.
        let name = "café-relatório.pdf";
        let out = validate_file_name(name).expect("utf8");
        assert_eq!(out, name);
    }

    #[test]
    fn file_summary_size_zero_is_valid() {
        let now = Utc::now();
        let row = FileRow {
            id: Uuid::new_v4(),
            name: "empty.txt".to_string(),
            mime_type: "text/plain".to_string(),
            size_bytes: 0,
            current_version: 1,
            total_versions: 1,
            folder_id: None,
            created_by: None,
            created_by_label: "x".to_string(),
            created_at: now,
            updated_at: now,
        };
        let summary = FileSummary::from(row);
        assert_eq!(summary.size_bytes, 0);
    }

    #[test]
    fn file_list_response_with_cursor() {
        let id1 = Uuid::new_v4();
        let now = Utc::now();
        let resp = FileListResponse {
            items: vec![FileSummary {
                id: id1,
                name: "a.txt".to_string(),
                mime_type: "text/plain".to_string(),
                size_bytes: 1,
                current_version: 1,
                total_versions: 1,
                folder_id: None,
                created_by: None,
                created_by_label: "x".to_string(),
                created_at: now,
                updated_at: now,
            }],
            next_cursor: Some(id1),
        };
        assert_eq!(resp.items.len(), 1);
        assert_eq!(resp.next_cursor, Some(id1));
    }
}
