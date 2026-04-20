//! Workspace audit events — plan 0021 (GAR-425).
//!
//! Sibling to [`crate::audit`] (the login-flow audit). Split into its
//! own module to keep the login flow's 6-variant enum untouched while
//! adding workspace-scoped actions (invite/member operations).
//!
//! Shares the same underlying `audit_events` table (schema in
//! `migrations/002_rbac_and_audit.sql`). See that migration's column
//! doc for field semantics.
//!
//! ## Column usage for workspace events
//!
//! | Column          | Workspace convention                                             |
//! |-----------------|------------------------------------------------------------------|
//! | `group_id`      | Always `Some(uuid)` — every workspace event is group-scoped.     |
//! | `actor_user_id` | Always `Some(uuid)` — Principal extractor already ran.           |
//! | `actor_label`   | `NULL` in v1 — extractor does not carry email into handlers.     |
//! | `action`        | `invite.accepted` / `member.role_changed` / `member.removed`.    |
//! | `resource_type` | `"group_invites"` for accept; `"group_members"` for setRole/del. |
//! | `resource_id`   | Invite id for accept; `"{group_id}:{user_id}"` for setRole/del.  |
//! | `ip`, `user_agent` | `NULL` in v1 — not plumbed through Principal extractor.      |
//! | `metadata`      | `jsonb` diff — see per-action comment below.                     |
//!
//! ## RLS requirement
//!
//! The `audit_events_group_or_self` policy (migration 007:161-168) requires:
//!
//! ```text
//! (group_id IS NOT NULL AND group_id = current_setting(app.current_group_id))
//! OR
//! (group_id IS NULL AND actor_user_id = current_setting(app.current_user_id))
//! ```
//!
//! All three workspace actions have `group_id IS NOT NULL`, so the **caller
//! must have `SET LOCAL app.current_group_id = '{group_id}'` executed in the
//! same transaction before calling this helper**. Callers set both
//! `app.current_user_id` (already present for tenant-context protocol) and
//! `app.current_group_id` (plan 0021 addition).

use serde_json::Value;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::error::AuthError;

/// Canonical action strings emitted by workspace operations.
///
/// Stored in `audit_events.action`. Stable strings — consumers filter by
/// these values. New variants MUST be added here AND in any downstream
/// consumer that dispatches on action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceAuditAction {
    /// Invite token accepted — a new `group_members` row was created.
    /// Emitted by `POST /v1/invites/{token}/accept` (plan 0019).
    ///
    /// `resource_type = "group_invites"`, `resource_id = invite.id`.
    /// Metadata: `{ invited_email, proposed_role }`.
    InviteAccepted,

    /// A member's role was changed via
    /// `POST /v1/groups/{id}/members/{user_id}/setRole` (plan 0020).
    ///
    /// `resource_type = "group_members"`,
    /// `resource_id = "{group_id}:{user_id}"`.
    /// Metadata: `{ target_user_id, old_role, new_role }`.
    MemberRoleChanged,

    /// A member was soft-deleted via
    /// `DELETE /v1/groups/{id}/members/{user_id}` (plan 0020).
    ///
    /// `resource_type = "group_members"`,
    /// `resource_id = "{group_id}:{user_id}"`.
    /// Metadata: `{ target_user_id, old_role }`.
    MemberRemoved,
}

impl WorkspaceAuditAction {
    /// Canonical string form stored in `audit_events.action`.
    pub fn as_str(self) -> &'static str {
        match self {
            WorkspaceAuditAction::InviteAccepted => "invite.accepted",
            WorkspaceAuditAction::MemberRoleChanged => "member.role_changed",
            WorkspaceAuditAction::MemberRemoved => "member.removed",
        }
    }
}

