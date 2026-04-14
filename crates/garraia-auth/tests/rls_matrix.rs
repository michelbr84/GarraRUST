//! GAR-392 — Pure RLS matrix.
//!
//! Exercises the three dedicated Postgres roles (`garraia_app`,
//! `garraia_login`, `garraia_signup`) against the 10 FORCE RLS tables
//! from migration 007 plus the tenant-root tables they each have grants
//! on. Each case is labeled with a stable `case_id`; failures are
//! collected into a single report at the end of the run instead of
//! aborting on the first mismatch.
//!
//! Plan 0013 path C — Task 8.
//! Design: docs/superpowers/specs/2026-04-14-gar-391d-392-authz-suite-design.md

#![cfg(feature = "test-support")]

mod common;

use common::cases::{DbRole, RlsCase, RlsExpected, SqlOp, TenantCtx};
use common::harness::Harness;
use common::oracle::{classify_count, classify_pg_error};
use common::tenants::Tenant;
use sqlx::PgPool;
use uuid::Uuid;

// ─── Matrix ────────────────────────────────────────────────────────────────
//
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

const RLS_MATRIX: &[RlsCase] = &[
    // ═══════════════════════════════════════════════════════════════════════
    // Block 1 — garraia_app × 10 FORCE RLS tables × 4 TenantCtx variants.
    //
    // For each FORCE RLS table, under Correct GUCs the pre-seeded row in
    // the primary group is visible; under any other GUC setting (wrong
    // group, unset, or wrong tenant) the `USING` clause filters it out
    // silently → RlsFilteredZero (fail-closed).
    //
    // Pre-seeding happens in `seed_primary_group` before the matrix
    // runs. Tables without a natural seed (`api_keys`, `sessions`,
    // `user_identities`) get their own targeted seeds.
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
    // Pre-seed is empty for this table — no natural message_threads row
    // is created. Under any ctx the visible count is 0; but the
    // distinction between "RlsFilteredZero from policy" and
    // "RlsFilteredZero from empty table" is not observable at the
    // oracle layer. We test all 4 ctx to catch a regression that would
    // turn the NULLIF fail-closed into an error.
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
    // No embedding seed; testing only the fail-closed paths across all
    // 4 ctx to catch any regression in policy composition.
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
    // sessions has its own RLS policy (by user_id via GUC), pre-seeded
    // with one session for tenant.member.
    RlsCase { case_id: "rls_app_sessions_select_correct", db_role: DbRole::App, table: "sessions", op: SqlOp::Select, tenant_ctx: TenantCtx::Correct, expected: RlsExpected::RowsVisible(1) },
    RlsCase { case_id: "rls_app_sessions_select_wrong_group", db_role: DbRole::App, table: "sessions", op: SqlOp::Select, tenant_ctx: TenantCtx::WrongGroupCorrectUser, expected: RlsExpected::RowsVisible(1) },
    RlsCase { case_id: "rls_app_sessions_select_both_unset", db_role: DbRole::App, table: "sessions", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::RlsFilteredZero },
    RlsCase { case_id: "rls_app_sessions_select_wrong_tenant", db_role: DbRole::App, table: "sessions", op: SqlOp::Select, tenant_ctx: TenantCtx::CorrectRoleWrongTenant, expected: RlsExpected::RlsFilteredZero },

    // ─── api_keys ───────────────────────────────────────────────────────
    // No pre-seed; exercise the fail-closed path across all ctx.
    RlsCase { case_id: "rls_app_api_keys_select_correct", db_role: DbRole::App, table: "api_keys", op: SqlOp::Select, tenant_ctx: TenantCtx::Correct, expected: RlsExpected::RlsFilteredZero },
    RlsCase { case_id: "rls_app_api_keys_select_wrong_group", db_role: DbRole::App, table: "api_keys", op: SqlOp::Select, tenant_ctx: TenantCtx::WrongGroupCorrectUser, expected: RlsExpected::RlsFilteredZero },
    RlsCase { case_id: "rls_app_api_keys_select_both_unset", db_role: DbRole::App, table: "api_keys", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::RlsFilteredZero },
    RlsCase { case_id: "rls_app_api_keys_select_wrong_tenant", db_role: DbRole::App, table: "api_keys", op: SqlOp::Select, tenant_ctx: TenantCtx::CorrectRoleWrongTenant, expected: RlsExpected::RlsFilteredZero },

    // ─── user_identities ────────────────────────────────────────────────
    // user_identities has RLS policy by user_id via GUC. Pre-seeded in
    // Tenant::new. Under Correct GUCs with tenant.member as
    // current_user_id, should see the member's identity row.
    RlsCase { case_id: "rls_app_user_identities_select_correct", db_role: DbRole::App, table: "user_identities", op: SqlOp::Select, tenant_ctx: TenantCtx::Correct, expected: RlsExpected::RowsVisible(1) },
    RlsCase { case_id: "rls_app_user_identities_select_wrong_group", db_role: DbRole::App, table: "user_identities", op: SqlOp::Select, tenant_ctx: TenantCtx::WrongGroupCorrectUser, expected: RlsExpected::RowsVisible(1) },
    RlsCase { case_id: "rls_app_user_identities_select_both_unset", db_role: DbRole::App, table: "user_identities", op: SqlOp::Select, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::RlsFilteredZero },
    RlsCase { case_id: "rls_app_user_identities_select_wrong_tenant", db_role: DbRole::App, table: "user_identities", op: SqlOp::Select, tenant_ctx: TenantCtx::CorrectRoleWrongTenant, expected: RlsExpected::RlsFilteredZero },

    // ═══════════════════════════════════════════════════════════════════════
    // Block 2 — garraia_app writes with WITH CHECK rejection scenarios.
    //
    // The RLS WITH CHECK clause runs on INSERT/UPDATE and rejects rows
    // that would not be visible to the current tenant. The `PermissionDenied`
    // oracle variant is reserved for exactly this case.
    //
    // Under Correct GUCs the INSERT succeeds (RowsVisible(1) verified by
    // the readback SELECT).
    //
    // Under WrongGroupCorrectUser GUCs the INSERT violates WITH CHECK
    // (the row's group_id would not match the tenant scope as seen
    // through the policy).
    //
    // Under BothUnset GUCs the WITH CHECK evaluates NULL = group_id
    // which is false (fail-closed), so the write is rejected as
    // PermissionDenied.
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
    //
    // garraia_login grants (migrations 008 + 010):
    //   users            : SELECT
    //   user_identities  : SELECT, UPDATE
    //   sessions         : SELECT, INSERT, UPDATE
    //   audit_events     : INSERT only (SELECT denied)
    //   group_members    : SELECT
    // Everything else   : no grant → InsufficientPrivilege
    //
    // GUCs are irrelevant (BYPASSRLS); all cases use BothUnset as a
    // fixed marker. The matrix verifies positive grants produce
    // RowsVisible and negative grants produce InsufficientPrivilege.
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

    // INSERT paths — exercises grant layer for writes, distinct from
    // SELECT grants. garraia_login has INSERT on audit_events (success)
    // but NOT on chats / memory_items (InsufficientPrivilege).
    RlsCase { case_id: "rls_login_audit_events_insert_allow", db_role: DbRole::Login, table: "audit_events", op: SqlOp::Insert, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::RowsVisible(1) },
    RlsCase { case_id: "rls_login_chats_insert_denied", db_role: DbRole::Login, table: "chats", op: SqlOp::Insert, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },
    RlsCase { case_id: "rls_login_memory_items_insert_denied", db_role: DbRole::Login, table: "memory_items", op: SqlOp::Insert, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },

    // ═══════════════════════════════════════════════════════════════════════
    // Block 4 — garraia_signup (BYPASSRLS) — grant layer only.
    //
    // garraia_signup grants (migration 010):
    //   users            : SELECT, INSERT
    //   user_identities  : SELECT, INSERT
    //   audit_events     : INSERT only (SELECT denied)
    //
    // This role must NOT be able to read or write anything tenant-scoped.
    // Gap B of ADR 0005 is validated here: every RLS-enforced table is
    // explicitly denied.
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

    // INSERT paths for garraia_signup: audit_events is allowed (GRANT
    // INSERT), chats and memory_items are both denied — no grant at all.
    RlsCase { case_id: "rls_signup_audit_events_insert_allow", db_role: DbRole::Signup, table: "audit_events", op: SqlOp::Insert, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::RowsVisible(1) },
    RlsCase { case_id: "rls_signup_chats_insert_denied", db_role: DbRole::Signup, table: "chats", op: SqlOp::Insert, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },
    RlsCase { case_id: "rls_signup_memory_items_insert_denied", db_role: DbRole::Signup, table: "memory_items", op: SqlOp::Insert, tenant_ctx: TenantCtx::BothUnset, expected: RlsExpected::InsufficientPrivilege },
];

