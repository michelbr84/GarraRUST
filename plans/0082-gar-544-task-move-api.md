# Plan 0082 — GAR-544: REST /v1 tasks slice 8 (task:move endpoint)

**Status:** Em execução
**Autor:** Claude Sonnet 4.6 (garra-routine 2026-05-08, America/New_York)
**Data:** 2026-05-08 (America/New_York)
**Issue:** [GAR-544](https://linear.app/chatgpt25/issue/GAR-544)
**Branch:** `routine/202605080615-task-move-api` (off `main` after PR #210 merges)
**Epic:** `epic:ws-api`, `epic:ws-tasks`
**Parent:** GAR-396

---

## §1 Goal

Land `POST /v1/groups/{group_id}/tasks/{task_id}:move` — the endpoint that
moves a task from one task list to another within the same group. This is the
last remaining non-WebSocket, non-attachment endpoint in ROADMAP §3.8 Tier 1.

**Request body:** `{ "target_list_id": "<uuid>" }`
**Response:** 200 + full task JSON (same shape as `GET …/tasks/{task_id}`).

No new migration needed. `tasks.list_id` and the compound FK
`(list_id, group_id) → task_lists(id, group_id)` already enforce the
same-group invariant at the DB layer.

---

## §2 Architecture

### Tenant context protocol

Same as all task handlers:
1. `SET LOCAL app.current_user_id = $1` + `SET LOCAL app.current_group_id = $2`
   via parameterised `set_config` (migration 007 FORCE RLS pattern).
2. Path `group_id` must equal `principal.group_id` → 403 otherwise.

### DB query (2-step, single transaction)

```sql
-- Step 1: verify target list exists in group and is not archived
SELECT id
FROM task_lists
WHERE id = $1
  AND group_id = $2
  AND archived_at IS NULL;

-- Step 2: update task, return full row
UPDATE tasks
SET    list_id    = $1,     -- target_list_id
       updated_at = now()
WHERE  id         = $3      -- task_id
  AND  group_id   = $2
  AND  deleted_at IS NULL
RETURNING id, list_id, group_id, parent_task_id, title, description_md,
          status, priority, due_at, started_at, completed_at,
          estimated_minutes, recurrence_rrule,
          created_by, created_by_label, deleted_at, created_at, updated_at;
```

The compound FK guarantees the update cannot silently move the task to a
cross-group list. If `target_list_id` is not in the group, Step 1's SELECT
returns 0 rows → 404 before the UPDATE runs.

### Activity log

After the UPDATE, INSERT one `task_activity` row with:
- `kind = 'moved'`
- `payload = { "from_list_id": "<old>", "to_list_id": "<new>" }`
- `actor_user_id` and `actor_label` from `principal`.

Uses the existing `insert_task_activity` helper (plan 0080 §3).

### Audit event

`audit_workspace_event` call with `WorkspaceAuditAction::TaskUpdated` after the
transaction commits.

---

## §3 Scope

**In scope:**
- Single handler `move_task` registered as `POST /v1/groups/{group_id}/tasks/{task_id}:move`
- Request/response types: `MoveTaskRequest`, response reuses `TaskRow` → `serde_json::Value`
- Activity log (`moved` kind)
- Audit event
- Integration tests (see §T tasks below)

**Out of scope:**
- Position/ordering column — migration 006 explicitly defers this
- Moving subtasks (parent → different list): allowed implicitly; schema has no cross-list subtask constraint
- WebSocket push on move — deferred to WS slice

---

## §4 Acceptance criteria

- [ ] `POST /v1/groups/{gid}/tasks/{tid}:move` with valid `target_list_id` returns 200 + task JSON with updated `list_id`
- [ ] Returns 404 when `target_list_id` is archived or non-existent in the group
- [ ] Returns 404 when `task_id` is soft-deleted or non-existent in the group
- [ ] Returns 403 when path `group_id` ≠ `principal.group_id`
- [ ] Returns 422 when request body is malformed (missing `target_list_id`)
- [ ] Activity entry with `kind='moved'` written atomically
- [ ] Audit event emitted
- [ ] Cross-group isolation test: user of group A cannot move task of group B (Rule 10 CLAUDE.md)
- [ ] `cargo check --workspace` and `cargo clippy --workspace -- -D warnings` green

---

## §5 File structure

Only one file changes:
```
crates/garraia-gateway/src/rest_v1/tasks.rs   — add handler + types + route
```

The handler is added at the bottom of `tasks.rs` (after `list_task_activity`).
Route registration in `tasks_router()` at the top of the file.

---

## §6 Task list (M-tasks)

- [ ] **T1 — Tests first (red):** add failing integration tests for:
  - happy path (200 + updated list_id)
  - archived target list → 404
  - cross-group attempt → 403/404
- [ ] **T2 — Implement `move_task` handler** (handler + `MoveTaskRequest` + route registration)
- [ ] **T3 — Verify tests green** + clippy clean + `cargo fmt`
- [ ] **T4 — Commit + push + PR**

---

## §7 Risk register

| Risk | Mitigation |
|------|-----------|
| Compound FK might reject the UPDATE silently | Step 1 SELECT catches it first → 404 |
| `task_activity.kind` CHECK constraint doesn't include `moved` | Already includes 12 values; `moved` IS one of them (verified in migration 006) |
| MSRV issue from new syntax | No new syntax; same patterns as slices 1-7 |

---

## §8 Rollback plan

Reversible: route can be removed without schema change. No migration to roll back.

---

## §9 Cross-references

- ROADMAP §3.8 Tier 1: `POST /v1/groups/{group_id}/tasks/{task_id}:move`
- Migration 006 (`crates/garraia-workspace/migrations/006_tasks_with_rls.sql`) — schema
- Plan 0080 (GAR-541) — `insert_task_activity` helper to reuse
- Plan 0066 (GAR-516) — `tasks_router()` and `TaskRow` patterns to follow
