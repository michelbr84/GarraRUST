# Plan 0199 — GAR-716: REST /v1 search slice 9 — `types=folders` folder name FTS

## Goal

Add `types=folders` to `GET /v1/search`, enabling callers to search folder names via
full-text search using `to_tsvector('simple', name)` on the `folders` table. Ninth slice
of the `/v1` unified search surface (Fase 3.4 / §3.4 "Busca unificada").

## Architecture

Same pattern as slices 5-7 (files/tasks/task_comments):

1. `SearchResultType::Folder` variant added to the result-type enum.
2. `include_folders: bool` field added to `ValidatedSearch`.
3. `parse_and_validate` recognizes `"folders"` in the `types` parameter; rejects
   `scope_type ≠ group` with 400.
4. `FolderSearchRow` struct (`sqlx::FromRow`): `id`, `score`, `name`, `group_id`,
   `created_by`, `created_at`.
5. `fetch_folders(tx, q, group_id, fetch_up_to)` — mirrors `fetch_files`, using
   `to_tsvector('simple', name)` and `websearch_to_tsquery('simple', $1)`.
6. Handler wires `if validated.include_folders { ... }` block after
   `include_task_comments`.
7. 6 new unit tests covering acceptance/rejection matrix.
8. ROADMAP.md + plans/README.md bookkeeping.

## Tech Stack

- Rust (stable 1.93), sqlx 0.8 (query_as), Axum 0.8
- Postgres 16: `to_tsvector('simple', name)` — runtime FTS, no new GIN index needed
  (folder tables are small; runtime tsvector is sufficient for initial slice)
- `garraia-gateway` crate only — no other crate touched

## Design Invariants

- **Scope restriction**: `types=folders` only valid for `scope_type=group` (same as
  `types=files`, `types=tasks`, `types=task_comments`). Reject with 400 for chat/user.
- **No new migration**: `folders` table exists since migration 003; `FORCE RLS` +
  `folders_group_isolation` policy already in place.
- **Explicit `group_id = $2`**: defense-in-depth even with FORCE RLS active.
- **Deleted folders excluded**: `deleted_at IS NULL` always enforced.
- **`kind = null`**: folders don't have a MIME type or status — leave `kind` as `None`.
- **`sender_user_id = created_by`**: consistent with `files` (uploader ↔ creator).
- **`excerpt = name`**: the full folder name is the meaningful excerpt.
- **No `from_date`/`to_date`/`author_id` filters**: not wired for folders in this slice
  (same decision as `fetch_files` which also doesn't take date/author filters). Can be
  added in a future slice if needed.
- **Tokenizer `'simple'`**: folder names are identifiers, not prose — no stemming.

## Validações pré-plano

- [x] `folders` has `FORCE ROW LEVEL SECURITY` (migration 003:215-216).
- [x] `folders_group_isolation` policy on `folders` (migration 003:218).
- [x] `garraia_app` has `GRANT SELECT ON folders` (migration 003:257).
- [x] `folders.name` NOT NULL, max 200 chars — safe for FTS.
- [x] `folders.deleted_at` column exists for soft-delete exclusion.
- [x] `folders.created_by` nullable FK — mirrors `files.created_by` handling.
- [x] No existing "search slice 9 folders" Linear issue (searched, none found).
- [x] Next plan number is 0199 (0196 used by health run 35 PR #536).

## Out of Scope

- `from_date`/`to_date`/`author_id` filters for folder results (future slice).
- `scope_type=chat` or `scope_type=user` for folders (not meaningful — folders are
  group-scoped by design).
- GIN index on `to_tsvector('simple', folders.name)` (runtime tsvector is sufficient
  for folder tables which are typically small; index can be added if benchmarks warrant).
- Any change to the `folders` REST CRUD endpoints.

## Rollback

- `search.rs` changes are purely additive (new variant, new bool, new function,
  additional if-block). Reverting the file to its `main`-branch state restores previous
  behavior without affecting any other endpoint.
- No migration changes → no DB rollback needed.

## §12 Open Questions

None — the implementation pattern is fully established by slices 5-7.

## File Structure

```
crates/garraia-gateway/src/rest_v1/
  search.rs   ← all changes (see Tasks)
plans/
  0199-gar-716-search-slice9-folders.md   ← this file
  README.md                                ← row 0199 added
ROADMAP.md                                 ← search checklist `[x]` for folders
```

## Tasks

### T1 — Implement `types=folders` in `search.rs`

- [x] Add `Folder` variant to `SearchResultType` enum.
- [x] Add `include_folders: bool` to `ValidatedSearch` struct.
- [x] In `parse_and_validate`: recognize `"folders"` in types loop; add to "at-least-one"
      guard; add `scope_type=group` restriction; return `include_folders` in struct.
- [x] Update `unknown type` error message to include `folders`.
- [x] Add `FolderSearchRow` struct (`sqlx::FromRow`).
- [x] Add `fetch_folders` async fn.
- [x] Wire handler block `if validated.include_folders { ... }`.
- [x] Update module-level doc comment (slices 1-9 header).

### T2 — Unit tests (6 new)

- [x] `types_folders_group_scope_accepted` — `include_folders = true`, others false.
- [x] `types_folders_chat_scope_rejected` — 400.
- [x] `types_folders_user_scope_rejected` — 400.
- [x] `types_folders_and_files_group_scope_accepted` — both flags true.
- [x] `types_folders_and_tasks_group_scope_accepted` — both flags true.
- [x] `types_all_six_group_scope_accepted` — messages, memory, files, tasks,
      task_comments, folders all true.

### T3 — Bookkeeping

- [x] ROADMAP.md: add `[x] GET /v1/search?...&types=folders` to "Busca unificada"
      checklist (after slice 8 sort_by entry).
- [x] plans/README.md: add row 0199.

## Risk Register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Clippy warning on new variant/field | Low | Low | `cargo clippy` before commit |
| RLS not firing for folders | Very Low | High | Explicit `group_id = $2` is defense-in-depth; folders_group_isolation already tested in production by REST CRUD endpoints |
| `deleted_at` filter typo | Very Low | Medium | Unit tests for accepted/rejected scope cover parsing; integration covered by CI suite |

## Acceptance Criteria

- `GET /v1/search?q=reports&scope_type=group&scope_id=<g>&types=folders` returns matching
  folder rows with `type: "folder"`.
- `types=folders,files,tasks` combined request works without error.
- `scope_type=chat&types=folders` → 400.
- `scope_type=user&types=folders` → 400.
- 6 new unit tests all pass locally.
- CI green (all 20 actual checks).
- ROADMAP.md checklist row added.
- plans/README.md row 0199 present.

## Cross-references

- GAR-716: https://linear.app/chatgpt25/issue/GAR-716
- Slice 5 (files): plan 0185 / GAR-703 / PR #505
- Slice 6 (tasks): plan 0192 / GAR-707 / PR #526
- Slice 7 (task_comments): plan 0193 / GAR-710 / PR #532
- Slice 8 (sort_by): plan 0195 / GAR-713 / PR #535
- Migration 003 (folders schema + FORCE RLS + policy): `crates/garraia-workspace/migrations/003_files_and_folders.sql`

## Estimativa

< 1 hour (follows established slice 5-7 pattern exactly; ~120 LOC implementation + 50 LOC tests).
