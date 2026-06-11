# Plan 0307 — GAR-845: Docs Tier 2 page versions (migration 028 + 3 endpoints)

**Status:** In Progress
**Linear:** [GAR-845](https://linear.app/chatgpt25/issue/GAR-845)
**Branch:** `routine/202606101815-doc-page-versions`
**Prerequisite:** GAR-840 (migration 027 `doc_blocks`) + GAR-837 (doc-pages single CRUD)

---

## Goal

Add version history to the Docs Tier 2 surface (ROADMAP §3.8 Tier 2):
- Migration 028: `doc_page_versions` table under FORCE RLS
- `POST /v1/doc-pages/{page_id}/versions` — manual snapshot (captures page + blocks)
- `GET  /v1/doc-pages/{page_id}/versions` — list version headers (cursor-paginated, no snapshot body)
- `GET  /v1/doc-pages/{page_id}/versions/{version_id}` — single version with full snapshot

## Architecture

### Schema (migration 028)

```sql
CREATE TABLE doc_page_versions (
    id               UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    page_id          UUID        NOT NULL REFERENCES doc_pages(id) ON DELETE CASCADE,
    group_id         UUID        NOT NULL,
    -- Compound FK: ensures page belongs to the same group.
    FOREIGN KEY (page_id, group_id) REFERENCES doc_pages(id, group_id) ON DELETE CASCADE,
    snapshot_jsonb   JSONB       NOT NULL,   -- {title, icon, parent_page_id, blocks:[...]}
    created_by       UUID        NOT NULL,   -- plain uuid, no FK (survives user deletion)
    created_by_label TEXT        NOT NULL,   -- display_name cache
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

FORCE RLS: `group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid`

### Snapshot content

`snapshot_jsonb` stores a point-in-time snapshot of the page:
```json
{
  "title": "Meeting notes",
  "icon": "📝",
  "parent_page_id": null,
  "blocks": [
    {"id": "...", "type": "heading", "position": 1.0, "content": {...}},
    {"id": "...", "type": "paragraph", "position": 2.0, "content": {...}}
  ]
}
```

### Authz

- `POST` → `Action::DocsWrite`
- `GET` (list + single) → `Action::DocsRead`

### Audit events

- `DocPageVersionCreated` → `"doc_page.version_created"` metadata: `{block_count: N}`

### Tenant-context protocol

Same pattern as `doc_blocks`: both RLS vars set via parameterised `set_config` in every transaction.

### Cross-group isolation

`page_id` is resolved inside the caller's RLS context. Cross-group `page_id` → 0 rows → 404.

## Pagination (GET list)

Cursor = `(created_at DESC, id DESC)` — consistent with other list endpoints.
Default limit: 20. Max: 100.
Response: `{items: [...headers...], next_cursor: <uuid | null>}`.

## Validations pré-plano

- [x] Migration 027 exists: `crates/garraia-workspace/migrations/027_doc_blocks.sql`
- [x] `doc_pages` has `UNIQUE(id, group_id)` — compound FK valid
- [x] `Action::DocsRead`, `Action::DocsWrite` already exist in `garraia-auth::action`
- [x] `audit_workspace_event` is available in `garraia-auth`
- [x] `RestV1FullState`, `RestError`, `Principal` patterns established in `doc_blocks.rs`

## Out of scope

- Auto-snapshot on PATCH (deferido — would touch GAR-837 code)
- Version restore endpoint (`POST /v1/doc-pages/{page_id}/versions/{id}/restore`)
- `doc_page_mentions` table
- `POST /v1/doc-pages/{page_id}/duplicate`

## File structure

```
crates/garraia-workspace/migrations/028_doc_page_versions.sql   (new)
crates/garraia-gateway/src/rest_v1/doc_versions.rs              (new)
crates/garraia-gateway/src/rest_v1/mod.rs                       (add routes + OpenAPI)
crates/garraia-auth/src/audit_workspace.rs                      (add DocPageVersionCreated)
plans/0307-gar-845-doc-page-versions.md                         (this file)
plans/README.md                                                  (add row)
```

## M1 Tasks

- [x] T1: Write migration 028 (`doc_page_versions` + FORCE RLS + index)
- [x] T2: Add `DocPageVersionCreated` to `WorkspaceAuditAction` in `garraia-auth`
- [x] T3: Write `doc_versions.rs` handler (3 endpoints + tests)
- [x] T4: Wire routes + OpenAPI in `rest_v1/mod.rs`
- [ ] T5: `cargo check -p garraia-gateway && cargo clippy --workspace -D warnings`
- [ ] T6: Update plans/README.md + commit

## Acceptance criteria

- `POST /v1/doc-pages/{page_id}/versions` returns 201 with `{id, page_id, created_by, created_by_label, created_at}` (no snapshot in response)
- `GET /v1/doc-pages/{page_id}/versions` returns cursor-paginated list of version headers
- `GET /v1/doc-pages/{page_id}/versions/{version_id}` returns full version including `snapshot`
- Cross-group `page_id` → 404 (RLS filter)
- Unknown `version_id` → 404
- All unit tests pass
- `cargo clippy --workspace -D warnings` clean

## Cross-references

- ROADMAP §3.8 Tier 2: `doc_page_versions` + `GET /v1/doc-pages/{page_id}/versions`
- GAR-835 (migration 026), GAR-837 (single-page CRUD), GAR-840 (migration 027 blocks CRUD)
- Plan 0304 (GAR-840) — template for handler pattern

## Estimativa

~350 LOC implementation + ~150 LOC tests. ~2h.
