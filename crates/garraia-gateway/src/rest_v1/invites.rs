//! `/v1/invites` handlers (plan 0019).
//!
//! ## `POST /v1/invites/{token}/accept`
//!
//! Accepts a pending group invite. The caller provides the plaintext
//! token in the path. The handler:
//!
//! ## Route shape note
//!
//! Plan 0019 drafted the path as `/v1/invites/{token}:accept` (Google
//! Cloud custom-action style). Axum 0.8 / `matchit` rejects mixed
//! `{param}:literal` in the same segment ("Only one parameter is
//! allowed per path segment"), so the delivered path is two segments:
//! `/v1/invites/{token}/accept`. Semantics unchanged — token is still
//! the primary resource identifier and `accept` is the verb sub-path.
//!
//! 1. Fetches all pending invites (`accepted_at IS NULL`).
//! 2. Verifies the token against each `token_hash` (Argon2id).
//! 3. Checks expiration (`expires_at >= now()`).
//! 4. Checks the caller is not already a member of the group.
//! 5. Atomically updates the invite and inserts a `group_members` row.
//!
//! The caller does NOT need an `X-Group-Id` header — the group is
//! resolved from the matched invite row.

use argon2::PasswordVerifier;
use axum::Json;
use axum::extract::{Path, State};
use chrono::{DateTime, Utc};
use garraia_auth::Principal;
use password_hash::PasswordHash;
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::RestV1FullState;
use super::problem::RestError;

/// Row shape for the pending-invites SELECT in [`accept_invite`].
/// Factored into an alias to keep the handler under `clippy::type_complexity`.
///
/// Layout: `(invite_id, group_id, token_hash, proposed_role, expires_at, accepted_at)`.
type PendingInviteRow = (
    Uuid,
    Uuid,
    String,
    String,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
);

/// Matched-invite tuple after the Argon2id hash search. Same shape as
/// `PendingInviteRow` minus `token_hash` (no longer needed post-match).
///
/// Layout: `(invite_id, group_id, role, expires_at, accepted_at)`.
type MatchedInvite = (Uuid, Uuid, String, DateTime<Utc>, Option<DateTime<Utc>>);

/// Response body for `POST /v1/invites/{token}/accept` (200 OK).
#[derive(Debug, Serialize, ToSchema)]
pub struct AcceptInviteResponse {
    /// The group the caller just joined.
    pub group_id: Uuid,
    /// The role assigned from the invite.
    pub role: String,
    /// The invite ID that was accepted.
    pub invite_id: Uuid,
}

