//! Response types, section caps and helpers of `GET /v1/me/export`.

// Request-path (#1569): panic aqui e controlavel por quem manda a requisicao.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

// ─── GET /v1/me/export (plan 0344 / GAR-885) ──────────────────────────────────

/// Profile section of the personal data export.
#[derive(Debug, Serialize, ToSchema)]
pub struct ExportMeProfile {
    pub user_id: Uuid,
    pub display_name: String,
    pub email: String,
    pub status: String,
    pub account_created_at: DateTime<Utc>,
}

/// One session entry in the personal data export.
#[derive(Debug, Serialize, ToSchema)]
pub struct ExportMeSession {
    pub id: Uuid,
    pub device_id: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// One API key entry in the personal data export (key hash never returned).
#[derive(Debug, Serialize, ToSchema)]
pub struct ExportMeApiKey {
    pub id: Uuid,
    pub label: String,
    pub scopes: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// One audit event entry in the personal data export.
#[derive(Debug, Serialize, ToSchema)]
pub struct ExportMeAuditEvent {
    pub id: Uuid,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

/// One group membership entry in the personal data export.
#[derive(Debug, Serialize, ToSchema)]
pub struct ExportMeGroupMembership {
    pub group_id: Uuid,
    pub group_name: String,
    pub group_type: String,
    pub role: String,
    pub joined_at: DateTime<Utc>,
}

// ─── Content sections (direitos dos titulares) ───────────────────────────────
//
// Row caps per section. An export is a synchronous request against a live
// tenant database; an uncapped `SELECT` over a heavy account would hold a
// connection and build a multi-hundred-megabyte JSON body in memory. The cap
// is declared to the subject through `truncated_sections` instead of silently
// trimming — an incomplete export that says so is honest, one that does not
// is a false compliance claim.
//
// Each query fetches `cap + 1` rows: the extra row is a sentinel that tells
// "exactly cap rows, nothing was cut" from "more rows existed and were cut".
// With `LIMIT cap` alone, an account with exactly 5 000 messages would be
// declared truncated when nothing was dropped — a false compliance claim in
// the other direction. `cap_section` drops the sentinel and records the flag.
pub(super) const EXPORT_MAX_MESSAGES: i64 = 5_000;
pub(super) const EXPORT_MAX_MENTIONS: i64 = 5_000;
pub(super) const EXPORT_MAX_FILES: i64 = 5_000;
pub(super) const EXPORT_MAX_MEMORY: i64 = 5_000;
pub(super) const EXPORT_MAX_TASKS: i64 = 5_000;
/// Cap on the user-scoped audit slice. Was an inline `LIMIT 1000`; named so
/// `truncated_sections` can report it like every other section.
pub(super) const EXPORT_MAX_AUDIT_EVENTS: i64 = 1_000;
/// Excerpt width for a message the subject did NOT write — identical to what
/// `GET /v1/me/mentions` already returns, so the export widens no exposure.
pub(super) const EXPORT_MENTION_EXCERPT_CHARS: i64 = 200;

/// Cuts a fetched section down to its cap and records the truncation.
///
/// The section was fetched with `cap + 1` rows (the sentinel), so `len > cap`
/// is the only honest truncation signal: exactly `cap` rows means nothing was
/// dropped and the flag stays off. Called from `export_me` before the audit
/// event and the response body, so the subject never receives the sentinel.
pub(super) fn cap_section<T>(
    rows: &mut Vec<T>,
    cap: i64,
    name: &str,
    truncated_sections: &mut Vec<String>,
) {
    if rows.len() as i64 > cap {
        rows.truncate(cap as usize);
        truncated_sections.push(name.to_string());
    }
}

/// One message **written by the subject**, with its full body.
#[derive(Debug, Serialize, ToSchema)]
pub struct ExportMeMessage {
    pub message_id: Uuid,
    pub chat_id: Uuid,
    pub group_id: Uuid,
    /// Chat name, so the export is readable without a second lookup.
    pub chat_name: String,
    /// Full body — this is the subject's own authored content.
    pub body: String,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edited_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<DateTime<Utc>>,
}

/// One @mention **received** by the subject — data *about* them, written by
/// somebody else.
///
/// Carries a [`EXPORT_MENTION_EXCERPT_CHARS`]-character excerpt, not the full
/// body: the body belongs to its author. See the `exclusions` section of the
/// export for the reasoning.
#[derive(Debug, Serialize, ToSchema)]
pub struct ExportMeMentionReceived {
    pub message_id: Uuid,
    pub chat_id: Uuid,
    pub group_id: Uuid,
    pub sender_user_id: Uuid,
    pub sender_label: String,
    pub body_excerpt: String,
    pub created_at: DateTime<Utc>,
}

/// One file uploaded by the subject. Metadata only — see `exclusions`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ExportMeFile {
    pub file_id: Uuid,
    pub group_id: Uuid,
    pub name: String,
    pub mime_type: String,
    pub size_bytes: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<DateTime<Utc>>,
}

/// One memory item the subject created, with its full content.
#[derive(Debug, Serialize, ToSchema)]
pub struct ExportMeMemoryItem {
    pub memory_item_id: Uuid,
    /// `user` (personal), `group` or `chat`.
    pub scope_type: String,
    /// `None` for personal memory, which has `group_id IS NULL`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<Uuid>,
    pub kind: String,
    pub sensitivity: String,
    /// Full content. Unlike `GET /v1/me/memory`, which returns a preview,
    /// portability entitles the subject to the whole record they authored.
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<DateTime<Utc>>,
}

/// One task the subject created and/or is assigned to.
#[derive(Debug, Serialize, ToSchema)]
pub struct ExportMeTask {
    pub task_id: Uuid,
    pub list_id: Uuid,
    pub group_id: Uuid,
    pub title: String,
    pub status: String,
    pub priority: String,
    /// `true` when the subject created this task.
    pub authored: bool,
    /// `true` when the subject is currently an assignee.
    pub assigned: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<DateTime<Utc>>,
}

/// One e-mail-correction request filed by the subject.
#[derive(Debug, Serialize, ToSchema)]
pub struct ExportMeEmailChangeRequest {
    pub request_id: Uuid,
    pub requested_email: String,
    pub status: String,
    pub requested_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<DateTime<Utc>>,
}

/// One account-deletion request filed by the subject.
#[derive(Debug, Serialize, ToSchema)]
pub struct ExportMeDeletionRequest {
    pub request_id: Uuid,
    pub status: String,
    pub requested_at: DateTime<Utc>,
    /// When the erasure becomes irreversible. Before this moment the subject
    /// can still ask support to cancel.
    pub purge_after: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
}

/// One machine-readable statement of what this export deliberately omits.
///
/// GDPR art. 15(1) makes the *scope* of an access response part of the
/// response. A reader who cannot tell "absent because none exists" from
/// "absent because we chose not to include it" cannot act on the export, so
/// the boundary ships inside the file rather than only in a code comment.
#[derive(Debug, Serialize, ToSchema)]
pub struct ExportMeExclusion {
    /// What is not here.
    pub scope: &'static str,
    /// Why not.
    pub reason: &'static str,
}

/// Row shape shared by the personal and the group-scoped memory queries in
/// [`export_me`](super::export_me), mapped in one place so the two cannot diverge.
pub(super) type ExportMemoryRow = (
    Uuid,
    String,
    Option<Uuid>,
    String,
    String,
    String,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
);

pub(super) fn map_memory_row(row: ExportMemoryRow) -> ExportMeMemoryItem {
    let (
        memory_item_id,
        scope_type,
        group_id,
        kind,
        sensitivity,
        content,
        ttl_expires_at,
        created_at,
        deleted_at,
    ) = row;
    ExportMeMemoryItem {
        memory_item_id,
        scope_type,
        group_id,
        kind,
        sensitivity,
        content,
        ttl_expires_at,
        created_at,
        deleted_at,
    }
}

/// The exclusion set, stated once so the export, the OpenAPI schema and
/// `docs/legal/data-subject-requests.md` cannot drift apart.
pub(super) fn export_exclusions() -> Vec<ExportMeExclusion> {
    vec![
        ExportMeExclusion {
            scope: "messages written by other people",
            reason: "A message another member wrote is that member's personal data, even in a \
                     chat the subject belongs to. Exporting a shared conversation wholesale \
                     would hand one person a copy of everyone else's content. Messages that \
                     name the subject appear under `mentions_received` as a 200-character \
                     excerpt — the same width `GET /v1/me/mentions` already returns.",
        },
        ExportMeExclusion {
            scope: "file contents (bytes)",
            reason: "Only file metadata is listed. The bytes stay in object storage and are \
                     fetched one at a time through the existing download endpoint, which keeps \
                     this response a bounded JSON document instead of an unbounded archive.",
        },
        ExportMeExclusion {
            scope: "group-scoped audit events",
            reason: "`audit_events` rows carrying a `group_id` are the organisation's security \
                     log, readable by its admins. Only the subject's user-scoped events are \
                     exported — the same slice `GET /v1/me/audit` shows.",
        },
        ExportMeExclusion {
            scope: "doc pages, blocks and versions",
            reason: "Wiki-style pages are organisational knowledge held jointly, and a page \
                     authored by the subject is routinely edited by others afterwards. Pages \
                     the subject authored are reachable through `GET /v1/me/doc-pages`; a \
                     copy of a jointly-edited document is handled as a manual request.",
        },
        ExportMeExclusion {
            scope: "credential material",
            reason: "Password hashes and raw API keys are never returned by any endpoint. API \
                     key metadata is exported; the secret is shown once at creation and never \
                     again.",
        },
        ExportMeExclusion {
            scope: "data about people without a Garra account",
            reason: "Someone who only talked to a bot on a messaging channel has no account \
                     here and so cannot authenticate to this endpoint. Their request runs \
                     through the manual channel in docs/legal/data-subject-requests.md.",
        },
    ]
}

/// Response body for `GET /v1/me/export`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ExportMeResponse {
    /// Timestamp when the export was generated (UTC).
    pub exported_at: DateTime<Utc>,
    /// Schema version for forward-compatibility. `"2"` since the content
    /// sections (messages, files, memory, tasks) landed; `"1"` was
    /// account-level only.
    pub schema_version: String,
    pub profile: ExportMeProfile,
    pub sessions: Vec<ExportMeSession>,
    pub api_keys: Vec<ExportMeApiKey>,
    pub audit_events: Vec<ExportMeAuditEvent>,
    pub group_memberships: Vec<ExportMeGroupMembership>,
    /// Messages the subject wrote, across every group they belong to.
    pub messages: Vec<ExportMeMessage>,
    /// @mentions the subject received (excerpt only — see `exclusions`).
    pub mentions_received: Vec<ExportMeMentionReceived>,
    /// Files the subject uploaded (metadata only — see `exclusions`).
    pub files: Vec<ExportMeFile>,
    /// Memory items the subject created, personal and group-scoped.
    pub memory_items: Vec<ExportMeMemoryItem>,
    /// Tasks the subject created and/or is assigned to.
    pub tasks: Vec<ExportMeTask>,
    /// E-mail-correction requests the subject filed.
    pub email_change_requests: Vec<ExportMeEmailChangeRequest>,
    /// Account-deletion requests the subject filed, with the date the
    /// erasure becomes irreversible.
    pub deletion_requests: Vec<ExportMeDeletionRequest>,
    /// Sections that hit their row cap — the export is INCOMPLETE for these.
    /// Empty means every section is whole.
    pub truncated_sections: Vec<String>,
    /// What this export deliberately does not contain, and why.
    pub exclusions: Vec<ExportMeExclusion>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use garraia_auth::WorkspaceAuditAction;

