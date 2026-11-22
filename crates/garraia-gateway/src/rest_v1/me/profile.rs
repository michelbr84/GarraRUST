//! `GET /v1/me` + `PATCH /v1/me` — caller identity and self-service
//! profile correction (display name inline, e-mail as a recorded request).

// Request-path (#1569): panic aqui e controlavel por quem manda a requisicao.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use axum::Json;
use axum::extract::State;
use chrono::{DateTime, Utc};
use garraia_auth::{Principal, WorkspaceAuditAction, audit_workspace_event};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::rest_v1::RestV1FullState;
use crate::rest_v1::problem::RestError;

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
        (status = 401, description = "Missing or invalid JWT", body = crate::rest_v1::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member of X-Group-Id", body = crate::rest_v1::problem::ProblemDetails),
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
    /// Requested NEW e-mail address (LGPD art. 18, III / GDPR art. 16).
    ///
    /// **This does not change the e-mail.** It files a `pending` request in
    /// `user_email_change_requests`; applying it requires out-of-band proof
    /// that the subject controls the new mailbox. See
    /// [`patch_me`] and `docs/legal/data-subject-requests.md`.
    pub email: Option<String>,
}

/// Longest address `PATCH /v1/me` accepts — the schema bound of
/// `user_email_change_requests.requested_email` (migration 034) and the
/// practical ceiling for an SMTP path.
const EMAIL_MAX_LEN: usize = 320;

/// Shortest syntactically conceivable address (`a@b`).
const EMAIL_MIN_LEN: usize = 3;

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
        if let Some(email) = &self.email {
            validate_email_shape(email)?;
        }
        Ok(())
    }
}

/// Structural sanity check on a requested e-mail address.
///
/// Deliberately shallow: this value is not used to send anything and is not
/// trusted as an identity. It is a request a human operator later verifies
/// out of band, so the check only rejects what cannot be an address at all
/// (no `@`, empty local or domain part, embedded whitespace or control
/// characters, absurd length). Full deliverability validation belongs to the
/// verification step, not here — a stricter regex would reject legitimate
/// intranet addresses and buy nothing, because possession is what actually
/// gates the change.
fn validate_email_shape(raw: &str) -> Result<(), &'static str> {
    let email = raw.trim();
    if email.len() < EMAIL_MIN_LEN {
        return Err("email is too short to be an address");
    }
    if email.len() > EMAIL_MAX_LEN {
        return Err("email exceeds the 320 character limit");
    }
    // Whitespace and control characters: a header-injection shape, and never
    // valid in the unquoted addresses this endpoint accepts.
    if email.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("email must not contain whitespace or control characters");
    }
    let mut parts = email.split('@');
    let local = parts.next().unwrap_or_default();
    let domain = parts.next().unwrap_or_default();
    if parts.next().is_some() {
        return Err("email must contain exactly one '@'");
    }
    if local.is_empty() {
        return Err("email is missing the part before '@'");
    }
    if domain.is_empty() {
        return Err("email is missing the domain after '@'");
    }
    Ok(())
}

/// A pending e-mail-correction request, as surfaced to its own subject.
#[derive(Debug, Serialize, ToSchema)]
pub struct PendingEmailChange {
    /// UUID of the request row — the handle the operator applies or rejects.
    pub request_id: Uuid,
    /// The address the subject asked to switch to. Returned only to the
    /// subject themselves (RLS owner-only on the table).
    pub requested_email: String,
    /// Always `"pending"` here; terminal states are not surfaced on `PATCH`.
    pub status: String,
    /// When the request was filed (UTC).
    pub requested_at: DateTime<Utc>,
}

/// Response body for `PATCH /v1/me`.
#[derive(Debug, Serialize, ToSchema)]
pub struct PatchMeResponse {
    /// UUID of the authenticated user.
    pub user_id: Uuid,
    /// Current email address (read-only, returned for client sync).
    ///
    /// Unchanged by an `email` field in the request: a correction request is
    /// recorded, never applied inline. Compare against
    /// `email_change_pending.requested_email` to show "pending change".
    pub email: String,
    /// Current display name after the update.
    pub display_name: String,
    /// Account creation timestamp (UTC).
    pub created_at: DateTime<Utc>,
    /// Last update timestamp (UTC).
    pub updated_at: DateTime<Utc>,
    /// The open e-mail-correction request, when this call filed one.
    /// Absent when the request body carried no `email`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email_change_pending: Option<PendingEmailChange>,
}

