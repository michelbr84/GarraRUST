//! `/v1/me` — authenticated caller identity + self-service profile update.
//!
//! `GET /v1/me` returns identity info from the `Principal` extractor only —
//! no SQL. `PATCH /v1/me` updates `display_name` on the `users` table via
//! the `garraia_app` AppPool. `users` is NOT FORCE-RLS group-scoped, so no
//! `SET LOCAL` context is needed; isolation is via `WHERE id = $1` with the
//! JWT-authenticated `principal.user_id`.
//!
//! `GET /v1/me/mentions` — cursor-paginated inbox of @mentions received by
//! the caller in a given group (plan 0237 / GAR-755).
//!
//! `GET /v1/me/tasks` — cursor-paginated inbox of tasks assigned to the caller
//! in a given group (plan 0242 / GAR-763).
//!
//! `GET /v1/me/chats` — cursor-paginated inbox of chats where the caller holds
//! a `chat_members` row in a given group (plan 0245 / GAR-765).
//!
//! `GET /v1/me/files` — cursor-paginated inbox of files uploaded by the caller
//! in a given group (plan 0246 / GAR-767).
//!
//! `GET /v1/me/invites` — cursor-paginated inbox of pending group invites
//! addressed to the caller's email address (plan 0255 / GAR-777).
//!
//! `POST /v1/me/invites/{invite_id}/decline` — invitee-side explicit decline of a
//! pending group invite (plan 0258 / GAR-783).
//!
//! `GET /v1/me/reactions` — cursor-paginated inbox of messages on which the caller
//! placed emoji reactions, grouped by message (plan 0260 / GAR-788).
//!
//! `GET /v1/me/threads` — cursor-paginated inbox of threads the caller created or
//! has replied to in a given group (plan 0261 / GAR-790).
//!
//! `GET /v1/me/doc-page-mentions` — cursor-paginated inbox of doc page @mentions
//! received by the caller in a given group (plan 0318 / GAR-858).
//!
//! `GET /v1/me/doc-pages` — cursor-paginated inbox of doc pages authored by
//! the caller in a given group (plan 0322 / GAR-860).
//!
//! `GET /v1/me/sessions` — cursor-paginated list of the caller's active sessions
//! (plan 0326 / GAR-866). No `X-Group-Id` required — sessions are user-scoped.
//!
//! `DELETE /v1/me/sessions/{session_id}` — revoke a specific session
//! (plan 0326 / GAR-866). Idempotent: already-revoked returns 204.
//!
//! `POST /v1/me/api-keys` — create a new API key (plan 0331 / GAR-871).
//! Returns the raw key **once only**; stored as Argon2id hash in `api_keys`.
//!
//! `GET /v1/me/api-keys` — cursor-paginated list of the caller's API keys
//! (plan 0331 / GAR-871). Key hash is never returned.
//!
//! `GET /v1/me/api-keys/{key_id}` — fetch single API key metadata.
//!
//! `DELETE /v1/me/api-keys/{key_id}` — revoke an API key (soft-delete,
//! sets `revoked_at`). Idempotent 204 (plan 0331 / GAR-871).
//!
//! `PATCH /v1/me/api-keys/{key_id}` — update label/scopes of an active API key
//! (plan 0334 / GAR-874). 409 if already revoked.
//!
//! `PATCH /v1/me/password` — change own password: verify current, re-hash with
//! Argon2id (plan 0335 / GAR-876). Uses `login_pool` BYPASSRLS for
//! `user_identities` — NEVER `app_pool` (CLAUDE.md Rule 12).
//!
//! `GET /v1/me/audit` — cursor-paginated personal audit trail showing the
//! caller's own user-scoped events (login, logout, password.changed, api_key.*,
//! session.*). No `X-Group-Id` required. Events filtered to
//! `actor_user_id = caller AND group_id = nil-uuid` (plan 0340 / GAR-881).
//!
//! `DELETE /v1/me` — self-service account deletion (plan 0343 / GAR-884).
//! Sets `users.status = 'deleted'`, revokes all active sessions AND files a
//! hard-deletion request in `account_deletion_requests` — all in one
//! transaction. Emits `account.self_deleted` + `account.purge_scheduled`.
//! Returns 204 No Content, 409 Conflict if already deleted or purged. No
//! password re-confirmation — the caller is authenticated via JWT.
//!
//! # Direitos dos titulares (LGPD arts. 18/20, GDPR arts. 15-20)
//!
//! Four endpoints carry the data-subject rights, and the boundary between
//! them is deliberate:
//!
//! | Endpoint | Right | Reaches |
//! |---|---|---|
//! | `GET /v1/me/export` | access + portability | account data **and** the subject's content in every group they belong to |
//! | `PATCH /v1/me` | rectification | `display_name` inline; e-mail as a **recorded request**, never a blind swap |
//! | `POST /v1/me/anonymize` | anonymisation | identifiers only, irreversible, content stays |
//! | `DELETE /v1/me` | erasure | tombstone now, content + identifiers erased by the purge worker after the grace period |
//!
//! Two invariants hold across all four, and every query in this module is
//! written to keep them:
//!
//! 1. **The tenant boundary.** A request by one subject never reads or writes
//!    another group's rows. Enforced twice over: FORCE RLS with a fail-closed
//!    `NULLIF` policy (an unset GUC yields NULL, hence zero rows) *and* an
//!    explicit `WHERE` on the caller's own id in each statement. Neither is
//!    relied upon alone.
//! 2. **Third parties and shared spaces.** One person's request does not
//!    empty an organisation. Messages other members wrote are not exported
//!    and not erased; they stay in the shared space. What leaves is the
//!    subject's own content and their identifiers — see the header of
//!    migration 034 for the table-by-table statement and
//!    `docs/legal/data-subject-requests.md` for the operator procedure and
//!    the known limits.

