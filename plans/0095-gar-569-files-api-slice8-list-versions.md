# Plan 0095 — GAR-569 — Files REST API slice 8: GET /v1/groups/{group_id}/files/{file_id}/versions

## Goal

Add the file-version list endpoint to the `/v1` REST surface:

* `GET /v1/groups/{group_id}/files/{file_id}/versions` — returns a paginated list of
  `FileVersionSummary` objects (version number, size, MIME, checksum, uploader, timestamp)
  for an existing non-deleted file in the group.

This is the eighth slice of the files API (plans 0088–0094). It exposes the per-file
version history that the schema (`file_versions` table, migration 003) and the upload
handler (plan 0094, GAR-567) already produce.

## Architecture

### Handler (`list_file_versions`)

1. `require_group_id` → 400 if `X-Group-Id` header missing
2. `can(&principal, Action::FilesRead)` → 403 if denied
3. `path_group_id ≠ principal.group_id` → 403 (cross-group guard)
4. Begin tx → `set_rls_context` (both `app.current_user_id` + `app.current_group_id`)
5. Verify the parent file exists and is not soft-deleted:
   ```sql
   SELECT id FROM files
   WHERE  id = $file_id
     AND  deleted_at IS NULL
   ```
   Returns `None` → 404 (not found or RLS-filtered)
6. Paginated version list (cursor = last seen `version` integer, descending):
   ```sql
   SELECT version, size_bytes, mime_type, checksum_sha256,
          created_by, created_by_label, created_at
   FROM   file_versions
   WHERE  file_id  = $file_id
     AND  group_id = $group_id
     AND  ($cursor IS NULL OR version < $cursor)
   ORDER  BY version DESC
   LIMIT  $limit + 1
   ```
   Fetch `limit + 1`; if `len > limit`, set `next_cursor = items[limit].version`
   and truncate to `limit`.
7. COMMIT tx (read-only — release connection)
8. Emit audit event `FileVersionsListed`:
   `{ file_id, group_id, version_count: usize }` — PII-safe.
9. Return 200 `{ items: Vec<FileVersionSummary>, next_cursor: Option<i32> }`

### Pagination

Uses integer `version` (not UUID) as cursor because versions are dense monotonic integers
(1, 2, 3, …). The cursor is the lowest version number on the last page, and the next page
fetches `version < cursor`. Descending order shows newest first, which is the most common
UI pattern for version history.

`next_cursor` is `i32` (matching the `version` column type) to match the schema exactly.
The query parameter is `cursor: Option<i32>`.

Default limit: 50. Maximum: 100.

### DTOs

```rust
pub struct FileVersionSummary {
    pub version: i32,
    pub size_bytes: i64,
    pub mime_type: String,
    pub checksum_sha256: String,
    pub created_by: Option<Uuid>,
    pub created_by_label: String,
    pub created_at: DateTime<Utc>,
}

pub struct FileVersionListResponse {
    pub items: Vec<FileVersionSummary>,
    pub next_cursor: Option<i32>,
}
```

Note: `object_key` and `integrity_hmac` are NEVER returned in HTTP responses
(internal storage details — ADR 0004 invariant).

## Tech stack

* Axum 0.8, `garraia-auth::Principal`
* `garraia-auth::Action::FilesRead`
* `sqlx` (Postgres), `set_rls_context` for FORCE RLS compliance
* `garraia-auth::WorkspaceAuditAction::FileVersionsListed` (new variant)
* `utoipa` annotations, registered in `openapi.rs`

## Design invariants

* FORCE RLS protocol: `SET LOCAL app.current_user_id` AND `app.current_group_id`
  before every SQL operation.
* PII-free audit: metadata carries `version_count: usize`, not raw file names or user data.
* `object_key` and `integrity_hmac` are NEVER returned in HTTP responses.
* Cross-group attempts on path `group_id` → 403 (not 404) — path validation happens
  before RLS context is set.
