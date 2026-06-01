# Plan 0246 — GAR-767: GET /v1/me/files (caller-scoped file inbox)

## Goal

Add `GET /v1/me/files` — cursor-paginated inbox of files uploaded by the
authenticated caller within a given group. Follows the same pattern as
`GET /v1/me/chats` (plan 0245 / GAR-765) but queries the `files` table
with `created_by = caller_user_id`.

## Linear

**GAR-767** — REST /v1 — GET /v1/me/files (caller-scoped file inbox)
<https://linear.app/chatgpt25/issue/GAR-767>

## Architecture

Single handler `me::list_my_files` added to `crates/garraia-gateway/src/rest_v1/me.rs`.

### FORCE RLS protocol

`files` is FORCE RLS via `files_group_isolation` policy (migration 003), keyed
on `app.current_group_id`. Per CLAUDE.md rule 10, the handler sets **both**
`app.current_user_id` and `app.current_group_id` inside a transaction, even
though the files policy only checks `current_group_id`.

`WHERE created_by = $1` is the per-caller filter (functional, not authz). The
FORCE RLS policy is the cross-group isolation guarantee.

### Pagination

Keyset cursor on `(files.created_at DESC, files.id DESC)`. Cursor token = `file_id`
(same pattern as `list_my_chats` which uses `chat_id`). Cursor subquery is
scoped to `group_id = $2` so files in other groups cannot poison the cursor.

### Optional filter

`folder_id: Option<Uuid>` — when present, adds `AND folder_id = $N` to the
query. This is a direct equality filter, no string interpolation.

### Query branches

4 static SQL strings (no concatenation):
1. First page, no folder filter
2. First page, with folder filter
3. Cursor page, no folder filter
4. Cursor page, with folder filter

## Tech stack

- Rust / Axum 0.8 / sqlx
- `files` table (migration 003, FORCE RLS `files_group_isolation`)
- utoipa `#[utoipa::path(...)]` for OpenAPI registration

## Design invariants

- FORCE RLS: `SET LOCAL app.current_user_id` + `SET LOCAL app.current_group_id` before every query.
- No PII in audit: this endpoint is read-only — no audit event emitted.
- Fail-closed cursor: if `after` file_id is not found (deleted or wrong group),
  the cursor subquery returns NULL → `(created_at, id) < NULL` is always false
  → empty safe result.
- Limit clamped: values > 100 clamped to 100; values < 1 clamped to 1; default 50.
- `deleted_at IS NULL` filter prevents soft-deleted files from appearing.

## Validações pré-plano

- [x] `files` FORCE RLS policy confirmed in migration 003: `group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid`.
- [x] `files.created_by` column exists (migration 003 §3.2).
- [x] `files.deleted_at` column exists (soft-delete, migration 003 §3.2).
- [x] Handler pattern confirmed in `list_my_chats` (plan 0245).
- [x] No new migration required (read-only slice).

## Out of scope

- Pagination by `updated_at` ordering (future)
- Mime-type filter (future)
- Files shared with caller (not uploaded by caller) (future)
- Folders the caller created (separate endpoint)

## Rollback

Route deletion + revert of `me.rs` additions + revert of `openapi.rs` additions +
revert of `mod.rs` route entries. No migration needed (read-only slice).

## File structure

```
crates/garraia-gateway/src/rest_v1/
  me.rs            ← add ListMyFilesQuery, MyFileSummary, MyFilesResponse,
                      list_my_files handler, 8 unit tests
  mod.rs           ← add /v1/me/files route in all 3 router branches
  openapi.rs       ← register list_my_files path + MyFilesResponse + MyFileSummary
ROADMAP.md         ← add [x] GET /v1/me/files checkbox in §3.4
plans/README.md    ← add plan 0246 row
TODO.md            ← update
```

## M1 tasks

- [x] T1: Add `ListMyFilesQuery`, `MyFileSummary`, `MyFilesResponse` types to `me.rs`
- [x] T2: Add `list_my_files` handler with 4 SQL branches
- [x] T3: Register route in `mod.rs` (3 router branches)
- [x] T4: Register path + components in `openapi.rs`
- [x] T5: Add 8 unit tests in `me.rs`
- [x] T6: Update `ROADMAP.md` §3.4 + `plans/README.md` + `TODO.md`
- [ ] T7: Push branch, open PR, wait for CI green
- [ ] T8: Squash-merge, mark GAR-767 Done

## Risk register

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| files RLS policy not applied (forgot SET LOCAL) | Low | High | Same pattern as list_my_chats; tested in CI integration suite |
| Cursor poisoning from another group | Low | Medium | Cursor subquery scoped to `group_id = $2` |
| Large `size_bytes` overflow | Very Low | Low | i64 (8 bytes) covers up to ~9.2 EB |

## Acceptance criteria

1. `GET /v1/me/files?group_id=<uuid>` returns 200 with paginated file list for caller's own files.
2. Files from another group are invisible (RLS isolation + explicit `group_id = $2`).
3. Files uploaded by another user are excluded (`created_by = $1`).
4. Soft-deleted files are excluded (`deleted_at IS NULL`).
5. `folder_id` filter narrows to that folder when provided.
6. Cursor pagination: second page starts correctly after `next_cursor`.
7. OpenAPI schema registered; `cargo clippy --workspace … -D warnings` passes.
8. ≥8 unit tests green.

## Cross-references

- Plan 0245 (GAR-765): `GET /v1/me/chats` — same handler pattern
- Plan 0242 (GAR-763): `GET /v1/me/tasks` — same handler pattern
- Migration 003 (`003_files_and_folders.sql`): `files` schema + FORCE RLS
- ROADMAP.md §3.4 "Arquivos" + §3.4 "GET /v1/me/files"

## Estimativa

LOC: ~250 (handler + types + tests). Time: 1 routine slice.
