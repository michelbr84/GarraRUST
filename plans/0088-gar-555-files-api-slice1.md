# Plan 0088 — Files REST API slice 1: GET files + GET folders + DELETE file

**GAR-555** · epic:ws-api · Fase 3.4 "Arquivos"  
**Branch:** `routine/202605090618-files-api-slice1`  
**Date:** 2026-05-09 (America/New_York)

---

## Goal

Land the first read-only + soft-delete slice of the Files REST surface (ROADMAP §3.4 "Arquivos"):

- `GET /v1/groups/{group_id}/files?folder_id=&cursor=&limit=` — cursor-paginated file listing
- `GET /v1/groups/{group_id}/folders?parent_id=&cursor=&limit=` — cursor-paginated folder listing
- `DELETE /v1/files/{file_id}` — idempotent soft-delete

Schema (`files`, `folders`, `file_versions`) and FORCE RLS are already live in migration 003 (GAR-387). This slice adds only the handler layer.

---

## Architecture

```
GET /v1/groups/{group_id}/files
  → RestV1FullState → AppPool → garraia_app role
  → SET LOCAL app.current_user_id / app.current_group_id via set_config (plan 0056 pattern)
  → SELECT FROM files WHERE deleted_at IS NULL
      AND (folder_id = $folder_id OR $folder_id IS NULL)
      AND (created_at, id) < (cursor_ts, cursor_id)   -- cursor filter
    ORDER BY created_at DESC, id DESC LIMIT $limit+1
  ← FileRow list + next_cursor

GET /v1/groups/{group_id}/folders
  → same pool + RLS set
  → SELECT FROM folders WHERE deleted_at IS NULL
      AND (parent_id = $parent_id OR ($parent_id IS NULL AND parent_id IS NULL))
      AND (created_at, id) < (cursor_ts, cursor_id)
    ORDER BY created_at DESC, id DESC LIMIT $limit+1
  ← FolderRow list + next_cursor

DELETE /v1/files/{file_id}
  → AppPool → RLS set (group_id = principal.group_id)
  → UPDATE files SET deleted_at = now()
      WHERE id = $file_id AND group_id = $group_id AND deleted_at IS NULL
  → returns 204 (file found and soft-deleted) or 204 (already deleted — idempotent)
  → audit_workspace_event WorkspaceAuditAction::FileDeleted
```

The `file_id`-only path for DELETE does **not** leak cross-group existence — RLS (`group_id = app.current_group_id`) filters the file out before the UPDATE runs, yielding `rows_affected = 0`. Both soft-delete paths (fresh and already-deleted) return 204 to remain idempotent per HTTP semantics (ROADMAP §3.4 "Arquivos" pattern mirrors `DELETE /v1/groups/{group_id}/task-lists/{list_id}`).

---

## Tech stack

- **Rust / Axum 0.8** — handler functions, `FromRequestParts`, `Path`, `Query`, `State`
- **sqlx 0.8** — parameterized queries via `sqlx::query!` / `sqlx::query_as!`  
- **garraia-auth** — `Principal`, `can()`, `Action::{FilesRead, FilesDelete}`, `audit_workspace_event`
- **utoipa** — `#[utoipa::path]` annotations for OpenAPI 3.1

---

## Design invariants

1. **RLS dual-GUC**: SET LOCAL `app.current_user_id` AND `app.current_group_id` via `set_config($1, $2, true)` before every query (plan 0056 established pattern).
2. **Group-ID cross-check**: path `{group_id}` must equal `principal.group_id`; mismatch → 403.
3. **No PII in audit metadata**: carry `file_id` and `name_len` — never `name` nor `created_by_label`.
4. **Idempotent soft-delete**: UPDATE WHERE `deleted_at IS NULL`; zero-row result also returns 204 (already deleted).
5. **Cursor ordering**: `(created_at DESC, id DESC)` — consistent with all other cursor-paginated endpoints.

---

## Validações pré-plano

