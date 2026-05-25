# Plan 0185 — GAR-703: Search slice 5 — types=files (file name FTS)

**Linear:** [GAR-703](https://linear.app/chatgpt25/issue/GAR-703)
**Branch:** `routine/202605251215-search-slice5-files`
**Date:** 2026-05-25 (America/New_York)

---

## Goal

Add `types=files` to `GET /v1/search` so callers can find group files by name using
full-text search. Follows the exact pattern of slice 1 (memory runtime tsvector)
with no new migration.

## Architecture

- **Crate boundary:** `garraia-gateway/src/rest_v1/search.rs` only.
- **Pool:** `AppPool` (`garraia_app` BYPASSRLS=false). RLS on `files` (migration 003 +
  007 `files_group_isolation` FORCE policy) transparently filters to
  `app.current_group_id`. No extra group predicate needed.
- **FTS tokenizer:** `'simple'` (no language-specific stemming) — file names are
  identifiers, not prose sentences. Mirrors the choice for future tag/slug searches.
- **Scope restriction:** `files` are group-scoped only. Rejected with 400 for
  `scope_type=chat` and `scope_type=user`.
- **No new migration:** files table (migration 003) already has `name`, `group_id`,
  `deleted_at`, `mime_type`, `created_by`, `created_at`.
- **Result mapping:** `type: "file"`, `excerpt` = name, `kind` = mime_type,
  `sender_user_id` = created_by. All other `SearchResult` fields remain `None`.

## Tech stack

- Rust / Axum 0.8 / sqlx (Postgres)
- `utoipa` annotations for OpenAPI 3.1

## Design invariants

1. `SET LOCAL app.current_user_id` AND `app.current_group_id` before any SELECT.
2. No PII in audit — no audit event emitted (search is read-only, no audit in existing slices).
3. Cross-tenant files are invisible via RLS → 0 rows (not 403).
4. `deleted_at IS NULL` predicate on every file query.
5. Default `types` = `"messages,memory"` unchanged — no breaking change.

## Validações pré-plano

- [x] `files` table (migration 003) has `name text NOT NULL`, `group_id uuid NOT NULL`,
      `deleted_at timestamptz`, `mime_type text NOT NULL`, `created_by uuid`.
- [x] RLS policy `files_group_isolation` (migration 007) is FORCE with USING
      `group_id = (SELECT current_setting('app.current_group_id', true)::uuid)`.
- [x] `garraia_app` role has `SELECT` on `files` (migration 003 GRANT block).
- [x] Existing search unit tests exercise `parse_and_validate` — changes must not break them.
- [x] `SearchResult` has `kind: Option<String>` and `sender_user_id: Option<Uuid>` — reusable for mime_type and created_by.

## Out of scope

- GIN index on `to_tsvector('simple', name)` — deferred (follow-up slice).
- Folder-scoped file search.
- File content (full-text) search.
- `from_date`/`to_date` date-range filters on files.
- `author_id` filter on files.

## Rollback

Pure code change in `search.rs`. Reverting = dropping `include_files` from
`ValidatedSearch` and the `File` variant from `SearchResultType`. No migration to undo.

## File structure

```
crates/garraia-gateway/src/rest_v1/search.rs   ← all changes
plans/0185-gar-703-search-slice5-files.md      ← this file
plans/README.md                                ← add row 0185
ROADMAP.md                                     ← add [ ]→[x] for slice 5
TODO.md                                        ← update
```

## M1 tasks

- [x] T1: Add `File` variant to `SearchResultType`
- [x] T2: Add `include_files: bool` to `ValidatedSearch`
- [x] T3: Update `parse_and_validate` — recognize `"files"`, reject non-group scope
- [x] T4: Add `FileSearchRow` struct + `fetch_files()` async function
- [x] T5: Update handler — call `fetch_files()` and map to `SearchResult`
- [x] T6: Add unit tests (6 new test cases)
- [x] T7: Update docs comments in handler error matrix
- [x] T8: Update plans/README.md + ROADMAP.md + TODO.md

## Risk register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| RLS bypassed | Low | Critical | Existing FORCE RLS policy + `set_rls_context` called before query |
| Runtime tsvector perf | Low | Low | Same pattern as memory; file counts are orders of magnitude lower than messages |
| Breaking API change | None | N/A | `files` was previously rejected as "unknown type"; no consumer could depend on it |

## Acceptance criteria

- `GET /v1/search?q=report&scope_type=group&scope_id=<g>&types=files` → 200, `items[*].type = "file"`.
- `GET /v1/search?q=x&scope_type=chat&...&types=files` → 400.
- `GET /v1/search?q=x&scope_type=user&...&types=files` → 400.
- `types=files,messages` → 200 (mixed results allowed).
- Default `types` unchanged → same behavior as before.
- All existing 20+ unit tests in `search.rs` still pass.
- `cargo clippy --workspace --tests --exclude garraia-desktop --no-deps -- -D warnings` green.

## Cross-references

- Slice 1: plan 0084 / GAR-549
- Slice 2: plan 0085 / GAR-551
- Slice 3: plan 0086 / GAR-552
- Slice 4: plan 0179 / GAR-697
- ROADMAP §3.4 "Busca unificada"
- Epic: GAR-WS-SEARCH / Fase 3.4

## Estimativa

~2 hours. ~200 LOC additions in one file, no schema changes.
