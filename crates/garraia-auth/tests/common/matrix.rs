//! `RLS_MATRIX` — the table-driven dataset for the GAR-392 pure-RLS suite.
//!
//! Lives in `common/` (not in `rls_matrix.rs`) because integration test
//! files in `tests/` are separate compilation units and cannot import
//! `pub const`s from each other. Both `rls_matrix.rs` (the runner) and
//! `meta_tripwires.rs` (Task 9) `mod common; use common::matrix::RLS_MATRIX;`
//! to see the same data.
//!
//! Plan 0013 path C — Task 8/9.
//! Design: docs/superpowers/specs/2026-04-14-gar-391d-392-authz-suite-design.md

use super::cases::{DbRole, RlsCase, RlsExpected, SqlOp, TenantCtx};

// Cases are grouped by `(db_role, table)`. Within each group, the
// `TenantCtx` axis explores the four GUC combinations that are
// semantically relevant for `garraia_app` (Correct / WrongGroup /
// BothUnset / CorrectRoleWrongTenant). For `garraia_login` and
// `garraia_signup`, the GUCs are irrelevant (both roles are BYPASSRLS
// and the oracle only measures GRANT-layer outcomes), so those blocks
// use `TenantCtx::BothUnset` as a fixed marker.
//
// See design doc §4.3 for the "tenant_ctx relevance per role" rule
// that drives this shape.

