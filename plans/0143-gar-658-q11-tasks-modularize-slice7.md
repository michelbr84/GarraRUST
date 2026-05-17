# Plan 0143 — GAR-658 Q11 slice 7: extract `rest_v1/tasks/attachments.rs`

**Linear:** [GAR-658](https://linear.app/chatgpt25/issue/GAR-658) (child of [GAR-635](https://linear.app/chatgpt25/issue/GAR-635))
**Branch:** `routine/202605171823-q11-tasks-slice7`
**Status:** 🚧 In Progress

## Context

Continuation of Q11. After slice 6 (`activity.rs`, PR #381), `tasks/mod.rs` is at ~1760 lines.
This slice extracts the task attachments section (plan 0096 / GAR-572 content, ~383 LOC) into
`rest_v1/tasks/attachments.rs`, bringing `mod.rs` to ~1376 lines.

## What changed

| File | Change |
|------|--------|
| `rest_v1/tasks/attachments.rs` | **New** — 5 types + 3 handlers (~383 LOC) |
| `rest_v1/tasks/mod.rs` | Remove attachments section (lines 1533–1919); add `pub mod attachments; pub use attachments::{...}` |

### Items extracted to `attachments.rs`

- `AttachFileRequest` (pub struct, `Deserialize` + `ToSchema`)
- `TaskAttachmentRow` (private struct, `sqlx::FromRow`)
- `TaskAttachmentResponse` (pub struct + `From<TaskAttachmentRow>` impl)
- `ListAttachmentsResponse` (pub struct)
- `ListAttachmentsQuery` (pub struct, `Deserialize` + `IntoParams`)
- `post_task_attachment` handler (`POST /v1/groups/{group_id}/tasks/{task_id}/attachments`)
- `list_task_attachments` handler (`GET /v1/groups/{group_id}/tasks/{task_id}/attachments`)
- `delete_task_attachment` handler (`DELETE /v1/groups/{group_id}/tasks/{task_id}/attachments/{file_id}`)

## Metrics after slice 7

- `tasks/mod.rs`: **~1376 lines** (was ~1919 after slice 5; ~1760 after slice 6)
- `tasks/attachments.rs`: ~383 LOC

## Zero-behavior guarantee

Pure structural refactor. All re-exports in `mod.rs` preserve existing call-sites.
No logic, SQL, RLS, or auth flow changed. No `openapi.rs` change needed — attachment
handlers are not registered in the OpenAPI spec (they have `#[utoipa::path]` annotations
but were never added to the `paths!(...)` macro in `openapi.rs`).

## Test plan

- [ ] `cargo check -p garraia-gateway` passes
- [ ] `cargo fmt -p garraia-gateway -- --check` clean
- [ ] CI green (20/20 checks)
- [ ] `tasks/mod.rs` reduced by ~383 lines

## M1 Tasks

- [x] T1: Read and understand the full attachments section (lines 1533–1919)
- [x] T2: Create `rest_v1/tasks/attachments.rs` with appropriate use imports + all items
- [x] T3: Update `tasks/mod.rs` — remove section, add `pub mod attachments; pub use attachments::{...}`
- [x] T4: `cargo check -p garraia-gateway` + `cargo fmt --check`
- [x] T5: Commit on branch `routine/202605171823-q11-tasks-slice7`
- [x] T6: Push + open PR (#388)
- [x] T7: CI green (20/20)
- [x] T8: Squash-merge (`e04fc2c`), mark GAR-658 Done, update plans/README.md

## Cross-references

- Parent: [GAR-635](https://linear.app/chatgpt25/issue/GAR-635) (Q11 modularize tasks)
- Slice 6 predecessor: plan 0141 / GAR-655 / PR #386 (`a82ef2b`)
- Original implementation: plan 0096 / GAR-572 / PR implementing task_attachments
