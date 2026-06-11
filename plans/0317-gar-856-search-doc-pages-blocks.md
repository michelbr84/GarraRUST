# Plan 0317 — GAR-856: Search slices 16+17 — `types=doc_pages` + `types=doc_blocks`

**Issue:** [GAR-856](https://linear.app/chatgpt25/issue/GAR-856)
**Branch:** `routine/202606111845-search-doc-pages-blocks`
**Date:** 2026-06-11 (Florida / America/New_York)
**Epic:** `epic:ws-docs` + `epic:ws-search`

---

## Goal

Extend `GET /v1/search` with two new `types` values so the unified search covers the Docs Tier 2 surface:

* `types=doc_pages` — FTS on `doc_pages.title` (group scope only; archived excluded)
* `types=doc_blocks` — FTS on `doc_blocks.content_jsonb::text` (group scope only; non-text block types excluded)

Closes ROADMAP §3.8 Tier 2 checklist items:
- "FTS indexa `doc_blocks.content_jsonb` via tsvector."
- "Busca unificada passa a cobrir `messages + files + memory + tasks + docs`."

---

## Architecture

Follows the identical pattern as slices 1–15 (see plan 0219 / GAR-737 as canonical):

1. Add `SearchResultType::DocPage` and `SearchResultType::DocBlock` enum variants.
2. Add `include_doc_pages` and `include_doc_blocks` booleans to `ValidatedSearch`.
3. Parse `"doc_pages"` and `"doc_blocks"` in `parse_and_validate`; both require `scope_type=group`.
4. Add `DocPageSearchRow` and `DocBlockSearchRow` structs (derive `sqlx::FromRow`).
5. Add `fetch_doc_pages` and `fetch_doc_blocks` async functions.
6. Wire into the handler after the `include_labels` block (before `tx.commit()`).
7. Unit tests (≥ 8) covering: accepted / scope-rejected / error-message / combined.

---

## Tech stack

* `sqlx::query_as` (parameterized — no string concat)
* `to_tsvector('simple', ...)` + `websearch_to_tsquery('simple', $1)` — same tokenizer as all other slices
* `content_jsonb::text` cast — converts JSONB to raw text for FTS (no new migration needed)
* FORCE RLS on `doc_pages` (migration 026) + FORCE RLS on `doc_blocks` (migration 027)
* Defense-in-depth: explicit `AND group_id = $2` in WHERE clause

---

## Design invariants

* NO `unwrap()` outside tests.
* NO SQL string concatenation.
* SET LOCAL `app.current_user_id` + `app.current_group_id` already done by `set_rls_context` before all fetches.
* `doc_blocks` result uses `chat_id` field to carry `page_id` (parent container — analogous to `message_threads.chat_id`).
* Non-text block types excluded: `divider`, `file_embed`, `task_embed`, `chat_embed`, `image`.
* Archived doc pages excluded via `AND dp.archived_at IS NULL`.

---

## Out of scope

* No new migration (no persistent `tsvector` column — uses runtime `to_tsvector` like slices 5–15).
* No `scope_type=chat` or `scope_type=user` support for doc types.
* No `author_id` filter for doc_blocks (no `created_by` column on `doc_blocks`).
* No from_date/to_date for doc_blocks initial slice (can be added later; kept consistent with blocks not having user-visible timestamps in the API).

Actually — reconsider: `doc_blocks` does have `created_at`. We'll support `from_date`/`to_date` for both types, consistent with all other group-scoped types.

---

## SQL

### `doc_pages`

```sql
SELECT dp.id,
       ts_rank(
           to_tsvector('simple', dp.title),
           websearch_to_tsquery('simple', $1)
       )::real AS score,
       dp.title,
       dp.group_id,
       dp.created_by,
       dp.created_at
FROM   doc_pages dp
WHERE  to_tsvector('simple', dp.title) @@ websearch_to_tsquery('simple', $1)
  AND  dp.group_id = $2
  AND  dp.archived_at IS NULL
  AND  ($3::timestamptz IS NULL OR dp.created_at >= $3)
  AND  ($4::timestamptz IS NULL OR dp.created_at <= $4)
ORDER BY score DESC, dp.created_at DESC, dp.id DESC
LIMIT $5
```

Result mapping:
- `result_type` = `DocPage`
- `excerpt` = `dp.title`
- `group_id` = `caller_group_id`
- `chat_id` = `None`
- `sender_user_id` = `dp.created_by` (nullable)
- `kind` = `None`

### `doc_blocks`

```sql
SELECT db.id,
       ts_rank(
           to_tsvector('simple', db.content_jsonb::text),
           websearch_to_tsquery('simple', $1)
       )::real AS score,
       left(db.content_jsonb::text, 200) AS excerpt,
       db.page_id,
       db.group_id,
       db.block_type,
       db.created_at
FROM   doc_blocks db
WHERE  to_tsvector('simple', db.content_jsonb::text) @@ websearch_to_tsquery('simple', $1)
  AND  db.group_id = $2
  AND  db.block_type NOT IN ('divider', 'file_embed', 'task_embed', 'chat_embed', 'image')
  AND  ($3::timestamptz IS NULL OR db.created_at >= $3)
  AND  ($4::timestamptz IS NULL OR db.created_at <= $4)
ORDER BY score DESC, db.created_at DESC, db.id DESC
LIMIT $5
```

Result mapping:
- `result_type` = `DocBlock`
- `excerpt` = first 200 chars of `content_jsonb::text`
- `group_id` = `caller_group_id`
- `chat_id` = `Some(db.page_id)` (page container, for navigation)
- `sender_user_id` = `None`
- `kind` = `Some(db.block_type)`

---

## M1 — Implement search.rs changes

- [ ] Add `DocPage` and `DocBlock` to `SearchResultType` enum
- [ ] Add `include_doc_pages` and `include_doc_blocks` to `ValidatedSearch`
- [ ] Add parsing in `parse_and_validate` (both group-only)
- [ ] Add `DocPageSearchRow` struct
- [ ] Add `DocBlockSearchRow` struct
- [ ] Add `fetch_doc_pages` function
- [ ] Add `fetch_doc_blocks` function
- [ ] Wire into handler
- [ ] Add unit tests (≥ 8)

---

## Risk register

| Risk | Severity | Mitigation |
|------|----------|-----------|
| `content_jsonb::text` generates noisy FTS (JSON syntax tokens) | Low | Acceptable for slice 1; can add `jsonb_to_tsvector` later |
| Performance on large `content_jsonb` blobs | Low | Runtime `to_tsvector` is bounded by `websearch_to_tsquery`; no full-scan without tsquery match |

---

## Acceptance criteria

- [x] `types=doc_pages` group scope → 200 with `DocPage` results
- [x] `types=doc_blocks` group scope → 200 with `DocBlock` results, `kind`=block_type, `chat_id`=page_id
- [x] Archived pages excluded
- [x] Non-text blocks excluded
- [x] `types=doc_pages` + non-group scope → 400
- [x] `types=doc_blocks` + non-group scope → 400
- [x] ≥ 8 unit tests
- [x] Clippy clean

---

## Cross-references

- ROADMAP §3.8 Tier 2 checklist
- `plans/0219-gar-737-search-labels.md` — canonical slice 15 pattern
- `crates/garraia-workspace/migrations/026_doc_pages.sql`
- `crates/garraia-workspace/migrations/027_doc_blocks.sql`
