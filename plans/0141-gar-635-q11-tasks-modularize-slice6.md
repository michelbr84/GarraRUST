# Plan 0141 — GAR-635 Q11 slice 6: extract `rest_v1/tasks/activity.rs`

**Issue:** [GAR-635](https://linear.app/chatgpt25/issue/GAR-635)
**Branch:** `claude/serene-fermat-5prxP`
**Status:** 🚧 In Progress

## Objective

Extract the task activity log section (plan 0080 / GAR-541) from
`rest_v1/tasks/mod.rs` into a new focused sub-module
`rest_v1/tasks/activity.rs`.

Continues the Q11 modularization series (slices 1–5 already merged).

## Scope

**New file:** `crates/garraia-gateway/src/rest_v1/tasks/activity.rs`

Contents moved:
- `ActivityRow` (private DB struct)
- `ActivityResponse` + `From<ActivityRow>`
- `ListActivityResponse`
- `ListActivityQuery`
- `list_task_activity` (GET /v1/groups/{group_id}/tasks/{task_id}/activity)

**`mod.rs` changes:**
- Add `pub mod activity; pub use activity::{...};` block after `subscriptions`
- Remove inlined activity section (~161 lines)
- Add tombstone comment

**`openapi.rs` changes:** none — `list_task_activity` is not registered in the
OpenAPI spec (it exists in the router but was never added to `paths(...)`).

## Key details

- Uses `DEFAULT_LIMIT`, `MAX_LIMIT` from `super` (pagination present)
- Imports `super::super::RestV1FullState`, `super::super::problem::RestError`,
  `super::{DEFAULT_LIMIT, MAX_LIMIT, check_group_match, require_group_id, set_rls_context}`
- `utoipa::path` body references `super::super::problem::ProblemDetails`
- Handler is wired in the router as `tasks::list_task_activity` — re-export preserves that

## Checklist

- [x] `activity.rs` created
- [x] `mod.rs` updated (pub mod + pub use + tombstone + inline removed)
- [x] `cargo check -p garraia-gateway` — clean
- [x] `cargo fmt -p garraia-gateway -- --check` — clean
- [x] `plans/README.md` updated (0140 → Merged, 0141 → In Progress)
- [ ] Commit + push + PR
- [ ] CI green
- [ ] Squash-merge
