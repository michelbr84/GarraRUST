# Plan 0092 — GAR-562 — Files REST API slice 5: folder POST + DELETE

## Goal

Add the two missing folder-mutation endpoints to the `/v1` REST surface:

* `POST   /v1/groups/{group_id}/folders`              — create a folder
* `DELETE /v1/groups/{group_id}/folders/{folder_id}`  — soft-delete a folder

Both were originally scoped for plan 0091 / PR #245 (full CRUD), but that PR was
superseded by PR #246 (PATCH-only, GAR-561). This plan delivers the deferred POST
and DELETE as a self-contained slice.

## Architecture

### POST handler (`create_folder`)

1. `require_group_id` + `check_group_match` (path ≠ principal.group_id → 403)
2. `can(&principal, Action::FilesWrite)` — 403 if denied
3. `validate_folder_name` — 400 on empty/>200 chars/contains `/`
4. Begin tx → `set_rls_context` (both `app.current_user_id` + `app.current_group_id`)
5. If `parent_id` supplied: SELECT to confirm it exists and is not soft-deleted (400 otherwise)
6. SELECT `display_name` from `users` for `created_by_label`
7. INSERT INTO `folders` RETURNING — map 23505 → 409 Conflict
8. Audit `folder.created` with `{ folder_id, group_id, name_len, has_parent }` (no raw names)
9. COMMIT → 201 + `FolderSummary`

### DELETE handler (`delete_folder`)

1. `require_group_id` + `check_group_match` → 403
2. `can(&principal, Action::FilesDelete)` → 403  ← **canonical project pattern**:
   matches `delete_file` from plan 0088 (the only existing soft-delete precedent
   in the `/v1` files surface). `FilesDelete` is granted to **Owner + Admin only**
   (NOT Member); `FilesWrite` would inherit Member, which is wrong for a destructive
   operation that can orphan children files. See review-side-by-side analysis 2026-05-09
   (PR #247 vs PR #248) for the rationale.
3. Begin tx → `set_rls_context`
4. SELECT `deleted_at, name` WHERE `id=$1 AND group_id=$2`
   - None → 404 (not found or cross-group)
   - Some, `deleted_at IS NOT NULL` → 204 idempotent (no audit, commit + return)
5. UPDATE `folders SET deleted_at=now()` WHERE `id=$1 AND deleted_at IS NULL`
6. Audit `folder.deleted` with `{ folder_id, group_id, name_len }` (no raw names)
7. COMMIT → 204

Children files/sub-folders are NOT cascade-deleted; they become root-level orphans.
Cascade semantics are deferred to a later slice.

## Tech stack

* Axum 0.8, `garraia-auth::Principal`
* `garraia-auth::Action::FilesWrite` (POST `create_folder`)
* `garraia-auth::Action::FilesDelete` (DELETE `delete_folder` — Owner/Admin only)
* `sqlx` (Postgres), `set_rls_context` for FORCE RLS compliance
* `utoipa` annotations (`#[utoipa::path(...)]`), registered in `openapi.rs`
* `garraia-auth::audit_workspace` — two new variants: `FolderCreated`, `FolderDeleted`

## Design invariants

* FORCE RLS protocol: `SET LOCAL app.current_user_id` AND `app.current_group_id`
  before every SQL operation — both must be set for FORCE RLS tables.
* PII-free audit: metadata carries `name_len` + `has_parent` booleans, never raw
  folder names.
* Idempotent DELETE: already-deleted folders return 204 without re-emitting an audit
  event (mirrors `delete_file` from plan 0088).
* No cascade: orphan children are acceptable for slice 5; cascade is slice 6+.

## Validações pré-plano

* PR #246 (GAR-561, PATCH-only) merged as `3679ccc` — PATCH `patch_folder` present.
* `validate_folder_name` (max 200 chars, no `/`, no NUL) already tested in PR #246.
* `FolderSummary` struct already present; fields: `id`, `name`, `parent_id`,
  `created_by`, `created_by_label`, `created_at`, `updated_at`.
* `folders_unique_name_per_parent_idx UNIQUE (group_id, COALESCE(parent_id, nil_uuid), name) WHERE deleted_at IS NULL` — 23505 maps to 409.

## Out of scope

* Cascade-delete of child folders/files.
* Moving a folder (`parent_id` change via PATCH is slice 6).
* Folder DELETE auth differentiation beyond Owner/Admin (`FilesDelete`) is
  intentional — Members can rename and create folders but NOT delete them.

## Rollback

* `mod.rs` router entries for `POST` and `DELETE` removed.
* `files.rs` handlers `create_folder` + `delete_folder` deleted.
* `openapi.rs` paths + schema registration reverted.
* `audit_workspace.rs` variants `FolderCreated` + `FolderDeleted` removed.
* Integration test file `rest_v1_folders_post_delete.rs` deleted.
* DB schema unchanged (soft-delete is reversible by clearing `deleted_at`).

## File structure

```text
crates/garraia-auth/src/audit_workspace.rs        — FolderCreated + FolderDeleted variants
crates/garraia-gateway/src/rest_v1/files.rs       — create_folder + delete_folder handlers
crates/garraia-gateway/src/rest_v1/mod.rs         — router wiring (3 modes)
crates/garraia-gateway/src/rest_v1/openapi.rs     — paths + schemas registration
crates/garraia-gateway/tests/rest_v1_folders_post_delete.rs  — 13 integration scenarios
```

## Tasks

- [x] M1-T1: `audit_workspace.rs` — add `FolderCreated` + `FolderDeleted` variants + `as_str` arms + tests
- [x] M1-T2: `files.rs` — add `CreateFolderRequest` DTO + `create_folder` handler
- [x] M1-T3: `files.rs` — add `delete_folder` handler
- [x] M1-T4: `mod.rs` — wire POST + DELETE in all 3 router build modes
- [x] M1-T5: `openapi.rs` — register `create_folder`, `delete_folder` in paths; `CreateFolderRequest` in schemas
- [x] M1-T6: `rest_v1_folders_post_delete.rs` — 13 scenarios (C1–C8 POST + D1–D5 DELETE)
- [x] M1-T7: `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean
- [x] M1-T8: `cargo fmt --check` clean
- [ ] M1-T9: commit + push → PR + CI green → merge

## Risk register

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| 23505 on UNIQUE index not mapped in test environment | Low | Test C8 exercises this path |
| RLS blocks cross-group SELECT due to set_rls_context filtering | Medium | D4 tests cross-group 404 |
| parent_id from different group sneaks through | Low | C6 validates app-layer SELECT within same tx |

## Acceptance criteria

1. `POST /v1/groups/{group_id}/folders` returns 201 with `FolderSummary` body; DB row inserted; audit `folder.created` emitted.
2. `DELETE /v1/groups/{group_id}/folders/{folder_id}` returns 204; DB row has `deleted_at` set; audit `folder.deleted` emitted.
3. Idempotent DELETE: already-deleted folder returns 204, no second audit row.
4. All 13 integration scenarios pass in CI (Postgres 16).
5. Zero `cargo clippy` warnings (workspace + tests).
6. `cargo fmt --check` clean.

## Cross-references

* Plan 0091 (GAR-561) — PATCH folder rename (shipped as PR #246, `3679ccc`)
* Plan 0088 (GAR-555) — `delete_file` (idempotent pattern used as model)
* ADR 0003 — Postgres for Group Workspace
* ADR 0004 — Object Storage (files schema in migration 003)

## Estimativa

~200 LOC production + ~300 LOC tests. Estimated 1 session.

## Linear issue

[GAR-562](https://linear.app/chatgpt25/issue/GAR-562)
