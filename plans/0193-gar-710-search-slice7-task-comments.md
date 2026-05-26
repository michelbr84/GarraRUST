# Plan 0193 — GAR-710: Search Slice 7 — `types=task_comments`

## Goal

Extend `GET /v1/search` with `types=task_comments`, enabling full-text search over
`task_comments.body_md` in the unified search surface. Seventh slice of §3.9.

## Architecture

- **No new migration.** Runtime `to_tsvector('simple', body_md)` expression.
- JOIN `task_comments tc → tasks t` to get `t.group_id` (same JOIN path used by
  `task_comments_through_tasks` RLS policy in migration 006).
- Group scope only (task comments are always group-scoped via their parent task).
- `deleted_at IS NULL` guard on both `tc` and implicit via RLS.
- `from_date` / `to_date` on `tc.created_at`.
- `author_id` on `tc.author_user_id` (nullable, NULL-safe `$5::uuid IS NULL OR …`).
- `excerpt` = first 200 chars of `body_md`.
- `kind` = `None` (comments have no status enum).
- `sender_user_id` = `tc.author_user_id`.
- Pattern follows plan 0192 (slice 6 / GAR-707) exactly.

## Tech stack

- `crates/garraia-gateway/src/rest_v1/search.rs` — only file changed for Rust.
- `plans/README.md`, `ROADMAP.md` — bookkeeping.

## Design invariants

1. Never `unwrap()` in production paths.
2. SQL via `sqlx::query_as` with positional params — no string concat.
3. `websearch_to_tsquery('simple', $1)` for user input (safe, no operator injection).
4. RLS context (`app.current_user_id` + `app.current_group_id`) set by `set_rls_context`
   before any SELECT — unchanged.
5. No PII in audit metadata.

## Validações pré-plano

- `task_comments` table: migration 006, columns `id`, `task_id`, `author_user_id`,
  `author_label`, `body_md`, `created_at`, `edited_at`, `deleted_at`. ✅ verified.
- `task_comments_through_tasks` RLS JOIN policy: covers `SELECT` via
  `task_comments.task_id → tasks.id WHERE tasks.group_id = app.current_group_id`. ✅ verified.
- No existing search slice for `task_comments` in Linear. ✅ confirmed GAR-710 is new.

## Out of scope

- FTS GIN index on `task_comments.body_md` (runtime tsvector is sufficient for slice 7;
  persistent index would be migration 021, deferred).
- `types=task_comments` for `scope_type=chat` or `scope_type=user` (group-only for now).
- Excerpt highlighting / snippet generation (future slice).

## Rollback

Git revert of the single commit. No migration to undo.

## Open questions

None. Pattern fully established by slices 5 and 6.

## File Structure

```text
crates/garraia-gateway/src/rest_v1/search.rs   — augmented (task_comments support)
plans/0193-gar-710-search-slice7-task-comments.md — this file
plans/README.md                                — row 0193 added
ROADMAP.md                                     — §3.9 slice 7 row added
```

## M1 Tasks

- [x] T1: Add `SearchResultType::TaskComment` variant
- [x] T2: Add `include_task_comments: bool` to `ValidatedSearch`
- [x] T3: Add `"task_comments"` arm to type parser + group-scope-only guard
- [x] T4: Add `TaskCommentSearchRow` struct
- [x] T5: Add `fetch_task_comments()` async function with SQL
- [x] T6: Add handler branch `if validated.include_task_comments { … }`
- [x] T7: Add 6 unit tests
- [x] T8: Update `plans/README.md` + `ROADMAP.md` + top-of-file doc comment

## Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| RLS not firing for JOIN path | Low | `task_comments_through_tasks` policy already proven in task_comments CRUD tests |
| `websearch_to_tsquery` parse failure on adversarial input | Very Low | Returns empty tsquery → 0 rows, not an error |

## Acceptance criteria

- `types=task_comments` + `scope_type=group` → parse succeeds (`include_task_comments=true`)
- `types=task_comments` + `scope_type=chat` → 400 (group-only guard)
- `types=task_comments` + `scope_type=user` → 400 (group-only guard)
- `types=task_comments,messages` + `scope_type=group` → parse succeeds (mixed)
- `types=task_comments,tasks` + `scope_type=group` → parse succeeds (mixed)
- `types=all_five` (`messages,memory,files,tasks,task_comments`) + `scope_type=group` → parse succeeds
- `cargo check -p garraia-gateway` 0 errors
- `cargo clippy --workspace --tests --exclude garraia-desktop … -- -D warnings` clean

## Cross-references

- Plan 0192 (GAR-707, slice 6/tasks) — pattern followed
- ROADMAP.md §3.4 "Busca unificada" + §3.9
- GAR-710 Linear issue

## Estimativa

< 200 LOC, 1 task, ~30 min implementation.
