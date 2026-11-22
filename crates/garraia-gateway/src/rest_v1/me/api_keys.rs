//! `/v1/me/api-keys[/{key_id}]` — create, list, read, update and revoke the
//! caller's API keys. The raw key is returned once; only its hash is stored.

// Request-path (#1569): panic aqui e controlavel por quem manda a requisicao.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use argon2::PasswordHasher;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use garraia_auth::{Principal, WorkspaceAuditAction, audit_workspace_event};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::rest_v1::RestV1FullState;
use crate::rest_v1::problem::RestError;

// ─── POST /v1/me/api-keys ────────────────────────────────────────────────────

/// Request body for `POST /v1/me/api-keys`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateApiKeyRequest {
    /// Human-readable label. 1–255 characters.
    pub label: String,
    /// Permission scope strings, e.g. `["workspace.read"]`. Defaults to `[]`.
    #[serde(default)]
    pub scopes: Vec<String>,
}

/// Response body for `POST /v1/me/api-keys` — raw key returned **once only**.
#[derive(Debug, Serialize, ToSchema)]
pub struct CreateApiKeyResponse {
    /// UUID of the newly created key row.
    pub id: Uuid,
    /// Label supplied in the request.
    pub label: String,
    /// Scope strings stored with the key.
    pub scopes: Vec<String>,
    /// Creation timestamp (UTC).
    pub created_at: DateTime<Utc>,
    /// Raw API key — show once and discard; only the Argon2id hash is stored.
    pub key: String,
}

/// One API key entry — `key` field is never present (raw key is one-time only).
#[derive(Debug, Serialize, ToSchema)]
pub struct ApiKeySummary {
    /// API key UUID.
    pub id: Uuid,
    /// Human-readable label.
    pub label: String,
    /// Permission scope strings.
    pub scopes: Vec<String>,
    /// When the key was created (UTC).
    pub created_at: DateTime<Utc>,
    /// When the key was last used to authenticate (UTC), if ever.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<DateTime<Utc>>,
    /// When the key was revoked (UTC). `null` means the key is still active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<DateTime<Utc>>,
}

/// Query parameters for `GET /v1/me/api-keys`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListMyApiKeysQuery {
    /// Cursor: `id` of the last key on the previous page.
    pub after: Option<Uuid>,
    /// Maximum items per page. Clamped to `[1, 100]`; default 20.
    pub limit: Option<i64>,
}

/// Response body for `GET /v1/me/api-keys`.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyApiKeysResponse {
    pub api_keys: Vec<ApiKeySummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Uuid>,
}