- [x] `files`, `folders`, `file_versions` tables exist (migration 003, GAR-387) ✅
- [x] FORCE RLS `files_group_isolation` + `folders_group_isolation` active (migration 003) ✅
- [x] `Action::FilesRead`, `Action::FilesDelete` present in `garraia-auth/src/action.rs` ✅
- [x] `can()` matrix covers FilesRead/FilesDelete for Owner/Admin/Member (CLAUDE.md §3.3) ✅
- [x] `audit_workspace_event` + `WorkspaceAuditAction` are public API ✅
- [x] `AppPool` + `Principal` + `set_config` pattern solid (plans 0054–0087) ✅

---

## Out of scope

- Upload endpoints (`initUpload`, `completeUpload`, tus) — ObjectStore wiring
- Download URLs / presigned URLs
- Version management (`newVersion`)
- Folder CRUD (create, rename, delete)
- Hard delete / trash restoration
- `has_attachment` search filter (deferred until after upload slice)

---

## Rollback

This slice is handler-only — no migrations. Rolling back = reverting the `files.rs` handler + three route registrations in `mod.rs`. No DB changes to undo.

---

## §12 Open questions

| # | Question | Decision |
|---|----------|----------|
| Q1 | Should `DELETE /v1/files/{file_id}` verify `group_id` in path (like tasks do) or rely purely on RLS? | App-layer verify: pass `group_id = principal.group_id` in WHERE clause. RLS would catch it anyway, but being explicit avoids 0-row ambiguity between "file not in group" and "file not found". |
| Q2 | Should soft-deleted folders still appear as a valid `folder_id` in `GET /v1/groups/{group_id}/files`? | No. If the folder is soft-deleted, files in it become root-level (folder_id still points to the folder row, but the folder itself is hidden). For now, allow the filter — RLS on folders is separate from files. |
| Q3 | Audit event for `GET` (list) endpoints? | No — read auditing deferred to a dedicated audit-trail slice per project pattern. |

---

## File structure

```
crates/garraia-gateway/src/rest_v1/
  files.rs        ← NEW — 3 handlers + types + unit tests
  mod.rs          ← ADD pub mod files; + 3 route registrations (all 3 modes)
  openapi.rs      ← ADD 3 paths to ApiDoc
```

---

## M1 task list

- [ ] **T1** — Create `crates/garraia-gateway/src/rest_v1/files.rs` with types only (no handlers), compile green
- [ ] **T2** — Add `list_files` handler + unit tests (red → green)
- [ ] **T3** — Add `list_folders` handler + unit tests (red → green)
- [ ] **T4** — Add `delete_file` handler + unit tests (red → green)
- [ ] **T5** — Wire routes in `mod.rs` (all 3 modes) + OpenAPI annotations in `openapi.rs`
- [ ] **T6** — `cargo check -p garraia-gateway` → green
- [ ] **T7** — `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` → clean
- [ ] **T8** — Update `plans/README.md` + `ROADMAP.md` (flip `[ ]` → `[x]` for 3 endpoints)

---

## Risk register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|-----------|
| sqlx compile fails without Postgres (unit tests) | Low | Medium | Use `#[cfg(test)]` mock helpers same as other handlers |
| `WorkspaceAuditAction::FileDeleted` doesn't exist | Low | Low | Add it to `audit.rs` if missing; small change |
| Cursor page boundary with NULL `folder_id` edge case | Medium | Low | Test explicitly in unit tests |

---

## Acceptance criteria

- `GET /v1/groups/{group_id}/files` returns only the requesting user's group's files (RLS enforced).
- `DELETE /v1/files/{file_id}` cross-group → 404; double-delete → 204 idempotent.
- Cursor pagination stable across inserts.
- ≥15 unit tests green.
- Clippy clean.
- All CI checks pass.

---

## Cross-references

- Migration 003: `crates/garraia-workspace/migrations/003_files_and_folders.sql`
- GAR-387: schema implementation
- GAR-394/395: `garraia-storage` crate (ObjectStore — used in slice 2+)
- ROADMAP §3.4 "Arquivos" checklist
- Plan 0084 (search slice 1) — establishes same cursor pattern for files will plug into `has_attachment` filter

---

## Estimativa

- T1–T8: ~3h
- LOC: ~350 (files.rs ~300 + mod.rs ~30 + openapi.rs ~20)
