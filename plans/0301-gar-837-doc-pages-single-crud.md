# Plan 0301 — GAR-837: GET/PATCH/DELETE /v1/doc-pages/{page_id} — Docs Tier 2 single-page CRUD

## Goal

Close the single-page CRUD gap in the Docs Tier 2 surface (ROADMAP §3.8 Tier 2).
Migration 026 (`doc_pages`) is already in main (PR #706 / plan 0297). This plan adds
three endpoints that operate on an individual doc page by its UUID.

## Architecture

Extends `crates/garraia-gateway/src/rest_v1/docs.rs` (same file as plan 0297).
Routes are registered at `/v1/doc-pages/{page_id}` (not group-scoped) — the FORCE RLS
policy from migration 026 enforces group isolation transparently.

## Tech stack

- Rust, Axum 0.8, sqlx (Postgres), utoipa OpenAPI annotations
- `garraia-auth`: `Action::DocsRead/Write/Delete`, `Principal` extractor,
  `WorkspaceAuditAction::DocPageUpdated/DocPageDeleted` (new variants)

## Design invariants

1. **FORCE RLS**: `SET LOCAL app.current_user_id` AND `app.current_group_id` before
   every SQL statement. Both vars set via `set_rls_context()` helper.
2. **Group isolation**: `doc_pages` table has `FORCE RLS` (migration 026). A row
   invisible to the RLS policy returns 0 rows → handler returns 404. No explicit
   cross-group check needed beyond RLS.
3. **Soft delete only**: `archived_at = COALESCE(archived_at, now())` — no physical
   delete. Idempotent: already-archived pages still return 204.
4. **GET includes archived pages**: caller can check `archived_at != null` to detect
   archived state. Allows restore UI to fetch the page before restoring.
5. **PATCH is a partial update**: only provided fields are updated. Empty body → 400.
6. **Audit PII-safety**: `DocPageUpdated` carries `fields_updated: [...]` (field names
   only, no values). `DocPageDeleted` carries `{}`.
7. **No new migration**: migration 026 already covers all needed columns.

## Out of scope

- `doc_blocks`, `doc_page_versions`, `doc_page_mentions` tables (future slices)
- Real-time CRDT / WebSocket streaming
- Search indexing of doc page content
- `POST /v1/doc-pages/{page_id}:duplicate`
- `GET /v1/doc-pages/{page_id}/versions`

## Rollback

No migration — rollback = revert the PR. Routes removed, audit variants removed.

## File structure

```
crates/garraia-auth/src/audit_workspace.rs     — add DocPageUpdated, DocPageDeleted
crates/garraia-gateway/src/rest_v1/docs.rs     — add PatchDocPageRequest + 3 handlers + 5 tests
crates/garraia-gateway/src/rest_v1/mod.rs      — wire routes in all 3 modes
crates/garraia-gateway/src/rest_v1/openapi.rs  — register 3 paths + PatchDocPageRequest schema
plans/0301-gar-837-doc-pages-single-crud.md    — this file
plans/README.md                                — plan 0301 row added
ROADMAP.md                                     — §3.8 Tier 2 doc_pages API items checked
```

## Tasks

- [x] T1: Add `DocPageUpdated` + `DocPageDeleted` to `WorkspaceAuditAction` + tests
- [x] T2: Add `PatchDocPageRequest` DTO + validation
- [x] T3: Implement `get_doc_page` handler
- [x] T4: Implement `patch_doc_page` handler
- [x] T5: Implement `delete_doc_page` handler
- [x] T6: Wire routes in `mod.rs` (all 3 modes)
- [x] T7: Register OpenAPI paths + schema in `openapi.rs`
- [x] T8: Add unit tests (≥5 new tests covering PatchDocPageRequest)
- [ ] T9: `cargo check` + `cargo clippy` clean
- [ ] T10: PR + CI green + merge

## Acceptance criteria

- `cargo check -p garraia-auth` clean
- `cargo check -p garraia-gateway` clean
- `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean
- `cargo test -p garraia-auth` ≥ N+2 (new audit assertions)
- `cargo test -p garraia-gateway rest_v1::docs` ≥ 11/11 (6 existing + 5 new)
- CI ≥ 20/20 green

## Linear

[GAR-837](https://linear.app/chatgpt25/issue/GAR-837)

## Cross-references

- Plan 0297 (GAR-834/835): migration 026 + POST/GET /v1/groups/{group_id}/doc-pages
- ROADMAP §3.8 Tier 2 — Docs
