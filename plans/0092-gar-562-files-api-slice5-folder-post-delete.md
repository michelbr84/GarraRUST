# Plan 0092 — GAR-562: Files REST API slice 5 — POST + DELETE folders

**Issue:** [GAR-562](https://linear.app/chatgpt25/issue/GAR-562)
**Parent epic:** `epic:ws-api` / Fase 3.4
**Branch:** `routine/202605091815-gar562-folder-post-delete`
**Date:** 2026-05-09 (America/New_York)

---

## §1 Goal

Ship the two remaining folder-mutation endpoints deferred from GAR-561 (plan 0091):

- `POST /v1/groups/{group_id}/folders` — create a folder (top-level or nested) → 201 `FolderSummary`
- `DELETE /v1/groups/{group_id}/folders/{folder_id}` — soft-delete a folder → 204

This completes the folder management surface established by plan 0088 (GET folders) and plan 0091 (PATCH rename). The schema (`folders` table, migration 003) and RLS policies are already in place.

---

## §2 Architecture

No new crates, no new migrations. All changes are additive within:

- `crates/garraia-auth/src/audit_workspace.rs` — two new `WorkspaceAuditAction` variants
- `crates/garraia-gateway/src/rest_v1/files.rs` — `CreateFolderRequest` DTO + two handlers
- `crates/garraia-gateway/src/rest_v1/mod.rs` — POST + DELETE routes wired in 3 router modes
- `crates/garraia-gateway/src/rest_v1/openapi.rs` — `create_folder` + `delete_folder` paths
- `crates/garraia-gateway/tests/rest_v1_folders_post_delete.rs` — integration suite

---

## §3 Tech stack

Same as plan 0091: `axum 0.8`, `sqlx 0.8`, `utoipa`, `garraia-auth` pool newtype, Postgres 16 FORCE RLS, pgvector testcontainer.

---

## §4 Design invariants

1. **No new migration** — `folders` table exists since migration 003 (`compound FK`, `parent_id`, `deleted_at` soft-delete, `FORCE RLS`).
2. **FORCE RLS** — `set_rls_context` (SET LOCAL `app.current_user_id` + `app.current_group_id`) before every SQL statement.
3. **`parent_id` validation** — if supplied, verify parent folder exists in same group and is not soft-deleted (404 if not).
4. **PII-safe audit** — metadata carries `name_len` not the raw folder name; `FolderCreated` metadata = `{ name_len, group_id, has_parent }`.
5. **Soft-delete only** — `DELETE` sets `deleted_at = now()`; children files become orphans (visible at root); no cascade.
6. **Auth gates** — POST requires `Action::FilesWrite`; DELETE requires `Action::FilesDelete`.
7. **Cross-group = 403** — path `group_id` ≠ principal group → 403 (same as all other files endpoints).
8. **Soft-deleted folder not found** — DELETE on already-soft-deleted or non-existent → 404.
9. **Zero `unwrap` / `expect` in production** — propagate via `?`.
10. **`validate_file_name`** is reused for folder name validation (1..=500 chars, no `/`, no NUL) — same as file names, matches `validate_folder_name` but that fn already exists in `files.rs` for the PATCH.

---

## §5 Out of scope

- Folder move (`parent_id` mutation)
- Cascading soft-delete of children
- Hard delete
- Undelete / restore
- `POST /v1/groups/{group_id}/files:initUpload` (separate slice)
- `GET /v1/files/{file_id}:download` (separate slice)

---

## §6 Rollback

Revert the squash commit. No DB migrations to roll back. The new `FolderCreated` and `FolderDeleted` enum variants are additive — removing them later only matters if any audit consumer dispatches on them (none today).

---

## §7 Validações pré-plano

- [x] `folders` table present since migration 003 with `parent_id`, `deleted_at`, FORCE RLS ✅
- [x] `validate_folder_name` function present in `files.rs` ✅
- [x] `set_rls_context` helper present in `files.rs` ✅
- [x] `Action::FilesWrite` + `Action::FilesDelete` defined in `garraia-auth` ✅
- [x] `audit_workspace_event` helper available ✅
- [x] `FolderRenamed` variant already merged (plan 0091) — adding `FolderCreated` / `FolderDeleted` is additive ✅

---

## §8 File structure

```
crates/garraia-auth/src/audit_workspace.rs   ← +FolderCreated, +FolderDeleted variants
crates/garraia-gateway/src/rest_v1/files.rs  ← +CreateFolderRequest, +create_folder, +delete_folder
crates/garraia-gateway/src/rest_v1/mod.rs    ← POST + DELETE routes wired
crates/garraia-gateway/src/rest_v1/openapi.rs← +create_folder path, +delete_folder path, +CreateFolderRequest schema
crates/garraia-gateway/tests/rest_v1_folders_post_delete.rs  ← NEW integration test
plans/0092-gar-562-files-api-slice5-folder-post-delete.md    ← this file
plans/README.md                              ← +row 0092 + mark 0091 merged
```

---

## §9 M1 tasks (TDD)

- [x] T1 — `FolderCreated` + `FolderDeleted` audit variants + `as_str` arms + tests
- [x] T2 — `CreateFolderRequest` DTO + `create_folder` handler
- [x] T3 — `delete_folder` handler
- [x] T4 — Wire POST + DELETE routes in all 3 router modes (mod.rs)
- [x] T5 — OpenAPI paths + schema (openapi.rs)
- [x] T6 — Integration test `rest_v1_folders_post_delete.rs` (10 scenarios)
- [x] T7 — `cargo clippy` clean + `cargo fmt`
- [x] T8 — Update `plans/README.md` + `ROADMAP.md`

---

## §10 Integration scenarios (test_id → description → expected)

| # | Endpoint | Scenario | Expected |
|---|----------|----------|----------|
| C1 | POST folders | Create root folder (no parent_id) | 201 + FolderSummary (parent_id null) |
| C2 | POST folders | Create nested folder (valid parent_id) | 201 + FolderSummary (parent_id echoed) |
| C3 | POST folders | path group_id ≠ principal | 403 |
| C4 | POST folders | name > 500 chars | 400 |
| C5 | POST folders | parent_id points to soft-deleted folder | 404 |
| D1 | DELETE folders | Soft-delete live folder | 204, deleted_at set |
| D2 | DELETE folders | Delete already-soft-deleted folder | 404 |
| D3 | DELETE folders | Delete non-existent folder_id | 404 |
| D4 | DELETE folders | path group_id ≠ principal | 403 |
| D5 | DELETE folders | Audit row present after delete | 204 + audit entry `folder.deleted` |

---

## §11 Risk register

| Risk | Severity | Mitigation |
|------|----------|-----------|
| `parent_id` validation misses RLS context | Med | `set_rls_context` called before parent lookup query |
| Soft-deleted parent accepted as valid | Med | `AND deleted_at IS NULL` in parent existence check |
| `FolderCreated` metadata leaks name | Low | Only `name_len`, `group_id`, `has_parent` in JSON |

---

## §12 Acceptance criteria

- `POST /v1/groups/{group_id}/folders` returns 201 with `FolderSummary` and audit row `folder.created`
- `DELETE /v1/groups/{group_id}/folders/{folder_id}` returns 204 and sets `deleted_at`
- All 10 integration scenarios pass
- `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean
- CI 18/18 green

---

## §13 Cross-references

- Plan 0088 (GAR-555): Files slice 1 — GET files + GET folders + DELETE file
- Plan 0089 (GAR-557): Files slice 2 — PATCH file rename
- Plan 0090 (GAR-559): Files slice 3 — GET single file + folder
- Plan 0091 (GAR-561): Files slice 4 — PATCH folder rename
- Migration 003 (GAR-387): `folders` / `files` / `file_versions` schema

---

## §14 Estimativa

- Low: 1h
- Probable: 2h
- High: 3h