/// Display delegates to [`WorkspaceAuditAction::as_str`] so
/// `tracing::info!(action = %action, ...)` works ergonomically
/// without wrapping `.as_str()` at every log site.
///
/// Plan 0022 T1 (GAR-426) — addressed code-review NIT from PR #30.
impl std::fmt::Display for WorkspaceAuditAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Insert one `audit_events` row inside the caller's transaction.
///
/// **Caller contract:**
/// 1. Transaction must already be open on a pool with `INSERT` grant on
///    `audit_events` (the `garraia_app` pool via `AppPool` qualifies —
///    grant from migration 007:70).
/// 2. `SET LOCAL app.current_user_id = '{uuid}'` must have been executed
///    in this transaction (standard tenant-context protocol).
/// 3. `SET LOCAL app.current_group_id = '{uuid}'` must have been
///    executed in this transaction — RLS policy
///    `audit_events_group_or_self` uses it to authorize the INSERT.
///    Without it, the INSERT fails with insufficient privilege
///    (RLS rejects the WITH CHECK).
///
/// The function performs one INSERT and returns. All mapping errors are
/// propagated via [`AuthError::Database`]. The `metadata` jsonb is stored
/// verbatim — caller is responsible for keeping it PII-safe.
pub async fn audit_workspace_event(
    tx: &mut Transaction<'_, Postgres>,
    action: WorkspaceAuditAction,
    actor_user_id: Uuid,
    group_id: Uuid,
    resource_type: &'static str,
    resource_id: String,
    metadata: Value,
) -> Result<(), AuthError> {
    sqlx::query(
        "INSERT INTO audit_events \
             (group_id, actor_user_id, action, resource_type, resource_id, metadata) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(group_id)
    .bind(actor_user_id)
    .bind(action.as_str())
    .bind(resource_type)
    .bind(resource_id)
    .bind(metadata)
    .execute(&mut **tx)
    .await
    .map_err(AuthError::Storage)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_audit_action_as_str_stable() {
        // These strings are on the wire — consumers match by value.
        // Changing them is a breaking change.
        assert_eq!(
            WorkspaceAuditAction::InviteAccepted.as_str(),
            "invite.accepted"
        );
        assert_eq!(
            WorkspaceAuditAction::MemberRoleChanged.as_str(),
            "member.role_changed"
        );
        assert_eq!(
            WorkspaceAuditAction::MemberRemoved.as_str(),
            "member.removed"
        );
    }

    #[test]
    fn workspace_audit_action_distinct_strings() {
        // Sanity: no variant accidentally shares a wire string with another.
        let strings = [
            WorkspaceAuditAction::InviteAccepted.as_str(),
            WorkspaceAuditAction::MemberRoleChanged.as_str(),
            WorkspaceAuditAction::MemberRemoved.as_str(),
        ];
        let unique: std::collections::HashSet<_> = strings.iter().collect();
        assert_eq!(unique.len(), strings.len(), "duplicate action strings");
    }

    #[test]
    fn workspace_audit_action_display_delegates_to_as_str() {
        // Plan 0022 T1 — Display impl delegates to as_str(), so
        // `tracing::info!(action = %action, ...)` produces the same
        // on-the-wire string as direct INSERT via audit_workspace_event.
        // Any divergence between Display and as_str would create a
        // silent mismatch between logged events and DB rows.
        use std::fmt::Write;

        let mut buf = String::new();
        write!(&mut buf, "{}", WorkspaceAuditAction::InviteAccepted).unwrap();
        assert_eq!(buf, "invite.accepted");

        assert_eq!(
            format!("{}", WorkspaceAuditAction::MemberRoleChanged),
            "member.role_changed"
        );
        assert_eq!(
            format!("{}", WorkspaceAuditAction::MemberRemoved),
            "member.removed"
        );

        // Concat test — verifies `format!` composition (common tracing
        // scenario with multiple placeholders).
        let combined = format!(
            "{} + {} + {}",
            WorkspaceAuditAction::InviteAccepted,
            WorkspaceAuditAction::MemberRoleChanged,
            WorkspaceAuditAction::MemberRemoved,
        );
        assert_eq!(
            combined,
            "invite.accepted + member.role_changed + member.removed"
        );
    }
}
