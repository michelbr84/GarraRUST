# Plan 0136 — GAR-635 Q11 slice 2: extract `rest_v1/tasks/comments.rs`

**Linear:** [GAR-635](https://linear.app/chatgpt25/issue/GAR-635)
**Branch:** `routine/202605170029-q11-tasks-slice2`
**Status:** 🚧 In Progress

## Context

Continuation of Q11 (plan 0135 = slice 1). After slice 1 extracted `task_lists.rs` (699 LOC),
`tasks/mod.rs` is at 3586 lines — still above the quality-ratchet baseline of 3240.

This slice extracts the comment CRUD section (plan 0069 / GAR-520, ~414 LOC) into
`rest_v1/tasks/comments.rs`, bringing `mod.rs` to ~3180 lines (below the 3240 baseline).

## What changed

| File | Change |
|------|--------|
| `rest_v1/tasks/comments.rs` | **New** — 3 handlers + 5 DTOs/structs (414 LOC) |
| `rest_v1/tasks/mod.rs` | Remove comment section; add `pub mod comments; pub use comments::{...}` |
| `rest_v1/openapi.rs` | Update handler paths to `super::tasks::comments::*` |

### Items extracted to `comments.rs`

- `CommentRow` (private, `sqlx::FromRow`)
- `CommentResponse` (pub, `Serialize`, `ToSchema`) + `From<CommentRow>`
- `CreateCommentRequest` (pub, `Deserialize`, `ToSchema`) + `validate()`
- `ListCommentsQuery` (pub, `Deserialize`, `IntoParams`)
- `ListCommentsResponse` (pub, `Serialize`, `ToSchema`)
- `create_task_comment` handler (`POST /v1/groups/{group_id}/tasks/{task_id}/comments`)
- `list_task_comments` handler (`GET /v1/groups/{group_id}/tasks/{task_id}/comments`)
- `delete_task_comment` handler (`DELETE /v1/groups/{group_id}/tasks/{task_id}/comments/{comment_id}`)

### Import pattern in `comments.rs`

```rust
use super::super::RestV1FullState;
use super::super::problem::RestError;
use super::{DEFAULT_LIMIT, MAX_LIMIT, check_group_match, insert_task_activity,
            require_group_id, set_rls_context};
```

utoipa body references use `super::super::problem::ProblemDetails` (two levels up from
`tasks/comments.rs` to `rest_v1/problem.rs`).

## Metrics after slice 2

- `tasks/mod.rs`: **3180 lines** (was 3586 after slice 1; ratchet baseline ≤ 3240 ✅)
- `tasks/comments.rs`: 337 LOC
- `files_over_1500`: reduced (mod.rs no longer over 1500 if ≤ 3240 is the max threshold
  — still over 1500 but mod.rs is the only file tracked there)

## Zero-behavior guarantee

Pure structural refactor. All re-exports in `mod.rs` preserve existing call-sites in
`router.rs` and `openapi.rs`. No logic, SQL, or auth flow changed.

## Test plan

- [x] `cargo check -p garraia-gateway` passes
- [x] `cargo fmt -p garraia-gateway -- --check` clean
- [ ] `cargo test -p garraia-gateway` passes
- [ ] CI green (20/20 checks)