* Non-existent or soft-deleted file → 404.
* An empty version list (hypothetical inconsistency) returns 200 `{ items: [], next_cursor: null }`.

## Validações pré-plano

* PR #252 (GAR-567, slice 7 — POST versions) merged — `file_versions` rows are being
  created by the new-version upload handler; the list endpoint has data to work with.
* `file_versions` table exists with columns: `file_id`, `group_id`, `version`, `object_key`,
  `etag`, `checksum_sha256`, `integrity_hmac`, `size_bytes`, `mime_type`, `created_by`,
  `created_by_label`, `created_at`. PK: `(file_id, version)`. Index: `(file_id, version DESC)`.
* `garraia-auth::Action::FilesRead` and `require_group_id` helpers established.
* `set_rls_context` helper in `files.rs` sets both RLS params parameterized.
* Route slot `/v1/groups/{group_id}/files/{file_id}/versions` is unregistered.

## Out of scope

* `GET /v1/groups/{group_id}/files/{file_id}/versions/{version}` — fetch a specific
  version's metadata (deferred to slice 9 if needed).
* Downloading a specific version (non-current) — deferred.
* Deleting a specific version — deferred (GDPR erasure path uses hard-delete on the
  parent file).
* Searching / filtering versions by MIME or date — deferred.

## Rollback

Purely additive:
1. Drop the new route from `mod.rs`.
2. Remove the `list_file_versions` function from `files.rs`.
3. Remove `FileVersionsListed` variant from `audit_workspace.rs`.
4. No schema changes, no migrations — fully reversible.

## §12 Open questions

None blocking. All design decisions resolved:
* Cursor type: `i32` (version integer), not UUID — matches schema, simpler for clients.
* Object key exposure: never in response — matches ADR 0004 invariant.
* Audit scope: read event is useful for LGPD compliance (art. 46-49); logged at INFO level.

## File structure

```
Modified:
  crates/garraia-auth/src/audit_workspace.rs   — +1 variant FileVersionsListed + as_str match arm
  crates/garraia-gateway/src/rest_v1/files.rs  — +handler list_file_versions + DTOs + row type
  crates/garraia-gateway/src/rest_v1/mod.rs    — +route /v1/groups/{group_id}/files/{file_id}/versions
  crates/garraia-gateway/src/openapi.rs        — +list_file_versions to ApiDoc::paths()

New:
  crates/garraia-gateway/tests/rest_v1_files_list_versions.rs  — integration tests
```

## M1 task checklist

### T1 — audit variant

- [ ] Add `FileVersionsListed` to `WorkspaceAuditAction` enum in `audit_workspace.rs`
- [ ] Add `WorkspaceAuditAction::FileVersionsListed => "file.versions.listed"` arm in `as_str()`
- [ ] Add assertion in existing `#[cfg(test)] mod tests` that `as_str()` returns the correct string
- [ ] `cargo test -p garraia-auth --lib` passes

### T2 — DTOs + row type

- [ ] Add `FileVersionRow` (private, `sqlx::FromRow`) with all non-secret columns
- [ ] Add `FileVersionSummary` (public, `Serialize + ToSchema`) with safe columns only (no `object_key`, no `integrity_hmac`)
- [ ] Add `FileVersionListResponse` (public, `Serialize + ToSchema`)
- [ ] Add `ListFileVersionsQuery` (`Deserialize + IntoParams`, fields: `cursor: Option<i32>`, `limit: Option<u32>`)

### T3 — handler + query (tests first)

- [ ] Write `tests/rest_v1_files_list_versions.rs` with test stubs (compile-fail red)
- [ ] Implement `pub async fn list_file_versions(...)` handler
- [ ] Register route in `mod.rs`: `.route("/v1/groups/{group_id}/files/{file_id}/versions", get(files::list_file_versions))`
- [ ] Register route in test router (both `build_router_for_test_with_storage` paths)
- [ ] `cargo check -p garraia-gateway` passes

