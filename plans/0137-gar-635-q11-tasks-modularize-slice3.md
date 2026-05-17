# Plan 0137 — GAR-635 Q11 slice 3: extract `rest_v1/tasks/assignees.rs`

**Linear:** [GAR-635](https://linear.app/chatgpt25/issue/GAR-635)
**Branch:** `routine/202605170326-q11-tasks-slice3`
**Status:** 🚧 In Progress

## Context

Continuation of Q11. After slice 2 (`comments.rs`, PR #370), `tasks/mod.rs` is at 3180 lines.
This slice extracts the assignee CRUD section (plan 0077 / GAR-533, ~333 LOC) into
`rest_v1/tasks/assignees.rs`, bringing `mod.rs` to ~2857 lines.

## What changed

| File | Change |
|------|--------|
| `rest_v1/tasks/assignees.rs` | **New** — 3 handlers + 2 DTOs (282 LOC) |
| `rest_v1/tasks/mod.rs` | Remove assignees section; add `pub mod assignees; pub use assignees::{...}` |
| `rest_v1/openapi.rs` | Update handler paths to `super::tasks::assignees::*` |

### Items extracted to `assignees.rs`

- `AssigneeRow` (private, `sqlx::FromRow`)
- `AssigneeResponse` (pub, `Serialize`, `ToSchema`)
- `AddAssigneeRequest` (pub, `Deserialize`, `ToSchema`)
- `add_task_assignee` handler (`POST /v1/groups/{group_id}/tasks/{task_id}/assignees`)
- `list_task_assignees` handler (`GET /v1/groups/{group_id}/tasks/{task_id}/assignees`)
- `remove_task_assignee` handler (`DELETE /v1/groups/{group_id}/tasks/{task_id}/assignees/{user_id}`)

## Metrics after slice 3

- `tasks/mod.rs`: **2857 lines** (was 3180 after slice 2)
- `tasks/assignees.rs`: 282 LOC

## Zero-behavior guarantee

Pure structural refactor. All re-exports in `mod.rs` preserve existing call-sites.
No logic, SQL, or auth flow changed.

## Test plan

- [x] `cargo check -p garraia-gateway` passes
- [x] `cargo fmt -p garraia-gateway -- --check` clean
- [ ] CI green (20/20 checks)
