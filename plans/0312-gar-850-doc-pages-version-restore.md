# Plan 0312 — GAR-850: Docs Tier 2 version restore (POST /v1/doc-pages/{page_id}/versions/{version_id}/restore)

**Status:** In Progress
**Linear:** [GAR-850](https://linear.app/chatgpt25/issue/GAR-850)
**Branch:** `routine/202606111634-doc-pages-version-restore`
**Prerequisite:** GAR-845 (migration 028 `doc_page_versions`) + GAR-847 (duplicate handler) — same series

---

## Goal

Add `POST /v1/doc-pages/{page_id}/versions/{version_id}/restore` — restores a doc page to a
previously snapshotted state by applying the version's `snapshot_jsonb` back to `doc_pages`
and `doc_blocks` (ROADMAP §3.8 Tier 2).

No new migration — reuses `doc_pages`, `doc_blocks`, `doc_page_versions` tables already shipped.

## Architecture

### Endpoint

`POST /v1/doc-pages/{page_id}/versions/{version_id}/restore`

- Authz: `Action::DocsWrite`
- Source page resolved inside caller's RLS context → cross-group = 404
- Version must belong to the given page (`page_id` checked) → cross-page = 404
- Restore operation (all in one TX):
  - UPDATE `doc_pages` SET `title`, `icon`, `parent_page_id`, `updated_at = NOW()`
  - DELETE FROM `doc_blocks` WHERE `page_id = $1`
  - INSERT INTO `doc_blocks` for each block in snapshot with `gen_random_uuid()` as new id
    - `parent_block_id` preserved from snapshot (may be dangling old UUID — known limitation)
    - `page_id`, `group_id`, `position`, `block_type`, `content_jsonb`, `created_by`, `created_by_label` from snapshot
- Returns `200 OK` + `DocPageResponse` (updated page)

### Snapshot format (from plan 0307 / GAR-845)

```json
{
  "title": "...",
  "icon": "...",
  "parent_page_id": "...",
  "blocks": [
    {
      "id": "<original-uuid>",
      "parent_block_id": null,
      "type": "paragraph",
      "position": 1.0,
      "content": { "text": "..." }
    }
  ]
}
```

Keys: `"type"` (not `"block_type"`), `"content"` (not `"content_jsonb"`).

### parent_block_id behaviour

`parent_block_id` values from snapshot point to old block UUIDs. Since all blocks are
re-inserted with new UUIDs, these references become dangling. This matches the same
deferred-remapping scope as the duplicate endpoint (plan 0309). Documented limitation.

### Audit event

`DocPageVersionRestored` → `"doc_page.version_restored"`
- `resource_type = "doc_page_versions"`, `resource_id = "{version_id}"`
- metadata: `{ source_version_id: UUID, block_count: N }`

### Tenant-context protocol

Same as all Docs handlers: `set_config('app.current_user_id', ...)` +
`set_config('app.current_group_id', ...)` in every transaction.

### Error matrix

| Condition | Status |
|---|---|
| Missing/invalid JWT | 401 |
| Caller not a group member | 403 |
| Missing X-Group-Id header | 400 |
| Page not found / cross-group | 404 |
| Version not found / belongs to different page | 404 |
| Happy path | 200 |

## Validations pré-plano

- [x] `doc_pages` exists (migration 026, GAR-835)
- [x] `doc_blocks` exists (migration 027, GAR-840)
- [x] `doc_page_versions` exists (migration 028, GAR-845); GRANT SELECT, INSERT only (no UPDATE/DELETE)
- [x] `Action::DocsWrite` exists in `garraia-auth::action`
- [x] `audit_workspace_event` available in `garraia-auth`
- [x] `DocPageResponse` + handler patterns established in `docs.rs` and `doc_versions.rs`
- [x] Snapshot format confirmed in `doc_versions.rs` (keys `"type"`, `"content"`)
- [x] Plan 0307 (GAR-845) is the canonical handler template
- [x] Plan 0309 (GAR-847) confirms parent_block_id deferred-remapping precedent

## Out of scope

- `parent_block_id` UUID remapping in restored blocks (deferred — requires two-pass insert)
- Creating a new version snapshot of the CURRENT state before restoring (auto-backup — deferred)
- Restoring into a different page (deferred)

## File structure

```
crates/garraia-gateway/src/rest_v1/doc_versions.rs    (add restore_doc_page_version handler)
crates/garraia-gateway/src/rest_v1/mod.rs             (add route in all 3 modes + OpenAPI)
crates/garraia-gateway/src/rest_v1/openapi.rs         (add restore_doc_page_version path)
crates/garraia-auth/src/audit_workspace.rs            (add DocPageVersionRestored)
plans/0312-gar-850-doc-pages-version-restore.md       (this file)
plans/README.md                                       (add row + mark 0309 merged)
```

## M1 Tasks

- [ ] T1: Add `DocPageVersionRestored` to `WorkspaceAuditAction` in `garraia-auth`
- [ ] T2: Write `restore_doc_page_version` handler in `doc_versions.rs` (restore page + blocks + audit)
- [ ] T3: Wire route + OpenAPI in `rest_v1/mod.rs` and `openapi.rs`
- [ ] T4: `cargo check -p garraia-gateway && cargo clippy --workspace -D warnings`
- [ ] T5: Update `plans/README.md` + commit

## Acceptance criteria

- `POST /v1/doc-pages/{page_id}/versions/{version_id}/restore` returns 200 with DocPageResponse
- Page title/icon/parent_page_id match the restored snapshot
- Block count in page matches block count in snapshot
- `parent_block_id` = dangling old UUID (known; no remapping)
- Cross-group `page_id` → 404 (RLS)
- Version belonging to a different page → 404
- `DocPageVersionRestored` audit event emitted with correct metadata
- All unit tests pass
- `cargo clippy --workspace -D warnings` clean

## Cross-references

- ROADMAP §3.8 Tier 2: `POST /v1/doc-pages/{page_id}/versions/{version_id}:restore` (TODO)
- GAR-835 (migration 026), GAR-840 (migration 027 blocks), GAR-845 (migration 028 versions)
- Plan 0307 (GAR-845) — canonical handler template
- Plan 0309 (GAR-847) — duplicate endpoint, same parent_block_id deferred-remapping scope

## Risk register

| Risk | Likelihood | Mitigation |
|---|---|---|
| Large page with many blocks exceeds TX timeout | Low | Block insert uses single bulk INSERT loop in TX |
| `parent_block_id` dangling breaks client rendering | Low | Documented scope limitation; matches duplicate behaviour |
| Snapshot JSON parse fails on malformed data | Very Low | `serde_json::Value` parse with explicit field access + 500 if malformed |

## Estimativa

~250 LOC implementation + ~80 LOC tests. ~1.5h.
