//! `/v1/groups` real handlers (plan 0016 M4).
//!
//! The two endpoints in this module are the first writes landing on
//! the `garraia_app` RLS-enforced pool (`AppPool::pool_for_handlers`).
//!
//! - `POST /v1/groups` — any authenticated caller can create a new
//!   group. The creator is inserted into `group_members` as `owner`
//!   inside the same transaction that inserts into `groups`.
//! - `GET /v1/groups/{id}` — returns the group row + the caller's
//!   role. Requires `X-Group-Id` header matching the path id (the
//!   `Principal` extractor's membership lookup is what enforces
//!   non-members -> 403).
//!
//! ## Tenant-context protocol
//!
//! Every transaction opens with `SET LOCAL app.current_user_id =
//! '{uuid}'` as its **first** statement (team-coordinator gate risk
//! #6). Without an explicit `tx = pool.begin().await?`, `SET LOCAL`
//! evaporates in Postgres auto-commit mode. Both handlers below
//! follow this rule.
//!
//! For M4 the `SET LOCAL` is defensive scaffolding — `groups` and
//! `group_members` are not under FORCE RLS (see migrations 001 and
//! 007). The `garraia_app` role has direct INSERT grants. The
//! setting becomes load-bearing in M5+ once RLS is extended to
//! these tables.
//!
//! ## SQL injection posture
//!
//! `SET LOCAL` does not accept bind parameters in Postgres, so the
//! `user_id` UUID is interpolated via `format!`. `Uuid::Display`
//! produces exactly 36 hex-with-dash characters and no metacharacters
//! — injection-safe by construction. All other parameters use
//! `sqlx::query::bind` as normal.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use garraia_auth::Principal;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::RestV1FullState;
use super::problem::RestError;

/// Accepted values for `CreateGroupRequest::group_type`.
///
/// Mirrors the `CHECK (type IN ('family','team','personal'))`
/// constraint on `groups.type` in migration 001, but **excludes**
/// `"personal"` — that variant is reserved for the GAR-413
/// SQLite→Postgres migration fallback and must not be exposed
/// via the API layer (see migration 001 line 114 comment).
const ALLOWED_GROUP_TYPES: &[&str] = &["family", "team"];

/// Request body for `POST /v1/groups`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateGroupRequest {
    /// Human-readable group name. Must not be empty after trim.
    pub name: String,
    /// Group type. Must be `"family"` or `"team"`.
    #[serde(rename = "type")]
    pub group_type: String,
}

/// Response body for `POST /v1/groups` (201 Created).
#[derive(Debug, Serialize, ToSchema)]
pub struct GroupResponse {
    pub id: Uuid,
    pub name: String,
    #[serde(rename = "type")]
    pub group_type: String,
    pub created_at: DateTime<Utc>,
}

/// Response body for `GET /v1/groups/{id}` (200 OK).
///
/// Includes the caller's `role` (from the `Principal` extractor)
/// so clients can avoid an extra `/v1/me` round-trip when rendering
/// per-role UI affordances.
#[derive(Debug, Serialize, ToSchema)]
pub struct GroupReadResponse {
    pub id: Uuid,
    pub name: String,
    #[serde(rename = "type")]
    pub group_type: String,
    pub created_at: DateTime<Utc>,
    pub created_by: Uuid,
    /// Caller's role in the group, e.g. `"owner"`, `"admin"`,
    /// `"member"`, `"guest"`, `"child"`.
    pub role: String,
}