    /// Fronteira do teto, lado de la: exatamente `cap` linhas NAO e
    /// truncamento — o fetch pediu `cap + 1` justamente para `len > cap`
    /// ser o unico sinal honesto, e declarar corte onde nada foi cortado
    /// seria mentira simetrica da outra.
    #[test]
    fn cap_section_exactly_at_cap_is_not_truncation() {
        let mut rows: Vec<u8> = vec![0; 4];
        let mut truncated = Vec::new();
        cap_section(&mut rows, 4, "messages", &mut truncated);
        assert_eq!(rows.len(), 4, "rows at the cap must not be cut");
        assert!(
            truncated.is_empty(),
            "exactly-at-cap must not record a truncation"
        );
    }

    /// Fronteira do teto, lado de fora: `cap + 1` (a linha-sentinela) e
    /// truncada e registrada — e a sentinela nunca sobrevive no resultado.
    #[test]
    fn cap_section_one_past_cap_truncates_and_records() {
        let mut rows: Vec<u8> = vec![0; 5];
        let mut truncated = Vec::new();
        cap_section(&mut rows, 4, "messages", &mut truncated);
        assert_eq!(rows.len(), 4, "the sentinel row must be dropped");
        assert_eq!(truncated, vec!["messages".to_string()]);
    }