// Tripwire: the executor also serves `meta_tripwires.rs` in Task 9. Expose
// the matrix slice as `pub` so the tripwire file can count it.
pub const MATRIX_FOR_TRIPWIRE: &[RlsCase] = RLS_MATRIX;

// ─── Runner ────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn matrix_rls() -> anyhow::Result<()> {
    let h = Harness::get().await;
    let tenant = Tenant::new(&h).await?;
    seed_primary_group(&h, &tenant).await?;

    let mut failures: Vec<String> = Vec::new();

    for case in RLS_MATRIX {
        let outcome = execute_case(&h, &tenant, case).await;
        if outcome != case.expected {
            failures.push(format!(
                "[{}] role={} table={} op={:?} ctx={:?}\n  expected={:?}\n  got     ={:?}",
                case.case_id,
                case.db_role.as_str(),
                case.table,
                case.op,
                case.tenant_ctx,
                case.expected,
                outcome,
            ));
        }
    }

    if !failures.is_empty() {
        panic!(
            "rls matrix: {}/{} failures:\n  {}",
            failures.len(),
            RLS_MATRIX.len(),
            failures.join("\n  "),
        );
    }

    println!("rls matrix: {} cases green", RLS_MATRIX.len());
    Ok(())
}

// ─── Pre-seeding ───────────────────────────────────────────────────────────
//
// `seed_primary_group` inserts one representative row into each
// tenant-scoped table in the primary group via the superuser pool.
// This is a fixture, not an RLS test — it runs once, bypasses RLS, and
// is the only legitimate non-admin write to these tables during the run.

