# Plan 0141 — GAR-655 Q11 slice 6: extract `rest_v1/tasks/activity.rs`

**Linear:** [GAR-655](https://linear.app/chatgpt25/issue/GAR-655) (child of [GAR-635](https://linear.app/chatgpt25/issue/GAR-635))
**Branch:** `routine/202605171215-q11-tasks-slice6-activity`
**Status:** 🚧 In Progress

## Context

Continuation of Q11. After slice 5 (`subscriptions.rs`, PR #376), `tasks/mod.rs` is at ~2071 lines.
This slice extracts the task activity section (plan 0080 / GAR-541 content, ~160 LOC) into
`rest_v1/tasks/activity.rs`, reducing `mod.rs` to ~1910 lines.

## What changed

| File | Change |
|------|--------|
| `rest_v1/tasks/activity.rs` | **New** — 4 types + 1 handler (~160 LOC) |
| `rest_v1/tasks/mod.rs` | Remove activity section; add `pub mod activity; pub use activity::{...}` |

### Items extracted to `activity.rs`

- `ActivityRow` (private, `sqlx::FromRow`)
- `ActivityResponse` (pub, `Serialize`, `ToSchema`) + `From<ActivityRow>` impl
- `ListActivityResponse` (pub, `Serialize`, `ToSchema`)
- `ListActivityQuery` (pub, `Deserialize`, `IntoParams`)
- `list_task_activity` handler (`GET /v1/groups/{group_id}/tasks/{task_id}/activity`)

## Metrics after slice 6

- `tasks/mod.rs`: **~1910 lines** (was 2071 after slice 5)
- `tasks/activity.rs`: ~160 LOC

## Zero-behavior guarantee

Pure structural refactor. All re-exports in `mod.rs` preserve existing call-sites.
No logic, SQL, or auth flow changed.

## Test plan

- [x] `cargo check -p garraia-gateway` passes
- [x] `cargo fmt -p garraia-gateway -- --check` clean
- [ ] CI green (20/20 checks)
