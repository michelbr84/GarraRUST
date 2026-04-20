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

use argon2::PasswordHasher;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use garraia_auth::{Action, Principal, WorkspaceAuditAction, audit_workspace_event, can};
use password_hash::rand_core::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::json;
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

/// Accepted values for `CreateInviteRequest::role`.
///
/// Mirrors the `CHECK (proposed_role IN ('admin','member','guest','child'))`
/// on `group_invites.proposed_role` in migration 001 line 141. `"owner"` is
/// excluded — owners are created during group bootstrap only (comment line 155).
const ALLOWED_INVITE_ROLES: &[&str] = &["admin", "member", "guest", "child"];

/// Accepted values for `SetRoleRequest::role` (plan 0020 slice 4).
///
/// Same subset as [`ALLOWED_INVITE_ROLES`]: excludes `"owner"` because
/// ownership transfer is a distinct operation (future endpoint). Excludes
/// `"personal"` — reserved type marker (see [`ALLOWED_GROUP_TYPES`]).
/// The partial unique index `group_members_single_owner_idx`
/// (migration 002 line 146) defensively rejects promote-to-owner at the
/// DB layer, but the handler filters first for a clearer 400 message.
const ALLOWED_SETROLE_VALUES: &[&str] = &["admin", "member", "guest", "child"];

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

/// Request body for `POST /v1/groups/{id}/invites` (plan 0018).
///
/// Creates a pending invite for the given email. The caller must have
/// `Action::MembersManage` (Owner or Admin).
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateInviteRequest {
    /// Email address to invite. Stored as `citext` (case-insensitive).
    pub email: String,
    /// Role to grant on acceptance. Must be one of: `admin`, `member`,
    /// `guest`, `child`. `owner` is not invitable.
    pub role: String,
}

impl CreateInviteRequest {
    /// Structural validation. PII-safe error messages only.
    pub fn validate(&self) -> Result<(), &'static str> {
        let trimmed = self.email.trim();
        if trimmed.is_empty() {
            return Err("email must not be empty");
        }
        if !trimmed.contains('@') {
            return Err("email must contain '@'");
        }
        if !ALLOWED_INVITE_ROLES.contains(&self.role.as_str()) {
            return Err("role must be one of: admin, member, guest, child");
        }
        Ok(())
    }
}

/// Response body for `POST /v1/groups/{id}/invites` (201 Created).
///
/// `token` is the **plaintext** invite token — returned exactly once.
/// The database stores only the Argon2id hash. Callers should forward
/// this token to the invitee (e.g. via email or direct link).
#[derive(Debug, Serialize, ToSchema)]
pub struct InviteResponse {
    pub id: Uuid,
    pub group_id: Uuid,
    pub invited_email: String,
    pub proposed_role: String,
    /// Opaque plaintext token. Share with the invitee. Returned once.
    pub token: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// Request body for `POST /v1/groups/{id}/members/{user_id}/setRole` (plan 0020 slice 4).
///
/// Changes a member's role inside the group. The caller must either be
/// acting on themselves (self-demote) or hold `Action::MembersManage`
/// (Owner/Admin). Hierarchy rules — Admin cannot modify Owner or another
/// Admin (non-self) — are enforced inside the handler, not here.
///
/// Accepted values: `admin`, `member`, `guest`, `child`. `owner` is
/// explicitly rejected (400 `cannot promote to owner via setRole`) —
/// ownership transfer is a separate operation. See [`ALLOWED_SETROLE_VALUES`].
///
/// ## Path convention
///
/// Linear canonical notation uses Google Cloud custom-verb `:setRole`;
/// Axum 0.8 / `matchit` does not support `{param}:literal` in the same
/// segment (same constraint that forced `/accept` over `:accept` in plan
/// 0019). Delivered path is `/setRole` as a literal segment.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetRoleRequest {
    /// New role. Must be one of: `admin`, `member`, `guest`, `child`.
    /// `owner` is rejected — ownership transfer is a separate operation.
    pub role: String,
}

