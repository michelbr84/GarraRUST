# Plan 0269 — GAR-806: GET /v1/groups/{group_id}/tasks/{task_id}/comments/{comment_id}

## Goal

Add `GET /v1/groups/{group_id}/tasks/{task_id}/comments/{comment_id}` to fetch a single
task comment by UUID, completing the task-comment CRUD surface.

## Architecture

Single new handler `get_task_comment` in `crates/garraia-gateway/src/rest_v1/tasks/comments.rs`,
following the established pattern of `get_task_label` (GAR-802) and `get_thread` (GAR-798).
Returns `CommentResponse` (already defined in the same file), no new types needed.

## Tech stack

- Rust / Axum 0.8 — handler follows existing `delete_task_comment` signature
- sqlx parameterized query, FORCE RLS via `set_rls_context`
- utoipa path annotation for OpenAPI

## Design invariants

- `deleted_at IS NULL` guard: deleted comments return 404 (no existence leak)
- `task_id` path param bound in query: cross-task UUID collisions return 404
- `set_rls_context(group_id)` enforces FORCE RLS JOIN policy on `task_comments`
- `Action::TasksRead` required
- No audit event: read-only operation

## Validações pré-plano

- `task_comments` schema confirmed: `id, task_id, author_user_id, author_label, body_md, created_at, edited_at, deleted_at`
- `CommentRow` + `CommentResponse` already defined and `From<CommentRow>` implemented
- `check_group_match`, `require_group_id`, `set_rls_context` already imported
- Route `/v1/groups/{group_id}/tasks/{task_id}/comments/{comment_id}` exists with `delete` + `patch` — adding `get` is additive

## Out of scope

- No new migration
- No audit event for reads
- No pagination

## Rollback

Revert the 4 file changes (comments.rs handler + test, mod.rs exports + route wire, openapi.rs registration). No schema changes.

## Tasks

- [ ] T1: Add `get_task_comment` handler + 6 unit tests to `comments.rs`
- [ ] T2: Wire route + export in `mod.rs`
- [ ] T3: Register in `openapi.rs`
- [ ] T4: Update `plans/README.md` + ROADMAP §3.8 checklist + TODO.md

## Acceptance criteria

- `GET /v1/groups/{group_id}/tasks/{task_id}/comments/{comment_id}` returns 200 + `CommentResponse`
- 404 for missing, deleted, or cross-group comment
- 403 for `TasksRead` missing
- `cargo clippy --workspace --tests --exclude garraia-desktop --no-deps -- -D warnings` clean
- CI 20/20 green

## Cross-references

- Parent epic: GAR-396
- Same pattern: plan 0267 (GAR-802 GET single task label), plan 0265 (GAR-798 GET single thread)
- Comments CRUD history: plan 0069 (GAR-520), plan 0264 (GAR-795 PATCH)

## Estimativa

~150 LOC, ~30 min implementation + CI wait.
