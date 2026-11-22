//! `GET /v1/me/export` — LGPD art. 20 / GDPR arts. 15 & 20 personal data export.

// Request-path (#1569): panic aqui e controlavel por quem manda a requisicao.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use axum::extract::State;
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use garraia_auth::{Principal, WorkspaceAuditAction, audit_workspace_event};
use serde_json::json;
use uuid::Uuid;

use crate::rest_v1::RestV1FullState;
use crate::rest_v1::problem::RestError;

mod group_content;
pub mod types;

use self::group_content::GroupContent;
use self::types::{
    EXPORT_MAX_AUDIT_EVENTS, EXPORT_MAX_FILES, EXPORT_MAX_MEMORY, EXPORT_MAX_MENTIONS,
    EXPORT_MAX_MESSAGES, EXPORT_MAX_TASKS, ExportMeApiKey, ExportMeAuditEvent,
    ExportMeDeletionRequest, ExportMeEmailChangeRequest, ExportMeGroupMembership,
    ExportMeMemoryItem, ExportMeProfile, ExportMeResponse, ExportMeSession, ExportMemoryRow,
    cap_section, export_exclusions, map_memory_row,
};

/// `GET /v1/me/export` — LGPD art. 20 / GDPR arts. 15 & 20 personal data export.
///
/// Returns a structured JSON export of the authenticated caller's account-level
/// personal data. The response carries `Content-Disposition: attachment` so
/// browsers download it as a file. No `X-Group-Id` header required.
///
/// ## What is in it
///
/// Account level: `profile`, `sessions`, `api_keys` (metadata only — the hash
/// is never selected), `audit_events` (user-scoped slice, cap 1000),
/// `group_memberships`, `email_change_requests`, `deletion_requests`.
///
/// Personal content, gathered across **every group the subject belongs to**:
/// `messages` they wrote (full body), `mentions_received` (excerpt — the body
/// belongs to its author), `files` they uploaded (metadata), `memory_items`
/// they created (full content, personal and group-scoped), and `tasks` they
/// created or are assigned to.
///
/// ## How the tenant boundary is held
///
/// The content queries run inside ONE transaction that re-points
/// `app.current_group_id` per group via `set_config(..., true)`, iterating
/// only the groups where the subject holds an **active** `group_members` row.
/// Every table touched is under FORCE RLS with a fail-closed policy
/// (`NULLIF` on an unset GUC yields NULL, hence zero rows), so a group the
/// subject does not belong to is never even reachable — and on top of that,
/// each query also filters on the subject's own `user_id`. Two independent
/// mechanisms, neither relied on alone.
///
/// A single transaction also means one consistent snapshot: the export cannot
/// show a message from before a concurrent write and a task from after it.
///
/// ## What is NOT in it
///
/// The response carries its own `exclusions` array — see
/// [`export_exclusions`] for the list and the reasoning. Sections that hit
/// their row cap are named in `truncated_sections` rather than silently
/// trimmed.
///
/// Emits `AccountDataExported` audit event with metadata
/// `{ "sections": [...], "truncated": [...] }`.
/// Email IS returned (right-to-portability requires the data subject's own email).
/// `password_hash` and raw API key are NEVER returned.
#[utoipa::path(
    get,
    path = "/v1/me/export",
    responses(
        (status = 200, description = "Personal data export (JSON attachment).", body = ExportMeResponse),
        (status = 401, description = "Missing or invalid JWT.", body = crate::rest_v1::problem::ProblemDetails),
    ),
    security(("bearer" = [])),
    tag = "me",
)]
pub async fn export_me(
    State(state): State<RestV1FullState>,
    principal: Principal,
) -> Result<axum::response::Response, RestError> {
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

    // Query 1 — profile (users is tenant-root, no FORCE RLS)
    type ProfileRow = (Uuid, String, String, String, DateTime<Utc>);
    let (user_id, display_name, email, status, account_created_at): ProfileRow = sqlx::query_as(
        "SELECT id, display_name, email::text, status, created_at \
             FROM users WHERE id = $1",
    )
    .bind(principal.user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let profile = ExportMeProfile {
        user_id,
        display_name,
        email,
        status,
        account_created_at,
    };

    // Query 2 — sessions (tenant-root, all rows for GDPR portability)
    type SessionRow = (Uuid, Option<String>, DateTime<Utc>, DateTime<Utc>);
    let session_rows: Vec<SessionRow> = sqlx::query_as(
        "SELECT id, device_id, expires_at, created_at \
         FROM sessions WHERE user_id = $1 ORDER BY created_at DESC",
    )
    .bind(principal.user_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let sessions: Vec<ExportMeSession> = session_rows
        .into_iter()
        .map(|(id, device_id, expires_at, created_at)| ExportMeSession {
            id,
            device_id,
            expires_at,
            created_at,
        })
        .collect();

    // Query 3 — api_keys metadata (hash never selected — CLAUDE.md Rule 6)
    type ApiKeyRow = (
        Uuid,
        String,
        serde_json::Value,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
    );
    let api_key_rows: Vec<ApiKeyRow> = sqlx::query_as(
        "SELECT id, label, scopes, created_at, revoked_at \
         FROM api_keys WHERE user_id = $1 ORDER BY created_at DESC",
    )
    .bind(principal.user_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let api_keys: Vec<ExportMeApiKey> = api_key_rows
        .into_iter()
        .map(
            |(id, label, scopes, created_at, revoked_at)| ExportMeApiKey {
                id,
                label,
                scopes,
                created_at,
                revoked_at,
            },
        )
        .collect();

    // Query 4 — audit_events (user-scoped: actor_user_id = $1 AND group_id = nil-uuid), cap 1000
    type AuditEventRow = (
        Uuid,
        String,
        String,
        Option<String>,
        serde_json::Value,
        DateTime<Utc>,
    );
    let audit_event_rows: Vec<AuditEventRow> = sqlx::query_as(
        "SELECT id, action, resource_type, resource_id, metadata, created_at \
         FROM audit_events \
         WHERE actor_user_id = $1 AND group_id = $2 \
         ORDER BY created_at DESC, id DESC LIMIT $3",
    )
    .bind(principal.user_id)
    .bind(nil_uuid)
    // cap + 1: the sentinel row tells "exactly the cap, nothing cut" from
    // "more existed". See the cap constants in `types.rs`.
    .bind(EXPORT_MAX_AUDIT_EVENTS + 1)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let mut audit_events: Vec<ExportMeAuditEvent> = audit_event_rows
        .into_iter()
        .map(
            |(id, action, resource_type, resource_id, metadata, created_at)| ExportMeAuditEvent {
                id,
                action,
                resource_type,
                resource_id,
                metadata,
                created_at,
            },
        )
        .collect();

    // Query 5 — group memberships (active only, JOIN groups for name+type)
    type MembershipRow = (Uuid, String, String, String, DateTime<Utc>);
    let membership_rows: Vec<MembershipRow> = sqlx::query_as(
        "SELECT gm.group_id, g.name, g.type, gm.role, gm.joined_at \
         FROM group_members gm JOIN groups g ON g.id = gm.group_id \
         WHERE gm.user_id = $1 AND gm.status = 'active' \
         ORDER BY gm.joined_at ASC",
    )
    .bind(principal.user_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let group_memberships: Vec<ExportMeGroupMembership> = membership_rows
        .into_iter()
        .map(
            |(group_id, group_name, group_type, role, joined_at)| ExportMeGroupMembership {
                group_id,
                group_name,
                group_type,
                role,
                joined_at,
            },
        )
        .collect();

    // Query 6 — e-mail-correction requests (FORCE RLS owner-only, migration 034)
    type EmailReqRow = (Uuid, String, String, DateTime<Utc>, Option<DateTime<Utc>>);
    let email_req_rows: Vec<EmailReqRow> = sqlx::query_as(
        "SELECT id, requested_email::text, status, requested_at, resolved_at \
         FROM user_email_change_requests WHERE user_id = $1 \
         ORDER BY requested_at DESC",
    )
    .bind(principal.user_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let email_change_requests: Vec<ExportMeEmailChangeRequest> = email_req_rows
        .into_iter()
        .map(
            |(request_id, requested_email, status, requested_at, resolved_at)| {
                ExportMeEmailChangeRequest {
                    request_id,
                    requested_email,
                    status,
                    requested_at,
                    resolved_at,
                }
            },
        )
        .collect();

    // Query 7 — deletion requests (FORCE RLS owner-only, migration 034).
    // The subject is entitled to know the date their erasure becomes
    // irreversible, which is the whole point of the grace period.
    type DeletionReqRow = (
        Uuid,
        String,
        DateTime<Utc>,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
    );
    let deletion_req_rows: Vec<DeletionReqRow> = sqlx::query_as(
        "SELECT id, status, requested_at, purge_after, completed_at \
         FROM account_deletion_requests WHERE user_id = $1 \
         ORDER BY requested_at DESC",
    )
    .bind(principal.user_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    let deletion_requests: Vec<ExportMeDeletionRequest> = deletion_req_rows
        .into_iter()
        .map(
            |(request_id, status, requested_at, purge_after, completed_at)| {
                ExportMeDeletionRequest {
                    request_id,
                    status,
                    requested_at,
                    purge_after,
                    completed_at,
                }
            },
        )
        .collect();

    // Query 8 — personal memory (`scope_type = 'user'`, `group_id IS NULL`).
    // Runs while the GUC still points at the nil uuid: branch 2 of the dual
    // policy `memory_items_group_or_self` (migration 007) matches on
    // `group_id IS NULL AND created_by = app.current_user_id`, so the group
    // GUC is irrelevant here and must NOT be a real group.
    let mut memory_items: Vec<ExportMeMemoryItem> = Vec::new();
    let personal_memory: Vec<ExportMemoryRow> = sqlx::query_as(
        "SELECT id, scope_type, group_id, kind, sensitivity, content, \
                ttl_expires_at, created_at, deleted_at \
         FROM memory_items \
         WHERE created_by = $1 AND group_id IS NULL \
         ORDER BY created_at DESC, id DESC \
         LIMIT $2",
    )
    .bind(principal.user_id)
    // cap + 1: sentinel row, see the cap constants in `types.rs`.
    .bind(EXPORT_MAX_MEMORY + 1)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    memory_items.extend(personal_memory.into_iter().map(map_memory_row));

    // Content, group by group — see `group_content`. It returns with
    // `app.current_group_id` back on the nil uuid: the audit INSERT below is
    // a user-scoped event and its RLS branch matches
    // `group_id = app.current_group_id`.
    let GroupContent {
        mut messages,
        mut mentions_received,
        mut files,
        mut memory_items,
        mut tasks,
    } = group_content::collect_group_content(
        &mut tx,
        principal.user_id,
        &group_memberships,
        memory_items,
    )
    .await?;

    // A section that reached its cap is declared, never silently trimmed.
    // `> cap` (not `>=`): each query fetched one sentinel row past the cap,
    // so exactly `cap` rows means nothing was cut and the flag must stay
    // off. The sentinel is dropped here, before serialization.
    let mut truncated_sections: Vec<String> = Vec::new();
    cap_section(
        &mut messages,
        EXPORT_MAX_MESSAGES,
        "messages",
        &mut truncated_sections,
    );
    cap_section(
        &mut mentions_received,
        EXPORT_MAX_MENTIONS,
        "mentions_received",
        &mut truncated_sections,
    );
    cap_section(
        &mut files,
        EXPORT_MAX_FILES,
        "files",
        &mut truncated_sections,
    );
    cap_section(
        &mut memory_items,
        EXPORT_MAX_MEMORY,
        "memory_items",
        &mut truncated_sections,
    );
    cap_section(
        &mut tasks,
        EXPORT_MAX_TASKS,
        "tasks",
        &mut truncated_sections,
    );
    cap_section(
        &mut audit_events,
        EXPORT_MAX_AUDIT_EVENTS,
        "audit_events",
        &mut truncated_sections,
    );

    // Emit AccountDataExported audit event. No PII in metadata.
    let sections = [
        "profile",
        "sessions",
        "api_keys",
        "audit_events",
        "group_memberships",
        "messages",
        "mentions_received",
        "files",
        "memory_items",
        "tasks",
        "email_change_requests",
        "deletion_requests",
    ];
    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::AccountDataExported,
        principal.user_id,
        nil_uuid,
        "users",
        principal.user_id.to_string(),
        json!({ "sections": sections, "truncated": truncated_sections }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!("{e}")))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let exported_at = Utc::now();
    let response_body = ExportMeResponse {
        exported_at,
        schema_version: "2".to_string(),
        profile,
        sessions,
        api_keys,
        audit_events,
        group_memberships,
        messages,
        mentions_received,
        files,
        memory_items,
        tasks,
        email_change_requests,
        deletion_requests,
        truncated_sections,
        exclusions: export_exclusions(),
    };

    let body_bytes =
        serde_json::to_vec(&response_body).map_err(|e| RestError::Internal(e.into()))?;
    let date = exported_at.format("%Y-%m-%d");
    let filename = format!("garraia-export-{date}.json");

    axum::response::Response::builder()
        .status(StatusCode::OK)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .header(
            axum::http::header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )
        .body(axum::body::Body::from(body_bytes))
        .map_err(|e| RestError::Internal(anyhow::anyhow!("{e}")))
}
