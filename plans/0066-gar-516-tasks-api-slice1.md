# Plan 0066 — GAR-516: REST /v1 Tasks slice 1 (task-lists + tasks CRUD)

**Status:** Em execução
**Autor:** Claude Sonnet 4.6 (garra-routine 2026-05-05, America/New_York)
**Data:** 2026-05-05 (America/New_York)
**Issue:** [GAR-516](https://linear.app/chatgpt25/issue/GAR-516)
**Branch:** `routine/202605051242-tasks-api-slice1`
**Epic:** `epic:ws-api`

---

## §1 Goal

Land the first REST slice for the tasks surface (ROADMAP §3.8 Tier 1),
delivering six endpoints on the `garraia_app` RLS-enforced pool:

- `POST /v1/groups/{group_id}/task-lists` — create task list
- `GET /v1/groups/{group_id}/task-lists` — cursor-paginated list of task lists
- `POST /v1/groups/{group_id}/task-lists/{list_id}/tasks` — create task
- `GET /v1/groups/{group_id}/task-lists/{list_id}/tasks` — cursor-paginated task list
- `PATCH /v1/groups/{group_id}/tasks/{task_id}` — update task fields
- `DELETE /v1/groups/{group_id}/tasks/{task_id}` — soft-delete

## §2 Architecture

`task_lists` and `tasks` live under FORCE RLS (migration 006) with direct
isolation policies:

- `task_lists_group_isolation`: `group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid`
- `tasks_group_isolation`: same pattern, `group_id` denormalized from `task_lists` via compound FK

Both SET LOCAL calls must be issued in every tx (standard `set_config` pattern
from plan 0056 / GAR-508). The compound FK `(list_id, group_id) →
task_lists(id, group_id)` prevents cross-group task drift at the DB level.

## §3 Tech stack

- Axum 0.8 + `RestV1FullState` (same as `memory.rs`, `chats.rs`)
- `sqlx::query` / `sqlx::query_as` + `#[derive(sqlx::FromRow)]` — parameterized Postgres
- `garraia_auth::{Action, Principal, WorkspaceAuditAction, audit_workspace_event}`
- `utoipa` for OpenAPI path/schema registration

## §4 Design invariants

1. **Group-scoped only**: tasks always belong to a group; no personal (user-scope) tasks in slice 1.
2. SET LOCAL both `app.current_user_id` AND `app.current_group_id` in every tx.
3. App-layer validation: path `group_id` must equal `principal.group_id` → 403 on mismatch.
4. List existence verified before task INSERT: `SELECT id FROM task_lists WHERE id = $1 AND archived_at IS NULL`.
5. Audit metadata is STRUCTURAL only — no `title` text (PII). Store `title_len`, `status`, `priority`, `name_len`, `type`.
6. Cross-tenant: 404 (not 403) for unknown/other-tenant tasks in PATCH/DELETE — same pattern as chats/messages.
7. Soft-delete via `deleted_at = now()`. Not exposed in list results (`AND deleted_at IS NULL`).
8. `settings` (jsonb) column is not exposed via API in slice 1 — inserted with DB default `'{}'::jsonb`.
9. Nullable fields (`due_at`, `description_md`) cannot be cleared via PATCH in slice 1 (COALESCE semantics). Documented limitation.

## §5 Validações pré-plano

- [x] `task_lists` + `tasks` FORCE RLS migration 006 in main ✅
- [x] `Action::TasksRead`, `Action::TasksWrite`, `Action::TasksAssign`, `Action::TasksDelete` in `action.rs` ✅
- [x] `set_config` parameterized SQL pattern established by plan 0056 ✅
- [x] `garraia_app` AppPool newtype available via `state.app_pool` ✅
- [x] Compound FK `(list_id, group_id) → task_lists(id, group_id)` in schema ✅
- [x] GAR-516 created in Linear ✅

## §6 Scope

**In scope:**
- `POST /v1/groups/{group_id}/task-lists` — create task list
- `GET /v1/groups/{group_id}/task-lists` — cursor-paginated list
- `POST /v1/groups/{group_id}/task-lists/{list_id}/tasks` — create task
- `GET /v1/groups/{group_id}/task-lists/{list_id}/tasks` — cursor-paginated task list (with optional `?status=` filter)
- `PATCH /v1/groups/{group_id}/tasks/{task_id}` — update title/status/priority/due_at/description_md/estimated_minutes
- `DELETE /v1/groups/{group_id}/tasks/{task_id}` — soft-delete

**Out of scope:**
- `settings` exposure via API (jsonb; plan 0066+)
- Clearing nullable fields via PATCH (plan 0066+)
- `task_assignees` / `task_labels` / `task_comments` / `task_activity` (plan 0066+)
- Subtask management (plan 0066+)
- `PATCH /v1/groups/{group_id}/task-lists/{list_id}` — update task list (plan 0066+)
- `DELETE /v1/groups/{group_id}/task-lists/{list_id}` — archive task list (plan 0066+)

## §7 Affected files

```
crates/garraia-auth/src/audit_workspace.rs         (+ 3 variants + unit tests)
crates/garraia-gateway/src/rest_v1/tasks.rs        (new, ~380 LOC)
crates/garraia-gateway/src/rest_v1/mod.rs          (route wiring, 3 modes + patch import)
crates/garraia-gateway/src/rest_v1/openapi.rs      (6 paths + 9 schemas)
crates/garraia-gateway/Cargo.toml                  (+ [[test]] rest_v1_tasks)
crates/garraia-gateway/tests/rest_v1_tasks.rs      (new, ~280 LOC)
plans/README.md                                    (+ row 0065, confirm 0062 ✅)
```

## §8 Rollback plan

Revert the branch. No schema migration, no data mutation. Fully reversible.

## §9 Risk register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| RLS SET LOCAL omitted | Low | High | Protocol enforced by explicit set_config helper; integration tests verify cross-group isolation |
| Cross-group task creation via compound FK race | Very Low | Medium | Compound FK (list_id, group_id) → task_lists prevents it at DB level |
| `title` text leaks in audit metadata | Low | Medium | Audit carries `title_len` only; snapshot test validates no title content |
| PATCH COALESCE silently clears nullable fields | N/A | N/A | COALESCE(NULL, col) = col; can't clear; documented in API |

## §10 Acceptance criteria

- `POST /v1/groups/{group_id}/task-lists` returns 201 with task list
- `GET /v1/groups/{group_id}/task-lists` returns 200 with list
- `POST /v1/groups/{group_id}/task-lists/{list_id}/tasks` returns 201
- `GET /v1/groups/{group_id}/task-lists/{list_id}/tasks` returns 200 with tasks; excludes deleted
- `PATCH /v1/groups/{group_id}/tasks/{task_id}` returns 200 with updated task
- `DELETE /v1/groups/{group_id}/tasks/{task_id}` returns 204; task no longer in list
- Cross-group PATCH/DELETE returns 404
- Wrong group_id in path returns 403
- `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` passes
- All CI checks pass

## §11 Cross-references

- ROADMAP §3.8 — Tasks Tier 1 Notion-like
- Plan 0054 (chats), 0062 (memory) — precedent pattern
- Plan 0056 — `set_config` parameterized SQL pattern
- Migration 006 (`task_lists` + `tasks` + FORCE RLS)
- `Action::TasksRead/Write/Assign/Delete` in `crates/garraia-auth/src/action.rs`

## §12 Open questions

None — schema and RLS already shipped; pattern established by plan 0056 + plan 0062.

## §13 Estimativa

- T1: Audit variants: 10 min
- T2-T6: tasks.rs handlers (~380 LOC): 50 min
- T7-T8: Routing + OpenAPI: 20 min
- T9: Integration tests (~280 LOC): 40 min
- CI + review follow-ups: 30 min
- **Total: ~2.5 hours**

## M1 Tasks

- [ ] T1: Add `TaskListCreated` + `TaskCreated` + `TaskDeleted` to `WorkspaceAuditAction` + unit tests
- [ ] T2: DTOs: `CreateTaskListRequest`, `TaskListResponse`, `TaskListSummary`, `ListTaskListsResponse`, `ListTaskListsQuery`
- [ ] T3: DTOs: `CreateTaskRequest`, `TaskResponse`, `TaskSummary`, `ListTasksResponse`, `ListTasksQuery`, `PatchTaskRequest`
- [ ] T4: Handlers: `create_task_list` + `list_task_lists`
- [ ] T5: Handlers: `create_task` + `list_tasks`
- [ ] T6: Handlers: `patch_task` + `delete_task`
- [ ] T7: Route wiring in `mod.rs` (mode 1 real + mode 2 fail-soft + mode 3 stub)
- [ ] T8: OpenAPI paths + schemas in `openapi.rs`
- [ ] T9: Integration tests `rest_v1_tasks.rs` (≥10 scenarios)