async fn seed_primary_group(h: &Harness, t: &Tenant) -> anyhow::Result<()> {
    let admin = PgPool::connect(&h.admin_url).await?;

    // chats: one channel in primary group.
    let chat_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO chats (id, group_id, type, name, created_by) \
         VALUES ($1, $2, 'channel', 'seed', $3)",
    )
    .bind(chat_id)
    .bind(t.group_id)
    .bind(t.owner.user_id)
    .execute(&admin)
    .await?;

    // chat_members: owner joins the seed chat.
    sqlx::query(
        "INSERT INTO chat_members (chat_id, user_id, role) \
         VALUES ($1, $2, 'owner')",
    )
    .bind(chat_id)
    .bind(t.owner.user_id)
    .execute(&admin)
    .await?;

    // messages: one message in the seed chat by member (so the
    // current_user_id = member.user_id also has policy relevance).
    sqlx::query(
        "INSERT INTO messages (chat_id, group_id, sender_user_id, sender_label, body) \
         VALUES ($1, $2, $3, 'Member', 'seed body')",
    )
    .bind(chat_id)
    .bind(t.group_id)
    .bind(t.member.user_id)
    .execute(&admin)
    .await?;

    // memory_items: group-scoped.
    sqlx::query(
        "INSERT INTO memory_items \
            (scope_type, scope_id, group_id, created_by, created_by_label, kind, content) \
         VALUES ('group', $1, $1, $2, 'Owner', 'note', 'seed note')",
    )
    .bind(t.group_id)
    .bind(t.owner.user_id)
    .execute(&admin)
    .await?;

    // audit_events: one event in primary group, actor = member (matches
    // the app.current_user_id GUC under `Correct` ctx).
    sqlx::query(
        "INSERT INTO audit_events \
            (group_id, actor_user_id, actor_label, action, resource_type) \
         VALUES ($1, $2, 'Member', 'seed.action', 'seed')",
    )
    .bind(t.group_id)
    .bind(t.member.user_id)
    .execute(&admin)
    .await?;

    // sessions: one session for the member. `expires_at` is NOT NULL —
    // set it to 1 hour in the future (irrelevant to RLS, just schema).
    sqlx::query(
        "INSERT INTO sessions (user_id, refresh_token_hash, expires_at) \
         VALUES ($1, $2, now() + interval '1 hour')",
    )
    .bind(t.member.user_id)
    .bind(format!("hash-{}", Uuid::now_v7()))
    .execute(&admin)
    .await?;

    admin.close().await;
    Ok(())
}

// ─── Executor ──────────────────────────────────────────────────────────────

