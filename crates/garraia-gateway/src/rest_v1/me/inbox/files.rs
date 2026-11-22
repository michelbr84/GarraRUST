//! `GET /v1/me/files` — inbox of files uploaded by the caller in a group.

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

// ─── GET /v1/me/files ────────────────────────────────────────────────────────

/// Query parameters for `GET /v1/me/files`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListMyFilesQuery {
    /// Group to scope the file inbox. Required — RLS needs `app.current_group_id`.
    pub group_id: Uuid,
    /// Cursor: `id` of the last file on the previous page (keyset on `created_at DESC, id DESC`).
    pub after: Option<Uuid>,
    /// Maximum items per page. Clamped to `[1, 100]`; default `50`.
    pub limit: Option<i64>,
    /// Optional folder filter. When set, only files directly inside this folder are returned.
    pub folder_id: Option<Uuid>,
}

/// One file entry in the `GET /v1/me/files` response.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyFileSummary {
    pub id: Uuid,
    pub group_id: Uuid,
    pub name: String,
    pub mime_type: String,
    pub size_bytes: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

/// Response body for `GET /v1/me/files`.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyFilesResponse {
    pub items: Vec<MyFileSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Uuid>,
}

/// RLS is enforced via `SET LOCAL app.current_user_id` and
/// `SET LOCAL app.current_group_id` — rows from other groups are invisible.
#[utoipa::path(
    get,
    path = "/v1/me/files",
    params(ListMyFilesQuery),
    responses(
        (status = 200, description = "List of files uploaded by caller, newest-created first.", body = MyFilesResponse),
        (status = 401, description = "Missing or invalid JWT.", body = crate::rest_v1::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_my_files(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Query(params): Query<ListMyFilesQuery>,
) -> Result<Json<MyFilesResponse>, RestError> {
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

    // Columns: id, group_id, name, mime_type, size_bytes, folder_id, created_at, updated_at
    type FileRow = (
        Uuid,
        Uuid,
        String,
        String,
        i64,
        Option<Uuid>,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
    );

    let rows: Vec<FileRow> = match (params.after, params.folder_id) {
        (None, None) => sqlx::query_as(
            "SELECT id, group_id, name, mime_type, size_bytes, folder_id, \
                    created_at, updated_at \
             FROM files \
             WHERE created_by = $1 \
               AND group_id = $2 \
               AND deleted_at IS NULL \
             ORDER BY created_at DESC, id DESC \
             LIMIT $3",
        )
        .bind(principal.user_id)
        .bind(group_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        (None, Some(folder_id)) => sqlx::query_as(
            "SELECT id, group_id, name, mime_type, size_bytes, folder_id, \
                    created_at, updated_at \
             FROM files \
             WHERE created_by = $1 \
               AND group_id = $2 \
               AND folder_id = $3 \
               AND deleted_at IS NULL \
             ORDER BY created_at DESC, id DESC \
             LIMIT $4",
        )
        .bind(principal.user_id)
        .bind(group_id)
        .bind(folder_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        (Some(after_id), None) => sqlx::query_as(
            "SELECT id, group_id, name, mime_type, size_bytes, folder_id, \
                    created_at, updated_at \
             FROM files \
             WHERE created_by = $1 \
               AND group_id = $2 \
               AND deleted_at IS NULL \
               AND (created_at, id) < ( \
                   SELECT f2.created_at, f2.id \
                   FROM files f2 \
                   WHERE f2.id = $3 \
                     AND f2.group_id = $2 \
               ) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $4",
        )
        .bind(principal.user_id)
        .bind(group_id)
        .bind(after_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        (Some(after_id), Some(folder_id)) => sqlx::query_as(
            "SELECT id, group_id, name, mime_type, size_bytes, folder_id, \
                    created_at, updated_at \
             FROM files \
             WHERE created_by = $1 \
               AND group_id = $2 \
               AND folder_id = $3 \
               AND deleted_at IS NULL \
               AND (created_at, id) < ( \
                   SELECT f2.created_at, f2.id \
                   FROM files f2 \
                   WHERE f2.id = $4 \
                     AND f2.group_id = $2 \
               ) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $5",
        )
        .bind(principal.user_id)
        .bind(group_id)
        .bind(folder_id)
        .bind(after_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,
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
            |(id, group_id, name, mime_type, size_bytes, folder_id, created_at, updated_at)| {
                MyFileSummary {
                    id,
                    group_id,
                    name,
                    mime_type,
                    size_bytes,
                    folder_id,
                    created_at,
                    updated_at,
                }
            },
        )
        .collect();

    Ok(Json(MyFilesResponse { items, next_cursor }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── MyFilesResponse / MyFileSummary serialization ─────────────────────────

    #[test]
    fn my_files_response_empty_no_cursor() {
        let resp = MyFilesResponse {
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
    fn my_files_response_with_cursor() {
        let cursor = Uuid::parse_str("eeeeeeee-0000-0000-0000-000000000005").unwrap();
        let resp = MyFilesResponse {
            items: vec![],
            next_cursor: Some(cursor),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["next_cursor"], "eeeeeeee-0000-0000-0000-000000000005");
    }

    #[test]
    fn my_file_summary_serializes_all_fields() {
        let id = Uuid::parse_str("aaaaaaaa-0000-0000-0000-000000000001").unwrap();
        let group_id = Uuid::parse_str("bbbbbbbb-0000-0000-0000-000000000001").unwrap();
        let folder_id = Uuid::parse_str("cccccccc-0000-0000-0000-000000000001").unwrap();
        let summary = MyFileSummary {
            id,
            group_id,
            name: "report.pdf".into(),
            mime_type: "application/pdf".into(),
            size_bytes: 12345,
            folder_id: Some(folder_id),
            created_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            updated_at: None,
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert_eq!(v["id"], "aaaaaaaa-0000-0000-0000-000000000001");
        assert_eq!(v["group_id"], "bbbbbbbb-0000-0000-0000-000000000001");
        assert_eq!(v["name"], "report.pdf");
        assert_eq!(v["mime_type"], "application/pdf");
        assert_eq!(v["size_bytes"], 12345);
        assert_eq!(v["folder_id"], "cccccccc-0000-0000-0000-000000000001");
        assert!(
            v.get("updated_at").is_none(),
            "absent updated_at must be omitted"
        );
    }

    #[test]
    fn my_file_summary_omits_folder_id_when_absent() {
        let summary = MyFileSummary {
            id: Uuid::nil(),
            group_id: Uuid::nil(),
            name: "photo.jpg".into(),
            mime_type: "image/jpeg".into(),
            size_bytes: 500_000,
            folder_id: None,
            created_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            updated_at: Some(chrono::DateTime::from_timestamp(1_000_000, 0).unwrap()),
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert!(
            v.get("folder_id").is_none(),
            "absent folder_id must be omitted"
        );
        assert!(
            v.get("updated_at").is_some(),
            "present updated_at must be serialized"
        );
    }

    #[test]
    fn list_my_files_query_defaults() {
        let q = ListMyFilesQuery {
            group_id: Uuid::nil(),
            after: None,
            limit: None,
            folder_id: None,
        };
        assert!(q.after.is_none());
        assert!(q.limit.is_none());
        assert!(q.folder_id.is_none());
    }

    #[test]
    fn list_my_files_query_with_all_fields() {
        let gid = Uuid::parse_str("dddddddd-0000-0000-0000-000000000004").unwrap();
        let after = Uuid::parse_str("eeeeeeee-0000-0000-0000-000000000005").unwrap();
        let fid = Uuid::parse_str("ffffffff-0000-0000-0000-000000000006").unwrap();
        let q = ListMyFilesQuery {
            group_id: gid,
            after: Some(after),
            limit: Some(25),
            folder_id: Some(fid),
        };
        assert_eq!(q.group_id, gid);
        assert_eq!(q.after, Some(after));
        assert_eq!(q.limit, Some(25));
        assert_eq!(q.folder_id, Some(fid));
    }

    #[test]
    fn list_my_files_limit_clamp() {
        let over: i64 = Some(200i64).map(|l| l.min(100)).unwrap_or(50).max(1);
        let under: i64 = Some(0i64).map(|l| l.min(100)).unwrap_or(50).max(1);
        let default: i64 = None::<i64>.map(|l| l.min(100)).unwrap_or(50).max(1);
        assert_eq!(over, 100);
        assert_eq!(under, 1);
        assert_eq!(default, 50);
    }

    #[test]
    fn my_file_summary_large_size_bytes() {
        let summary = MyFileSummary {
            id: Uuid::nil(),
            group_id: Uuid::nil(),
            name: "large.bin".into(),
            mime_type: "application/octet-stream".into(),
            size_bytes: 5_368_709_120i64,
            folder_id: None,
            created_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            updated_at: None,
        };
        let v = serde_json::to_value(&summary).unwrap();
        assert_eq!(v["size_bytes"], 5_368_709_120i64);
    }
}
