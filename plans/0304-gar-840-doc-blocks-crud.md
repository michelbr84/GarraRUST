# Plan 0302 — GAR-840: Docs Tier 2 blocks CRUD (migration 027 + 4 endpoints)

**Status:** In Progress
**Linear:** [GAR-840](https://linear.app/chatgpt25/issue/GAR-840)
**Branch:** `routine/202606100620-doc-blocks-crud`
**Prerequisite:** GAR-835 (migration 026 `doc_pages`) + GAR-837 (doc-pages single CRUD)

---

## Goal

Add block-level content to the Docs Tier 2 surface (ROADMAP §3.8 Tier 2):
- Migration 027: `doc_blocks` table under FORCE RLS
- `POST /v1/doc-pages/{page_id}/blocks` — create a block
- `GET /v1/doc-pages/{page_id}/blocks` — list blocks (position-ordered)
- `PATCH /v1/doc-blocks/{block_id}` — update content/position/type
- `DELETE /v1/doc-blocks/{block_id}` — hard delete

## Architecture

### Schema (migration 027)

```sql
CREATE TABLE doc_blocks (
    id               UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    page_id          UUID        NOT NULL REFERENCES doc_pages(id) ON DELETE CASCADE,
    group_id         UUID        NOT NULL,
    -- compound FK for RLS integrity (doc_pages has UNIQUE(id, group_id))
    FOREIGN KEY (page_id, group_id) REFERENCES doc_pages(id, group_id) ON DELETE CASCADE,
    parent_block_id  UUID        REFERENCES doc_blocks(id) ON DELETE SET NULL,
    position         FLOAT8      NOT NULL DEFAULT 0,
    block_type       TEXT        NOT NULL CHECK(block_type IN (
                         'heading','paragraph','todo','bullet','numbered',
                         'code','quote','callout','divider',
                         'file_embed','task_embed','chat_embed','image')),
    content_jsonb    JSONB       NOT NULL DEFAULT '{}',
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

FORCE RLS: `group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid`

### Tenant-context protocol

Same pattern as `doc_pages`: `SET LOCAL app.current_user_id` + `SET LOCAL app.current_group_id`
via parameterised `set_config` before any SQL in every transaction.

### Authz

- `POST`, `PATCH`, `DELETE` → `Action::DocsWrite`
- `GET` → `Action::DocsRead`
- `DELETE` also accepts admins via `Action::DocsDelete` (Owner/Admin only)

### Audit events

- `DocBlockCreated` → `doc_block.created` (block_type, position)
- `DocBlockUpdated` → `doc_block.updated` (fields_updated list)
- `DocBlockDeleted` → `doc_block.deleted` (block_type)

## Tech stack

- Rust: `sqlx`, `axum`, `utoipa`, `serde`, `uuid`, `chrono`
- Pattern: identical to `rest_v1/docs.rs` (plan 0297)
- File: `crates/garraia-gateway/src/rest_v1/doc_blocks.rs` (new)
- Audit: `garraia_auth::WorkspaceAuditAction` (add 3 new variants)

## Design invariants

- NO `unwrap()` outside tests
- NO SQL string concatenation — use parameterised `sqlx::query_as`
- SET LOCAL BOTH `app.current_user_id` AND `app.current_group_id` before every query
- Cross-group test: `page_id` from group A returns 404 when caller is group B (RLS filters)
- PII-safe audit metadata: `block_type` and `position` only, never `content_jsonb`
- `position` default: `COALESCE((SELECT MAX(position) FROM doc_blocks WHERE page_id = $1 AND group_id = $2), 0) + 1.0`

## Out of scope

- `doc_page_versions` (future slice)
- Real-time CRDT / WebSocket (ADR 0008 not yet Accepted)
- FTS indexing on `content_jsonb` (future slice after ADR 0006)
- `GET /v1/doc-pages/{page_id}` with embedded blocks (follow-up parameter on existing endpoint)

## Rollback

Migration 027 is forward-only. To roll back: `DROP TABLE doc_blocks CASCADE` — safe since no existing data references it.

## M1: Tasks

- [x] T1: Migration 027 (`027_doc_blocks.sql`)
- [x] T2: Add `DocBlockCreated/Updated/Deleted` to `WorkspaceAuditAction` in `garraia-auth`
- [x] T3: `doc_blocks.rs` handler — `create_doc_block` (POST 201)
- [x] T4: `doc_blocks.rs` handler — `list_doc_blocks` (GET 200)
- [x] T5: `doc_blocks.rs` handler — `update_doc_block` (PATCH 200)
- [x] T6: `doc_blocks.rs` handler — `delete_doc_block` (DELETE 204)
- [x] T7: Router wiring + OpenAPI registration
- [x] T8: Unit tests (≥ 8), plans/README.md + ROADMAP.md update

## Risk register

| Risk | Mitigation |
|---|---|
| compound FK syntax varies in sqlx | Test migration in testcontainer CI |
| `position` float ordering ties | Secondary sort by `id ASC` |
| Cross-group isolation via RLS | Integration test in existing test-helpers pattern |

## Acceptance criteria

- [ ] `cargo check -p garraia-workspace` — clean (migration compiles)
- [ ] `cargo check -p garraia-auth` — clean (new audit variants)
- [ ] `cargo check -p garraia-gateway` — clean
- [ ] `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` — 0 warnings
- [ ] `cargo test -p garraia-auth` — all pass (new audit variant tests)
- [ ] `cargo test -p garraia-gateway rest_v1::doc_blocks` — all pass
- [ ] CI 20/20 green

## Cross-references

- Prerequisite: GAR-835 / plan 0297 (migration 026 `doc_pages`)
- Prerequisite: GAR-837 / plan 0301 (doc-pages single CRUD)
- ROADMAP §3.8 Tier 2

## Estimativa

0.5 / 1 / 1.5 days (shovel-ready — pattern established in plan 0297/0301)
