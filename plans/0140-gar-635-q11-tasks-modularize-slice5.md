# Plan 0140 — GAR-635 Q11 slice 5: extract `rest_v1/tasks/subscriptions.rs`

**Linear:** [GAR-635](https://linear.app/chatgpt25/issue/GAR-635)
**Branch:** `routine/202605170632-q11-tasks-slice5`
**Status:** 🚧 In Progress

## Context

Continuation of Q11. After slice 4 (`labels.rs`, PR #372), `tasks/mod.rs` is at 2341 lines.
This slice extracts the subscriptions section (plan 0079 / GAR-539, ~279 LOC) into
`rest_v1/tasks/subscriptions.rs`, bringing `mod.rs` to ~2067 lines.

## What changed

| File | Change |
|------|--------|
| `rest_v1/tasks/subscriptions.rs` | **New** — 3 handlers + 2 structs (299 LOC) |
| `rest_v1/tasks/mod.rs` | Remove subscriptions section; add `pub mod subscriptions; pub use subscriptions::{...}` |
| `rest_v1/openapi.rs` | Update handler paths to `super::tasks::subscriptions::*` |

### Items extracted to `subscriptions.rs`

- `SubscriptionRow` (private, `sqlx::FromRow`)
- `SubscriptionResponse` (pub, `Serialize`, `ToSchema`)
- `subscribe_to_task` handler (`POST /v1/groups/{group_id}/tasks/{task_id}/subscriptions`)
- `list_task_subscriptions` handler (`GET /v1/groups/{group_id}/tasks/{task_id}/subscriptions`)
- `unsubscribe_from_task` handler (`DELETE /v1/groups/{group_id}/tasks/{task_id}/subscriptions`)

## Metrics after slice 5

- `tasks/mod.rs`: **2067 lines** (was 2341 after slice 4)
- `tasks/subscriptions.rs`: 299 LOC

## Zero-behavior guarantee

Pure structural refactor. All re-exports in `mod.rs` preserve existing call-sites.
No logic, SQL, or auth flow changed.

## Test plan

- [x] `cargo check -p garraia-gateway` passes
- [x] `cargo fmt -p garraia-gateway -- --check` clean
- [ ] CI green (20/20 checks)