    // ─── export_me unit tests (plan 0344 / GAR-885) ──────────────────────────

    #[test]
    fn export_me_response_serializes_all_top_level_keys() {
        let now = DateTime::<Utc>::from_timestamp(0, 0).unwrap();
        let resp = ExportMeResponse {
            exported_at: now,
            schema_version: "1".to_string(),
            profile: ExportMeProfile {
                user_id: Uuid::nil(),
                display_name: "Test".to_string(),
                email: "test@example.com".to_string(),
                status: "active".to_string(),
                account_created_at: now,
            },
            sessions: vec![],
            api_keys: vec![],
            audit_events: vec![],
            group_memberships: vec![],
            messages: vec![],
            mentions_received: vec![],
            files: vec![],
            memory_items: vec![],
            tasks: vec![],
            email_change_requests: vec![],
            deletion_requests: vec![],
            truncated_sections: vec![],
            exclusions: vec![],
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert!(v.get("exported_at").is_some());
        assert_eq!(v["schema_version"], "1");
        assert!(v.get("profile").is_some());
        assert!(v["sessions"].as_array().unwrap().is_empty());
        assert!(v["api_keys"].as_array().unwrap().is_empty());
        assert!(v["audit_events"].as_array().unwrap().is_empty());
        assert!(v["group_memberships"].as_array().unwrap().is_empty());
    }

    #[test]
    fn export_me_profile_does_not_contain_password_hash_field() {
        let now = DateTime::<Utc>::from_timestamp(0, 0).unwrap();
        let profile = ExportMeProfile {
            user_id: Uuid::nil(),
            display_name: "Alice".to_string(),
            email: "alice@example.com".to_string(),
            status: "active".to_string(),
            account_created_at: now,
        };
        let v = serde_json::to_value(&profile).unwrap();
        assert!(
            v.get("password_hash").is_none(),
            "password_hash must never appear in export"
        );
        assert_eq!(v["email"], "alice@example.com");
    }

    #[test]
    fn export_me_api_key_revoked_at_is_null_when_absent() {
        let now = DateTime::<Utc>::from_timestamp(0, 0).unwrap();
        let key = ExportMeApiKey {
            id: Uuid::nil(),
            label: "ci-key".to_string(),
            scopes: serde_json::json!(["chats.read"]),
            created_at: now,
            revoked_at: None,
        };
        let v = serde_json::to_value(&key).unwrap();
        assert!(v["revoked_at"].is_null(), "absent revoked_at must be null");
        assert_eq!(v["label"], "ci-key");
    }

    #[test]
    fn export_me_audit_event_resource_id_null_when_absent() {
        let now = DateTime::<Utc>::from_timestamp(0, 0).unwrap();
        let event = ExportMeAuditEvent {
            id: Uuid::nil(),
            action: "account.data_exported".to_string(),
            resource_type: "users".to_string(),
            resource_id: None,
            metadata: serde_json::json!({}),
            created_at: now,
        };
        let v = serde_json::to_value(&event).unwrap();
        assert!(
            v["resource_id"].is_null(),
            "absent resource_id must be null"
        );
        assert_eq!(v["action"], "account.data_exported");
    }

    #[test]
    fn export_me_audit_action_string_is_stable() {
        assert_eq!(
            WorkspaceAuditAction::AccountDataExported.as_str(),
            "account.data_exported",
            "action string must never change — consumers filter by this value"
        );
    }

    #[test]
    fn export_me_group_membership_serializes_all_fields() {
        let now = DateTime::<Utc>::from_timestamp(0, 0).unwrap();
        let membership = ExportMeGroupMembership {
            group_id: Uuid::nil(),
            group_name: "My Team".to_string(),
            group_type: "team".to_string(),
            role: "member".to_string(),
            joined_at: now,
        };
        let v = serde_json::to_value(&membership).unwrap();
        assert_eq!(v["group_id"], "00000000-0000-0000-0000-000000000000");
        assert_eq!(v["group_name"], "My Team");
        assert_eq!(v["group_type"], "team");
        assert_eq!(v["role"], "member");
        assert!(v.get("joined_at").is_some());
    }
}