impl SetRoleRequest {
    /// Structural validation. PII-safe error messages only.
    ///
    /// Order of checks matters: the `"owner"` rejection produces a clearer
    /// error message ("cannot promote to owner via setRole") than the
    /// generic `"role must be one of ..."`, so we filter `"owner"` first.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.role == "owner" {
            return Err("cannot promote to owner via setRole");
        }
        if !ALLOWED_SETROLE_VALUES.contains(&self.role.as_str()) {
            return Err("role must be one of: admin, member, guest, child");
        }
        Ok(())
    }
}

/// Response body for setRole (200 OK).
///
/// DELETE member returns 204 No Content without a body. This struct is
/// used only by setRole. Fields mirror the shape of a `group_members`
/// row plus the `updated_at` timestamp set by the UPDATE.
#[derive(Debug, Serialize, ToSchema)]
pub struct MemberResponse {
    pub group_id: Uuid,
    pub user_id: Uuid,
    pub role: String,
    pub status: String,
    pub updated_at: DateTime<Utc>,
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

/// `POST /v1/groups/{id}/invites` — create a pending invite.
///
/// Generates a 32-byte random token, hashes it with Argon2id, stores the
/// hash in `group_invites.token_hash`, and returns the plaintext token
/// exactly once in the response body.
///
/// Duplicate check: if a pending invite (`accepted_at IS NULL`) already
/// exists for the same `(group_id, invited_email)`, returns 409 Conflict.
///
/// ## Error matrix
///
/// | Condition                                    | Status | Guard          |
/// |----------------------------------------------|--------|----------------|
/// | Missing/invalid JWT                          | 401    | Principal      |
/// | Non-member                                   | 403    | Principal      |
/// | X-Group-Id / path id mismatch                | 400    | handler        |
/// | Role is Member/Guest/Child                   | 403    | `can()`        |
/// | Invalid body (email, role)                   | 400    | validate()     |
/// | Duplicate pending invite                     | 409    | SELECT check   |
/// | Happy path                                   | 201    |                |
#[utoipa::path(
    post,
    path = "/v1/groups/{id}/invites",
    params(
        ("id" = Uuid, Path, description = "Group UUID. Must match the `X-Group-Id` header."),
    ),
    request_body = CreateInviteRequest,
    responses(
        (status = 201, description = "Invite created; response carries the plaintext token (returned once).", body = InviteResponse),
        (status = 400, description = "Invalid body, header/path mismatch, or reserved role.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller lacks `members.manage` capability.", body = super::problem::ProblemDetails),
        (status = 409, description = "Pending invite already exists for this email+group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn create_invite(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateInviteRequest>,
) -> Result<(StatusCode, Json<InviteResponse>), RestError> {
    // 1. Header/path coherence (same pattern as get_group/patch_group).
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
    if !can(&principal, Action::MembersManage) {
        return Err(RestError::Forbidden);
    }

    // 3. Structural body validation.
    body.validate()
        .map_err(|msg| RestError::BadRequest(msg.into()))?;

    // 4. Generate invite token: 32 random bytes → URL-safe base64.
    //    Uses `password_hash::rand_core::OsRng` (rand_core 0.6) to
    //    avoid version conflict with argon2 0.5's rand_core dep.
    let mut token_bytes = [0u8; 32];
    password_hash::rand_core::OsRng.fill_bytes(&mut token_bytes);
    let token_plaintext = URL_SAFE_NO_PAD.encode(token_bytes);

    // 5. Hash the token with Argon2id. The token is a random secret,
    //    not a user password, so we use default Argon2 params directly
    //    rather than the garraia-auth RFC 9106 tuned params.
    let salt = password_hash::SaltString::generate(&mut password_hash::rand_core::OsRng);
    let token_hash = argon2::Argon2::default()
        .hash_password(token_plaintext.as_bytes(), &salt)
        .map_err(|e| RestError::Internal(anyhow::anyhow!("argon2 hash failure: {e}")))?
        .to_string();

    // 6. Transactional INSERT with duplicate check.
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

    // 6. INSERT the invite with conflict-safe duplicate detection.
    //
    //    Migration 011 adds a partial unique index:
    //      CREATE UNIQUE INDEX group_invites_pending_unique
    //        ON group_invites(group_id, invited_email)
    //        WHERE accepted_at IS NULL;
    //
    //    If a concurrent request inserts the same (group_id, email)
    //    pair, Postgres raises SQLSTATE 23505 (unique_violation) which
    //    we catch and map to 409 Conflict. This is race-free — the
    //    database enforces uniqueness atomically, no TOCTOU.
    let email = body.email.trim();

    let insert_result: Result<(Uuid, String, DateTime<Utc>, DateTime<Utc>), sqlx::Error> =
        sqlx::query_as(
            "INSERT INTO group_invites \
                 (group_id, invited_email, proposed_role, token_hash, expires_at, created_by) \
             VALUES ($1, $2, $3, $4, now() + interval '7 days', $5) \
             RETURNING id, invited_email, expires_at, created_at",
        )
        .bind(id)
        .bind(email)
        .bind(&body.role)
        .bind(&token_hash)
        .bind(principal.user_id)
        .fetch_one(&mut *tx)
        .await;

    let row = match insert_result {
        Ok(r) => r,
        Err(sqlx::Error::Database(ref db_err)) if db_err.code().as_deref() == Some("23505") => {
            return Err(RestError::Conflict(
                "a pending invite already exists for this email in this group".into(),
            ));
        }
        Err(e) => return Err(RestError::Internal(e.into())),
    };

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok((
        StatusCode::CREATED,
        Json(InviteResponse {
            id: row.0,
            group_id: id,
            invited_email: row.1,
            proposed_role: body.role,
            token: token_plaintext,
            expires_at: row.2,
            created_at: row.3,
        }),
    ))
}

/// `POST /v1/groups/{id}/members/{user_id}/setRole` — change a member's role (plan 0020 slice 4).
///
/// ## Authz model
///
/// Two layers:
///
/// 1. **Capability gate:** caller must hold `Action::MembersManage`
///    (Owner/Admin) — *unless* `target_user_id == principal.user_id`,
///    in which case the capability check is bypassed (self-demote /
///    self-action path). This preserves the ability for any member to
///    self-demote subject to the last-owner invariant below.
/// 2. **Hierarchy gate (non-self only):** Admin callers cannot modify
///    Owner or other Admins. Owner callers can modify any role.
///
/// ## Self-action bypass — intent
///
/// Both gates skip their check when `target == caller`. This is
/// **deliberate**: a Member/Guest/Child must be able to self-downgrade
/// (e.g. drop their own role before a leave in a future endpoint), an
/// Admin must be able to self-demote without contacting an Owner, and
/// an Owner must be able to self-demote to transfer de-facto control to
/// a co-owner. The safety net for all these paths is the **last-owner
/// invariant** enforced post-UPDATE inside the same transaction — not
/// the capability/hierarchy gates. This separation means that even
/// though self-setRole is broadly permissive, the system can never end
/// up in a zero-active-owner state (returns 409 and rolls back). Plan
/// 0020 security review (SEC-MED) confirmed this design; any future
/// tightening of self-demotion (e.g. Admin → Guest/Child) should live
/// in a dedicated check with its own test matrix rather than a blanket
/// restriction on the bypass.
///
/// ## Last-owner invariant
///
/// After the UPDATE, the handler counts active owners in the same
/// transaction. If the count drops to zero (the last Owner was demoted
/// — possibly via self-demote), the transaction is aborted without
/// commit and 409 Conflict is returned. The UPDATE is rolled back.
///
/// ## Promote-to-owner rejection
///
/// Body with `role = "owner"` is rejected with 400 at the
/// [`SetRoleRequest::validate`] gate before any DB access.
/// `group_members_single_owner_idx` (migration 002 line 146) provides
/// defense-in-depth at the DB layer.
///
/// ## Serialization / concurrency
///
/// `SELECT ... FOR UPDATE` on the target's active membership row
/// acquires a per-row lock; concurrent setRole/delete calls on the
/// same `(group_id, user_id)` serialize through this lock and each
/// re-evaluates hierarchy + last-owner rules against a consistent
/// snapshot.
///
/// ## Error matrix
///
/// | Condition                                      | Status | Guard          |
/// |------------------------------------------------|--------|----------------|
/// | Missing/invalid JWT                            | 401    | Principal ext. |
/// | Non-member of target group                     | 403    | Principal ext. |
/// | `X-Group-Id` missing / mismatched              | 400    | this handler   |
/// | Invalid body (unknown role, `role = "owner"`)  | 400    | validate()     |
/// | Non-self caller lacks `MembersManage`          | 403    | `can()`        |
/// | Admin caller tries to modify Owner/Admin       | 403    | hierarchy      |
/// | Target not found / not active                  | 404    | SELECT/UPDATE  |
/// | Would demote last Owner                        | 409    | post-UPDATE    |
/// | Happy path                                     | 200    |                |
#[utoipa::path(
    post,
    path = "/v1/groups/{id}/members/{user_id}/setRole",
    params(
        ("id" = Uuid, Path, description = "Group UUID. Must match the `X-Group-Id` header."),
        ("user_id" = Uuid, Path, description = "Target member's user id."),
    ),
    request_body = SetRoleRequest,
    responses(
        (status = 200, description = "Role updated; response carries the fresh member row.", body = MemberResponse),
        (status = 400, description = "Invalid body, header/path mismatch, or `role = owner`.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller lacks `members.manage` or violates hierarchy.", body = super::problem::ProblemDetails),
        (status = 404, description = "Target member not found or not active.", body = super::problem::ProblemDetails),
        (status = 409, description = "Would leave the group without an owner.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn set_member_role(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((id, target_user_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<SetRoleRequest>,
) -> Result<Json<MemberResponse>, RestError> {
    // 1. Header/path coherence (same pattern as get_group/patch_group).
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

    // 2. Structural body validation (no DB access, PII-safe messages).
    //    Rejects `role = "owner"` with a dedicated message and unknown
    //    roles with the generic subset message.
    body.validate()
        .map_err(|msg| RestError::BadRequest(msg.into()))?;

    // 3. Camada 1 — capability gate with self-action bypass.
    //    Self (caller == target) may always attempt the operation;
    //    the last-owner invariant below catches the dangerous case.
    let is_self = principal.user_id == target_user_id;
    if !is_self && !can(&principal, Action::MembersManage) {
        return Err(RestError::Forbidden);
    }

    // 4. Resolve caller's role for the hierarchy gate. Same invariant
    //    as `get_group` / `patch_group`: if `group_id` is Some then
    //    `role` must be Some — extractor guarantees it; break = 500.
    let caller_role = principal
        .role
        .as_ref()
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

    // 5. Open tx and set tenant context. `SET LOCAL` MUST be the first
    //    statement — plan 0016 M4 pattern. `Uuid::Display` is 36 hex
    //    chars with dashes, injection-safe.
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

    // Plan 0021 T4: also set `app.current_group_id` — required by the
    // `audit_events_group_or_self` RLS policy (migration 007:161-168)
    // for the audit INSERT at the end of this handler. Uuid Display
    // is injection-safe.
    sqlx::query(&format!("SET LOCAL app.current_group_id = '{id}'"))
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // 6. Fetch target's current role under FOR UPDATE lock. Serializes
    //    concurrent setRole/delete on the same member — the second
    //    caller blocks here until the first commits, then sees the
    //    new role and can re-apply hierarchy + last-owner rules.
    //    Filters `status = 'active'` — soft-deleted targets → 404.
    let target_row: Option<(String,)> = sqlx::query_as(
        "SELECT role FROM group_members \
         WHERE group_id = $1 AND user_id = $2 AND status = 'active' \
         FOR UPDATE",
    )
    .bind(id)
    .bind(target_user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let target_role = match target_row {
        Some((r,)) => r,
        None => return Err(RestError::NotFound),
    };

    // 7. Camada 2 — hierarchy gate (non-self only). Admin cannot modify
    //    Owner nor other Admins. Owner may modify any role (the
    //    last-owner invariant below still applies).
    if !is_self && caller_role == "admin" && (target_role == "owner" || target_role == "admin") {
        return Err(RestError::Forbidden);
    }

    // 8. UPDATE the role. `now()` in RETURNING yields the statement
    //    timestamp of this tx — used as `updated_at` in the response
    //    since `group_members` has no persisted updated_at column.
    //    Filter `status = 'active'` defends against a race where the
    //    row becomes 'removed' between the SELECT FOR UPDATE and now
    //    (not possible under the current lock, but the redundant
    //    guard is cheap insurance).
    let updated: Option<(Uuid, Uuid, String, String, DateTime<Utc>)> = sqlx::query_as(
        "UPDATE group_members \
            SET role = $3 \
          WHERE group_id = $1 AND user_id = $2 AND status = 'active' \
      RETURNING group_id, user_id, role, status, now() AS updated_at",
    )
    .bind(id)
    .bind(target_user_id)
    .bind(&body.role)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let (group_id, user_id, role, status, updated_at) = updated.ok_or(RestError::NotFound)?;

    // 9. Last-owner invariant (post-UPDATE). Because the UPDATE is
    //    visible inside its own tx, this COUNT reflects the state
    //    that *would* be committed if we proceeded. Zero owners =>
    //    abort and return 409. Dropping `tx` without `.commit()`
    //    rolls back the UPDATE automatically.
    //
    //    TODO(plan-0021): `group_members_single_owner_idx` (migration
    //    002:146) is a partial UNIQUE `WHERE role = 'owner'` — it does
    //    NOT filter by `status = 'active'`, which means the DB-level
    //    constraint diverges from this COUNT's predicate. The gap is
    //    safe today (API has no way to create two active owners —
    //    setRole rejects role='owner') but a follow-up plan should
    //    amend the partial index to `WHERE role = 'owner' AND status =
    //    'active'` via forward-only migration so DB and app-layer
    //    invariants stay aligned.
    let (owners_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::bigint \
           FROM group_members \
          WHERE group_id = $1 AND role = 'owner' AND status = 'active'",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if owners_count == 0 {
        return Err(RestError::Conflict(
            "cannot leave the group without an owner".into(),
        ));
    }

    // Plan 0021 T4: audit_events row for member.role_changed. Runs
    // AFTER the COUNT guard so a 409 rolls back without emitting an
    // audit row for a mutation that did not stick (consistent with
    // the login flow's "audit only for committed events" policy).
    //
    // Metadata carries the diff (old/new role) and the target. No
    // PII — only UUIDs and role enum strings.
    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::MemberRoleChanged,
        principal.user_id,
        id,
        "group_members",
        format!("{id}:{target_user_id}"),
        json!({
            "target_user_id": target_user_id,
            "old_role": target_role,
            "new_role": body.role,
        }),
    )
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    // 10. Commit. If the commit fails, the UPDATE rolls back and the
    //     caller sees a 500 — never a partial state.
    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(Json(MemberResponse {
        group_id,
        user_id,
        role,
        status,
        updated_at,
    }))
}

/// `DELETE /v1/groups/{id}/members/{user_id}` — soft-delete a member (plan 0020 slice 4).
///
/// The row is not physically deleted — `status` is flipped to `'removed'`
/// so FKs in `messages.author_id`, `tasks.created_by`, etc. continue to
/// resolve. Reactivation is out of scope for v1 (plan 0022+).
///
/// ## Authz model
///
/// Same two layers as [`set_member_role`]:
///
/// 1. **Capability gate:** `Action::MembersManage` (Owner/Admin) — bypassed
///    when `target_user_id == principal.user_id` (leave-group path).
/// 2. **Hierarchy gate (non-self only):** Admin cannot delete Owner or
///    another Admin. Owner can delete any role.
///
/// ## Self-action bypass — intent
///
/// Both gates skip their check for self-DELETE so any member (regardless
/// of role) can leave the group. The last-owner invariant post-UPDATE
/// catches the only dangerous self-path (sole Owner leaving an otherwise
/// empty of owners group → 409 + rollback). See the equivalent note on
/// [`set_member_role`] for the full design rationale.
///
/// ## Last-owner invariant
///
/// Post-UPDATE, active owners count must be ≥ 1. If the DELETE
/// removed the last Owner (possibly via self-leave), the transaction
/// aborts without commit and 409 is returned.
///
/// ## Idempotence
///
/// DELETE on an already-removed (`status = 'removed'`) or non-existent
/// member returns 404, not 204 — this matches plan 0019's convention for
/// `accept`: callers can distinguish "I performed the removal" (204) from
/// "already gone or never existed" (404). Re-invite + accept of a soft-
/// deleted user will 409 on PK collision (`(group_id, user_id)` unique)
/// — limitation documented in plan 0020 §out of scope.
///
/// ## Error matrix
///
/// | Condition                                      | Status | Guard          |
/// |------------------------------------------------|--------|----------------|
/// | Missing/invalid JWT                            | 401    | Principal ext. |
/// | Non-member of target group                     | 403    | Principal ext. |
/// | `X-Group-Id` missing / mismatched              | 400    | this handler   |
/// | Non-self caller lacks `MembersManage`          | 403    | `can()`        |
/// | Admin caller tries to delete Owner/Admin       | 403    | hierarchy      |
/// | Target not found / already removed             | 404    | SELECT/UPDATE  |
/// | Would leave group ownerless                    | 409    | post-UPDATE    |
/// | Happy path                                     | 204    |                |
#[utoipa::path(
    delete,
    path = "/v1/groups/{id}/members/{user_id}",
    params(
        ("id" = Uuid, Path, description = "Group UUID. Must match the `X-Group-Id` header."),
        ("user_id" = Uuid, Path, description = "Target member's user id."),
    ),
    responses(
        (status = 204, description = "Member soft-deleted (`status = 'removed'`). No response body."),
        (status = 400, description = "`X-Group-Id` header missing or mismatched.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller lacks `members.manage` or violates hierarchy.", body = super::problem::ProblemDetails),
        (status = 404, description = "Target member not found or already removed.", body = super::problem::ProblemDetails),
        (status = 409, description = "Would leave the group without an owner.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn delete_member(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((id, target_user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, RestError> {
    // 1. Header/path coherence.
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

    // 2. Camada 1 — capability gate with self-action bypass.
    let is_self = principal.user_id == target_user_id;
    if !is_self && !can(&principal, Action::MembersManage) {
        return Err(RestError::Forbidden);
    }

    // 3. Resolve caller's role for hierarchy gate (same invariant as
    //    set_member_role / patch_group: Principal.role is Some here).
    let caller_role = principal
        .role
        .as_ref()
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

    // 4. Open tx + SET LOCAL tenant context.
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

    // 5. Fetch target role under FOR UPDATE lock (same serialization
    //    guarantee as set_member_role). Filters `status = 'active'`
    //    — already-removed targets ⇒ 404.
    let target_row: Option<(String,)> = sqlx::query_as(
        "SELECT role FROM group_members \
         WHERE group_id = $1 AND user_id = $2 AND status = 'active' \
         FOR UPDATE",
    )
    .bind(id)
    .bind(target_user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let target_role = match target_row {
        Some((r,)) => r,
        None => return Err(RestError::NotFound),
    };

    // 6. Camada 2 — hierarchy gate (non-self only).
    if !is_self && caller_role == "admin" && (target_role == "owner" || target_role == "admin") {
        return Err(RestError::Forbidden);
    }

    // 7. Soft-delete: UPDATE status = 'removed'. `RETURNING 1` so we
    //    can detect the zero-row case (concurrent removal between
    //    SELECT FOR UPDATE and here — effectively impossible under
    //    the lock but cheap guard).
    let removed: Option<(i32,)> = sqlx::query_as(
        "UPDATE group_members \
            SET status = 'removed' \
          WHERE group_id = $1 AND user_id = $2 AND status = 'active' \
      RETURNING 1",
    )
    .bind(id)
    .bind(target_user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if removed.is_none() {
        return Err(RestError::NotFound);
    }

    // 8. Last-owner invariant (post-UPDATE). If we just removed an
    //    Owner and there are no other active Owners left, abort
    //    and return 409. Running the COUNT unconditionally (rather
    //    than only when `target_role == "owner"`) keeps the logic
    //    uniform and the cost is negligible.
    let (owners_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::bigint \
           FROM group_members \
          WHERE group_id = $1 AND role = 'owner' AND status = 'active'",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if owners_count == 0 {
        return Err(RestError::Conflict(
            "cannot leave the group without an owner".into(),
        ));
    }

    // 9. Commit.
    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::NO_CONTENT)
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

    // ── CreateInviteRequest validation (plan 0018 t2) ────────

    #[test]
    fn create_invite_request_valid() {
        let req = CreateInviteRequest {
            email: "alice@example.com".into(),
            role: "member".into(),
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn create_invite_request_rejects_empty_email() {
        let req = CreateInviteRequest {
            email: "   ".into(),
            role: "member".into(),
        };
        assert_eq!(req.validate().unwrap_err(), "email must not be empty");
    }

    #[test]
    fn create_invite_request_rejects_missing_at() {
        let req = CreateInviteRequest {
            email: "not-an-email".into(),
            role: "member".into(),
        };
        assert_eq!(req.validate().unwrap_err(), "email must contain '@'");
    }

    #[test]
    fn create_invite_request_rejects_owner_role() {
        let req = CreateInviteRequest {
            email: "bob@example.com".into(),
            role: "owner".into(),
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "role must be one of: admin, member, guest, child"
        );
    }

    #[test]
    fn create_invite_request_rejects_unknown_role() {
        let req = CreateInviteRequest {
            email: "bob@example.com".into(),
            role: "superadmin".into(),
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "role must be one of: admin, member, guest, child"
        );
    }

    #[test]
    fn create_invite_request_all_valid_roles() {
        for role in &["admin", "member", "guest", "child"] {
            let req = CreateInviteRequest {
                email: "x@y.com".into(),
                role: role.to_string(),
            };
            assert!(req.validate().is_ok(), "role '{role}' should be valid");
        }
    }

    // ── SetRoleRequest validation (plan 0020 t1) ────────

    #[test]
    fn set_role_request_rejects_owner() {
        let req: SetRoleRequest = serde_json::from_str(r#"{"role":"owner"}"#).unwrap();
        assert_eq!(
            req.validate().unwrap_err(),
            "cannot promote to owner via setRole"
        );
    }

    #[test]
    fn set_role_request_rejects_unknown_role() {
        let req: SetRoleRequest = serde_json::from_str(r#"{"role":"superadmin"}"#).unwrap();
        assert_eq!(
            req.validate().unwrap_err(),
            "role must be one of: admin, member, guest, child"
        );
    }

    #[test]
    fn set_role_request_accepts_admin_member_guest_child() {
        for role in &["admin", "member", "guest", "child"] {
            let req = SetRoleRequest {
                role: role.to_string(),
            };
            assert!(req.validate().is_ok(), "role '{role}' should be valid");
        }
    }

    #[test]
    fn set_role_request_rejects_unknown_field() {
        // deny_unknown_fields: typo-protects clients from silent
        // drops when they misspell `role` (same pattern as
        // UpdateGroupRequest/CreateInviteRequest).
        let err = serde_json::from_str::<SetRoleRequest>(r#"{"rle":"admin"}"#);
        assert!(err.is_err(), "deny_unknown_fields should reject `rle`");
    }

    #[test]
    fn allowed_setrole_values_mirror_invite_roles() {
        // Invariant: setRole accepts the same subset as create-invite.
        // Owner and personal stay excluded. If this ever changes,
        // re-evaluate the plan 0020 design invariants §6 and the
        // hierarchy model before accepting the divergence.
        assert_eq!(ALLOWED_SETROLE_VALUES, ALLOWED_INVITE_ROLES);
    }
}