async fn execute_case(h: &Harness, t: &Tenant, case: &RlsCase) -> RlsExpected {
    let pool: &PgPool = match case.db_role {
        DbRole::App => &h.app_pool,
        DbRole::Login => h.login_pool.raw(),
        DbRole::Signup => h.signup_pool.raw(),
    };

    let mut conn = match pool.acquire().await {
        Ok(c) => c,
        Err(e) => {
            panic!("[{}] acquire connection failed: {e}", case.case_id);
        }
    };

    // Set GUCs. `set_config('key', 'value', true)` means "set local to
    // the current transaction"; we wrap the op in an explicit
    // transaction so the GUCs take effect.
    sqlx::query("BEGIN").execute(&mut *conn).await.ok();

    match case.tenant_ctx {
        TenantCtx::Correct => {
            set_guc(&mut conn, "app.current_user_id", &t.member.user_id.to_string()).await;
            set_guc(&mut conn, "app.current_group_id", &t.group_id.to_string()).await;
        }
        TenantCtx::WrongGroupCorrectUser => {
            set_guc(&mut conn, "app.current_user_id", &t.member.user_id.to_string()).await;
            set_guc(&mut conn, "app.current_group_id", &Uuid::now_v7().to_string()).await;
        }
        TenantCtx::BothUnset => {
            // Intentionally nothing.
        }
        TenantCtx::CorrectRoleWrongTenant => {
            let other_user = Uuid::now_v7().to_string();
            let other_group = Uuid::now_v7().to_string();
            set_guc(&mut conn, "app.current_user_id", &other_user).await;
            set_guc(&mut conn, "app.current_group_id", &other_group).await;
        }
    }

    let outcome = match case.op {
        SqlOp::Select => execute_select(&mut conn, case.table).await,
        SqlOp::Insert => execute_insert(&mut conn, case, t).await,
        SqlOp::Update => unimplemented!("no UPDATE cases in the RLS matrix yet"),
        SqlOp::Delete => unimplemented!("no DELETE cases in the RLS matrix yet"),
    };

    // Roll back the transaction regardless — we never want side-effects
    // from matrix execution to contaminate the next case.
    sqlx::query("ROLLBACK").execute(&mut *conn).await.ok();

    outcome
}

async fn execute_select(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
    table: &str,
) -> RlsExpected {
    let sql = format!("SELECT count(*) FROM {table}");
    match sqlx::query_scalar::<_, i64>(&sql).fetch_one(&mut **conn).await {
        Ok(n) => classify_count(n),
        Err(e) => classify_pg_error(&e).unwrap_or_else(|| {
            panic!("unclassified error on SELECT {table}: {e}");
        }),
    }
}

async fn execute_insert(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
    case: &RlsCase,
    t: &Tenant,
) -> RlsExpected {
    let result = match case.table {
        "chats" => {
            sqlx::query(
                "INSERT INTO chats (group_id, type, name, created_by) \
                 VALUES ($1, 'channel', 'matrix-insert', $2)",
            )
            .bind(t.group_id)
            .bind(t.owner.user_id)
            .execute(&mut **conn)
            .await
        }
        "memory_items" => {
            sqlx::query(
                "INSERT INTO memory_items \
                    (scope_type, scope_id, group_id, created_by, created_by_label, kind, content) \
                 VALUES ('group', $1, $1, $2, 'Owner', 'note', 'matrix-insert')",
            )
            .bind(t.group_id)
            .bind(t.owner.user_id)
            .execute(&mut **conn)
            .await
        }
        "audit_events" => {
            sqlx::query(
                "INSERT INTO audit_events \
                    (group_id, actor_user_id, actor_label, action, resource_type) \
                 VALUES ($1, $2, 'Member', 'matrix.insert', 'seed')",
            )
            .bind(t.group_id)
            .bind(t.member.user_id)
            .execute(&mut **conn)
            .await
        }
        other => panic!("execute_insert: no template for table `{other}`"),
    };

    match result {
        Ok(qr) => classify_count(qr.rows_affected() as i64),
        Err(e) => classify_pg_error(&e).unwrap_or_else(|| {
            panic!(
                "[{}] unclassified error on INSERT {}: {e}",
                case.case_id, case.table
            );
        }),
    }
}

async fn set_guc(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
    key: &str,
    val: &str,
) {
    sqlx::query("SELECT set_config($1, $2, true)")
        .bind(key)
        .bind(val)
        .execute(&mut **conn)
        .await
        .expect("set_config");
}
