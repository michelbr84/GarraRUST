# Plan 0068 — GAR-518: REST /v1 Tasks slice 2 (single task GET + task-list PATCH/DELETE)

**Status:** Em execução
**Autor:** Claude Sonnet 4.6 (garra-routine 2026-05-05, America/New_York)
**Data:** 2026-05-05 (America/New_York)
**Issue:** [GAR-518](https://linear.app/chatgpt25/issue/GAR-518)
**Branch:** `routine/<UTC-yyyymmddhhmm>-tasks-api-slice2`
**Epic:** `epic:ws-api`

---

## §1 Goal

Land tasks REST slice 2, delivering three endpoints deferred from plan 0066:

- `GET /v1/groups/{group_id}/tasks/{task_id}` — fetch a single task by ID
- `PATCH /v1/groups/{group_id}/task-lists/{list_id}` — update task list name/description/type
- `DELETE /v1/groups/{group_id}/task-lists/{list_id}` — archive task list (soft-delete via `archived_at`)

## §2 Architecture

Same RLS pattern as plan 0066 (task_lists + tasks FORCE RLS, migration 006). All handlers
follow the `set_config` parameterized SQL protocol (plan 0056): both `app.current_user_id`
AND `app.current_group_id` SET LOCAL in every transaction.

- `GET /{task_id}`: single row fetch with `SELECT … WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL` — cross-group → 404 (not 403), same as PATCH/DELETE in slice 1.
- `PATCH task-lists/{list_id}`: COALESCE semantics for name/type; nullable description can be **explicitly cleared** in this slice via `Option<Option<String>>` with `#[serde(default)]`.
- `DELETE task-lists/{list_id}`: `UPDATE task_lists SET archived_at = now(), updated_at = now() WHERE id = $1 AND group_id = $2 AND archived_at IS NULL` — idempotent 204 if already archived.

## §3 Tech stack

- Axum 0.8 + `RestV1FullState` (same as tasks.rs slice 1)
- `sqlx::query_as` + existing `TaskRow` / `TaskListRow` db-row structs (no new DB structs needed)
- `garraia_auth::{Action, Principal, WorkspaceAuditAction, audit_workspace_event}`
- `utoipa` OpenAPI annotations

## §4 Design invariants

1. Path `group_id` must equal `principal.group_id` → 403 on mismatch.
2. SET LOCAL both `app.current_user_id` AND `app.current_group_id` in every tx.
3. `GET /{task_id}` → 404 for missing/cross-tenant/deleted tasks (no 403 leak).
4. `PATCH task-lists/{list_id}` allows clearing `description` via `{"description": null}` using `Option<Option<String>>` serde pattern.
5. `DELETE task-lists/{list_id}` archives the list; tasks inside are NOT deleted (they become orphaned from the default UI view). Idempotent: already-archived list → 204.
6. Audit metadata is STRUCTURAL only: `name_len` not `name`, `type`, `status`. No PII.
7. `PATCH task-lists` does NOT expose or mutate `settings` jsonb (plan 0068+).
8. Cross-tenant PATCH/DELETE task-lists → 404 (RLS filters to 0 rows; UPDATE affects 0 rows → 404).

## §5 Validações pré-plano

- [x] `task_lists` + `tasks` FORCE RLS migration 006 in main ✅
- [x] `Action::TasksRead`, `Action::TasksWrite`, `Action::TasksDelete` in `action.rs` ✅
- [x] `set_config` parameterized SQL pattern established by plan 0056 ✅
- [x] `TaskRow` + `TaskListRow` + `TaskResponse` + `TaskListResponse` structs in tasks.rs (plan 0066) ✅
- [x] `RestV1FullState` + route wiring pattern confirmed in tasks.rs ✅
- [x] Existing `ALLOWED_LIST_TYPES` constant usable for PATCH type validation ✅

## §6 Scope

**In scope:**
- `GET /v1/groups/{group_id}/tasks/{task_id}` — fetch single task
- `PATCH /v1/groups/{group_id}/task-lists/{list_id}` — update name/description/type (with description null-clear support)
- `DELETE /v1/groups/{group_id}/task-lists/{list_id}` — archive task list

**Out of scope:**
- `task_assignees` / `task_labels` / `task_comments` / `task_activity` (plan 0068+)
- Subtask management (plan 0068+)
- `settings` jsonb exposure via API (plan 0068+)
- Task hard-delete or task list hard-delete (not in schema)
- `PATCH /v1/groups/{group_id}/tasks/{task_id}` nullable-clear fix (already works via COALESCE; full null-clear deferred to plan 0068+)

## §7 Affected files

```
crates/garraia-auth/src/audit_workspace.rs         (+ 2 variants: TaskListUpdated, TaskListArchived + unit tests)
crates/garraia-gateway/src/rest_v1/tasks.rs        (+ 3 handlers + 2 new DTOs, ~180 LOC)
crates/garraia-gateway/src/rest_v1/mod.rs          (3 route entries: GET + PATCH + DELETE task-lists)
crates/garraia-gateway/src/rest_v1/openapi.rs      (+ 3 paths + 3 schemas)
crates/garraia-gateway/tests/rest_v1_tasks.rs      (extend: +6 scenarios, ~120 LOC)
plans/README.md                                    (+ row 0068)
```

## §8 Rollback plan

Revert the branch. No schema migration, no data mutation. Fully reversible.

## §9 Risk register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| RLS SET LOCAL omitted | Low | High | Parameterized set_config pattern; tests verify cross-group isolation |
| `description: null` PATCH clears unintentionally | Low | Low | Only triggered by explicit `{"description": null}`; omitting key = COALESCE keep |
| Archive races (two workers archive same list) | Very Low | None | UPDATE WHERE archived_at IS NULL is a no-op on already-archived; both get 204 |
| `title` text leaks in audit metadata | Low | Medium | Audit carries `name_len` only; snapshot test validates no content |

## §10 Acceptance criteria

- `GET /v1/groups/{group_id}/tasks/{task_id}` returns 200 with full task body
- `GET /{task_id}` returns 404 for deleted/missing/cross-group tasks
- `PATCH /v1/groups/{group_id}/task-lists/{list_id}` returns 200 with updated list; `{"description": null}` clears the field
- `PATCH` returns 404 for missing/cross-group/archived lists
- `DELETE /v1/groups/{group_id}/task-lists/{list_id}` returns 204; list no longer appears in `GET /task-lists`
- `DELETE` is idempotent: second call on already-archived list returns 204
- Wrong `group_id` in path returns 403 (not 404)
- `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` passes
- All CI checks pass

## §11 Cross-references

- ROADMAP §3.8 — Tasks Tier 1 Notion-like
- Plan 0066 (GAR-516) — tasks slice 1; this plan extends its handlers and tests
- Plan 0056 — `set_config` parameterized SQL pattern
- Migration 006 (`task_lists` + `tasks` + FORCE RLS)

## §12 Open questions

None — schema and RLS already shipped; all DTOs and DB row structs in tasks.rs from plan 0066.

## §13 Estimativa

- T1: Audit variants TaskListUpdated + TaskListArchived: 10 min
- T2: DTOs PatchTaskListRequest: 10 min
- T3: Handler `get_task`: 20 min
- T4: Handler `patch_task_list`: 25 min
- T5: Handler `delete_task_list` (archive): 15 min
- T6: Route wiring + OpenAPI: 20 min
- T7: Integration tests (+6 scenarios): 30 min
- CI + review follow-ups: 20 min
- **Total: ~2 hours**

## M1 Tasks

- [ ] T1: Add `TaskListUpdated` + `TaskListArchived` to `WorkspaceAuditAction` + unit tests
- [ ] T2: DTO: `PatchTaskListRequest` (name/description Option<Option<String>>/type all optional)
- [ ] T3: Handler: `get_task` — `GET /v1/groups/{group_id}/tasks/{task_id}`
- [ ] T4: Handler: `patch_task_list` — `PATCH /v1/groups/{group_id}/task-lists/{list_id}`
- [ ] T5: Handler: `delete_task_list` — `DELETE /v1/groups/{group_id}/task-lists/{list_id}` (archive, idempotent)
- [ ] T6: Route wiring in `mod.rs` + OpenAPI paths + schemas in `openapi.rs`
- [ ] T7: Integration tests (get_task 200/404/cross-group, patch_task_list 200/404/null-clear, delete_task_list 204/idempotent)
