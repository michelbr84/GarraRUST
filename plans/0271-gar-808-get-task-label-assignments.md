# Plan 0271 — GAR-808: GET /v1/groups/{group_id}/tasks/{task_id}/labels

## Goal

Add `GET /v1/groups/{group_id}/tasks/{task_id}/labels` to list all labels currently
assigned to a task, completing the task-label-assignment CRUD surface.

## Architecture

Single new handler `list_task_label_assignments` in
`crates/garraia-gateway/src/rest_v1/tasks/labels.rs`, following the established
pattern from `list_task_labels` (GAR-536) and `get_task_label` (GAR-802).

Query JOINs `task_label_assignments` with `task_labels` to return enriched label
data (`TaskLabelResponse`) ordered by `assigned_at ASC`. The task_id path param
is bound in the WHERE clause for cross-task guard.

## Tech stack

- Rust / Axum 0.8 — handler follows existing `assign_task_label` signature
- sqlx parameterized JOIN query, FORCE RLS via `set_rls_context`
- utoipa path annotation for OpenAPI

## Design invariants

- `deleted_at IS NULL` guard on tasks: deleted tasks return 404
- `task_id` path param bound in WHERE clause: cross-task UUID collisions return 404
- `set_rls_context(user_id, group_id)` enforces FORCE RLS on both
  `task_label_assignments` (via JOIN through tasks) and `task_labels` (via group_id)
- `Action::TasksRead` required
- Returns empty `[]` when task has no labels (not 404)
- No audit event for read-only operation

## Validações pré-plano

- `task_label_assignments` schema confirmed: `(task_id, label_id, assigned_at)`
- `task_labels` schema confirmed: `id, group_id, name, color, created_by, created_by_label, created_at`
- `TaskLabelResponse` already defined and used in `list_task_labels` / `get_task_label`
- `check_group_match`, `require_group_id`, `set_rls_context` already imported
- Route `/v1/groups/{group_id}/tasks/{task_id}/labels` exists with `post` only —
  adding `get` is additive (no route conflict)

## Out of scope

- No new migration
- No audit event for reads
- No cursor pagination (tasks rarely have more than a handful of labels)

## Rollback

Revert 4 file changes: labels.rs (handler), mod.rs (export + route wire),
openapi.rs (registration), ROADMAP + plans/README.md. No schema changes.

## Tasks

- [x] T1: Add `list_task_label_assignments` handler + 6 unit tests to `labels.rs`
- [x] T2: Wire route (.get) + export in `mod.rs`
- [x] T3: Register in `openapi.rs`
- [x] T4: Update `plans/README.md` + ROADMAP §3.8 checklist + TODO.md

## Acceptance criteria

- `GET /v1/groups/{group_id}/tasks/{task_id}/labels` returns 200 + `Vec<TaskLabelResponse>`
- Empty array when task has no labels
- 404 for missing, deleted, or cross-group task
- 403 for `TasksRead` missing
- `cargo clippy --workspace --tests --exclude garraia-desktop --no-deps -- -D warnings` clean
- CI all checks green

## Cross-references

- Parent epic: GAR-396
- Same pattern: plan 0267 (GAR-802 GET single task label), plan 0269 (GAR-806 GET single task comment)
- Assignment CRUD history: plan 0078 (GAR-536 POST + DELETE)

## Estimativa

~180 LOC, ~30 min implementation + CI wait.