#[derive(sqlx::FromRow)]
struct UserRow {
    id: Uuid,
    email: String,
    display_name: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// `PATCH /v1/me` — self-service correction of the caller's own profile
/// (LGPD art. 18, III / GDPR art. 16: right to rectification).
///
/// ## `display_name` — applied immediately
///
/// Free-form label, not an identity key. Written straight to `users`.
///
/// ## `email` — recorded as a request, NEVER applied inline
///
/// `users.email` is the login key: `user_identities.provider_sub` carries the
/// same address for `provider = 'internal'` (migration 001) and
/// `verify_internal` matches on it. Switching it on the strength of an access
/// token alone is an account-takeover primitive — a stolen token repoints the
/// account at the attacker's mailbox and captures every future recovery flow.
/// So possession of the new mailbox has to be proven, and a blind swap is not
/// implemented here on purpose.
///
/// Proving possession means e-mailing a confirmation link, and **this
/// repository has no mail infrastructure** (no crate depends on
/// SMTP/lettre/a provider). Rather than invent one or skip the proof, this
/// endpoint implements the honest minimum the repo supports: it files a
/// `pending` row in `user_email_change_requests`, emits
/// `account.email_change_requested`, and leaves `users.email` untouched. An
/// operator verifies possession out of band and applies it — the procedure is
/// in `docs/legal/data-subject-requests.md`. When mail delivery lands, the
/// verification step slots in ahead of the same `applied` transition without
/// changing this contract.
///
/// At most one request is open per subject (partial unique index); a second
/// `PATCH` supersedes the first rather than queueing a second for the
/// operator to disambiguate.
///
/// ### No duplicate-address check, on purpose
///
/// The handler does **not** tell the caller whether the requested address
/// already belongs to another account. Answering that turns this endpoint
/// into an account-enumeration oracle for any address an attacker wants to
/// test. A collision surfaces to the operator at verification time, where it
/// costs an attacker nothing to learn and reveals nothing over HTTP.
///
/// ## Error matrix
///
/// | Condition                                         | Status |
/// |---------------------------------------------------|--------|
/// | Missing/invalid JWT                               | 401    |
/// | Malformed `display_name` or `email` shape         | 400    |
/// | Requested e-mail equals the current one           | 400    |
/// | Account deleted / anonymized / purged             | 409    |
/// | Unknown field in the body                         | 422    |
/// | Happy path                                        | 200    |
#[utoipa::path(
    patch,
    path = "/v1/me",
    request_body = PatchMeRequest,
    responses(
        (status = 200, description = "Profile updated. An `email` in the request is recorded as a pending correction request, not applied.", body = PatchMeResponse),
        (status = 400, description = "Validation error.", body = crate::rest_v1::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = crate::rest_v1::problem::ProblemDetails),
        (status = 409, description = "Account is deleted, anonymized or purged.", body = crate::rest_v1::problem::ProblemDetails),
        (status = 422, description = "Unknown field or malformed body.", body = crate::rest_v1::problem::ProblemDetails),
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

    // Fast path: nothing to change and no request to file — one SELECT, no
    // transaction. Keeps the pre-existing `{}` no-op contract free.
    if body.display_name.is_none() && body.email.is_none() {
        let row: UserRow = sqlx::query_as(
            "SELECT id, email::text AS email, display_name, created_at, updated_at \
             FROM users WHERE id = $1",
        )
        .bind(principal.user_id)
        .fetch_one(pool)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

        return Ok(Json(PatchMeResponse {
            user_id: row.id,
            email: row.email,
            display_name: row.display_name,
            created_at: row.created_at,
            updated_at: row.updated_at,
            email_change_pending: None,
        }));
    }

    let nil_uuid = Uuid::nil();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // Both GUCs: `user_email_change_requests` is under FORCE RLS (owner-only)
    // and the audit INSERT needs the group context too.
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

    // FOR UPDATE: serialises against a concurrent anonymize/delete so the
    // status guard below cannot be read stale.
    let current: Option<(String, String)> =
        sqlx::query_as("SELECT status, email::text FROM users WHERE id = $1 FOR UPDATE")
            .bind(principal.user_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

    let (status, current_email) = match current {
        None => {
            return Err(RestError::Internal(anyhow::anyhow!(
                "user row not found for authenticated principal"
            )));
        }
        Some(pair) => pair,
    };

    // A profile under erasure or already anonymised must not be edited: a
    // rectification has nothing to rectify, and accepting the write would
    // re-introduce an identifier into a row that was just stripped of them.
    if matches!(status.as_str(), "deleted" | "anonymized" | "purged") {
        return Err(RestError::Conflict(format!(
            "account status '{status}' does not accept profile changes"
        )));
    }

    let row: UserRow = if let Some(dn) = body.display_name.as_deref() {
        sqlx::query_as(
            "UPDATE users SET display_name = $2, updated_at = now() \
             WHERE id = $1 \
             RETURNING id, email::text AS email, display_name, created_at, updated_at",
        )
        .bind(principal.user_id)
        .bind(dn)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    } else {
        sqlx::query_as(
            "SELECT id, email::text AS email, display_name, created_at, updated_at \
             FROM users WHERE id = $1",
        )
        .bind(principal.user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
    };

    let mut email_change_pending = None;

    if let Some(requested_raw) = body.email.as_deref() {
        let requested = requested_raw.trim();

        // citext compares case-insensitively, so compare the same way the
        // database would. "Correcting" to the address already on file is a
        // no-op the operator should never have to look at.
        if requested.eq_ignore_ascii_case(current_email.trim()) {
            return Err(RestError::BadRequest(
                "requested email is already the address on this account".into(),
            ));
        }

        // Supersede any open request. The partial unique index allows exactly
        // one `pending` row per subject, so this has to run before the INSERT.
        let superseded = sqlx::query(
            "UPDATE user_email_change_requests \
             SET status = 'superseded', resolved_at = now() \
             WHERE user_id = $1 AND status = 'pending'",
        )
        .bind(principal.user_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?
        .rows_affected();

        type RequestRow = (Uuid, String, String, DateTime<Utc>);
        let (request_id, requested_email, req_status, requested_at): RequestRow = sqlx::query_as(
            "INSERT INTO user_email_change_requests (user_id, requested_email) \
             VALUES ($1, $2) \
             RETURNING id, requested_email::text, status, requested_at",
        )
        .bind(principal.user_id)
        .bind(requested)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

        // Audit metadata carries the LENGTH, never the address: the requested
        // e-mail is PII and `audit_events` is readable by group admins on the
        // group-scoped branch of its policy.
        audit_workspace_event(
            &mut tx,
            WorkspaceAuditAction::AccountEmailChangeRequested,
            principal.user_id,
            nil_uuid,
            "user_email_change_requests",
            request_id.to_string(),
            json!({
                "requested_email_len": requested.chars().count(),
                "superseded": superseded > 0,
            }),
        )
        .await
        .map_err(|e| RestError::Internal(anyhow::anyhow!("{e}")))?;

        email_change_pending = Some(PendingEmailChange {
            request_id,
            requested_email,
            status: req_status,
            requested_at,
        });
    }

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(Json(PatchMeResponse {
        user_id: row.id,
        email: row.email,
        display_name: row.display_name,
        created_at: row.created_at,
        updated_at: row.updated_at,
        email_change_pending,
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
            email: None,
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
            email: None,
        };
        assert!(req.validate().is_err(), "empty name must fail validation");
    }

    #[test]
    fn patch_me_request_allows_none() {
        let req = PatchMeRequest {
            display_name: None,
            email: None,
        };
        assert!(req.validate().is_ok(), "None display_name is valid (no-op)");
    }

    #[test]
    fn patch_me_request_rejects_unknown_fields() {
        let json = r#"{"display_name": "Alice", "status": "deleted"}"#;
        let result: Result<PatchMeRequest, _> = serde_json::from_str(json);
        assert!(result.is_err(), "unknown field 'status' must be rejected");
    }
}