/// Creates a new API key for the authenticated caller.
///
/// Generates a cryptographically-random `gai_<base64url>` key, stores only
/// its Argon2id hash, and returns the raw key **once** in the 201 response.
/// The raw key is unrecoverable afterwards.
///
/// Label must be 1–255 characters. Scopes default to `[]`.
/// No `X-Group-Id` header required — API keys are user-scoped.
#[utoipa::path(
    post,
    path = "/v1/me/api-keys",
    request_body = CreateApiKeyRequest,
    responses(
        (status = 201, description = "API key created. Raw key returned once only.", body = CreateApiKeyResponse),
        (status = 400, description = "Validation error (label empty or >255 chars).", body = crate::rest_v1::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = crate::rest_v1::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn create_my_api_key(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Json(body): Json<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<CreateApiKeyResponse>), RestError> {
    let label = body.label.trim().to_owned();
    if label.is_empty() || label.len() > 255 {
        return Err(RestError::BadRequest(
            "label must be 1–255 characters".into(),
        ));
    }
    for scope in &body.scopes {
        if scope.is_empty() {
            return Err(RestError::BadRequest(
                "scopes must be non-empty strings".into(),
            ));
        }
    }

    // Generate 32 random bytes → "gai_<url-safe-base64-no-pad>"
    let mut key_bytes = [0u8; 32];
    getrandom::fill(&mut key_bytes)
        .map_err(|e| RestError::Internal(anyhow::anyhow!("getrandom failure: {e}")))?;
    let raw_key = format!("gai_{}", URL_SAFE_NO_PAD.encode(key_bytes));

    // Argon2id hash of the raw key (PHC string format). Salt generated
    // internally by `hash_password` (password-hash 0.6 via getrandom).
    let key_hash = argon2::Argon2::default()
        .hash_password(raw_key.as_bytes())
        .map_err(|e| RestError::Internal(anyhow::anyhow!("argon2 hash failure: {e}")))?
        .to_string();

    let scopes_json = serde_json::to_value(&body.scopes)
        .map_err(|e| RestError::Internal(anyhow::anyhow!("scopes serialize: {e}")))?;

    let nil_uuid = Uuid::nil();
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
        .bind(nil_uuid.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let (key_id, created_at): (Uuid, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO api_keys (user_id, label, key_hash, scopes) \
         VALUES ($1, $2, $3, $4) \
         RETURNING id, created_at",
    )
    .bind(principal.user_id)
    .bind(&label)
    .bind(&key_hash)
    .bind(&scopes_json)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::ApiKeyCreated,
        principal.user_id,
        nil_uuid,
        "api_keys",
        key_id.to_string(),
        json!({ "label_len": label.len() }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!("{e}")))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok((
        StatusCode::CREATED,
        Json(CreateApiKeyResponse {
            id: key_id,
            label,
            scopes: body.scopes,
            created_at,
            key: raw_key,
        }),
    ))
}

// ─── GET /v1/me/api-keys ─────────────────────────────────────────────────────

/// Lists all API keys for the authenticated caller (active + revoked history).
///
/// Cursor-paginated (`after` = last `id` from previous page, `limit` default 20).
/// The raw key is never returned — only metadata. No `X-Group-Id` required.
#[utoipa::path(
    get,
    path = "/v1/me/api-keys",
    params(ListMyApiKeysQuery),
    responses(
        (status = 200, description = "Paginated list of the caller's API keys.", body = MyApiKeysResponse),
        (status = 401, description = "Missing or invalid JWT.", body = crate::rest_v1::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_my_api_keys(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Query(params): Query<ListMyApiKeysQuery>,
) -> Result<Json<MyApiKeysResponse>, RestError> {
    let limit = params.limit.map(|l| l.min(100)).unwrap_or(20).max(1);
    let nil_uuid = Uuid::nil();

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
        .bind(nil_uuid.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    type ApiKeyRow = (
        Uuid,
        String,
        serde_json::Value,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
    );

    let rows: Vec<ApiKeyRow> = match params.after {
        None => sqlx::query_as(
            "SELECT id, label, scopes, created_at, last_used_at, revoked_at \
             FROM api_keys \
             WHERE user_id = $1 \
             ORDER BY created_at DESC, id DESC \
             LIMIT $2",
        )
        .bind(principal.user_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?,

        Some(after_id) => sqlx::query_as(
            "SELECT id, label, scopes, created_at, last_used_at, revoked_at \
             FROM api_keys \
             WHERE user_id = $1 \
               AND (created_at, id) < ( \
                   SELECT created_at, id FROM api_keys \
                   WHERE id = $2 AND user_id = $1 \
               ) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $3",
        )
        .bind(principal.user_id)
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

    let api_keys = rows
        .into_iter()
        .map(
            |(id, label, scopes_val, created_at, last_used_at, revoked_at)| ApiKeySummary {
                id,
                label,
                scopes: serde_json::from_value(scopes_val).unwrap_or_default(),
                created_at,
                last_used_at,
                revoked_at,
            },
        )
        .collect();

    Ok(Json(MyApiKeysResponse {
        api_keys,
        next_cursor,
    }))
}

// ─── GET /v1/me/api-keys/{key_id} ────────────────────────────────────────────

/// Fetches metadata for a single API key owned by the authenticated caller.
///
/// Returns 404 if the key does not exist or belongs to another user
/// (FORCE RLS via `api_keys_owner_only`). Raw key is never returned.
#[utoipa::path(
    get,
    path = "/v1/me/api-keys/{key_id}",
    params(
        ("key_id" = Uuid, Path, description = "API key UUID."),
    ),
    responses(
        (status = 200, description = "API key metadata.", body = ApiKeySummary),
        (status = 401, description = "Missing or invalid JWT.", body = crate::rest_v1::problem::ProblemDetails),
        (status = 404, description = "Key not found or belongs to another user.", body = crate::rest_v1::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn get_my_api_key(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(key_id): Path<Uuid>,
) -> Result<Json<ApiKeySummary>, RestError> {
    let nil_uuid = Uuid::nil();

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
        .bind(nil_uuid.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    type ApiKeyRow = (
        Uuid,
        String,
        serde_json::Value,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
    );

    let row: Option<ApiKeyRow> = sqlx::query_as(
        "SELECT id, label, scopes, created_at, last_used_at, revoked_at \
         FROM api_keys \
         WHERE id = $1 AND user_id = $2",
    )
    .bind(key_id)
    .bind(principal.user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    match row {
        None => Err(RestError::NotFound),
        Some((id, label, scopes_val, created_at, last_used_at, revoked_at)) => {
            Ok(Json(ApiKeySummary {
                id,
                label,
                scopes: serde_json::from_value(scopes_val).unwrap_or_default(),
                created_at,
                last_used_at,
                revoked_at,
            }))
        }
    }
}

// ─── DELETE /v1/me/api-keys/{key_id} ─────────────────────────────────────────

/// Revokes an API key owned by the authenticated caller (soft-delete).
///
/// Sets `revoked_at = now()`. Idempotent: already-revoked keys return 204.
/// Returns 404 if the key is not found or belongs to another user (FORCE RLS).
/// No `X-Group-Id` required.
#[utoipa::path(
    delete,
    path = "/v1/me/api-keys/{key_id}",
    params(
        ("key_id" = Uuid, Path, description = "API key UUID to revoke."),
    ),
    responses(
        (status = 204, description = "API key revoked, or already revoked (idempotent)."),
        (status = 401, description = "Missing or invalid JWT.", body = crate::rest_v1::problem::ProblemDetails),
        (status = 404, description = "Key not found or belongs to another user.", body = crate::rest_v1::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn revoke_my_api_key(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(key_id): Path<Uuid>,
) -> Result<StatusCode, RestError> {
    let nil_uuid = Uuid::nil();

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
        .bind(nil_uuid.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // FORCE RLS (api_keys_owner_only) ensures cross-user key_id returns 0 rows.
    let result = sqlx::query(
        "UPDATE api_keys SET revoked_at = now() \
         WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL",
    )
    .bind(key_id)
    .bind(principal.user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if result.rows_affected() == 0 {
        // Distinguish "already revoked" (204) from "not found / cross-user" (404).
        let exists: Option<(bool,)> =
            sqlx::query_as("SELECT true FROM api_keys WHERE id = $1 AND user_id = $2")
                .bind(key_id)
                .bind(principal.user_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| RestError::Internal(e.into()))?;

        tx.commit()
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

        return if exists.is_some() {
            Ok(StatusCode::NO_CONTENT)
        } else {
            Err(RestError::NotFound)
        };
    }

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::ApiKeyRevoked,
        principal.user_id,
        nil_uuid,
        "api_keys",
        key_id.to_string(),
        json!({}),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!("{e}")))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::NO_CONTENT)
}

// ─── PATCH /v1/me/api-keys/{key_id} ──────────────────────────────────────────

/// Request body for `PATCH /v1/me/api-keys/{key_id}`.
/// At least one field must be present.
#[derive(Debug, Deserialize, ToSchema)]
pub struct PatchMyApiKeyRequest {
    /// New human-readable label (1–255 chars, trimmed).
    pub label: Option<String>,
    /// Replacement scopes array (non-empty strings).
    pub scopes: Option<Vec<String>>,
}

/// Updates the label and/or scopes of an API key owned by the authenticated caller.
///
/// At least one field must be provided. Returns 400 if neither is given or validation
/// fails. Returns 404 if the key is not found or belongs to another user. Returns 409
/// if the key has already been revoked. No `X-Group-Id` header required.
#[utoipa::path(
    patch,
    path = "/v1/me/api-keys/{key_id}",
    params(
        ("key_id" = Uuid, Path, description = "API key UUID to update."),
    ),
    request_body = PatchMyApiKeyRequest,
    responses(
        (status = 200, description = "API key updated.", body = ApiKeySummary),
        (status = 400, description = "Validation error (no fields, label too long, empty scope).", body = crate::rest_v1::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = crate::rest_v1::problem::ProblemDetails),
        (status = 404, description = "Key not found or belongs to another user.", body = crate::rest_v1::problem::ProblemDetails),
        (status = 409, description = "Key is already revoked.", body = crate::rest_v1::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn patch_my_api_key(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(key_id): Path<Uuid>,
    Json(body): Json<PatchMyApiKeyRequest>,
) -> Result<Json<ApiKeySummary>, RestError> {
    // Validate: at least one field required.
    if body.label.is_none() && body.scopes.is_none() {
        return Err(RestError::BadRequest(
            "at least one field (label or scopes) must be provided".into(),
        ));
    }

    // Validate label if present.
    let new_label: Option<String> = body.label.map(|l| l.trim().to_owned());
    if let Some(ref lbl) = new_label
        && (lbl.is_empty() || lbl.len() > 255)
    {
        return Err(RestError::BadRequest(
            "label must be 1–255 characters".into(),
        ));
    }

    // Validate scopes if present.
    if let Some(ref scopes) = body.scopes {
        for scope in scopes {
            if scope.is_empty() {
                return Err(RestError::BadRequest(
                    "scopes must be non-empty strings".into(),
                ));
            }
        }
    }

    let scopes_json: Option<serde_json::Value> = match &body.scopes {
        Some(s) => Some(
            serde_json::to_value(s)
                .map_err(|e| RestError::Internal(anyhow::anyhow!("scopes serialize: {e}")))?,
        ),
        None => None,
    };

    let nil_uuid = Uuid::nil();
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
        .bind(nil_uuid.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // COALESCE: only provided fields change.
    // FORCE RLS (api_keys_owner_only) + `AND user_id = $2` guard cross-user.
    // `AND revoked_at IS NULL` guard: 0 rows → disambiguate revoked vs not-found.
    type UpdatedRow = (
        Uuid,
        String,
        serde_json::Value,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
    );
    let updated: Option<UpdatedRow> = sqlx::query_as(
        "UPDATE api_keys \
         SET label  = COALESCE($1, label), \
             scopes = COALESCE($2, scopes) \
         WHERE id = $3 AND user_id = $4 AND revoked_at IS NULL \
         RETURNING id, label, scopes, created_at, last_used_at, revoked_at",
    )
    .bind(&new_label)
    .bind(&scopes_json)
    .bind(key_id)
    .bind(principal.user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let Some((id, label, scopes_val, created_at, last_used_at, revoked_at)) = updated else {
        // Disambiguate: revoked (409) vs not-found/cross-user (404).
        let row: Option<(bool,)> =
            sqlx::query_as("SELECT true FROM api_keys WHERE id = $1 AND user_id = $2")
                .bind(key_id)
                .bind(principal.user_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| RestError::Internal(e.into()))?;

        tx.commit()
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

        return if row.is_some() {
            Err(RestError::Conflict("api key is already revoked".into()))
        } else {
            Err(RestError::NotFound)
        };
    };

    let label_len = label.len();
    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::ApiKeyUpdated,
        principal.user_id,
        nil_uuid,
        "api_keys",
        id.to_string(),
        json!({ "label_len": label_len }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!("{e}")))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(Json(ApiKeySummary {
        id,
        label,
        scopes: serde_json::from_value(scopes_val).unwrap_or_default(),
        created_at,
        last_used_at,
        revoked_at,
    }))
}

#[cfg(test)]
mod tests;
