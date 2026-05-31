# Plan 0242 — GAR-763: GET /v1/me/tasks (caller-scoped task inbox)

## Goal

Add `GET /v1/me/tasks` — cursor-paginated inbox of tasks assigned to the
authenticated caller within a given group. Follows the same pattern as
`GET /v1/me/mentions` (plan 0237 / GAR-755) but queries `task_assignees ⋈ tasks`.

## Linear

**GAR-763** — REST /v1 — GET /v1/me/tasks (caller-scoped task inbox)
<https://linear.app/chatgpt25/issue/GAR-763>

## Architecture

Single handler `me::list_my_tasks` added to `crates/garraia-gateway/src/rest_v1/me.rs`.

### FORCE RLS protocol

`tasks` and `task_assignees` are both FORCE RLS:
- `tasks` → direct policy via `group_id` (migration 006 §6.9)
- `task_assignees` → JOIN policy via `task_id IN (SELECT id FROM tasks)` (migration 006 §6.9)

The handler opens a transaction, issues `SET LOCAL app.current_user_id` and
`SET LOCAL app.current_group_id`, then queries both tables. The FORCE RLS
policies are the cross-group isolation guarantee; `WHERE ta.user_id = $1`
is the per-caller filter (not an authz mechanism — it is a functional filter).

### Pagination

Keyset cursor on `(tasks.created_at DESC, tasks.id DESC)`. Cursor token = `task_id`
(same pattern as `GET /v1/me/mentions` which uses `message_id`). Cursor subquery
is scoped to the same `group_id` so deleted tasks in other groups cannot poison it.

### Status filter

Optional `?status=<str>`. Validated against the 6-value enum from migration 006:
`backlog | todo | in_progress | review | done | canceled`. Invalid values → 400.
The status filter is applied as a SQL literal parameter only — no string
interpolation, always `AND t.status = $N`.

### Query branches

4 static SQL strings (no concatenation):
1. First page, no status filter
2. First page, with status filter
3. Cursor page, no status filter
4. Cursor page, with status filter

## Tech stack

- Rust / Axum 0.8 / sqlx
- `task_assignees JOIN tasks` (migration 006)
- utoipa `#[utoipa::path(...)]` for OpenAPI registration

## Design invariants

- FORCE RLS: `SET LOCAL app.current_user_id` + `SET LOCAL app.current_group_id` before every query.
- No PII in audit: this endpoint is read-only — no audit event emitted.
- Fail-closed cursor: if `after` task_id is not found (deleted or wrong group),
  the cursor subquery returns NULL → `(created_at, id) < NULL` is always false
  → empty safe result (same pattern as `list_my_mentions`).
- Limit clamped: values > 100 clamped to 100; values < 1 clamped to 1; default 50.
- `deleted_at IS NULL` filter on `tasks` prevents soft-deleted tasks from appearing.

## Out of scope

- Pagination by `due_at` ordering (possible future plan)
- Priority filter (future)
- Notification badges / unread count (future)
- WebSocket push on new assignment (future)

## Rollback

Route deletion + revert of `me.rs` additions + revert of `openapi.rs` additions +
revert of `mod.rs` route entries. No migration needed (read-only slice).

## File structure

```
crates/garraia-gateway/src/rest_v1/
  me.rs           — add ListTasksQuery, TaskAssignmentSummary, TasksListResponse,
                    list_my_tasks handler + ≥ 6 unit tests
  mod.rs          — add route "/v1/me/tasks" in modes 1, 2, 3
  openapi.rs      — add list_my_tasks to paths, TaskAssignmentSummary +
                    TasksListResponse to schemas
plans/
  0242-gar-763-me-tasks-inbox.md   ← this file
```

## M1 tasks

- [x] T1 — Write plan + create GAR-763 Linear issue
- [x] T2 — Implement `ListTasksQuery`, `TaskAssignmentSummary`, `TasksListResponse`,
           `list_my_tasks` handler, ≥ 6 unit tests
- [x] T3 — Register route in `mod.rs` (modes 1 + 2 + 3)
- [x] T4 — Register in `openapi.rs` (paths + schemas)
- [ ] T5 — `cargo check -p garraia-gateway` green
- [ ] T6 — `cargo test -p garraia-gateway` green
- [ ] T7 — Clippy clean
- [ ] T8 — Commit, push, open PR, CI green, squash-merge, mark GAR-763 Done

## Acceptance criteria

1. `GET /v1/me/tasks?group_id=<uuid>` returns 200 with `{ items: [...], next_cursor? }`.
2. Tasks filtered to caller's assignments (`task_assignees.user_id = principal.user_id`).
3. Tasks with `deleted_at IS NOT NULL` never appear.
4. `?status=unknown_value` → 400 Bad Request.
5. `?limit=200` → clamped to 100.
6. Cursor pagination works: `?after=<task_id>` returns older tasks.
7. Cross-group isolation: FORCE RLS blocks all rows from other groups.
8. ≥ 6 unit tests green.

## Cross-references

- Plan 0237 (GAR-755) — `GET /v1/me/mentions` (same caller-scoped pattern)
- Migration 006 §6.2/§6.3 — `tasks` + `task_assignees` schema
- ROADMAP §3.4 — REST /v1 task inbox

## Estimativa

~3h (handler + routing + openapi + tests)