/// `POST /v1/groups` — create a new group. The authenticated caller
/// becomes the `owner` in an atomic transaction with the group
/// creation.
///
/// Does NOT require an `X-Group-Id` header — the caller is creating
/// a brand-new group, not operating on an existing one.
#[utoipa::path(
    post,
    path = "/v1/groups",
    request_body = CreateGroupRequest,
    responses(
        (status = 201, description = "Group created; caller is auto-enrolled as owner.", body = GroupResponse),
        (status = 400, description = "Invalid group type or empty name.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn create_group(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Json(req): Json<CreateGroupRequest>,
) -> Result<(StatusCode, Json<GroupResponse>), RestError> {
    // 1. Validate body.
    if !ALLOWED_GROUP_TYPES.contains(&req.group_type.as_str()) {
        return Err(RestError::BadRequest(
            "group type must be 'family' or 'team'".into(),
        ));
    }
    if req.name.trim().is_empty() {
        return Err(RestError::BadRequest("group name must not be empty".into()));
    }

    // 2a. Capture the trimmed name once so the database row and the
    //     response body cannot diverge if someone later adds another
    //     use site.
    let trimmed_name = req.name.trim().to_string();

    // 2b. Open transaction on `app_pool`. The SET LOCAL below is
    //     transaction-scoped — if we skipped `begin()` and used the
    //     pool directly, auto-commit would wrap each statement in
    //     its own transaction and the tenant context would be lost.
    //     (team-coordinator M4 gate risk #6)
    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // 3. SET LOCAL tenant context. MUST be the first statement
    //    inside the transaction. Uuid Display is 36 hex-dashed
    //    chars, injection-safe.
    sqlx::query(&format!(
        "SET LOCAL app.current_user_id = '{}'",
        principal.user_id
    ))
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    // 4. INSERT groups, returning the generated id + timestamps.
    let (id, created_at): (Uuid, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO groups (name, type, created_by) \
         VALUES ($1, $2, $3) \
         RETURNING id, created_at",
    )
    .bind(&trimmed_name)
    .bind(&req.group_type)
    .bind(principal.user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    // 5. INSERT group_members. The creator becomes the `owner`.
    sqlx::query(
        "INSERT INTO group_members (group_id, user_id, role, status) \
         VALUES ($1, $2, 'owner', 'active')",
    )
    .bind(id)
    .bind(principal.user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    // 6. Commit. On failure, both INSERTs roll back and the caller
    //    sees an Internal 500 — the creator does NOT end up with
    //    a dangling group row.
    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok((
        StatusCode::CREATED,
        Json(GroupResponse {
            id,
            name: trimmed_name,
            group_type: req.group_type,
            created_at,
        }),
    ))
}

/// `GET /v1/groups/{id}` — read a single group by id.
///
/// The `Principal` extractor requires an `X-Group-Id` header
/// matching the path id so the caller's membership is verified
/// before this handler runs (non-members -> 403 at extractor time).
/// The handler validates that the header's group id matches the
/// path id; any mismatch is 400.
#[utoipa::path(
    get,
    path = "/v1/groups/{id}",
    params(
        ("id" = Uuid, Path, description = "Group UUID. Must match the `X-Group-Id` header."),
    ),
    responses(
        (status = 200, description = "Group details + caller's role.", body = GroupReadResponse),
        (status = 400, description = "`X-Group-Id` header missing or mismatches the path id.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member of the requested group.", body = super::problem::ProblemDetails),
        (status = 404, description = "Group not found (deleted between extractor lookup and read).", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn get_group(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(id): Path<Uuid>,
) -> Result<Json<GroupReadResponse>, RestError> {
    // 1. Validate path vs header. The Principal extractor already
    //    403'd any non-member when the header was supplied; its
    //    absence or mismatch is a 400 from this handler.
    match principal.group_id {
        Some(hdr) if hdr == id => {}
        Some(_) => {
            return Err(RestError::BadRequest(
                "X-Group-Id header and path id must match".into(),
            ));
        }
        None => {
            return Err(RestError::BadRequest(
                "X-Group-Id header is required".into(),
            ));
        }
    }

    // 2. Fetch the group row. `groups` is not under FORCE RLS today
    //    (migration 001 line 100 + migration 007), so no
    //    tenant-context `SET LOCAL` is needed for this SELECT and
    //    `fetch_optional` runs directly on the pool without a
    //    transaction wrapper. `garraia_app` has direct SELECT
    //    grants on `groups`.
    //
    //    FIXME(plan-0016-M5 or whenever `groups` gains FORCE RLS):
    //    convert this to a `tx = pool.begin()` + `SET LOCAL
    //    app.current_user_id` sequence mirroring `create_group`
    //    above. The M4 security-auditor flagged this as a
    //    forward-compat hazard — silent `groups` SELECT bypass
    //    if RLS is later applied and this handler is left
    //    untouched.
    let pool = state.app_pool.pool_for_handlers();
    let row: Option<(Uuid, String, String, DateTime<Utc>, Uuid)> =
        sqlx::query_as("SELECT id, name, type, created_at, created_by FROM groups WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

    let (id, name, group_type, created_at, created_by) = row.ok_or(RestError::NotFound)?;

    // 3. Assemble response. The caller's `Principal.role` MUST be
    //    `Some(_)` here: the extractor only populates `group_id`
    //    when the `group_members` membership lookup found an
    //    active row, which by definition includes a role. If this
    //    invariant ever breaks (e.g. a refactor that populates
    //    `group_id` without pairing a role), we want to fail loud
    //    with a 500 rather than silently emitting `role: ""` to
    //    the client. Plan 0016 M4 review fix (code-reviewer HIGH).
    let role = principal
        .role
        .ok_or_else(|| {
            tracing::error!(
                user_id = %principal.user_id,
                group_id = ?principal.group_id,
                "Principal.role is None with group_id Some — invariant violated by extractor"
            );
            RestError::Internal(anyhow::anyhow!(
                "Principal.role is None despite group_id being Some"
            ))
        })?
        .as_str()
        .to_string();

    Ok(Json(GroupReadResponse {
        id,
        name,
        group_type,
        created_at,
        created_by,
        role,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_group_types_are_family_and_team_only() {
        assert_eq!(ALLOWED_GROUP_TYPES, &["family", "team"]);
        // `personal` is RESERVED per migration 001 line 114 and
        // must not be exposed via the API.
        assert!(!ALLOWED_GROUP_TYPES.contains(&"personal"));
    }

    #[test]
    fn create_group_response_serializes_with_type_field() {
        let resp = GroupResponse {
            id: Uuid::nil(),
            name: "Test".into(),
            group_type: "team".into(),
            created_at: Utc::now(),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["id"], "00000000-0000-0000-0000-000000000000");
        assert_eq!(v["type"], "team");
        assert_eq!(v["name"], "Test");
        assert!(v.get("group_type").is_none(), "type rename missing");
    }

    #[test]
    fn group_read_response_includes_role_and_created_by() {
        let resp = GroupReadResponse {
            id: Uuid::nil(),
            name: "Test".into(),
            group_type: "family".into(),
            created_at: Utc::now(),
            created_by: Uuid::nil(),
            role: "owner".into(),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["type"], "family");
        assert_eq!(v["role"], "owner");
        assert_eq!(v["created_by"], "00000000-0000-0000-0000-000000000000");
    }

    #[test]
    fn create_group_request_deserializes_with_type_rename() {
        let s = r#"{"name":"alpha","type":"team"}"#;
        let parsed: CreateGroupRequest = serde_json::from_str(s).unwrap();
        assert_eq!(parsed.name, "alpha");
        assert_eq!(parsed.group_type, "team");
    }
}