/// `POST /v1/invites/{token}/accept` — accept a pending group invite.
///
/// The plaintext invite token travels in the path. The handler verifies
/// it against Argon2id hashes stored in `group_invites.token_hash`.
///
/// ## Error matrix
///
/// | Condition                          | Status | Guard          |
/// |------------------------------------|--------|----------------|
/// | Missing/invalid JWT                | 401    | Principal      |
/// | Token not found (no hash match)    | 404    | handler        |
/// | Invite already accepted            | 404    | handler (\*)   |
/// | Invite expired                     | 410    | handler        |
/// | Caller already member of group     | 409    | handler        |
/// | Happy path                         | 200    |                |
///
/// (\*) Already-accepted invites are filtered out by the `accepted_at
/// IS NULL` pending-set SELECT, so a double-accept attempt does not
/// find any matching hash and returns 404. The defensive
/// `accepted_at.is_some()` branch below is dead code in practice but
/// kept as a belt-and-suspenders guard against a future refactor that
/// broadens the SELECT.
///
/// ## SQL injection posture
///
/// `SET LOCAL` does not accept bind parameters in Postgres, so the
/// `user_id` UUID is interpolated via `format!`. `Uuid::Display`
/// produces exactly 36 hex-with-dash characters and no metacharacters
/// — injection-safe by construction. All other parameters use
/// `sqlx::query::bind` as normal. Same pattern as `groups.rs`.
#[utoipa::path(
    post,
    path = "/v1/invites/{token}/accept",
    params(
        ("token" = String, Path, description = "Plaintext invite token (URL-safe base64)."),
    ),
    responses(
        (status = 200, description = "Invite accepted; caller is now a group member.", body = AcceptInviteResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 404, description = "No pending invite matches this token.", body = super::problem::ProblemDetails),
        (status = 409, description = "Invite already accepted or caller already a member.", body = super::problem::ProblemDetails),
        (status = 410, description = "Invite has expired.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn accept_invite(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(token): Path<String>,
) -> Result<Json<AcceptInviteResponse>, RestError> {
    let pool = state.app_pool.pool_for_handlers();

    // 1. Fetch ALL pending invites. For v1 volume this is acceptable
    //    (low absolute count; the `group_invites_pending_unique`
    //    partial index from migration 011 bounds it to at most one
    //    row per `(group_id, email)`). If scale becomes a concern,
    //    a future optimization is a `LEFT(token_hash, 8)` bloom hint
    //    — out of scope for plan 0019.
    let pending: Vec<PendingInviteRow> = sqlx::query_as(
        "SELECT id, group_id, token_hash, proposed_role, expires_at, accepted_at \
             FROM group_invites \
             WHERE accepted_at IS NULL \
             ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    // 2. Verify token against each hash until match. Malformed hashes
    //    (shouldn't happen — `create_invite` always emits PHC strings)
    //    are skipped with a warning rather than crashing the handler.
    let argon = argon2::Argon2::default();
    let mut matched: Option<MatchedInvite> = None;

    for (invite_id, group_id, hash, role, expires_at, accepted_at) in &pending {
        let Ok(parsed) = PasswordHash::new(hash) else {
            tracing::warn!(
                invite_id = %invite_id,
                "malformed token_hash in group_invites; skipping"
            );
            continue;
        };
        if argon.verify_password(token.as_bytes(), &parsed).is_ok() {
            matched = Some((
                *invite_id,
                *group_id,
                role.clone(),
                *expires_at,
                *accepted_at,
            ));
            break;
        }
    }

    let (invite_id, group_id, role, expires_at, accepted_at) =
        matched.ok_or(RestError::NotFound)?;

    // 3. Defensive double-accept guard. Dead code in practice because
    //    the SELECT above already filters `accepted_at IS NULL`.
    if accepted_at.is_some() {
        return Err(RestError::Conflict(
            "this invite has already been accepted".into(),
        ));
    }

    // 4. Expiration check.
    if expires_at < Utc::now() {
        return Err(RestError::Gone("this invite has expired".into()));
    }

    // 5. Transactional: SET LOCAL + UPDATE invite + INSERT member.
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

    // 5a. Mark invite as accepted.
    sqlx::query("UPDATE group_invites SET accepted_at = now(), accepted_by = $1 WHERE id = $2")
        .bind(principal.user_id)
        .bind(invite_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // 5b. Insert group_members. SQLSTATE 23505 (PK violation on
    //     `(group_id, user_id)`) means the caller is already a
    //     member of this group — 409 Conflict. The `tx` is dropped
    //     without commit, rolling back both the invite UPDATE and
    //     the failed INSERT — the invite stays pending.
    let insert_result = sqlx::query(
        "INSERT INTO group_members (group_id, user_id, role, status, invited_by) \
         VALUES ($1, $2, $3, 'active', $4)",
    )
    .bind(group_id)
    .bind(principal.user_id)
    .bind(&role)
    .bind(principal.user_id)
    .execute(&mut *tx)
    .await;

    match insert_result {
        Ok(_) => {}
        Err(sqlx::Error::Database(ref db_err)) if db_err.code().as_deref() == Some("23505") => {
            return Err(RestError::Conflict(
                "you are already a member of this group".into(),
            ));
        }
        Err(e) => return Err(RestError::Internal(e.into())),
    }

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(Json(AcceptInviteResponse {
        group_id,
        role,
        invite_id,
    }))
}
