# Plan 0297 — GAR-835: Docs Tier 2 scaffold — migration 026 + POST/GET /v1/groups/{group_id}/doc-pages

## Goal

Bootstrap the Docs Tier 2 surface with:

1. Migration `026_doc_pages.sql` — `doc_pages` table under FORCE RLS.
2. `POST /v1/groups/{group_id}/doc-pages` — create a doc page.
3. `GET /v1/groups/{group_id}/doc-pages` — list doc pages (cursor-paginated).

Closes the first slice of ROADMAP §3.8 Tier 2 (Docs), which was entirely
unchecked. `DocsRead`/`DocsWrite`/`DocsDelete` actions already exist in
`garraia-auth::Action` (no auth change needed).

## Architecture

Additive handler module `crates/garraia-gateway/src/rest_v1/docs.rs` following
the same patterns as `tasks/mod.rs` and `files.rs`.

Single new migration (`026`) creates `doc_pages` with FORCE RLS, grants, and
indexes. No migration required for `doc_blocks` — that is a future slice.

Audit: `DocPageCreated` → `"doc_page.created"` added to `WorkspaceAuditAction`.

## Tech stack

- Rust / Axum 0.8
- `sqlx` raw queries (INSERT RETURNING, SELECT cursor-keyset)
- `garraia_auth::{Action, Principal, can}` for authz
- `audit_workspace_event` for audit (inside tx, before commit)
- utoipa annotation for OpenAPI

## Design invariants

- SET LOCAL `app.current_user_id` AND `app.current_group_id` before any SQL
  (FORCE RLS requirement).
- No existence leak: 404 for "parent_page not in group" follows RLS — UPDATE
  under RLS on a cross-group row returns 0.
- Auth: `DocsWrite` for create, `DocsRead` for list.
- Keyset cursor: `(created_at DESC, id DESC)`, fetch `limit + 1`, return
  last item id as `next_cursor` when `has_next`.
- `created_by_label` frozen at creation time (denorm pattern used everywhere).
- Limit clamped to [1, 100], default 50.

## File structure (changes)

```
crates/garraia-workspace/migrations/026_doc_pages.sql       (new)
crates/garraia-auth/src/audit_workspace.rs                  (+3 lines: variant + match arm + test assert)
crates/garraia-gateway/src/rest_v1/docs.rs                  (new, ~250 lines)
crates/garraia-gateway/src/rest_v1/mod.rs                   (+8 lines: pub mod + 3-branch routes)
crates/garraia-gateway/src/rest_v1/openapi.rs               (+8 lines: imports + paths + schemas)
plans/README.md                                              (+1 row)
ROADMAP.md                                                   (+2 lines: §3.8 Tier 2 first two items ✅)
```

## M1 tasks

- [x] T1: Migration `026_doc_pages.sql`
- [x] T2: Add `DocPageCreated` to `WorkspaceAuditAction`
- [x] T3: Create `docs.rs` with handlers + 6 unit tests
- [x] T4: Wire routes in `mod.rs` (pub mod + 3 branches)
- [x] T5: Register in `openapi.rs`
- [x] T6: Update ROADMAP.md + plans/README.md; push + open PR

## Acceptance criteria

- `POST /v1/groups/{id}/doc-pages` with `{"title":"My Page"}` → 201 `DocPageResponse`
- `GET /v1/groups/{id}/doc-pages` → 200 `ListDocPagesResponse`
- Cross-group attempt → 403 (RLS + group check)
- `cargo clippy --workspace --tests --exclude garraia-desktop -- -D warnings` clean
- `cargo test -p garraia-gateway` passes

## Cross-references

- ROADMAP §3.8 Tier 2 (Docs)
- `garraia-auth::Action::{DocsRead, DocsWrite, DocsDelete}` (already present)
- Migration 003 (files table — `cover_file_id` FK)
- Linear: [GAR-835](https://linear.app/chatgpt25/issue/GAR-835)

## Estimativa

0.5 h (padrão establish — cópia + adaptação de task create/list)
