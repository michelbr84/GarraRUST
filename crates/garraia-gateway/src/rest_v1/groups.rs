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
use garraia_auth::{Action, Principal, can};
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

/// Request body for `PATCH /v1/groups/{group_id}` (plan 0017).
///
/// All fields are `Option<T>` — only the fields explicitly set in the
/// JSON body are applied via `COALESCE($new, column)` in the UPDATE.
/// Empty body `{}` (all-None) is rejected by [`UpdateGroupRequest::validate`]
/// with a 400: a no-op PATCH is always a client mistake.
///
/// `type = "personal"` is rejected with 400 — same rule as
/// [`CreateGroupRequest`] (see [`ALLOWED_GROUP_TYPES`] and migration
/// 001 line 114).
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateGroupRequest {
    /// New display name. Rejected if empty after trim.
    #[serde(default)]
    pub name: Option<String>,
    /// New group type. Must be `"family"` or `"team"`. `"personal"` is
    /// reserved programmatic-only and rejected with 400.
    #[serde(default, rename = "type")]
    pub group_type: Option<String>,
}

impl UpdateGroupRequest {
    /// True when no field was set. Empty body = client error = 400.
    pub fn is_empty(&self) -> bool {
        self.name.is_none() && self.group_type.is_none()
    }

    /// Structural validation. Returns `Ok(())` if the body is coherent,
    /// `Err(&'static str)` with a PII-safe detail otherwise. The error
    /// string is emitted verbatim to clients in the Problem Details
    /// body, so it must not contain user-identifying data.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.is_empty() {
            return Err("patch body must set at least one field");
        }
        if let Some(name) = &self.name
            && name.trim().is_empty()
        {
            return Err("name must not be empty");
        }
        if let Some(t) = &self.group_type
            && !ALLOWED_GROUP_TYPES.contains(&t.as_str())
        {
            return Err("group type must be 'family' or 'team'");
        }
        Ok(())
    }
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

