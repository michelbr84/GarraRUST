# Plan 0083 — GAR-546: REST /v1 tasks slice 9 (subtasks API)

**Status:** Em execução
**Autor:** Claude Sonnet 4.6 (garra-routine 2026-05-08, America/New_York)
**Data:** 2026-05-08 (America/New_York)
**Issue:** [GAR-546](https://linear.app/chatgpt25/issue/GAR-546)
**Branch:** `routine/202605081215-task-subtasks-slice9`
**Epic:** `epic:ws-api`, `epic:ws-tasks`
**Parent:** GAR-396

---

## §1 Goal

Expose the subtask relationship already modelled in the DB (`tasks.parent_task_id UUID REFERENCES tasks(id)`) through the REST API:

1. **`parent_task_id` in `CreateTaskRequest`** — callers can create a task as a child of an existing task in the same group.
2. **`GET /v1/groups/{group_id}/tasks/{task_id}/subtasks`** — cursor-paginated list of direct children.

No schema migration required; `parent_task_id` is already in migration 006 and is already returned by every `TaskResponse`.

---

## §2 Architecture

```
crates/garraia-gateway/src/
  rest_v1/
    tasks.rs           — add parent_task_id to CreateTaskRequest + validate +
                         update INSERT + new list_subtasks handler
    mod.rs             — register GET /v1/groups/{group_id}/tasks/{task_id}/subtasks
    openapi.rs         — add list_subtasks to OpenAPI doc
tests/
  rest_v1_tasks_integration.rs  — integration tests S1–S8
ROADMAP.md             — add two new ✅ checkboxes under §3.8 Task API
plans/README.md        — add row 0083
```

---

## §3 Tech stack

- Rust / Axum 0.8, sqlx Postgres (query string, not macro — consistent with existing task handlers)
- Testcontainers pgvector/pgvector:pg16 (existing integration test harness)

---

## §4 Design invariants

1. **Same group enforcement**: parent task must belong to `path_group_id` (checked via RLS + explicit query on `group_id` column).
2. **Depth limit = 1**: parent task must have `parent_task_id IS NULL` — nesting beyond one level returns 400 `"max nesting depth exceeded"` (simple MVP; unlimited depth deferred).
3. **Not-deleted parent**: parent must have `deleted_at IS NULL`; if soft-deleted → 404.
4. **RLS context**: every DB transaction sets `app.current_user_id` + `app.current_group_id` via existing `set_rls_context()`.
5. **PII-free audit**: `task_activity` payload carries `{ "parent_task_id": "<uuid>" }` — UUIDs are not PII.
6. **Idempotent read**: `GET /subtasks` is a pure read, no side effects.
7. `TaskSummary` already lacks `parent_task_id` (compact view) — `list_subtasks` reuses it for response items.

---

## §5 Validações pré-plano

- [x] `parent_task_id` column exists: `migration 006` has `parent_task_id UUID REFERENCES tasks(id) ON DELETE CASCADE`
- [x] `TaskRow` already selects `parent_task_id`
- [x] `TaskResponse` already exposes `parent_task_id: Option<Uuid>`
- [x] `CreateTaskRequest` does NOT have `parent_task_id` (gap to close)
- [x] No existing `list_subtasks` handler or route (gap to close)
- [x] Existing integration test harness in `tests/rest_v1_tasks_integration.rs`

---

## §6 Out of scope

- Unlimited nesting depth (deferred)
- `PATCH` to reparent a task (separate slice)
- Subtask ordering / position column (deferred — doesn't exist yet)
- WebSocket live subtask updates

---

## §7 Rollback

Pure additive changes: new optional field in request, new endpoint, new route. Rollback = revert the commits. No DB migration to undo.

---

## §8 Rollback plan

`git revert <commit-sha>` on the implementation commit. No migration involved.

---

## §9 File structure (changes)

```
crates/garraia-gateway/src/rest_v1/tasks.rs
  + parent_task_id: Option<Uuid> in CreateTaskRequest
  + validate() addition: parent_task_id cross-group check
  + create_task() INSERT includes parent_task_id
  + pub async fn list_subtasks(...)

crates/garraia-gateway/src/rest_v1/mod.rs
  + route GET /v1/groups/{group_id}/tasks/{task_id}/subtasks

crates/garraia-gateway/src/rest_v1/openapi.rs
  + list_subtasks in paths!()

tests/rest_v1_tasks_integration.rs
  + tests S1–S8 (subtask create, list, cross-group, depth limit, pagination)

ROADMAP.md §3.8
  + [x] parent_task_id in CreateTaskRequest
  + [x] GET /v1/groups/{group_id}/tasks/{task_id}/subtasks
```

---

## §10 M1 tasks

- [x] T1 — Add `parent_task_id: Option<Uuid>` to `CreateTaskRequest`; validate in `validate()`; update `create_task` INSERT + `task_activity` payload
- [x] T2 — Implement `list_subtasks` handler: `GET /v1/groups/{group_id}/tasks/{task_id}/subtasks?cursor=&limit=&status=`
- [x] T3 — Register route in `rest_v1/mod.rs` + `openapi.rs`; add integration tests S1–S8
- [x] T4 — Update `ROADMAP.md` §3.8 + `plans/README.md` row 0083

---

## §11 Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Depth check misses edge case (task is its own ancestor) | Low | Explicit `WHERE parent_task_id = $id AND id != $id` not needed; depth=1 check (`parent IS NULL`) covers it |
| Cross-group injection via `parent_task_id` | Medium | Explicit SELECT on `group_id` column + RLS blocks it |
| Existing tests broken by new field in request | Low | `parent_task_id` is optional with serde default `None`; `deny_unknown_fields` only rejects unknown keys, not missing optional ones |

---

## §12 Open questions

None — design settled.

---

## §13 Acceptance criteria

- `POST /v1/groups/{group_id}/task-lists/{list_id}/tasks` with valid `parent_task_id` → 201 with `parent_task_id` in response body.
- `POST` with `parent_task_id` belonging to different group → 400.
- `POST` with `parent_task_id` of a soft-deleted task → 404.
- `POST` with `parent_task_id` that itself has a parent → 400 "max nesting depth exceeded".
- `GET /v1/groups/{group_id}/tasks/{task_id}/subtasks` returns only direct children with `deleted_at IS NULL`.
- Cursor pagination works (S7 test with 3 subtasks, limit=2).
- All 8 existing task integration test scenarios remain green.

---

## §14 Cross-references

- ROADMAP.md §3.8 Tier 1 Tasks API
- Migration 006 (schema — `parent_task_id` column)
- Plan 0082 (GAR-544, task move — sibling slice)
- GAR-396 (epic parent)

---

## §15 Estimativa

- LOC: ~280 (tasks.rs +180, mod.rs +8, openapi.rs +2, tests +90)
- Time: ~2h implementation + CI wait
