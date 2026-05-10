# Plan 0096 — GAR-572: REST /v1 tasks — task_attachments (migration 017 + POST/GET/DELETE)

**Status:** Em execução
**Autor:** Claude Sonnet 4.6 (garra-routine 2026-05-10, America/New_York)
**Data:** 2026-05-10 (America/New_York)
**Issue:** [GAR-572](https://linear.app/chatgpt25/issue/GAR-572)
**Branch:** `routine/202505101823-task-attachments-api`
**Epic:** `epic:ws-api`, `epic:ws-tasks`
**Parent:** GAR-396

---

## §1 Goal

Land the `task_attachments` join table (migration 017) and three REST endpoints
on the `garraia_app` RLS-enforced pool, closing the ROADMAP §3.8 Tier 1 item
deferred since migration 006 pending GAR-387 (files schema, now Done).

**Endpoints:**
- `POST /v1/groups/{group_id}/tasks/{task_id}/attachments` — attach an existing file (201 / 409 dup / 404 cross-group)
- `GET /v1/groups/{group_id}/tasks/{task_id}/attachments` — cursor-paginated list (200)
- `DELETE /v1/groups/{group_id}/tasks/{task_id}/attachments/{file_id}` — detach (204, idempotent)

---

## §2 Architecture

### RLS pattern

`task_attachments` uses the **JOIN via tasks** pattern (same as `task_assignees`,
`task_label_assignments` from migration 006):

```sql
CREATE POLICY task_attachments_group_isolation ON task_attachments
    USING (
        task_id IN (
            SELECT id FROM tasks
            WHERE group_id = NULLIF(current_setting('app.current_group_id', TRUE), '')::uuid
        )
    );
```

`tasks` already has FORCE RLS filtering by `app.current_group_id`, so the JOIN
transitively scopes attachments to the current group without duplicating the predicate.

### Cross-group file injection guard

Before INSERT: verify `files.group_id = path_group_id AND files.deleted_at IS NULL`
using the RLS-scoped tx. File belonging to another group is invisible via RLS → `SELECT`
returns 0 rows → 404 (never 403, anti-enumeration per ADR 0004 §7).

### Unique violation

`task_attachments` PK is `(task_id, file_id)` — SQLSTATE `23505` → 409 Conflict.

---

## §3 Tech stack

- Axum 0.8 + `RestV1FullState` (same as `tasks.rs`)
- `sqlx::query` / `sqlx::query_as` (no SQL string concat)
- `garraia_auth::{Action, Principal, WorkspaceAuditAction, audit_workspace_event}`
- New `WorkspaceAuditAction` variants: `TaskFileAttached`, `TaskFileDetached`
- `utoipa` OpenAPI annotations

---

## §4 Design invariants

1. SET LOCAL both `app.current_user_id` AND `app.current_group_id` in every tx.
2. Cross-group file injection → 404 (RLS + explicit `files.group_id = $path_group_id` check).
3. Soft-deleted file (`files.deleted_at IS NOT NULL`) → 404 on attach.
4. Audit metadata PII-safe: `{ file_id, task_id }` (both UUIDs, no display name).
5. DELETE always 204 idempotent — no 404 if row already absent.
6. No `unwrap()` in production; no SQL string concat.
7. `object_key` and `integrity_hmac` never appear in any response field (ADR 0004 §Security 3).

---

## §5 Validações pré-plano

- [x] Migration 003 (`files`, `file_versions`) — Done via GAR-387
- [x] Migration 006 (`tasks`, `task_assignees` RLS pattern) — Done via GAR-390
- [x] `AppPool` + `Principal` + `set_config` RLS wiring — Done via plan 0016
- [x] `WorkspaceAuditAction` enum + `audit_workspace_event` fn — Done via plans 0054+
- [x] `cargo check -p garraia-gateway` builds green on main
- [x] Next free migration slot = 017 (015 = pinned_at, 016 = task_activity.kind)

---

## §6 Out of scope

- `message_attachments` (different table, different slice)
- `task_attachments` reordering / position column
- Inline file preview in task view (frontend concern)
- `has_attachment` search filter (deferred: requires messages/files schema change, separate slice)
- Any changes to `files` or `file_versions` tables

---

## §7 Rollback

Migration 017 is forward-only. To undo: `DROP TABLE task_attachments;` (no dependent tables).
No breaking API changes: three new routes added, no existing routes modified.

---

## §8 Open questions

None — pattern is established by `task_assignees` (plan 0077) and `task_label_assignments` (plan 0078).

---

## §9 File structure

```
crates/garraia-workspace/migrations/
  017_task_attachments.sql                     NEW — migration
crates/garraia-auth/src/
  audit_workspace.rs                           EDIT — add TaskFileAttached, TaskFileDetached
crates/garraia-gateway/src/
  rest_v1/tasks.rs                             EDIT — 3 handlers + route wiring
  rest_v1/mod.rs                              EDIT — route registration
tests/
  rest_v1_task_attachments.rs                 NEW — 8 integration test scenarios
plans/
  0096-gar-572-task-attachments-api.md        THIS FILE
  README.md                                   EDIT — add row 0096
ROADMAP.md                                    EDIT — tick [ ] task_attachments
```

---

## §10 M1 tasks (TDD order)

- [ ] **T1** — `audit_workspace.rs`: add `TaskFileAttached` + `TaskFileDetached` variants, update `as_str()` match arm, add unit tests asserting the string values.
- [ ] **T2** — `migrations/017_task_attachments.sql`: `CREATE TABLE task_attachments`, composite PK, FK to `tasks` (CASCADE) + `files`, FORCE RLS + policy via JOIN on tasks.
- [ ] **T3** — `rest_v1/tasks.rs`: implement `post_task_attachment`, `list_task_attachments`, `delete_task_attachment` handlers + DTOs; wire routes in `mod.rs`.
- [ ] **T4** — `tests/rest_v1_task_attachments.rs`: 8 integration test scenarios AT1–AT8.
- [ ] **T5** — OpenAPI `#[utoipa::path]` annotations, `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings`, `cargo fmt`.
- [ ] **T6** — Update `ROADMAP.md` (`[ ] task_attachments` → `[x]`) + `plans/README.md` row 0096.

---

## §11 Risk register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Migration slot conflict | Low | Low | Verified: 017 is free |
| RLS JOIN vs direct policy confusion | Low | Medium | Follow plan 0077 pattern exactly |
| `file.deleted_at` check missed | Low | High | Test AT6 specifically covers this |
| Cross-group file injection | Low | High | Test AT5 + explicit `files.group_id` check |

---

## §12 Acceptance criteria

1. `POST /v1/groups/{group_id}/tasks/{task_id}/attachments` → 201 + `{ task_id, file_id, attached_by, attached_at }`.
2. `GET /v1/groups/{group_id}/tasks/{task_id}/attachments` → 200 + `{ items, next_cursor }`.
3. `DELETE /v1/groups/{group_id}/tasks/{task_id}/attachments/{file_id}` → 204 (idempotent).
4. 409 on duplicate `(task_id, file_id)`.
5. 404 when file belongs to a different group.
6. 404 when attaching a soft-deleted file.
7. `cargo clippy --workspace ... -- -D warnings` clean.
8. 8 integration tests (AT1–AT8) pass.
9. `TaskFileAttached` + `TaskFileDetached` audit actions asserted in unit tests.

---

## §13 Cross-references

- Plan 0077 (GAR-533) — task assignees (same RLS pattern, reference implementation)
- Plan 0078 (GAR-536) — task labels (same RLS pattern)
- Plan 0088 (GAR-555) — files API slice 1 (files table established)
- Migration 003 (`003_files_and_folders.sql`) — files schema prerequisite
- Migration 006 (`006_tasks_with_rls.sql`) — tasks schema prerequisite
- ROADMAP §3.8 Tier 1 — `[ ] task_attachments (task_id, file_id)`
- ADR 0004 — object storage security policy (object_key never in HTTP responses)

---

## §14 Estimativa

- T1 (audit): 15 min
- T2 (migration): 20 min
- T3 (handlers + route): 45 min
- T4 (tests): 40 min
- T5 (OpenAPI + clippy): 15 min
- T6 (ROADMAP + plans README): 10 min
- Total: **~2h 25min** (low 2h / high 3.5h)
- LOC delta: ~380 LOC new (migration ~35, audit ~20, handlers ~150, tests ~150, DTOs ~25)