// Request-path (#1569): panic aqui e controlavel por quem manda a requisicao.
#![deny(clippy::unwrap_used, clippy::expect_used)]

mod anonymize;
mod api_keys;
mod audit;
mod delete;
mod export;
mod inbox;
mod invite_accept;
mod invites;
mod password;
mod profile;
mod sessions;

// Tudo o que era `pub` no antigo `me.rs` continua em `rest_v1::me::*`: o
// router (`rest_v1/mod.rs`) e o `openapi.rs` nao mudaram. Os `__path_*` gerados
// pelo `#[utoipa::path]` sao re-exportados porque o `openapi.rs` lista os
// handlers como `super::me::<handler>`, e no utoipa 6 a tag padrao da operacao
// vem desse caminho (`super::me`). Apontar para o submodulo mudaria a tag e,
// com ela, o spec publicado.
pub use anonymize::{__path_anonymize_me, anonymize_me};
pub use api_keys::{
    __path_create_my_api_key, __path_get_my_api_key, __path_list_my_api_keys,
    __path_patch_my_api_key, __path_revoke_my_api_key, ApiKeySummary, CreateApiKeyRequest,
    CreateApiKeyResponse, ListMyApiKeysQuery, MyApiKeysResponse, PatchMyApiKeyRequest,
    create_my_api_key, get_my_api_key, list_my_api_keys, patch_my_api_key, revoke_my_api_key,
};
pub use audit::{
    __path_list_my_audit, ListMyAuditQuery, MyAuditResponse, PersonalAuditEventSummary,
    list_my_audit,
};
pub use delete::{__path_delete_me, delete_me};
pub use export::types::{
    ExportMeApiKey, ExportMeAuditEvent, ExportMeDeletionRequest, ExportMeEmailChangeRequest,
    ExportMeExclusion, ExportMeFile, ExportMeGroupMembership, ExportMeMemoryItem,
    ExportMeMentionReceived, ExportMeMessage, ExportMeProfile, ExportMeResponse, ExportMeSession,
    ExportMeTask,
};
pub use export::{__path_export_me, export_me};
pub use inbox::chats::{
    __path_list_my_chats, ChatMembershipSummary, ListMyChatsQuery, MyChatsMembershipResponse,
    list_my_chats,
};
pub use inbox::doc_page_mentions::{
    __path_list_my_doc_page_mentions, DocPageMentionInboxSummary, DocPageMentionsInboxResponse,
    ListMyDocPageMentionsQuery, list_my_doc_page_mentions,
};
pub use inbox::doc_pages::{
    __path_list_my_doc_pages, ListMyDocPagesQuery, MyDocPageSummary, MyDocPagesResponse,
    list_my_doc_pages,
};
pub use inbox::files::{
    __path_list_my_files, ListMyFilesQuery, MyFileSummary, MyFilesResponse, list_my_files,
};
pub use inbox::memory::{
    __path_list_my_memory, ListMyMemoryQuery, MyMemoryResponse, MyMemorySummary, list_my_memory,
};
pub use inbox::mentions::{
    __path_list_my_mentions, ListMentionsQuery, MentionSummary, MentionsListResponse,
    list_my_mentions,
};
pub use inbox::reactions::{
    __path_list_my_reactions, ListReactionsQuery, MyReactionSummary, MyReactionsResponse,
    list_my_reactions,
};
pub use inbox::tasks::{
    __path_list_my_tasks, ListTasksQuery, TaskAssignmentSummary, TasksListResponse, list_my_tasks,
};
pub use inbox::threads::{
    __path_list_my_threads, ListMyThreadsQuery, MyThreadSummary, MyThreadsResponse, list_my_threads,
};
pub use invite_accept::{__path_accept_my_invite, AcceptMyInviteResponse, accept_my_invite};
pub use invites::{
    __path_decline_invite, __path_list_my_invites, ListMyInvitesQuery, MyInvitesResponse,
    PendingInviteSummary, decline_invite, list_my_invites,
};
pub use password::{__path_patch_my_password, PatchMyPasswordRequest, patch_my_password};
pub use profile::{
    __path_get_me, __path_patch_me, MeResponse, PatchMeRequest, PatchMeResponse,
    PendingEmailChange, get_me, patch_me,
};
pub use sessions::{
    __path_list_my_sessions, __path_revoke_all_my_sessions, __path_revoke_my_session,
    ListMySessionsQuery, MySessionsResponse, SessionSummary, list_my_sessions,
    revoke_all_my_sessions, revoke_my_session,
};
