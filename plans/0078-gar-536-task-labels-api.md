# Plan 0078 — GAR-536: REST /v1 tasks slice 5 (task labels API)

**Status:** Em execução
**Autor:** Claude Sonnet 4.6 (garra-routine 2026-05-07, America/New_York)
**Data:** 2026-05-07 (America/New_York)
**Issue:** [GAR-536](https://linear.app/chatgpt25/issue/GAR-536)
**Branch:** `routine/202505071223-task-labels-api`
**Epic:** `epic:ws-api`, `epic:ws-tasks`
**Parent:** GAR-396

---

## §1 Goal

Land the task labels REST API (ROADMAP §3.8 Tier 1), delivering five endpoints
on the `garraia_app` RLS-enforced pool:

**Group-level label management:**
- `POST /v1/groups/{group_id}/task-labels` — create label (201, 409 on duplicate name)
- `GET /v1/groups/{group_id}/task-labels` — list all labels for the group (200)
- `DELETE /v1/groups/{group_id}/task-labels/{label_id}` — delete label (204, idempotent; CASCADE removes assignments)

**Task-level label assignment:**
- `POST /v1/groups/{group_id}/tasks/{task_id}/labels` — assign label to task (201, 409 if already assigned)
- `DELETE /v1/groups/{group_id}/tasks/{task_id}/labels/{label_id}` — remove label from task (204, idempotent)

## §2 Architecture

### RLS patterns (both from migration 006)

`task_labels` — **direct group_id** isolation (same class as `tasks`, `task_lists`):
```sql
CREATE POLICY task_labels_group_isolation ON task_labels
    USING (group_id = NULLIF(current_setting('app.current_group_id', TRUE), '')::uuid);
```

`task_label_assignments` — **JOIN via tasks** (same class as `task_assignees`, `task_comments`):
```sql
CREATE POLICY task_label_assignments_through_tasks ON task_label_assignments
    USING (task_id IN (SELECT id FROM tasks));
```
Since `tasks` is filtered by `app.current_group_id`, this transitively scopes
label assignments to the current group.

### Cross-group safety

- `POST task-labels/{label_id}/task-labels` verifies `label.group_id = path_group_id` (RLS alone handles isolation, but explicit check gives clear 404 vs silent RLS filter).
- `POST tasks/{task_id}/labels` verifies the label belongs to the same group before INSERT.
- Both DELETE operations are idempotent (no 404 if row absent).

### Unique violation handling

- `task_labels.UNIQUE (group_id, name)` → SQLSTATE `23505` → 409 Conflict.
- `task_label_assignments.PRIMARY KEY (task_id, label_id)` → SQLSTATE `23505` → 409 Conflict.

### CASCADE behavior

`DELETE /v1/groups/{group_id}/task-labels/{label_id}` issues `DELETE FROM task_labels WHERE id=$1 AND group_id=$2`. Postgres CASCADE removes all `task_label_assignments` rows referencing this label automatically — no extra DELETE needed.

## §3 Tech stack

- Axum 0.8 + `RestV1FullState` (same as tasks.rs)
- `sqlx::query` / `sqlx::query_as` (no SQL string concat)
- `garraia_auth::{Action, Principal, WorkspaceAuditAction, audit_workspace_event}`
- New `WorkspaceAuditAction` variants: `TaskLabelCreated`, `TaskLabelDeleted`, `TaskLabelAssigned`, `TaskLabelRemoved`
- `utoipa` OpenAPI annotations

## §4 Design invariants

1. SET LOCAL both `app.current_user_id` AND `app.current_group_id` in every tx.
2. Audit metadata is STRUCTURAL: `{ name_len: N, color }` for label create — no display names or content.
3. Cross-group label injection returns 404 (RLS + explicit label.group_id check).
4. DELETE operations are always 204 (idempotent — no 404 for already-removed rows).
5. POST label 409 on SQLSTATE `23505` duplicate name within group.
6. POST assignment 409 on SQLSTATE `23505` duplicate (task_id, label_id).
7. No `unwrap()` in production code.
8. `color` stored as-is; validated by DB CHECK `#RRGGBB`; any invalid value returns 422 via `RestError::UnprocessableEntity`.

## §5 Validações pré-plano

- [x] `task_labels` schema in migration 006 ✅
- [x] `task_label_assignments` schema in migration 006 ✅
- [x] `task_labels_group_isolation` FORCE RLS direct policy ✅
- [x] `task_label_assignments_through_tasks` FORCE RLS JOIN policy ✅
- [x] `Action::TasksWrite` / `Action::TasksRead` in action.rs ✅
- [x] `set_rls_context` parameterized SQL pattern (plan 0056) ✅
- [x] `audit_workspace_event` function signature confirmed ✅
- [x] 23505 unique violation catch pattern (plan 0077 / assignees) ✅
- [x] `created_by_label` cache pattern (same as task_lists, task_comments) ✅

## §6 Scope

**In scope:**
- `POST /v1/groups/{group_id}/task-labels`
- `GET /v1/groups/{group_id}/task-labels`
- `DELETE /v1/groups/{group_id}/task-labels/{label_id}`
- `POST /v1/groups/{group_id}/tasks/{task_id}/labels`
- `DELETE /v1/groups/{group_id}/tasks/{task_id}/labels/{label_id}`
- `WorkspaceAuditAction::TaskLabelCreated` + `TaskLabelDeleted` + `TaskLabelAssigned` + `TaskLabelRemoved`
- 10 integration test scenarios (L1–L10)

**Out of scope:**
- `PATCH /v1/groups/{group_id}/task-labels/{label_id}` (rename/recolor) — deferred
- `GET /v1/groups/{group_id}/tasks/{task_id}/labels` (list task's labels) — can add later; label list endpoint covers this via join
- task_subscriptions API — separate slice
- task_activity API — separate slice
- WebSocket kanban stream — GAR-396 parent scope

## §7 Rollback

No schema changes. If implementation is defective, revert handler registrations in `rest_v1/mod.rs` and remove the 5 handler functions from `tasks.rs`. No migration rollback needed.

## §8 Open questions

None. All design decisions confirmed by existing patterns in plans 0069 (comments) and 0077 (assignees).

## §9 File structure

```
crates/
  garraia-auth/
    src/
      audit_workspace.rs        ← +4 variants (TaskLabelCreated/Deleted/Assigned/Removed)
  garraia-gateway/
    src/
      rest_v1/
        mod.rs                  ← +5 route registrations
        tasks.rs                ← +5 handler functions + structs
    tests/
      rest_v1_task_labels.rs    ← NEW — 10 integration scenarios (L1–L10)
plans/
  README.md                     ← new row for 0078
```

## §10 M1 tasks

### T1 — Add 4 WorkspaceAuditAction variants to garraia-auth
- [ ] Add `TaskLabelCreated`, `TaskLabelDeleted`, `TaskLabelAssigned`, `TaskLabelRemoved` to `audit_workspace.rs`
- [ ] Add `as_str()` entries: `"task_label.created"`, `"task_label.deleted"`, `"task.label.assigned"`, `"task.label.removed"`
- [ ] Verify distinct-strings test still passes (now 30 entries)
- [ ] `cargo check -p garraia-auth` green

### T2 — Implement 5 handler functions in tasks.rs
- [ ] `create_task_label` (POST, 201, 409 on 23505 UNIQUE name)
- [ ] `list_task_labels` (GET, 200, ordered by `created_at ASC`)
- [ ] `delete_task_label` (DELETE, 204, idempotent; verify label.group_id = path_group_id first)
- [ ] `assign_task_label` (POST, 201, 409 on 23505 PK; verify label.group_id = task.group_id before INSERT)
- [ ] `remove_task_label_from_task` (DELETE, 204, idempotent; verify task in group first)
- [ ] `cargo check -p garraia-gateway --features test-helpers` green

### T3 — Register 5 routes in rest_v1/mod.rs
- [ ] `POST/GET /v1/groups/{group_id}/task-labels`
- [ ] `DELETE /v1/groups/{group_id}/task-labels/{label_id}`
- [ ] `POST /v1/groups/{group_id}/tasks/{task_id}/labels`
- [ ] `DELETE /v1/groups/{group_id}/tasks/{task_id}/labels/{label_id}`
- [ ] `cargo check -p garraia-gateway --features test-helpers` green

### T4 — Write integration tests (tests/rest_v1_task_labels.rs)
- [ ] L1 — create label 201
- [ ] L2 — create label 409 duplicate name
- [ ] L3 — list labels returns created label
- [ ] L4 — delete label 204 idempotent
- [ ] L5 — assign label to task 201
- [ ] L6 — assign label 409 duplicate
- [ ] L7 — assign label cross-group (label from group B → task in group A) → 404
- [ ] L8 — remove label from task 204 idempotent
- [ ] L9 — list labels empty when none created
- [ ] L10 — create label cross-group path mismatch → 403

### T5 — Workspace-wide clippy + fmt
- [ ] `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean
- [ ] `cargo fmt --check` clean

### T6 — Update plans/README.md
- [ ] Add row for plan 0078 (GAR-536, this plan)
- [ ] Update tasks.rs module doc comment (add Slice 5 mention)

## §11 Risk register

| Risk | Likelihood | Mitigation |
|---|---|---|
| `created_by_label` lookup fails if user has no display_name | Low | Use `COALESCE(display_name, email)` same as task_lists.rs |
| color validation: DB CHECK fires as internal 500 | Medium | Validate hex format in handler before INSERT; return 422 |
| DELETE cascade surprise (all assignments gone) | Design intent | Documented; audit `task_label.deleted` includes `assignments_cascade: true` |

## §12 Acceptance criteria

- `cargo test --workspace --exclude garraia-desktop` all green including L1–L10
- `cargo clippy --workspace ... -- -D warnings` clean
- 10 integration scenarios pass against testcontainers Postgres
- Cross-group label assignment returns 404 (L7)
- Duplicate label name returns 409 (L2)
- DELETE label is idempotent 204 (L4)

## §13 Cross-references

- Plan 0077 (GAR-533, task assignees) — identical RLS + 23505 pattern
- Plan 0069 (GAR-520, task comments) — JOIN RLS pattern
- ROADMAP §3.8 Tier 1 — task labels schema
- Migration 006 (`006_tasks_with_rls.sql`) — `task_labels`, `task_label_assignments`

## §14 Estimativa

**Baixa:** 1.5h | **Provável:** 2.5h | **Alta:** 4h
