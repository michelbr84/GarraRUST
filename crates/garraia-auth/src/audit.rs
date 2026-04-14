//! `audit_login` — central helper for inserting `audit_events` rows from
//! the login flow.
//!
//! Schema reference: `migrations/002_rbac_and_audit.sql`. Real column names
//! (this corrects plan 0011 §6.3 which had stale names from an earlier draft):
//!
//! ```text
//! audit_events:
//!   id              uuid    PK
//!   group_id        uuid    NULL  (always NULL for auth events — they are not group-scoped)
//!   actor_user_id   uuid    NULL  (NULL for failure_user_not_found, otherwise user_id)
//!   actor_label     text    NULL  (cached email at event time, survives erasure)
//!   action          text    NOT NULL
//!   resource_type   text    NOT NULL  ("user_identities")
//!   resource_id     text    NULL      (identity_id::text when known)
//!   ip              inet    NULL      (top-level column, NOT in metadata jsonb)
//!   user_agent      text    NULL      (top-level column, NOT in metadata jsonb)
//!   metadata        jsonb   NOT NULL DEFAULT '{}'
//!   created_at      timestamptz NOT NULL DEFAULT now()
//! ```
//!
//! `actor_user_id` has NO foreign key (intentional — see migration 002 comment)
//! so deleting a user does NOT cascade-delete the audit row. `actor_label`
//! survives the erasure as a snapshot of who the actor was.
//!
//! The insert runs INSIDE the same transaction as `verify_credential` for
//! v1 atomicity. Future hardening can move audit to a fire-and-forget channel.

use serde_json::json;
use sqlx::{Postgres, Transaction};
use std::net::IpAddr;
use uuid::Uuid;

use crate::error::AuthError;
use crate::types::RequestCtx;

/// Canonical action strings emitted by the login flow.
///
/// Stored in `audit_events.action` as a plain string. New variants must be
/// added here AND in any downstream consumer that filters by action.
#[derive(Debug, Clone, Copy)]
pub enum AuditAction {
    /// Successful login. `actor_user_id` is set.
    LoginSuccess,
    /// Email did not resolve to any `user_identities` row. `actor_user_id` is NULL.
    LoginFailureUserNotFound,
    /// User exists, password wrong. `actor_user_id` is set.
    LoginFailureWrongPassword,
    /// User exists, password correct, but `users.status != 'active'`.
    LoginFailureAccountNotActive,
    /// `password_hash` was successfully upgraded from PBKDF2 → Argon2id
    /// in the same transaction as a successful login. Always paired with
    /// `LoginSuccess` (two rows per upgrade event).
    PasswordHashUpgraded,
    /// Stored hash had an unrecognized prefix. Operational misconfiguration.
    /// Recorded for forensic visibility; tx is rolled back by the caller.
    LoginFailureUnknownHash,
}

impl AuditAction {
    pub fn as_str(self) -> &'static str {
        match self {
            AuditAction::LoginSuccess => "login.success",
            AuditAction::LoginFailureUserNotFound => "login.failure_user_not_found",
            AuditAction::LoginFailureWrongPassword => "login.failure_wrong_password",
            AuditAction::LoginFailureAccountNotActive => "login.failure_account_suspended",
            AuditAction::PasswordHashUpgraded => "login.password_hash_upgraded",
            AuditAction::LoginFailureUnknownHash => "login.failure_unknown_hash",
        }
    }
}

/// Insert one `audit_events` row inside the caller's transaction.
///
/// `actor_user_id` is `None` only for `LoginFailureUserNotFound`; for every
/// other action the user has been resolved (even if the verification then
/// failed) and the row carries the user_id.
///
/// `actor_label` is a snapshot of the email at event time. For
/// `LoginFailureUserNotFound` it is the attempted email so post-incident
/// forensics can answer "what address did the attacker try?".
///
/// `request_ctx` fields populate the dedicated `ip`/`user_agent` columns
/// (NOT inside `metadata`); the jsonb `metadata` carries `request_id` and
/// any future event-specific context.
#[allow(clippy::too_many_arguments)]
pub async fn audit_login(
    tx: &mut Transaction<'_, Postgres>,
    action: AuditAction,
    actor_user_id: Option<Uuid>,
    actor_label: &str,
    identity_id: Option<Uuid>,
    request_ctx: &RequestCtx,
) -> Result<(), AuthError> {
    // request_id (and any future event-specific context) lives in metadata.
    // ip + user_agent are top-level columns per the real schema.
    let metadata = json!({
        "request_id": request_ctx.request_id,
    });

    // sqlx 0.8 binds `Option<IpAddr>` to `inet` only when the `ipnetwork`
    // feature is enabled (which we set in Cargo.toml). The bind is direct.
    let ip: Option<IpAddr> = request_ctx.ip;
    let resource_id_text: Option<String> = identity_id.map(|id| id.to_string());

    sqlx::query(
        "INSERT INTO audit_events ( \
             group_id, actor_user_id, actor_label, action, \
             resource_type, resource_id, ip, user_agent, metadata \
         ) VALUES ( \
             NULL, $1, $2, $3, \
             'user_identities', $4, $5, $6, $7 \
         )",
    )
    .bind(actor_user_id)
    .bind(actor_label)
    .bind(action.as_str())
    .bind(resource_id_text)
    .bind(ip)
    .bind(request_ctx.user_agent.as_deref())
    .bind(metadata)
    .execute(&mut **tx)
    .await
    .map_err(AuthError::Storage)?;

    Ok(())
}
