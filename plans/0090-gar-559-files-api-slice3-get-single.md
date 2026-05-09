# Plan 0090 — Files REST API slice 3: GET single file + GET single folder

**GAR-559** · epic:ws-api · Fase 3.4 "Arquivos"
**Branch:** `routine/202605091430-files-slice3-get-single`
**Date:** 2026-05-09 (America/New_York)

---

## Goal

Land slice 3 of the files REST surface: two read endpoints that return a
single resource by UUID.

- `GET /v1/groups/{group_id}/files/{file_id}` → 200 `FileSummary` | 403 | 404
- `GET /v1/groups/{group_id}/folders/{folder_id}` → 200 `FolderSummary` | 403 | 404

Schema (`files`, `folders`) and FORCE RLS are live (migration 003, GAR-387).
`Action::FilesRead`, `check_group_match`, `set_rls_context`, `FileRow`,
`FolderRow`, `FileSummary`, `FolderSummary` all exist from slices 1–2.
This slice adds two pure-read handlers with zero new schema changes.

---

## Architecture

- **No new audit events.** Read operations do not write to `audit_events`.
- **SQL pattern:** `SELECT ... WHERE id = $1 AND group_id = $2 AND deleted_at IS NULL`
  — 0 rows covers soft-deleted + cross-group (RLS) + not-found, all returning 404.
- **Route collision:** `GET` and `PATCH` on the same `/files/{file_id}` path are
  chained in Axum: `get(files::get_file).patch(files::patch_file)`.
- **Fail-soft + no-auth stubs** updated in the same mod.rs blocks as slices 1–2.

---

## Tech stack

Unchanged from slices 1–2: Axum 0.8, sqlx, utoipa, garraia-auth `Principal`.

---

## Design invariants

- NEVER read `deleted_at IS NOT NULL` rows — treat as 404 (same as not-found).
- NEVER expose cross-group files — RLS ensures `group_id` col is bound.
- SET LOCAL both `app.current_user_id` AND `app.current_group_id` (FORCE RLS).
- No PII in audit (no audit emitted here anyway).

---

## Out of scope

- Download URLs / presigned S3 (slice 4+).
- File versions listing / single version fetch.
- Folder tree traversal (already covered by list_folders with parent_id).

---

## Rollback

Route additions are additive. Rolling back = reverting two `.route(...)` calls
and removing the two handler functions. Zero migration involved.

---

## File structure

```
crates/garraia-gateway/src/rest_v1/files.rs          — +2 handlers
crates/garraia-gateway/src/rest_v1/mod.rs            — +2 routes × 3 build modes
crates/garraia-gateway/src/openapi.rs                — +2 utoipa paths
crates/garraia-gateway/tests/rest_v1_files_get_single.rs  — NEW integration tests
plans/README.md                                      — +1 row (T8)
ROADMAP.md                                           — check 2 items (T8)
```

---

## Tasks

### M1 — Integration tests (RED)

- [ ] Create `crates/garraia-gateway/tests/rest_v1_files_get_single.rs`
- [ ] Gate with `#![cfg(feature = "test-helpers")]`
- [ ] Scenarios G1–G7 (see below) all compile → fail (404 / method-not-allowed)

### M2 — Handler implementation (GREEN)

- [ ] Add `get_file` handler to `files.rs`
- [ ] Add `get_folder` handler to `files.rs`
- [ ] Update routes in `mod.rs` (handler / unconfigured / no-auth — 3 blocks)
- [ ] Add utoipa paths to `openapi.rs`
- [ ] `cargo check -p garraia-gateway` passes

### M3 — Tests green + lint

- [ ] All G1–G7 pass
- [ ] `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings`
- [ ] `cargo fmt --check`

### M4 — Commit + push

- [ ] Commit `feat(files): GAR-NNN — Files REST API slice 3: GET single file + folder`

### T8 — Bookkeeping

- [ ] `plans/README.md` row for plan 0090
- [ ] ROADMAP.md: check `GET /v1/groups/{group_id}/files/{file_id}` + `GET /v1/groups/{group_id}/folders/{folder_id}`

---

## Test scenarios

| ID | Endpoint | Input | Expected |
|----|----------|-------|----------|
| G1 | GET file | live file, correct group | 200 FileSummary, all fields match |
| G2 | GET file | soft-deleted file | 404 |
| G3 | GET file | non-existent file_id | 404 |
| G4 | GET file | path group_id ≠ principal | 403 |
| G5 | GET folder | live folder, correct group | 200 FolderSummary, all fields match |
| G6 | GET folder | soft-deleted folder | 404 |
| G7 | GET folder | non-existent folder_id | 404 |

---

## Acceptance criteria

1. All 7 integration scenarios pass against the test Postgres.
2. `cargo clippy -D warnings` (workspace, no-deps, test-helpers feature) — zero warnings.
3. `cargo fmt --check` — no diff.
4. OpenAPI JSON at `/api-docs/openapi.json` includes both new paths.
5. ROADMAP §3.4 files checklist has 2 new `[x]` lines.

---

## Cross-references

- Plan 0088 (GAR-555) — slice 1: list + delete
- Plan 0089 (GAR-557) — slice 2: rename
- Migration 003 — `files`, `folders`, `file_versions` schema

---

## Estimativa

~250 LOC (handler 100 + routes 30 + tests 120). 1 task, 1 commit.
