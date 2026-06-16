# Plan 0346 — GAR-890: DELETE /v1/groups/{group_id} — owner-only group soft-deletion

## Goal

Add the missing CRUD endpoint `DELETE /v1/groups/{group_id}` that allows a group
owner to soft-delete (archive) their group. The RBAC infrastructure (`Action::GroupDelete`,
Owner-only per `can.rs:34`) is already in place. Only the DB column, handler, and
filter updates are needed.

## Architecture

- **Migration 031** — `ALTER TABLE groups ADD COLUMN archived_at timestamptz DEFAULT NULL`.
  `NULL` = active; non-NULL = archived. Forward-only, zero downtime (nullable column add).
- **`WorkspaceAuditAction::GroupArchived`** — new variant in `garraia-auth`.
- **`delete_group` handler** in `crates/garraia-gateway/src/rest_v1/groups.rs`.
- **Filter updates** in `get_group`, `list_groups`, `patch_group` — reject archived groups
  with 404 (get/list) or 409 (patch).
- **Route registration** — add `.delete(groups::delete_group)` to `/v1/groups/{id}` in `mod.rs`.
- **OpenAPI** — `#[utoipa::path(delete)]` annotation + `openapi.rs` path registration.

## Tech Stack

- Rust / Axum 0.8 / sqlx / utoipa — same as all other handlers.
- No new dependencies.

## Design Invariants

1. **Owner-only**: `Action::GroupDelete` only granted to Owner role per `can.rs:34` table.
   Admin, Member, Guest, Child all return 403.
2. **Idempotent**: Second `DELETE` on an already-archived group returns 204 without error.
3. **No existence leak**: A caller trying to archive a group they don't belong to gets 403
   (from the Principal extractor's membership check), not 404.
4. **PII-safe audit**: Metadata carries `name_len: usize` (structural), never the group name.
5. **FORCE RLS compliance**: Every transaction opens with `SET LOCAL app.current_user_id`
   AND `SET LOCAL app.current_group_id` (migration 018 FORCE RLS on `groups`).
6. **Forward-only migration**: `archived_at timestamptz DEFAULT NULL` — existing rows remain
   active (NULL). No table rewrite, no backfill needed.

## Out of Scope

- Hard delete (data purge) — deferred to Fase 5.3 retention worker.
- Revoking existing member sessions — members keep valid tokens until natural expiry.
- Cascading effects (archiving chats, files, etc.) — group data stays queryable via
  existing authorized tokens until they expire.

## Rollback

If migration 031 causes issues: `ALTER TABLE groups DROP COLUMN IF EXISTS archived_at;`
(nullable column, no NOT NULL constraint, safe to drop immediately).

## M1 Tasks

- [x] Write plan 0346
- [ ] T1: Migration 031 — add `archived_at` to `groups`
- [ ] T2: `WorkspaceAuditAction::GroupArchived` in `garraia-auth`
- [ ] T3: `delete_group` handler in `groups.rs`
- [ ] T4: Filter `archived_at IS NULL` in `get_group`, `list_groups`, `patch_group`
- [ ] T5: Route registration in `mod.rs`
- [ ] T6: OpenAPI registration in `openapi.rs`
- [ ] T7: 6+ unit tests in `groups.rs`
- [ ] T8: Update ROADMAP + plans/README.md

## Acceptance Criteria

- `DELETE /v1/groups/{id}` returns 204 (idempotent).
- `GET /v1/groups/{id}` returns 404 after archival.
- `GET /v1/groups` excludes archived groups.
- `PATCH /v1/groups/{id}` returns 409 for archived groups.
- Non-owner (Admin) receives 403.
- All 20 CI checks green.
- ROADMAP §3.4 Grupos updated with `[x] DELETE /v1/groups/{group_id}`.

## Cross-References

- GAR-890 — Linear issue
- `crates/garraia-auth/src/can.rs:34` — Owner-only for GroupDelete
- `crates/garraia-auth/src/audit_workspace.rs` — WorkspaceAuditAction enum
- `crates/garraia-gateway/src/rest_v1/groups.rs` — handlers
- `crates/garraia-gateway/src/rest_v1/tasks/task_lists.rs:573` — archive pattern reference
- `crates/garraia-workspace/migrations/001_initial_users_groups.sql:102` — groups table
- plans/0054-gar-ws-chat-slice1-chats-crud.md — canonical plan shape

## Estimativa

~200 LOC net (1 migration + 1 handler + filter updates + tests).
