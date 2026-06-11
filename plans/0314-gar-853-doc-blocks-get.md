# Plan 0314 — GAR-853: GET /v1/doc-blocks/{block_id} — Docs Tier 2 single block fetch

**Type:** Feature — REST API slice (Docs Tier 2)
**Linear:** [GAR-853](https://linear.app/chatgpt25/issue/GAR-853)
**Date:** 2026-06-11 (Florida local time)
**Branch:** `routine/202606111221-doc-blocks-get`

---

## Goal

Add `GET /v1/doc-blocks/{block_id}` to fetch a single doc block by UUID.

Currently `PATCH /v1/doc-blocks/{block_id}` and `DELETE /v1/doc-blocks/{block_id}` exist but there is no `GET`. This gap makes it impossible for API consumers (editors, clients) to fetch the current state of a known block without listing all blocks on the page.

Also sync ROADMAP §3.8 to mark recently completed endpoints as `[x]`.

---

## Architecture

- Handler added to `crates/garraia-gateway/src/rest_v1/doc_blocks.rs`.
- Route registered in `mod.rs` for all three router modes (full / auth-only stub / no-auth stub).
- OpenAPI path entry added via `utoipa::path`.
- Follows the pattern of `update_doc_block`: open transaction, SET LOCAL RLS context, fetch row (RLS filters cross-group → 0 rows → 404), commit.
- Authz: `Action::DocsRead` (same as `list_doc_blocks`).

## Tech stack

- Rust, Axum 0.8, `sqlx`, `utoipa`, `garraia_auth::Principal`, `garraia_auth::can`.
- No new migration. No new dependencies.

## Design invariants

- `SET LOCAL app.current_user_id` + `app.current_group_id` before any SQL in every transaction (FORCE RLS requirement).
- Cross-group `block_id` → 0 rows via RLS USING policy → 404 (no information disclosure).
- Missing `X-Group-Id` header → 400 (same as other doc endpoints).
- No `unwrap()` in production paths.

## Out of scope

- Block move/reorder (separate slice).
- `doc_page_mentions` schema (separate slice).
- CRDT / real-time collaboration (deferred, needs ADR 0008).

## Rollback

Single handler addition — revert commit is sufficient. No migration to roll back.

---

## Tasks

### T1 — Add `get_doc_block` handler to `doc_blocks.rs`

- [ ] Write failing unit test `get_doc_block_response_serializes_correctly` (TDD red).
- [ ] Implement handler: `GET /v1/doc-blocks/{block_id}` → 200 + `DocBlockResponse`.
- [ ] Unit tests green (TDD green).
- [ ] `cargo clippy -p garraia-gateway --tests -- -D warnings` clean.

### T2 — Wire route in `mod.rs`

- [ ] Add `get(doc_blocks::get_doc_block)` to the `/v1/doc-blocks/{block_id}` route in all three router modes.
- [ ] Add `utoipa::path` entry to `OpenApiDoc`.
- [ ] `cargo check -p garraia-gateway` passes.

### T3 — ROADMAP §3.8 sync

- [ ] Mark `doc_blocks` schema, `doc_page_versions` schema, `POST /v1/doc-pages/{page_id}/blocks`, `PATCH /v1/doc-blocks/{block_id}`, `DELETE /v1/doc-blocks/{block_id}`, `POST /v1/doc-pages/{page_id}:duplicate`, `GET /v1/doc-pages/{page_id}/versions` as `[x]` in ROADMAP.md.

### T4 — plans/README.md update

- [ ] Add plan 0314 row.
- [ ] Mark plan 0312 (GAR-850) as Merged.

### T5 — Final CI validation

- [ ] `cargo clippy --workspace --tests --exclude garraia-desktop -- -D warnings` green.

---

## Acceptance criteria

- `GET /v1/doc-blocks/{block_id}` returns 200 + `DocBlockResponse` when block belongs to caller's group.
- Cross-group or missing block → 404.
- Missing `X-Group-Id` → 400.
- Unauthenticated → 401.
- `DocsRead` not held → 403.
- Route registered in all three router modes.
- OpenAPI spec includes the new path.
- ≥ 4 unit tests passing.
- All CI checks green.

---

## Risk register

| Risk | Mitigation |
|------|-----------|
| Route collision with PATCH/DELETE | Axum allows multiple methods on same path; `.get()` added alongside existing handlers |
| RLS context not set | Follows exact same pattern as `update_doc_block` |

---

## Cross-references

- Plan 0304 / GAR-840: doc_blocks CRUD (introduced the module).
- ROADMAP §3.8 Docs Tier 2.
- `crates/garraia-gateway/src/rest_v1/doc_blocks.rs`.
- `crates/garraia-gateway/src/rest_v1/mod.rs`.

---

## Estimativa

- Implementação: 30 min
- Testes: 15 min
- CI: ~25 min
- Total: ~70 min
