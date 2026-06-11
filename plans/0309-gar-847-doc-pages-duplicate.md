# Plan 0309 — GAR-847: Docs Tier 2 page duplication (POST /v1/doc-pages/{page_id}/duplicate)

**Status:** In Progress
**Linear:** [GAR-847](https://linear.app/chatgpt25/issue/GAR-847)
**Branch:** `routine/202606110018-doc-pages-duplicate`
**Prerequisite:** GAR-845 (migration 028 `doc_page_versions`) — same series, not a hard blocker

---

## Goal

Add `POST /v1/doc-pages/{page_id}/duplicate` — deep-copies a doc page and all its blocks
into a new page in the same group (ROADMAP §3.8 Tier 2).

No new migration — reuses `doc_pages` + `doc_blocks` tables already shipped.

## Architecture

### Endpoint

`POST /v1/doc-pages/{page_id}/duplicate`

- Authz: `Action::DocsWrite`
- Source page resolved inside caller's RLS context → cross-group = 404
- New page:
  - `title = "{original} (copy)"`
  - `icon`, `parent_page_id` copied verbatim
  - `group_id = principal.group_id` (same group)
  - `created_by = principal.user_id`, `created_by_label = display_name`
- All `doc_blocks` rows from source are deep-copied:
  - New `id`s (gen_random_uuid)
  - `page_id` → new page id
  - `group_id`, `parent_block_id` (NULL for now — parent_block_id remapping deferred),
    `position`, `block_type`, `content_jsonb`, `created_by`, `created_by_label` copied verbatim
- Returns `201 Created` + `DocPageResponse` (same shape as POST create)

### parent_block_id remapping

Deep-copying nested blocks (where `parent_block_id` points to another block in the same page)
requires a UUID mapping pass. This is intentionally deferred — for now `parent_block_id` is
set to NULL in the copy. This matches the scope of this slice.

### Audit event

`DocPageDuplicated` → `"doc_page.duplicated"`
- metadata: `{ source_page_id: UUID, block_count: N }`

### Tenant-context protocol

Same as all Docs handlers: `set_config('app.current_user_id', ...)` +
`set_config('app.current_group_id', ...)` in every transaction.

### Error matrix

| Condition | Status |
|---|---|
| Missing/invalid JWT | 401 |
| Caller not a group member | 403 |
| Missing X-Group-Id header | 400 |
| Source page not found / cross-group | 404 |
| Happy path | 201 |

## Validations pré-plano

- [x] `doc_pages` exists (migration 026, GAR-835)
- [x] `doc_blocks` exists (migration 027, GAR-840)
- [x] `Action::DocsWrite` exists in `garraia-auth::action`
- [x] `audit_workspace_event` available in `garraia-auth`
- [x] `DocPageResponse` + `create_doc_page` patterns established in `docs.rs`
- [x] Plan 0307 (GAR-845) is the canonical handler template

## Out of scope

- `parent_block_id` remapping in copies (deferred — would require two-pass insert)
- Auto-link from new page back to source (deferred)
- Duplicate into a different parent page (path param `?parent_id=...` — deferred)
- `doc_page_mentions` table

## File structure

```
crates/garraia-gateway/src/rest_v1/docs.rs         (add duplicate_doc_page handler)
crates/garraia-gateway/src/rest_v1/mod.rs          (add route + OpenAPI)
crates/garraia-auth/src/audit_workspace.rs         (add DocPageDuplicated)
plans/0309-gar-847-doc-pages-duplicate.md          (this file)
plans/README.md                                    (add row)
```

## M1 Tasks

- [ ] T1: Add `DocPageDuplicated` to `WorkspaceAuditAction` in `garraia-auth`
- [ ] T2: Write `duplicate_doc_page` handler in `docs.rs` (copy page + blocks + audit)
- [ ] T3: Wire route + OpenAPI in `rest_v1/mod.rs`
- [ ] T4: `cargo check -p garraia-gateway && cargo clippy --workspace -D warnings`
- [ ] T5: Update `plans/README.md` + commit

## Acceptance criteria

- `POST /v1/doc-pages/{page_id}/duplicate` returns 201 with new page UUID ≠ source
- New page title = `"{original} (copy)"`
- Block count in new page matches block count in source
- `parent_block_id` = NULL in all copied blocks
- Cross-group `page_id` → 404 (RLS)
- `DocPageDuplicated` audit event emitted with correct metadata
- All unit tests pass
- `cargo clippy --workspace -D warnings` clean

## Cross-references

- ROADMAP §3.8 Tier 2: `POST /v1/doc-pages/{page_id}:duplicate` (listed as TODO)
- GAR-835 (migration 026), GAR-840 (migration 027 blocks), GAR-845 (migration 028 versions)
- Plan 0307 (GAR-845) — canonical handler template
- GAR-544 precedent: `:move` → `/move` Axum convention

## Risk register

| Risk | Likelihood | Mitigation |
|---|---|---|
| Large page with many blocks exceeds TX timeout | Low | Block copy uses single bulk INSERT … SELECT |
| `parent_block_id` null breaks client rendering | Low | Documented scope limitation; flat copy is valid |

## Estimativa

~200 LOC implementation + ~100 LOC tests. ~1.5h.
