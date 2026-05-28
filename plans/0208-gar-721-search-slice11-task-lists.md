# Plan 0208 — GAR-721: REST /v1 search slice 11 — `types=task_lists` task list name/description FTS

## Goal

Add `types=task_lists` to `GET /v1/search`, enabling callers to search task list names and
descriptions via full-text search using
`to_tsvector('simple', name || ' ' || coalesce(description, ''))` on the `task_lists` table.
Eleventh slice of the `/v1` unified search surface (Fase 3.4 / §3.4 "Busca unificada").

## Architecture

Same pattern as slices 5-9 (files/tasks/task_comments/folders/chats):

1. `SearchResultType::TaskList` variant added to the result-type enum.
2. `include_task_lists: bool` field added to `ValidatedSearch`.
3. `parse_and_validate` recognizes `"task_lists"` in the `types` parameter; rejects
   `scope_type ≠ group` with 400.
4. `TaskListSearchRow` struct (`sqlx::FromRow`): `id`, `score`, `name`, `group_id`,
   `list_type` (aliased from `type`), `created_by`, `created_at`.
5. `fetch_task_lists(tx, q, group_id, fetch_up_to)` — searches
   `task_lists.name || ' ' || coalesce(task_lists.description, '')` via runtime
   `to_tsvector('simple', ...)`.
6. Handler wires `if validated.include_task_lists { ... }` block after `include_folders`.
7. 6 new unit tests covering acceptance/rejection matrix.
8. ROADMAP.md + plans/README.md bookkeeping.

## Tech Stack

- Rust (stable 1.93), sqlx 0.8 (query_as), Axum 0.8
- Postgres 16: `to_tsvector('simple', ...)` — runtime FTS, no new GIN index needed
- `garraia-gateway` crate only — no other crate touched

## Design Invariants

- **Scope restriction**: `types=task_lists` only valid for `scope_type=group`. Reject with
  400 for chat/user.
- **No new migration**: `task_lists` table exists since migration 006; `FORCE RLS` +
  `task_lists_group_isolation` policy already in place.
- **Explicit `group_id = $2`**: defense-in-depth even with FORCE RLS active.
- **Archived lists excluded**: `archived_at IS NULL` always enforced (task_lists uses
  `archived_at`, not `deleted_at`).
- **`kind = type`**: maps `task_lists.type` ('list', 'board', 'calendar') to the `kind`
  field — tells callers the view mode of the list.
- **`sender_user_id = created_by`**: consistent with files/folders (creator attribution).
- **`excerpt = name`**: the list name is the meaningful excerpt.
- **`chat_id = null`**: task lists are group-scoped, not chat-scoped.
- **No `from_date`/`to_date`/`author_id` filters**: not wired in this slice (consistent
  with files/folders). Can be added later if needed.
- **Tokenizer `'simple'`**: list names are short identifiers, not prose — no stemming.

## Validações pré-plano

- [x] `task_lists` has `FORCE ROW LEVEL SECURITY` (migration 006:226-227).
- [x] `task_lists_group_isolation` policy on `task_lists` (migration 006:228).
- [x] `garraia_app` has `GRANT SELECT ON task_lists` (migration 006:286).
- [x] `task_lists.name` NOT NULL, max 200 chars — safe for FTS.
- [x] `task_lists.description` nullable text — coalesce to empty string for concat.
- [x] `task_lists.archived_at` column exists for soft-archive exclusion.
- [x] `task_lists.created_by` nullable FK — mirrors folders/files handling.
- [x] `task_lists.type` column exists (CHECK 'list'|'board'|'calendar') — used as `kind`.
- [x] GAR-721 Linear issue exists and is In Progress.
- [x] Next plan number is 0208 (0200-0207 used by slice 10 + health runs 38-44).

## Out of Scope

- New migration (not needed — schema already in place).
- `from_date`/`to_date`/`author_id` filters for task_lists (future slice).
- GIN index on `task_lists.name` (runtime FTS sufficient for initial slice).
- `types=chats` slice (covered by GAR-718 / PR #543, slice 10).

## Rollback

Revert the diff to `search.rs`. No migration to roll back.

## File Structure

```
crates/garraia-gateway/src/rest_v1/search.rs  ← only file changed
plans/0208-gar-721-search-slice11-task-lists.md  ← this file
plans/README.md  ← row added
ROADMAP.md  ← checklist row added
```

## M1 Tasks

- [x] T1: Add `SearchResultType::TaskList` variant.
- [x] T2: Add `include_task_lists: bool` to `ValidatedSearch`.
- [x] T3: Update `parse_and_validate` (types loop + group-scope validation + error messages).
- [x] T4: Add `TaskListSearchRow` struct.
- [x] T5: Implement `fetch_task_lists()`.
- [x] T6: Wire handler block.
- [x] T7: Add 6 unit tests.
- [x] T8: Update ROADMAP.md checklist row + plans/README.md.

## Acceptance Criteria

- [x] `GET /v1/search?q=sprint&scope_type=group&scope_id=<g>&types=task_lists` → results
      with `type: "task_list"`, `kind` = list type ('list'/'board'/'calendar').
- [x] `types=task_lists,tasks` combined → works.
- [x] `scope_type=chat&types=task_lists` → 400.
- [x] `scope_type=user&types=task_lists` → 400.
- [x] 6 new unit tests all pass (70 total).
- [x] `cargo check -p garraia-gateway --features test-helpers` → 0 errors.
- [x] `cargo clippy -p garraia-gateway --features test-helpers --no-deps -- -D warnings` → clean.
- [ ] CI green (20/20 checks).
- [ ] ROADMAP.md checklist row marked `[x]`.

## Cross-references

- GAR-721 Linear issue: https://linear.app/chatgpt25/issue/GAR-721
- Predecessor: plan 0200 (GAR-718, slice 10 types=chats)
- Parent: Fase 3.4 / §3.9 "Busca unificada" in ROADMAP.md
- Schema: `crates/garraia-workspace/migrations/006_tasks_with_rls.sql` lines 43-68

## Estimativa

~150 LOC diff in search.rs. No new files except this plan.
