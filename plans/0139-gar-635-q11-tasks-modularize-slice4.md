# Plan 0138 — GAR-635 Q11 slice 4: extract `rest_v1/tasks/labels.rs`

**Linear:** [GAR-635](https://linear.app/chatgpt25/issue/GAR-635)
**Branch:** `routine/202605170404-q11-tasks-slice4`
**Status:** 🚧 In Progress

## Context

Continuation of Q11. After slice 3 (`assignees.rs`, PR #371), `tasks/mod.rs` is at 2857 lines.
This slice extracts the labels CRUD section (plan 0078 / GAR-536, ~524 LOC) into
`rest_v1/tasks/labels.rs`, bringing `mod.rs` to ~2341 lines.

## What changed

| File | Change |
|------|--------|
| `rest_v1/tasks/labels.rs` | **New** — 5 handlers + 6 DTOs/structs (546 LOC) |
| `rest_v1/tasks/mod.rs` | Remove labels section; add `pub mod labels; pub use labels::{...}` |
| `rest_v1/openapi.rs` | Update handler paths to `super::tasks::labels::*` |

### Items extracted to `labels.rs`

- `TaskLabelRow` (private, `sqlx::FromRow`)
- `TaskLabelResponse` (pub, `Serialize`, `ToSchema`) + `From<TaskLabelRow>`
- `CreateTaskLabelRequest` (pub, `Deserialize`, `ToSchema`)
- `LabelAssignmentRow` (private, `sqlx::FromRow`)
- `LabelAssignmentResponse` (pub, `Serialize`, `ToSchema`)
- `AssignTaskLabelRequest` (pub, `Deserialize`, `ToSchema`)
- `create_task_label` handler (`POST /v1/groups/{group_id}/task-labels`)
- `list_task_labels` handler (`GET /v1/groups/{group_id}/task-labels`)
- `delete_task_label` handler (`DELETE /v1/groups/{group_id}/task-labels/{label_id}`)
- `assign_task_label` handler (`POST /v1/groups/{group_id}/tasks/{task_id}/labels`)
- `remove_task_label_from_task` handler (`DELETE /v1/groups/{group_id}/tasks/{task_id}/labels/{label_id}`)
- `is_valid_hex_color` (private helper, moved from end of labels section)

## Metrics after slice 4

- `tasks/mod.rs`: **2341 lines** (was 2857 after slice 3)
- `tasks/labels.rs`: 546 LOC

## Zero-behavior guarantee

Pure structural refactor. All re-exports in `mod.rs` preserve existing call-sites.
No logic, SQL, or auth flow changed.

## Test plan

- [x] `cargo check -p garraia-gateway` passes
- [x] `cargo fmt -p garraia-gateway -- --check` clean
- [ ] CI green (20/20 checks)
