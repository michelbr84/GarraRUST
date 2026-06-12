# Plan 0320 — GAR-858: Doc page mentions — migration 029 + POST/GET/DELETE /v1/doc-pages/{page_id}/mentions + GET /v1/me/doc-page-mentions

**Issue:** [GAR-858](https://linear.app/chatgpt25/issue/GAR-858)
**Branch:** `routine/202506120015-doc-page-mentions`
**Date:** 2026-06-12 (Florida / America/New_York)
**Epic:** `epic:ws-docs` + `epic:ws-api`

---

## Goal

Add user @mention tracking for doc pages (ROADMAP §3.8 Tier 2 `[ ] doc_page_mentions` schema checklist item).

Four endpoints:
- `POST /v1/doc-pages/{page_id}/mentions` — add a user mention to a page (DocsWrite)
- `GET /v1/doc-pages/{page_id}/mentions` — list mentions in a page, cursor-paginated (DocsRead)
- `DELETE /v1/doc-pages/{page_id}/mentions/{user_id}` — remove mention (DocsWrite, idempotent)
- `GET /v1/me/doc-page-mentions` — caller inbox of doc page @mentions, cursor-paginated

---

## Architecture

Follows the `message_mentions` pattern (migration 022 / GAR-755 / plan 0237) exactly.

`doc_page_mentions` is a simple join-table. FORCE RLS via direct `group_id` (same class as
`message_mentions`, `message_reactions`). `group_id` is denormalized from `doc_pages` at INSERT
time — no join required at query time.

---

## Tech stack

- Postgres 16 FORCE RLS + NULLIF fail-closed
- sqlx raw queries (no `sqlx::query!` macro — no compile-time DB needed in CI)
- Axum 0.8, `garraia_auth::{Principal, Action, can, WorkspaceAuditAction, audit_workspace_event}`
- utoipa for OpenAPI schemas

---

## Design invariants

1. PK `(page_id, mentioned_user_id)` — one row per (page, user). Idempotent POST via `ON CONFLICT DO NOTHING`.
2. `group_id` populated at INSERT from `doc_pages.group_id` (requires lookup before insert).
3. FORCE RLS: `app.current_group_id` must be set via `set_config` before every query.
4. Both `SET LOCAL app.current_user_id` and `SET LOCAL app.current_group_id` always set (CLAUDE.md rule).
5. Cross-group `page_id` → 404 (RLS filters it invisibly — same pattern as all other doc handlers).
6. `DELETE` idempotent: 204 even if row doesn't exist.
7. Audit event: `DocPageMentionAdded` → `"doc_page.mention_added"` — PII-safe metadata only.
8. `GET /v1/me/doc-page-mentions` requires `group_id` query param; returns only pages visible in that group.

---

## Out of scope

- `mentioned_task_id` / `mentioned_file_id` (future Embeds slice)
- Extending existing `GET /v1/me/mentions` to merge doc-page mentions (separate types, different shape)
- Notifications / push for doc mentions
- CRDT / WebSocket streaming for doc edits

---

## Rollback

Forward-only migration. No destructive ALTER. To revert: `DROP TABLE doc_page_mentions` (no downstream FK). Safe.

---

## M1 — Migration 029

- [x] Create `crates/garraia-workspace/migrations/029_doc_page_mentions.sql`
- [x] Table `doc_page_mentions` with FORCE RLS + NULLIF fail-closed
- [x] Index `doc_page_mentions_user_created_idx (mentioned_user_id, created_at DESC)`
- [x] GRANT SELECT, INSERT, DELETE TO garraia_app

## M2 — Audit action

- [x] Add `DocPageMentionAdded` variant to `WorkspaceAuditAction` in `garraia-auth/src/audit_workspace.rs`
- [x] Map to `"doc_page.mention_added"`
- [x] Add test assertion

## M3 — Handler `doc_mentions.rs`

- [x] `post_doc_page_mention` — POST 201 / idempotent 200
- [x] `list_doc_page_mentions` — GET cursor-paginated, DocsRead
- [x] `delete_doc_page_mention` — DELETE 204 idempotent, DocsWrite
- [x] 6 unit tests (response shapes, nil UUID, cursor)

## M4 — Inbox `GET /v1/me/doc-page-mentions` in `me.rs`

- [x] `list_my_doc_page_mentions` — cursor-paginated, requires `group_id` query param
- [x] `DocPageMentionSummary` response type
- [x] 6 unit tests

## M5 — Router wiring + OpenAPI

- [x] Wire routes in all 3 `mod.rs` branches (full / auth-stub / no-auth stub)
- [x] Register paths + schemas in `openapi.rs`

## M6 — ROADMAP + plans/README bookkeeping

- [x] Mark `[ ] doc_page_mentions` schema ✅ in ROADMAP §3.8 Tier 2
- [x] Add plan row to `plans/README.md`
- [x] Commit docs update

---

## Acceptance criteria

- Migration 029 applies cleanly.
- `cargo check -p garraia-workspace -p garraia-gateway` passes.
- `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean.
- Unit tests green (≥12 new tests).
- CI ≥16 checks green.

---

## Risk register

| Risk | Mitigation |
|------|-----------|
| `doc_pages.group_id` lookup fails under RLS | Pre-flight SELECT uses same tx with SET LOCAL already applied |
| Duplicate POST causes 409 | ON CONFLICT DO NOTHING → return existing row with 200 |
| Wrong migration number collision | Verified: 028 is latest |

---

## Cross-references

- Migration 022 (message_mentions) — same FORCE RLS pattern
- GAR-755 / plan 0237 — message mentions implementation
- GAR-840 / plan 0304 — doc_blocks (parent surface)
- ROADMAP §3.8 Tier 2 `doc_page_mentions` schema checklist
