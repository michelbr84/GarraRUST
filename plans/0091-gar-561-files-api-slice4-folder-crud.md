# Plan 0091 — Files REST API slice 4: Folder CRUD

**GAR-561** · epic:ws-api · Fase 3.4 "Arquivos"
**Branch:** `routine/202605091645-files-slice4-folder-crud`
**Date:** 2026-05-09 (America/New_York)

---

## Goal

Land slice 4 of the files REST surface: three write endpoints on the
`folders` table that mirror the file rename + delete pattern from slices
2–3.

- `POST /v1/groups/{group_id}/folders` → 201 `FolderSummary` | 403 | 422
- `PATCH /v1/groups/{group_id}/folders/{folder_id}` → 200 `FolderSummary` | 403 | 404 | 422
- `DELETE /v1/groups/{group_id}/folders/{folder_id}` → 204 | 403 | 404

Schema (`folders`) and FORCE RLS live in migration 003 (GAR-387).
`Action::FilesWrite`, `Action::FilesDelete`, `check_group_match`,
`set_rls_context`, `FolderRow`, `FolderSummary` all exist from slices
1–3. New: `CreateFolderRequest`, `PatchFolderRequest` DTOs and three
audit variants in `WorkspaceAuditAction`.

---

## Architecture

- **Audit events:** POST → `folder.created`, PATCH → `folder.renamed`,
  DELETE → `folder.deleted` (PII-safe: `name_len` not literal name).
- **SQL — POST:** `INSERT INTO folders (id, group_id, parent_id, name, created_by, created_by_label) VALUES ($1,$2,$3,$4,$5,$6) RETURNING ...`
  — parent_id validation: if `parent_id` is set, verify it exists in
  the same group and is not soft-deleted.
- **SQL — PATCH:** `UPDATE folders SET name=$1, updated_at=now() WHERE id=$2 AND group_id=$3 AND deleted_at IS NULL RETURNING ...`
- **SQL — DELETE:** `UPDATE folders SET deleted_at=now() WHERE id=$1 AND group_id=$2 AND deleted_at IS NULL` — 0 rows → 404.
- **Soft-delete invariant:** files with `folder_id` pointing to a
  deleted folder are NOT cascaded; they become orphans visible in the
  group root listing. Recoverable by assigning a new `folder_id`.
- **Route collision:** `GET` and `DELETE` on the same
  `/folders/{folder_id}` path are chained:
  `get(files::get_folder).patch(files::patch_folder).delete(files::delete_folder)`.

---

## Tech stack

Unchanged from slices 1–3: Axum 0.8, sqlx, utoipa, garraia-auth
`Principal`, `WorkspaceAuditAction`.

---

## Design invariants

- NEVER read `deleted_at IS NOT NULL` rows — treat as 404.
- NEVER expose cross-group folders — RLS + `check_group_match`.
- SET LOCAL both `app.current_user_id` AND `app.current_group_id`.
- PII-safe audit: `name_len: usize`, NOT the literal folder name.
- `parent_id` validation is same-group guard, not recursive cycle check
  (cycle detection deferred — depth limit is not enforced in this
  slice).

---

## Out of scope

- Folder move (change `parent_id`) — slice 5+.
- Cascading soft-delete of children files or subfolders.
- Hard delete / restore from trash.
- Cycle detection for nested folder graphs.

---

## Rollback

Route additions are additive. Roll back = revert three `.route(...)` calls
and remove the three handler functions + two DTOs. Zero migration changes.

---

## File structure

```
crates/garraia-auth/src/audit_workspace.rs          — +3 audit variants
crates/garraia-gateway/src/rest_v1/files.rs          — +3 handlers + 2 DTOs
crates/garraia-gateway/src/rest_v1/mod.rs            — +3 routes × 3 build modes
crates/garraia-gateway/src/openapi.rs                — +3 utoipa paths + 2 schemas
crates/garraia-gateway/tests/rest_v1_files_folder_crud.rs  — NEW integration tests
plans/README.md                                      — +1 row (T9)
ROADMAP.md                                           — check 3 items (T9)
```

---

## Tasks

### M1 — Integration tests (RED)

- [ ] Create `crates/garraia-gateway/tests/rest_v1_files_folder_crud.rs`
- [ ] Gate with `#![cfg(feature = "test-helpers")]`
- [ ] Scenarios F1–F10 all compile → fail (404 / method-not-allowed)

### M2 — Audit variants (prereq)

- [ ] Add `FolderCreated`, `FolderRenamed`, `FolderDeleted` to
  `WorkspaceAuditAction` in `audit_workspace.rs`
- [ ] `cargo check -p garraia-auth` passes

### M3 — Handler implementation (GREEN)

- [ ] Add `CreateFolderRequest` + `PatchFolderRequest` DTOs to `files.rs`
- [ ] Add `create_folder` handler to `files.rs`
- [ ] Add `patch_folder` handler to `files.rs`
- [ ] Add `delete_folder` handler to `files.rs`
- [ ] Update routes in `mod.rs` (3 build modes)
- [ ] Add utoipa paths + schema registrations to `openapi.rs`
- [ ] `cargo check -p garraia-gateway` passes

### M4 — Tests green + lint

- [ ] All F1–F10 pass
- [ ] `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings`
- [ ] `cargo fmt --check`

### M5 — Commit + push

- [ ] Commit `feat(files): GAR-561 — Files REST API slice 4: folder CRUD (plan 0091)`

### T9 — Bookkeeping

- [ ] `plans/README.md` row for plan 0091
- [ ] ROADMAP.md: check `POST/PATCH/DELETE /v1/groups/{group_id}/folders/{folder_id}`

---

## Test scenarios

| ID | Endpoint | Input | Expected |
|----|----------|-------|----------|
| F1 | POST folder | valid name, top-level (no parent_id) | 201 FolderSummary |
| F2 | POST folder | valid name, nested (valid parent_id) | 201 FolderSummary |
| F3 | POST folder | path group_id ≠ principal | 403 |
| F4 | POST folder | name empty or > 500 chars | 422 |
| F5 | PATCH folder | rename live folder | 200 FolderSummary |
| F6 | PATCH folder | non-existent folder_id | 404 |
| F7 | PATCH folder | path group_id ≠ principal | 403 |
| F8 | DELETE folder | soft delete live folder | 204 |
| F9 | DELETE folder | already-deleted folder | 404 |
| F10 | DELETE folder | path group_id ≠ principal | 403 |

---

## Acceptance criteria

1. All 10 integration scenarios pass against the test Postgres.
2. `cargo clippy -D warnings` (workspace, no-deps, test-helpers) — zero warnings.
3. `cargo fmt --check` — no diff.
4. OpenAPI JSON at `/api-docs/openapi.json` includes all three new paths.
5. ROADMAP §3.4 has 3 new `[x]` lines for folder CRUD.

---

## Cross-references

- Plan 0088 (GAR-555) — slice 1: list + delete file
- Plan 0089 (GAR-557) — slice 2: rename file
- Plan 0090 (GAR-559) — slice 3: GET single file + folder
- Migration 003 — `folders`, `files`, `file_versions` schema

---

## Estimativa

~380 LOC (handlers + DTOs 180 + routes 40 + audit variants 20 + tests 140).
1 task, 1 commit.