### T4 — integration tests (green)

Test matrix (run in order; each isolated transaction with FORCE RLS):

- [ ] VL1: 200 happy path — 2 versions exist, returns them newest-first, `next_cursor = null`
- [ ] VL2: 200 pagination — 3 versions, limit=2 → first page has 2 + `next_cursor = 2`, second page has 1 + null cursor
- [ ] VL3: 404 non-existent `file_id`
- [ ] VL4: 404 soft-deleted file
- [ ] VL5: 403 `path_group_id ≠ principal.group_id` (cross-group guard)
- [ ] VL6: 403 role lacks `FilesRead` (child role without permission)
- [ ] VL7: 400 missing `X-Group-Id` header
- [ ] VL8: 200 empty list — file exists but zero versions (edge case for consistency)

- [ ] `cargo test -p garraia-gateway --test rest_v1_files_list_versions` passes

### T5 — OpenAPI + clippy

- [ ] Add `#[utoipa::path(...)]` annotation to `list_file_versions`
- [ ] Register in `openapi.rs` `ApiDoc::paths()`
- [ ] `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean

### T6 — per-task commit + push

- [ ] Commit T1: `feat(auth): add FileVersionsListed audit action`
- [ ] Commit T2+T3+T4: `feat(files): GAR-569 — GET /v1/groups/{group_id}/files/{file_id}/versions`
- [ ] Commit T5: `feat(openapi): register list_file_versions in ApiDoc`
- [ ] Push branch and open PR

### T7 — ROADMAP + plans/README bookkeeping (merge commit)

- [ ] ROADMAP.md §3.4 Arquivos: tick `[ ] GET /v1/groups/{group_id}/files/{file_id}/versions`
- [ ] `plans/README.md`: add row `| 0095 | ... | GAR-569 | ✅ Merged ... via PR #NNN |`

## Risk register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| `object_key` accidentally exposed | Low | High | DTO only includes safe columns; `FileVersionRow` uses select-list not `SELECT *` |
| Cross-group data leak via version list | Low | High | Path-level group check (403) + RLS `set_config` on both params |
| Pagination off-by-one | Medium | Low | VL2 test covers multi-page exactly |
| Soft-deleted file still returns versions | Medium | Medium | Parent-file existence check (step 5) before version query |

## Acceptance criteria

1. `GET /v1/groups/{group_id}/files/{file_id}/versions` → 200 + `{ items, next_cursor }`
2. Items sorted newest-first (version DESC).
3. Cursor pagination works for multi-page results.
4. `object_key` and `integrity_hmac` are NOT present in any response field.
5. Cross-group guard: `path_group_id ≠ principal.group_id` → 403.
6. Soft-deleted parent file → 404.
7. `cargo clippy --workspace ... -- -D warnings` clean.
8. 8 integration tests pass (VL1–VL8).
9. `FileVersionsListed` audit action asserted in `garraia-auth` tests.

## Cross-references

* Plan 0094 (GAR-567) — POST /v1/groups/{group_id}/files/{file_id}/versions (creates versions)
* Plan 0093 (GAR-564) — GET /v1/files/{file_id}/download (streams current version)
* Plan 0090 (GAR-559) — GET /v1/groups/{group_id}/files/{file_id} (shows current_version + total_versions)
* Migration 003 (`003_files_and_folders.sql`) — `file_versions` table schema
* ADR 0004 — object storage security policy (object_key never in HTTP responses)
* ROADMAP §3.4 — Arquivos checklist

## Estimativa

* T1 (audit): 15 min
* T2 (DTOs): 20 min
* T3 (handler + route): 30 min
* T4 (tests): 40 min
* T5 (OpenAPI + clippy): 15 min
* Total: **~2h** (low estimate 1.5h / high 3h)
* LOC delta: ~250 LOC new (handler ~80, tests ~120, DTOs ~40, audit ~10)
