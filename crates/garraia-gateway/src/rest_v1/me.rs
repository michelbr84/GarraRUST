//! `/v1/me` — authenticated caller identity + self-service profile update.
//!
//! `GET /v1/me` returns identity info from the `Principal` extractor only —
//! no SQL. `PATCH /v1/me` updates `display_name` on the `users` table via
//! the `garraia_app` AppPool. `users` is NOT FORCE-RLS group-scoped, so no
//! `SET LOCAL` context is needed; isolation is via `WHERE id = $1` with the
//! JWT-authenticated `principal.user_id`.

use axum::Json;
use axum::extract::State;
use chrono::{DateTime, Utc};
use garraia_auth::Principal;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
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
}
