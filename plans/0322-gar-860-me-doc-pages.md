# Plan 0322 — GAR-860: GET /v1/me/doc-pages — caller-scoped authored doc pages inbox

> **Status:** In Progress
> **Linear:** [GAR-860](https://linear.app/chatgpt25/issue/GAR-860)
> **Branch:** `routine/202606121215-me-doc-pages`
> **Parent plan:** 0321

## Goal

Add `GET /v1/me/doc-pages` — cursor-paginated inbox of doc pages authored by
the authenticated caller in a specific group. Extends the `/me/*` inbox family
(chats, files, tasks, mentions, invites, reactions, threads, doc-page-mentions).

## Architecture

Thin handler in `crates/garraia-gateway/src/rest_v1/me.rs` following the
established `/me/*` inbox pattern:

1. FORCE RLS via `set_config('app.current_user_id', ...)` +
   `set_config('app.current_group_id', ...)` before any SQL.
2. SELECT from `doc_pages` where `group_id = $1 AND created_by = $2`.
3. Optional `include_archived` bool (default `false`) gates `archived_at IS NULL`.
4. Keyset cursor on `(created_at DESC, id DESC)`.

No new migration — uses `doc_pages` from migration 026 (GAR-834 / plan 0297).

## Tech stack

Rust (Axum 0.8), sqlx (Postgres), utoipa (OpenAPI), `garraia_auth::Principal`.

## Design invariants

- NO `unwrap()` outside tests.
- NO SQL string concat — `sqlx::query_as` with positional `$N` params.
- SET LOCAL both `app.current_user_id` AND `app.current_group_id` before SQL.
- Cross-group cursor attack is fail-closed: cursor subquery anchors to
  `group_id = $1`, so a foreign `after=` returns 0 rows (no info leak).
- Archived pages excluded by default; `include_archived=true` opts in.
- `next_cursor` absent (omitted in JSON) when the page is the last one.

## Validações pré-plano

- `doc_pages` table has `group_id`, `created_by`, `archived_at`, `created_at`,
  `id` columns — confirmed in `DocPageRow` in `docs.rs`.
- `/me/*` inbox pattern well-established — 9 prior inboxes in `me.rs`.
- No new OpenAPI component types needed — follows `MyFilesResponse` shape.

## Out of scope

- Creating or modifying doc pages (covered by `docs.rs`).
- Returning blocks, versions, or mentions alongside pages.
- Any change to the `doc_pages` schema.

## Rollback

Revert the PR. No migration to undo.

## §12 Open questions

None — acceptance criteria fully specified in GAR-860.

## File Structure

```
crates/garraia-gateway/src/rest_v1/
  me.rs          ← add ListMyDocPagesQuery, MyDocPageSummary,
                     MyDocPagesResponse, list_my_doc_pages handler + 6 unit tests
  mod.rs         ← register route in mode-1 (full), mode-2 (stub), mode-3 (stub)
  openapi.rs     ← add list_my_doc_pages to ApiDoc paths
plans/
  0322-gar-860-me-doc-pages.md   ← this file
  README.md                       ← add row for plan 0322
ROADMAP.md                        ← flip [ ] → [x] for GET /v1/me/doc-pages
```

## M1 Tasks

- [x] T1: Create `plans/0322-gar-860-me-doc-pages.md` + `plans/README.md` row
- [ ] T2: Implement `list_my_doc_pages` handler in `me.rs` (types + handler + 6 unit tests)
- [ ] T3: Register route in `mod.rs` (mode-1 real + mode-2 stub + mode-3 stub)
- [ ] T4: Register in `openapi.rs`
- [ ] T5: Update `me.rs` module doc-comment to mention new endpoint
- [ ] T6: Update `ROADMAP.md` §3.4 checklist (`[ ]` → `[x]`)
- [ ] T7: `cargo check -p garraia-gateway` + clippy clean + tests pass
- [ ] T8: Commit, push, open PR, CI green, squash-merge

## Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| `created_by` IS NULL rows silently excluded | Low | Acceptable: NULL means no owner; endpoint is "my pages" |
| Cross-group cursor leaks info | Low | Cursor subquery anchors `group_id = $1` |
| `include_archived` bool param serde ambiguity | Low | `Option<bool>` default false; no validation needed |

## Acceptance criteria

- `GET /v1/me/doc-pages?group_id=<uuid>` returns pages where
  `created_by = caller_user_id` in the group, newest-first.
- Cursor pagination via `after=<page_uuid>` + `limit=1..100` (default 20).
- `include_archived=true` includes archived pages; default excludes them.
- FORCE RLS: `SET LOCAL app.current_user_id` AND `app.current_group_id`.
- Cross-group `after=` returns 0 rows (fail-closed).
- Registered in `openapi.rs`.
- ROADMAP §3.4 updated.
- `cargo clippy --workspace ... -- -D warnings` passes.
- 6 unit tests in `me.rs` pass.

## Cross-references

- Migration 026 (`doc_pages`) — GAR-834 / plan 0297
- Established inbox pattern — plans 0237, 0242, 0245, 0246, 0249, 0255, 0260, 0261, 0318
- Parent plan: 0321 (health run 121)
- GAR-860 Linear issue

## Estimativa

< 2 hours. ~150 LOC new code (handler + tests + routes).