/// `PATCH /v1/groups/{id}` — partial modification of an existing group (plan 0017).
///
/// Authz model (mirrors `get_group`'s extractor-first approach):
///
/// 1. The `Principal` extractor requires an `X-Group-Id` header and 403's
///    any caller who is not a member of the target group — so by the time
///    this handler runs, `principal.group_id` is `Some(_)` and
///    `principal.role` is `Some(_)`. **Non-members never reach this code.**
/// 2. This handler then validates `principal.group_id == path_id` (400 if
///    mismatch, same rule as `get_group`).
/// 3. Authz capability check via `can(&principal, Action::GroupSettings)`:
///    - Owner, Admin → allowed (200 on success)
///    - Member, Guest, Child → 403 Forbidden
/// 4. Request body is validated structurally before any DB access (empty
///    body, bad name, reserved `type = "personal"` → 400).
/// 5. UPDATE runs inside a transaction with `SET LOCAL app.current_user_id`
///    as the first statement (same pattern as `create_group`). `updated_at
///    = now()` is set **explicitly** because `groups` has no trigger
///    (see migration 001 line 115). COALESCE($new, column) implements PATCH
///    partial-update semantics — only fields the caller sent are overwritten.
/// 6. UPDATE ... RETURNING fetches the fresh row in the same roundtrip. If
///    the row vanished between the extractor's membership lookup and this
///    statement (group hard-deleted), RETURNING is empty → 404.
///
/// ## Error response status table
///
/// | Condition                                  | Status | Source         |
/// |--------------------------------------------|--------|----------------|
/// | No JWT                                     | 401    | JWT extractor  |
/// | Non-member of target group                 | 403    | Principal ext. |
/// | `X-Group-Id` header missing / mismatched   | 400    | this handler   |
/// | Body is `{}` or all-None                   | 400    | validate()     |
/// | `name` present but blank                   | 400    | validate()     |
/// | `type` is `"personal"` or unknown          | 400    | validate()     |
/// | Role is Member/Guest/Child                 | 403    | `can()`        |
/// | Group row deleted concurrently             | 404    | UPDATE RETURN. |
/// | Happy path                                 | 200    |                |
#[utoipa::path(
    patch,
    path = "/v1/groups/{id}",
    params(
        ("id" = Uuid, Path, description = "Group UUID. Must match the `X-Group-Id` header."),
    ),
    request_body = UpdateGroupRequest,
    responses(
        (status = 200, description = "Group updated; response carries the fresh row + caller's role.", body = GroupReadResponse),
        (status = 400, description = "Invalid body, header/path mismatch, or reserved group type.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller lacks `group.settings` capability.", body = super::problem::ProblemDetails),
        (status = 404, description = "Group vanished between extractor lookup and UPDATE.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn patch_group(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateGroupRequest>,
) -> Result<Json<GroupReadResponse>, RestError> {
    // 1. Header/path coherence (same rule as `get_group`).
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

    // 2. Capability check. Owner/Admin pass; Member/Guest/Child get 403.
    //    Non-members already got 403 at the extractor.
    if !can(&principal, Action::GroupSettings) {
        return Err(RestError::Forbidden);
    }

    // 3. Structural body validation (no DB access, PII-safe messages).
    body.validate()
        .map_err(|msg| RestError::BadRequest(msg.into()))?;

    // 4. Resolve role once for the response payload. Same invariant as
    //    `get_group`: if `group_id` is Some then `role` must be Some —
    //    the extractor guarantees it, and breaking that invariant is
    //    a 500, not a silent empty-role leak.
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

    // 5. Transactional UPDATE. `SET LOCAL` must be the first statement
    //    inside the tx — plan 0016 M4 pattern. This handler mirrors
    //    `create_group` (not `get_group`, which has a FIXME about the
    //    same hazard) so the tenant context is already correct when
    //    `groups` eventually gains FORCE RLS.
    //
    //    SET LOCAL does not accept bind params in Postgres; Uuid Display
    //    is 36 hex-dashed chars, injection-safe by construction.
    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    sqlx::query(&format!(
        "SET LOCAL app.current_user_id = '{}'",
        principal.user_id
    ))
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    // 6. UPDATE with COALESCE ($new, column) — partial modification:
    //    only fields the caller set are overwritten, the rest stay
    //    untouched. `updated_at = now()` is ALWAYS in the SET clause
    //    because `groups` has no trigger (migration 001 line 115).
    //    RETURNING gives us the fresh row in the same roundtrip.
    let row: Option<(Uuid, String, String, DateTime<Utc>, Uuid)> = sqlx::query_as(
        r#"
        UPDATE groups
           SET name       = COALESCE($1, name),
               type       = COALESCE($2, type),
               updated_at = now()
         WHERE id = $3
     RETURNING id, name, type, created_at, created_by
        "#,
    )
    .bind(body.name.as_deref())
    .bind(body.group_type.as_deref())
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let (id, name, group_type, created_at, created_by) = row.ok_or(RestError::NotFound)?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

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

    // ─── plan 0017 Task 1: UpdateGroupRequest ──────────────────────────

    #[test]
    fn update_group_request_empty_body_all_none() {
        let req: UpdateGroupRequest = serde_json::from_str("{}").unwrap();
        assert!(req.name.is_none());
        assert!(req.group_type.is_none());
        assert!(req.is_empty());
    }

    #[test]
    fn update_group_request_deserializes_type_with_rename() {
        let req: UpdateGroupRequest = serde_json::from_str(r#"{"type":"family"}"#).unwrap();
        assert_eq!(req.group_type.as_deref(), Some("family"));
        assert!(req.name.is_none());
        assert!(!req.is_empty());
    }

    #[test]
    fn update_group_request_name_only_is_not_empty() {
        let req: UpdateGroupRequest = serde_json::from_str(r#"{"name":"new name"}"#).unwrap();
        assert_eq!(req.name.as_deref(), Some("new name"));
        assert!(req.group_type.is_none());
        assert!(!req.is_empty());
    }

    #[test]
    fn update_group_request_validate_rejects_personal_type() {
        let req = UpdateGroupRequest {
            name: None,
            group_type: Some("personal".into()),
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "group type must be 'family' or 'team'"
        );
    }

    #[test]
    fn update_group_request_validate_rejects_empty_name() {
        let req = UpdateGroupRequest {
            name: Some("   ".into()),
            group_type: None,
        };
        assert_eq!(req.validate().unwrap_err(), "name must not be empty");
    }

    #[test]
    fn update_group_request_validate_rejects_empty_body() {
        let req = UpdateGroupRequest {
            name: None,
            group_type: None,
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "patch body must set at least one field"
        );
    }

    #[test]
    fn update_group_request_validate_accepts_valid_family_rename() {
        let req = UpdateGroupRequest {
            name: Some("Updated".into()),
            group_type: Some("family".into()),
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn update_group_request_rejects_unknown_fields() {
        // deny_unknown_fields: typo-protects clients from silent
        // drops when they misspell `name`/`type`.
        let err = serde_json::from_str::<UpdateGroupRequest>(r#"{"nmae":"typo"}"#);
        assert!(err.is_err(), "deny_unknown_fields should reject `nmae`");
    }
}
