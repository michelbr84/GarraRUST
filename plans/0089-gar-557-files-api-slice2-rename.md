# Plan 0089 — Files REST API slice 2: PATCH /v1/groups/{group_id}/files/{file_id} (rename)

**GAR-557** · epic:ws-api · Fase 3.4 "Arquivos"  
**Branch:** `routine/202505091215-files-api-slice2-rename`  
**Date:** 2026-05-09 (America/New_York)

---

## Goal

Land the rename endpoint for the Files REST surface (ROADMAP §3.4 "Arquivos"):

- `PATCH /v1/groups/{group_id}/files/{file_id}` — rename a file (only `name` may change in this slice). Returns updated `FileSummary`.

Schema (`files`, `folders`, `file_versions`) and FORCE RLS are already live in migration 003 (GAR-387). Handlers for read + soft-delete are live (plan 0088, GAR-555). This slice adds only the rename handler.

---

## Architecture

```
PATCH /v1/groups/{group_id}/files/{file_id}
  → RestV1FullState → AppPool → garraia_app role
  → SET LOCAL app.current_user_id / app.current_group_id (plan 0056 pattern)
  → Validate body: name trimmed, 1..=500 chars, no "/" or NUL
  → UPDATE files SET name = $new_name, updated_at = now()
      WHERE id = $file_id AND group_id = $group_id AND deleted_at IS NULL
    RETURNING id, name, mime_type, size_bytes, current_version, total_versions,
              folder_id, created_by, created_by_label, created_at, updated_at
  → if 0 rows → 404 (not found, cross-group, or soft-deleted)
  → audit_workspace_event FileRenamed { name_len, group_id }
  ← 200 FileSummary
```

---

## Tech stack

- **Rust / Axum 0.8** — `State`, `Path`, `Json` extractors
- **sqlx 0.8** — `sqlx::query_as` parameterized UPDATE RETURNING
- **garraia-auth** — `Principal`, `can()`, `Action::FilesWrite`, `audit_workspace_event`, `WorkspaceAuditAction::FileRenamed` (new variant)
- **utoipa** — `#[utoipa::path]` for OpenAPI 3.1

---

## Design invariants

1. **RLS dual-GUC**: SET LOCAL both `app.current_user_id` AND `app.current_group_id` (plan 0056 pattern).
2. **Group-ID cross-check**: path `{group_id}` must equal `principal.group_id`; mismatch → 403.
3. **No PII in audit metadata**: carry `name_len` not `name`, `group_id` for forensics.
4. **Soft-delete aware**: WHERE `deleted_at IS NULL` — renamed a deleted file → 404 (consistent with DELETE semantics).
5. **Validation**: name trimmed, 1..=500 chars, reject `/` and NUL byte → 422.
6. **Single UPDATE RETURNING** — avoids separate SELECT + UPDATE (eliminates TOCTOU race).

---

## Validações pré-plano

- [x] `files` table exists with `name`, `updated_at` columns (migration 003) ✅
- [x] FORCE RLS `files_group_isolation` active (migration 003) ✅
- [x] `Action::FilesWrite` present in `garraia-auth/src/action.rs` ✅
- [x] `can()` matrix covers FilesWrite for Owner/Admin/Member ✅
- [x] `audit_workspace_event` + `WorkspaceAuditAction` are public API ✅
- [x] `AppPool` + `Principal` + `set_config` pattern solid (plans 0054–0088) ✅
- [ ] `WorkspaceAuditAction::FileRenamed` — MISSING, must add to `audit_workspace.rs`

---

## Out of scope

- Move between folders (`folder_id` mutation)
- Folder rename (mirror endpoint for folders)
- File restore from trash
- MIME / settings overrides
- Upload initiation / presigned URLs

---

## Rollback

Handler-only change — no migrations. Rolling back = reverting the `patch_file` handler, route registration, and `FileRenamed` audit variant. No DB changes to undo.

---

## §12 Open questions

| # | Question | Decision |
|---|----------|----------|
| Q1 | Should the endpoint accept partial updates (any subset of mutable fields) or only `name`? | Only `name` in this slice. Other mutations (folder move, MIME override) need additional validation logic and are deferred. |
| Q2 | Return 422 or 400 for invalid `name`? | 422 Unprocessable Entity — body is well-formed JSON but fails semantic validation (RFC 9110 §15.5.21). |
| Q3 | Should `updated_at` be updated in the DB? | Yes — UPDATE sets `updated_at = now()` so clients can detect staleness. |

---

## File structure

```
crates/garraia-auth/src/
  audit_workspace.rs   ← ADD FileRenamed variant + "file.renamed" string + test

crates/garraia-gateway/src/rest_v1/
  files.rs             ← ADD PatchFileRequest, patch_file handler + unit tests
  mod.rs               ← ADD .route("/v1/groups/{group_id}/files/{file_id}", patch(files::patch_file)) ×3 modes
  openapi.rs           ← ADD patch_file to ApiDoc paths + PatchFileRequest to schemas

crates/garraia-gateway/tests/
  rest_v1_files_patch.rs  ← NEW — 6-8 integration scenarios
```

---

## M1 task list

- [x] **T1** — Add `WorkspaceAuditAction::FileRenamed` + `"file.renamed"` to `audit_workspace.rs`; add unit test assertion
- [x] **T2** — Add `PatchFileRequest` struct + `patch_file` handler to `files.rs` + unit tests (red → green)
- [x] **T3** — Wire PATCH route in `mod.rs` (all 3 modes: full, unconfigured, no-auth stub)
- [x] **T4** — Add `patch_file` + `PatchFileRequest` to `openapi.rs`
- [x] **T5** — Write integration test `tests/rest_v1_files_patch.rs` (6-8 scenarios, single `#[tokio::test]`)
- [x] **T6** — `cargo check -p garraia-gateway` → green
- [x] **T7** — `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` → clean
- [x] **T8** — Update `plans/README.md` + `ROADMAP.md` (add `PATCH /v1/groups/{group_id}/files/{file_id}` ✅ entry)

---

## Risk register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|-----------|
| `files.updated_at` column not present in schema | Low | Medium | Verified migration 003 includes `updated_at` via GAR-387 |
| UPDATE RETURNING not finding FileRow columns | Low | Low | Unit tests exercise From<FileRow> → FileSummary path |
| `name` DB CHECK constraint mismatch (our 500 vs DB length) | Low | Low | Match DB CHECK `CHECK (char_length(name) BETWEEN 1 AND 500)` exactly |

---

## Acceptance criteria

- `PATCH /v1/groups/{group_id}/files/{file_id}` with valid name → 200 + updated `FileSummary`.
- Cross-group file_id → 404.
- Soft-deleted file → 404.
- Empty/too-long name → 422.
- Name with `/` or NUL → 422.
- Group mismatch in path → 403.
- `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test -p garraia-gateway --test rest_v1_files_patch` all pass.
- `cargo audit --no-fetch` continues 0 errors.
- CI ≥16 checks green.

---

## Cross-references

- Migration 003: `crates/garraia-workspace/migrations/003_files_and_folders.sql`
- GAR-387: schema implementation
- Plan 0088 / GAR-555: slice 1 (list files + list folders + delete file)
- ROADMAP §3.4 "Arquivos" checklist

---

## Estimativa

- T1–T8: ~2h
- LOC: ~200 (files.rs ~120 + mod.rs ~6 + openapi.rs ~10 + integration test ~80)
