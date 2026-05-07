# Plan 0077 — GAR-533: REST /v1 tasks slice 4 (task assignees API)

**Status:** Em execução
**Autor:** Claude Sonnet 4.6 (garra-routine 2026-05-07, America/New_York)
**Data:** 2026-05-07 (America/New_York)
**Issue:** [GAR-533](https://linear.app/chatgpt25/issue/GAR-533)
**Branch:** `routine/202505070621-task-assignees-api`
**Epic:** `epic:ws-api`, `epic:ws-tasks`
**Parent:** GAR-396

---

## §1 Goal

Land the task assignees REST API (ROADMAP §3.8 Tier 1), delivering three endpoints
on the `garraia_app` RLS-enforced pool:

- `POST /v1/groups/{group_id}/tasks/{task_id}/assignees` — assign group member (201)
- `GET /v1/groups/{group_id}/tasks/{task_id}/assignees` — list assignees (200)
- `DELETE /v1/groups/{group_id}/tasks/{task_id}/assignees/{user_id}` — remove assignee (204, idempotent)

## §2 Architecture

`task_assignees` uses FORCE RLS via the `task_assignees_through_tasks` JOIN policy:

```sql
CREATE POLICY task_assignees_through_tasks ON task_assignees
    USING (task_id IN (SELECT id FROM tasks));
```

Since `tasks` itself is filtered by `app.current_group_id`, this transitively
scopes assignees to the current group. The pattern is identical to
`task_comments_through_tasks` (plan 0069).

**Cross-group injection guard:** Before inserting, we verify the target `user_id`
is an active member of the current group via a SELECT on `group_members`. If the
user is not found in the group, we return 404 (same as "not found") — never 403,
to avoid confirming user existence.

**Idempotent DELETE:** `DELETE FROM task_assignees WHERE task_id=$1 AND user_id=$2`
always returns 204 regardless of whether the row existed.

**409 Conflict:** INSERT fails on the composite PK (`task_id, user_id`). We catch
the Postgres unique violation SQLSTATE `23505` and return 409.

## §3 Tech stack

- Axum 0.8 + `RestV1FullState` (same as tasks.rs)
- `sqlx::query` / `sqlx::query_as` (no SQL string concat)
- `garraia_auth::{Action, Principal, WorkspaceAuditAction, audit_workspace_event}`
- New `WorkspaceAuditAction` variants: `TaskAssigneeAdded`, `TaskAssigneeRemoved`
- `utoipa` OpenAPI annotations

## §4 Design invariants

1. SET LOCAL both `app.current_user_id` AND `app.current_group_id` in every tx.
2. Audit metadata is STRUCTURAL only: `{ task_id, assignee_user_id_len: 36 }` — never user display names.
3. Cross-group injection returns 404 (group_members check + RLS filter).
4. DELETE is always 204 (idempotent — no 404 for already-removed).
5. POST 409 on duplicate (PK violation SQLSTATE 23505).
6. No `unwrap()` in production code.
7. `assignee_label` NOT stored — this is not needed for audit; task detail can JOIN if needed.

## §5 Validações pré-plano

- [x] `task_assignees` table in migration 006 ✅
- [x] `task_assignees_through_tasks` FORCE RLS JOIN policy ✅
- [x] `Action::TasksWrite` / `Action::TasksRead` in action.rs ✅
- [x] `set_config` parameterized SQL pattern (plan 0056) ✅
- [x] `group_members` membership lookup pattern in groups.rs ✅
- [x] `audit_workspace_event` function signature confirmed ✅
- [x] 23505 unique violation catch pattern in signup_pool.rs ✅

## §6 Scope

**In scope:**
- `POST /v1/groups/{group_id}/tasks/{task_id}/assignees`
- `GET /v1/groups/{group_id}/tasks/{task_id}/assignees`
- `DELETE /v1/groups/{group_id}/tasks/{task_id}/assignees/{user_id}`
- `WorkspaceAuditAction::TaskAssigneeAdded` + `TaskAssigneeRemoved`
- 8+ integration tests in `tests/rest_v1_task_assignees.rs`

**Out of scope:**
- `assigned_by_label` caching (overkill for this surface)
- `task_subscriptions` fan-out on assignment
- PATCH to re-assign (use DELETE + POST)
- Pagination (assignees per task are a small set; full list is fine)

## §7 Affected files

```
crates/garraia-auth/src/audit_workspace.rs         (+2 variants + as_str() + tests)
crates/garraia-gateway/src/rest_v1/tasks.rs        (+3 handlers, +3 DTOs, ~220 LOC)
crates/garraia-gateway/src/rest_v1/mod.rs          (+3 route entries)
crates/garraia-gateway/src/rest_v1/openapi.rs      (+3 paths + schemas)
crates/garraia-gateway/Cargo.toml                  (+[[test]] required-features entry)
crates/garraia-gateway/tests/rest_v1_task_assignees.rs  (new file, 8+ scenarios)
plans/README.md                                    (+row 0077)
plans/0076-gar-530-chat-mgmt-slice4.md             (status → ✅ Merged, after PR #181)
ROADMAP.md                                         (§3.8 task API checkboxes)
```

## §8 Rollback plan

No schema migration. Revert branch on main. Fully reversible.

## §9 Risk register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| RLS JOIN not triggered (missing set_config) | Low | High | Both vars set at tx start; tests verify cross-group isolation |
| group_members check race (user removed between check and insert) | Very Low | Low | INSERT succeeds (FK on users); RLS blocks future reads; acceptable |
| task not found / deleted — 404 passthrough | Low | Low | tasks SELECT inside RLS sees nothing; foreign key fails gracefully |
| 23505 catch missing — duplicate returns 500 | Low | High | Explicit SQLSTATE match in error handler |

## §10 Acceptance criteria

- `POST` returns 201 with `AssigneeResponse`; audit row `task.assignee.added`
- `POST` returns 409 if user already assigned
- `POST` returns 404 if target user is not a group member
- `GET` returns 200 with list of assignees for the task
- `DELETE` returns 204 (idempotent — succeeds even if not assigned)
- Cross-group task returns 404 (RLS + group_id check)
- Unknown task returns 404
- `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` passes
- All 18 CI checks green

## §11 Cross-references

- Plan 0066 (GAR-516) — slice 1: task-list + task handlers
- Plan 0068 (GAR-518) — slice 2: single task GET + task-list PATCH/DELETE
- Plan 0069 (GAR-520) — slice 3: task comments (same RLS JOIN pattern)
- Plan 0076 (GAR-530) — chat management slice 4 (same `set_config` pattern)
- Plan 0056 — set_config parameterized SQL pattern
- Migration 006 (`task_assignees` table + RLS)
- ROADMAP §3.8

## §12 Open questions

None.

## §13 Estimativa

- T1: Audit variants `TaskAssigneeAdded` + `TaskAssigneeRemoved`: 15 min
- T2: DTOs (`AssigneeRow`, `AssigneeResponse`, `AddAssigneeRequest`): 15 min
- T3: Handler `add_task_assignee` (POST): 25 min
- T4: Handler `list_task_assignees` (GET): 20 min
- T5: Handler `remove_task_assignee` (DELETE, idempotent): 15 min
- T6: Route wiring + OpenAPI: 15 min
- T7: Integration tests (8+ scenarios): 35 min
- CI + follow-ups: 20 min
- **Total: ~2.5 hours**

## M1 Tasks

- [ ] T1: Add `TaskAssigneeAdded` + `TaskAssigneeRemoved` to `WorkspaceAuditAction`
- [ ] T2: DTOs in tasks.rs (`AssigneeRow`, `AssigneeResponse`, `AddAssigneeRequest`)
- [ ] T3: Handler `add_task_assignee`
- [ ] T4: Handler `list_task_assignees`
- [ ] T5: Handler `remove_task_assignee`
- [ ] T6: Route wiring + OpenAPI
- [ ] T7: Integration tests `tests/rest_v1_task_assignees.rs`