pub const RLS_MATRIX: &[RlsCase] = &[
    // ═══════════════════════════════════════════════════════════════════════
    // Block 1 — garraia_app × 10 FORCE RLS tables × 4 TenantCtx variants.
    // ═══════════════════════════════════════════════════════════════════════

    // ─── messages ───────────────────────────────────────────────────────
    RlsCase { case_id: "rls_app_messages_select_correct", db_role: DbRole::App, table: "messages", op: SqlOp::Select, tenant_ctx: TenantCtx::Correct, expected: RlsExpected::RowsVisible(1) },
    RlsCase { case_id: "rls_app_messages_select_wrong_group", db_role: DbRole::App, table: "messages", op: SqlOp::Select, tenant_ctx: TenantCtx::WrongGroupCorrectUser, expected: RlsExpected::RlsFilteredZero },
    RlsCase { case_id: "rls_app_messages_select_both_unset", db_role: DbRole::App, table: "messages", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::RlsFilteredZero },
    RlsCase { case_id: "rls_app_messages_select_wrong_tenant", db_role: DbRole::App, table: "messages", op: SqlOp::Select, tenant_ctx: TenantCtx::CorrectRoleWrongTenant, expected: RlsExpected::RlsFilteredZero },

    // ─── chats ──────────────────────────────────────────────────────────
    RlsCase { case_id: "rls_app_chats_select_correct", db_role: DbRole::App, table: "chats", op: SqlOp::Select, tenant_ctx: TenantCtx::Correct, expected: RlsExpected::RowsVisible(1) },
    RlsCase { case_id: "rls_app_chats_select_wrong_group", db_role: DbRole::App, table: "chats", op: SqlOp::Select, tenant_ctx: TenantCtx::WrongGroupCorrectUser, expected: RlsExpected::RlsFilteredZero },
    RlsCase { case_id: "rls_app_chats_select_both_unset", db_role: DbRole::App, table: "chats", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::RlsFilteredZero },
    RlsCase { case_id: "rls_app_chats_select_wrong_tenant", db_role: DbRole::App, table: "chats", op: SqlOp::Select, tenant_ctx: TenantCtx::CorrectRoleWrongTenant, expected: RlsExpected::RlsFilteredZero },

    // ─── chat_members ───────────────────────────────────────────────────
    RlsCase { case_id: "rls_app_chat_members_select_correct", db_role: DbRole::App, table: "chat_members", op: SqlOp::Select, tenant_ctx: TenantCtx::Correct, expected: RlsExpected::RowsVisible(1) },
    RlsCase { case_id: "rls_app_chat_members_select_wrong_group", db_role: DbRole::App, table: "chat_members", op: SqlOp::Select, tenant_ctx: TenantCtx::WrongGroupCorrectUser, expected: RlsExpected::RlsFilteredZero },
    RlsCase { case_id: "rls_app_chat_members_select_both_unset", db_role: DbRole::App, table: "chat_members", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::RlsFilteredZero },
    RlsCase { case_id: "rls_app_chat_members_select_wrong_tenant", db_role: DbRole::App, table: "chat_members", op: SqlOp::Select, tenant_ctx: TenantCtx::CorrectRoleWrongTenant, expected: RlsExpected::RlsFilteredZero },

    // ─── message_threads ────────────────────────────────────────────────
    RlsCase { case_id: "rls_app_message_threads_select_correct", db_role: DbRole::App, table: "message_threads", op: SqlOp::Select, tenant_ctx: TenantCtx::Correct, expected: RlsExpected::RlsFilteredZero },
    RlsCase { case_id: "rls_app_message_threads_select_wrong_group", db_role: DbRole::App, table: "message_threads", op: SqlOp::Select, tenant_ctx: TenantCtx::WrongGroupCorrectUser, expected: RlsExpected::RlsFilteredZero },
    RlsCase { case_id: "rls_app_message_threads_select_both_unset", db_role: DbRole::App, table: "message_threads", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::RlsFilteredZero },
    RlsCase { case_id: "rls_app_message_threads_select_wrong_tenant", db_role: DbRole::App, table: "message_threads", op: SqlOp::Select, tenant_ctx: TenantCtx::CorrectRoleWrongTenant, expected: RlsExpected::RlsFilteredZero },

    // ─── memory_items ───────────────────────────────────────────────────
    RlsCase { case_id: "rls_app_memory_items_select_correct", db_role: DbRole::App, table: "memory_items", op: SqlOp::Select, tenant_ctx: TenantCtx::Correct, expected: RlsExpected::RowsVisible(1) },
    RlsCase { case_id: "rls_app_memory_items_select_wrong_group", db_role: DbRole::App, table: "memory_items", op: SqlOp::Select, tenant_ctx: TenantCtx::WrongGroupCorrectUser, expected: RlsExpected::RlsFilteredZero },
    RlsCase { case_id: "rls_app_memory_items_select_both_unset", db_role: DbRole::App, table: "memory_items", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::RlsFilteredZero },
    RlsCase { case_id: "rls_app_memory_items_select_wrong_tenant", db_role: DbRole::App, table: "memory_items", op: SqlOp::Select, tenant_ctx: TenantCtx::CorrectRoleWrongTenant, expected: RlsExpected::RlsFilteredZero },

    // ─── memory_embeddings ──────────────────────────────────────────────
    RlsCase { case_id: "rls_app_memory_embeddings_select_correct", db_role: DbRole::App, table: "memory_embeddings", op: SqlOp::Select, tenant_ctx: TenantCtx::Correct, expected: RlsExpected::RlsFilteredZero },
    RlsCase { case_id: "rls_app_memory_embeddings_select_wrong_group", db_role: DbRole::App, table: "memory_embeddings", op: SqlOp::Select, tenant_ctx: TenantCtx::WrongGroupCorrectUser, expected: RlsExpected::RlsFilteredZero },
    RlsCase { case_id: "rls_app_memory_embeddings_select_both_unset", db_role: DbRole::App, table: "memory_embeddings", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::RlsFilteredZero },
    RlsCase { case_id: "rls_app_memory_embeddings_select_wrong_tenant", db_role: DbRole::App, table: "memory_embeddings", op: SqlOp::Select, tenant_ctx: TenantCtx::CorrectRoleWrongTenant, expected: RlsExpected::RlsFilteredZero },

    // ─── audit_events ───────────────────────────────────────────────────
    RlsCase { case_id: "rls_app_audit_events_select_correct", db_role: DbRole::App, table: "audit_events", op: SqlOp::Select, tenant_ctx: TenantCtx::Correct, expected: RlsExpected::RowsVisible(1) },
    RlsCase { case_id: "rls_app_audit_events_select_wrong_group", db_role: DbRole::App, table: "audit_events", op: SqlOp::Select, tenant_ctx: TenantCtx::WrongGroupCorrectUser, expected: RlsExpected::RlsFilteredZero },
    RlsCase { case_id: "rls_app_audit_events_select_both_unset", db_role: DbRole::App, table: "audit_events", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::RlsFilteredZero },
    RlsCase { case_id: "rls_app_audit_events_select_wrong_tenant", db_role: DbRole::App, table: "audit_events", op: SqlOp::Select, tenant_ctx: TenantCtx::CorrectRoleWrongTenant, expected: RlsExpected::RlsFilteredZero },

    // ─── sessions ───────────────────────────────────────────────────────
    RlsCase { case_id: "rls_app_sessions_select_correct", db_role: DbRole::App, table: "sessions", op: SqlOp::Select, tenant_ctx: TenantCtx::Correct, expected: RlsExpected::RowsVisible(1) },
    RlsCase { case_id: "rls_app_sessions_select_wrong_group", db_role: DbRole::App, table: "sessions", op: SqlOp::Select, tenant_ctx: TenantCtx::WrongGroupCorrectUser, expected: RlsExpected::RowsVisible(1) },
    RlsCase { case_id: "rls_app_sessions_select_both_unset", db_role: DbRole::App, table: "sessions", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::RlsFilteredZero },
    RlsCase { case_id: "rls_app_sessions_select_wrong_tenant", db_role: DbRole::App, table: "sessions", op: SqlOp::Select, tenant_ctx: TenantCtx::CorrectRoleWrongTenant, expected: RlsExpected::RlsFilteredZero },

    // ─── api_keys ───────────────────────────────────────────────────────
    RlsCase { case_id: "rls_app_api_keys_select_correct", db_role: DbRole::App, table: "api_keys", op: SqlOp::Select, tenant_ctx: TenantCtx::Correct, expected: RlsExpected::RlsFilteredZero },
    RlsCase { case_id: "rls_app_api_keys_select_wrong_group", db_role: DbRole::App, table: "api_keys", op: SqlOp::Select, tenant_ctx: TenantCtx::WrongGroupCorrectUser, expected: RlsExpected::RlsFilteredZero },
    RlsCase { case_id: "rls_app_api_keys_select_both_unset", db_role: DbRole::App, table: "api_keys", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::RlsFilteredZero },
    RlsCase { case_id: "rls_app_api_keys_select_wrong_tenant", db_role: DbRole::App, table: "api_keys", op: SqlOp::Select, tenant_ctx: TenantCtx::CorrectRoleWrongTenant, expected: RlsExpected::RlsFilteredZero },

    // ─── user_identities ────────────────────────────────────────────────
    RlsCase { case_id: "rls_app_user_identities_select_correct", db_role: DbRole::App, table: "user_identities", op: SqlOp::Select, tenant_ctx: TenantCtx::Correct, expected: RlsExpected::RowsVisible(1) },
    RlsCase { case_id: "rls_app_user_identities_select_wrong_group", db_role: DbRole::App, table: "user_identities", op: SqlOp::Select, tenant_ctx: TenantCtx::WrongGroupCorrectUser, expected: RlsExpected::RowsVisible(1) },
    RlsCase { case_id: "rls_app_user_identities_select_both_unset", db_role: DbRole::App, table: "user_identities", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::RlsFilteredZero },
    RlsCase { case_id: "rls_app_user_identities_select_wrong_tenant", db_role: DbRole::App, table: "user_identities", op: SqlOp::Select, tenant_ctx: TenantCtx::CorrectRoleWrongTenant, expected: RlsExpected::RlsFilteredZero },

    // ═══════════════════════════════════════════════════════════════════════
    // Block 2 — garraia_app INSERT with WITH CHECK rejection scenarios.
    // ═══════════════════════════════════════════════════════════════════════

    RlsCase { case_id: "rls_app_chats_insert_correct", db_role: DbRole::App, table: "chats", op: SqlOp::Insert, tenant_ctx: TenantCtx::Correct, expected: RlsExpected::RowsVisible(1) },
    RlsCase { case_id: "rls_app_chats_insert_wrong_group", db_role: DbRole::App, table: "chats", op: SqlOp::Insert, tenant_ctx: TenantCtx::WrongGroupCorrectUser, expected: RlsExpected::PermissionDenied },
    RlsCase { case_id: "rls_app_chats_insert_both_unset", db_role: DbRole::App, table: "chats", op: SqlOp::Insert, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::PermissionDenied },

    RlsCase { case_id: "rls_app_memory_items_insert_correct", db_role: DbRole::App, table: "memory_items", op: SqlOp::Insert, tenant_ctx: TenantCtx::Correct, expected: RlsExpected::RowsVisible(1) },
    RlsCase { case_id: "rls_app_memory_items_insert_wrong_group", db_role: DbRole::App, table: "memory_items", op: SqlOp::Insert, tenant_ctx: TenantCtx::WrongGroupCorrectUser, expected: RlsExpected::PermissionDenied },

    RlsCase { case_id: "rls_app_audit_events_insert_correct", db_role: DbRole::App, table: "audit_events", op: SqlOp::Insert, tenant_ctx: TenantCtx::Correct, expected: RlsExpected::RowsVisible(1) },
    RlsCase { case_id: "rls_app_audit_events_insert_wrong_group", db_role: DbRole::App, table: "audit_events", op: SqlOp::Insert, tenant_ctx: TenantCtx::WrongGroupCorrectUser, expected: RlsExpected::PermissionDenied },

    // ═══════════════════════════════════════════════════════════════════════
    // Block 3 — garraia_login (BYPASSRLS) — grant layer only.
    // ═══════════════════════════════════════════════════════════════════════

    RlsCase { case_id: "rls_login_users_select_allow", db_role: DbRole::Login, table: "users", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::RowsVisible(1) },
    RlsCase { case_id: "rls_login_user_identities_select_allow", db_role: DbRole::Login, table: "user_identities", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::RowsVisible(1) },
    RlsCase { case_id: "rls_login_sessions_select_allow", db_role: DbRole::Login, table: "sessions", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::RowsVisible(1) },
    RlsCase { case_id: "rls_login_group_members_select_allow", db_role: DbRole::Login, table: "group_members", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::RowsVisible(1) },

    RlsCase { case_id: "rls_login_audit_events_select_denied", db_role: DbRole::Login, table: "audit_events", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },
    RlsCase { case_id: "rls_login_chats_select_denied", db_role: DbRole::Login, table: "chats", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },
    RlsCase { case_id: "rls_login_messages_select_denied", db_role: DbRole::Login, table: "messages", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },
    RlsCase { case_id: "rls_login_memory_items_select_denied", db_role: DbRole::Login, table: "memory_items", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },
    RlsCase { case_id: "rls_login_memory_embeddings_select_denied", db_role: DbRole::Login, table: "memory_embeddings", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },
    RlsCase { case_id: "rls_login_chat_members_select_denied", db_role: DbRole::Login, table: "chat_members", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },
    RlsCase { case_id: "rls_login_message_threads_select_denied", db_role: DbRole::Login, table: "message_threads", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },
    RlsCase { case_id: "rls_login_api_keys_select_denied", db_role: DbRole::Login, table: "api_keys", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },
    RlsCase { case_id: "rls_login_tasks_select_denied", db_role: DbRole::Login, table: "tasks", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },
    RlsCase { case_id: "rls_login_task_lists_select_denied", db_role: DbRole::Login, table: "task_lists", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },
    RlsCase { case_id: "rls_login_groups_select_denied", db_role: DbRole::Login, table: "groups", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },

    RlsCase { case_id: "rls_login_audit_events_insert_allow", db_role: DbRole::Login, table: "audit_events", op: SqlOp::Insert, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::RowsVisible(1) },
    RlsCase { case_id: "rls_login_chats_insert_denied", db_role: DbRole::Login, table: "chats", op: SqlOp::Insert, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },
    RlsCase { case_id: "rls_login_memory_items_insert_denied", db_role: DbRole::Login, table: "memory_items", op: SqlOp::Insert, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },

    // ═══════════════════════════════════════════════════════════════════════
    // Block 4 — garraia_signup (BYPASSRLS) — grant layer only.
    // ═══════════════════════════════════════════════════════════════════════

    RlsCase { case_id: "rls_signup_users_select_allow", db_role: DbRole::Signup, table: "users", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::RowsVisible(1) },
    RlsCase { case_id: "rls_signup_user_identities_select_allow", db_role: DbRole::Signup, table: "user_identities", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::RowsVisible(1) },

    RlsCase { case_id: "rls_signup_audit_events_select_denied", db_role: DbRole::Signup, table: "audit_events", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },
    RlsCase { case_id: "rls_signup_sessions_select_denied", db_role: DbRole::Signup, table: "sessions", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },
    RlsCase { case_id: "rls_signup_group_members_select_denied", db_role: DbRole::Signup, table: "group_members", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },
    RlsCase { case_id: "rls_signup_chats_select_denied", db_role: DbRole::Signup, table: "chats", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },
    RlsCase { case_id: "rls_signup_messages_select_denied", db_role: DbRole::Signup, table: "messages", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },
    RlsCase { case_id: "rls_signup_memory_items_select_denied", db_role: DbRole::Signup, table: "memory_items", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },
    RlsCase { case_id: "rls_signup_memory_embeddings_select_denied", db_role: DbRole::Signup, table: "memory_embeddings", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },
    RlsCase { case_id: "rls_signup_chat_members_select_denied", db_role: DbRole::Signup, table: "chat_members", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },
    RlsCase { case_id: "rls_signup_api_keys_select_denied", db_role: DbRole::Signup, table: "api_keys", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },
    RlsCase { case_id: "rls_signup_tasks_select_denied", db_role: DbRole::Signup, table: "tasks", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },
    RlsCase { case_id: "rls_signup_groups_select_denied", db_role: DbRole::Signup, table: "groups", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },

    RlsCase { case_id: "rls_signup_audit_events_insert_allow", db_role: DbRole::Signup, table: "audit_events", op: SqlOp::Insert, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::RowsVisible(1) },
    RlsCase { case_id: "rls_signup_chats_insert_denied", db_role: DbRole::Signup, table: "chats", op: SqlOp::Insert, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },
    RlsCase { case_id: "rls_signup_memory_items_insert_denied", db_role: DbRole::Signup, table: "memory_items", op: SqlOp::Insert, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },
];
