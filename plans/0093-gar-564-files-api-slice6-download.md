# Plan 0093 — GAR-564 — Files REST API slice 6: GET /v1/files/{file_id}/download

## Goal

Add streaming file download to the `/v1` REST surface:

* `GET /v1/files/{file_id}/download` — streams the current version of a file as binary
  content (200 + Content-Type + Content-Disposition: attachment).

This is the sixth slice of the files API (plans 0088–0092). It closes the core
download use-case for both LocalFs (dev) and S3-backed (production) deployments.

## Architecture

### Handler (`download_file`)

1. `require_group_id` → 400 if `X-Group-Id` header missing
2. `can(&principal, Action::FilesRead)` → 403 if denied
3. Begin tx → `set_rls_context` (both `app.current_user_id` + `app.current_group_id`)
4. ```sql
   SELECT f.name, fv.object_key, fv.mime_type, fv.size_bytes
   FROM   files f
   JOIN   file_versions fv ON fv.file_id = f.id
                          AND fv.version  = f.current_version
                          AND fv.group_id = f.group_id
   WHERE  f.id         = $file_id
     AND  f.deleted_at IS NULL
   ```
   Returns `None` → 404 (not found or RLS-filtered cross-group)
5. COMMIT tx (read-only — release connection)
6. Require `AppState::object_store` (or fail 503)
7. Call `object_store.get(object_key)` → map errors:
   - `StorageError::NotFound` → 404
   - Other → 500
8. Emit audit event `file.download_issued`:
   `{ file_id, group_id, filename_len: usize }` — no raw filename (PII)
9. Return `axum::response::Response` with:
   - Status: 200
   - `Content-Type: <mime_type from file_versions>`
   - `Content-Disposition: attachment; filename="download"` (no raw name in header —
     clients that need the filename already have it from the GET /files/{id} response)
   - `Content-Length: <size_bytes>`
   - Body: `bytes::Bytes` from `GetResult`

### Content-Disposition rationale

Raw filenames in `Content-Disposition` require RFC 5987 percent-encoding for
non-ASCII characters, which adds complexity. The file name is always available
via `GET /v1/groups/{group_id}/files/{file_id}` (plan 0090). For this slice,
use the literal `filename="download"` sentinel — a pragmatic choice that avoids
PII leak in logs and the encoding edge-cases. A follow-up slice can add RFC 5987
encoding if clients request it.

## Tech stack

* Axum 0.8, `garraia-auth::Principal`
* `garraia-auth::Action::FilesRead`
* `sqlx` (Postgres), `set_rls_context` for FORCE RLS compliance
* `garraia-storage::ObjectStore::get` (streaming bytes)
* `garraia-auth::audit_workspace` — new variant `FileDownloadIssued`
* `utoipa` annotations, registered in `openapi.rs`

## Design invariants

* FORCE RLS protocol: `SET LOCAL app.current_user_id` AND `app.current_group_id`
  before every SQL operation.
* PII-free audit: metadata carries `filename_len: usize`, never the raw file name.
* `object_key` is NEVER returned in HTTP responses (internal detail).
* Cross-group attempts return 404 via RLS filtering (not 403 — avoids oracle).
* No presigned-URL path in this slice — a future S3 slice adds `presign_get` redirect.

## Validações pré-plano

* PR #247 (GAR-562, slice 5 folder POST + DELETE) merged as `28b3b0f` — all
  file/folder CRUD patterns established.
* `ObjectStore::get(key) -> Result<GetResult>` is fully implemented in both
  `LocalFs` and `S3Compatible`.
* `AppState::object_store: Option<Arc<dyn ObjectStore>>` available in all handlers.
* `build_router_for_test_with_storage` exists for integration tests (used by
  `rest_v1_uploads_patch.rs`).
* `WorkspaceAuditAction` enum is extensible (variants added per slice).

## Out of scope

* Presigned URL (S3 redirect) — `LocalFs::presign_get` returns `Unsupported`;
  this will be a dedicated S3-only slice.
* Range requests (HTTP 206 Partial Content) — future enhancement.
* `Content-Disposition: inline` option — future.
* Version-specific download (`?version=N`) — future.
* `file_shares` table — deferred (ADR 0004 v1 drops sharing).

## Rollback plan

Reversible: the endpoint is additive (new route). Removing it is a one-line route
deletion. No schema changes, no migrations.

## §8 Rollback plan (formal)

Add-only change — revert is `git revert <commit>` on the handler + route registration
lines. No DB state is modified. Object store content is read-only.

## §12 Open questions

None — all building blocks confirmed present.

## File structure

```
crates/garraia-gateway/src/rest_v1/files.rs       (handler + unit tests)
crates/garraia-gateway/src/rest_v1/mod.rs          (route registration × 3 branches)
crates/garraia-gateway/src/rest_v1/openapi.rs      (utoipa registration)
crates/garraia-auth/src/audit_workspace.rs          (FileDownloadIssued variant)
crates/garraia-gateway/tests/rest_v1_files_download.rs  (integration tests)
```

## M1 — Tests first (red)

- [ ] Add `FileDownloadIssued` variant to `WorkspaceAuditAction`
- [ ] Add `download_file` handler skeleton (returns 501) in `files.rs`
- [ ] Register route in `mod.rs` (all 3 branches)
- [ ] Write integration test `rest_v1_files_download.rs` (D1–D6 scenarios)
- [ ] `cargo test -p garraia-gateway --test rest_v1_files_download --features test-helpers` → red

## M2 — Implementation (green)

- [ ] Implement `download_file` handler (SQL lookup + ObjectStore::get + response)
- [ ] Add utoipa annotation in `openapi.rs`
- [ ] `cargo test -p garraia-gateway --test rest_v1_files_download --features test-helpers` → green

## M3 — Unit tests

- [ ] Add unit tests inside `files.rs` covering helper functions
- [ ] `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` → clean

## Risk register

| Risk | Mitigation |
|---|---|
| `file_versions` row missing (schema invariant violated) | JOIN returns None → 404 (safe fallback) |
| `object_key` stored but object physically deleted | `StorageError::NotFound` → 404 |
| Large file OOM | `ObjectStore::get` buffers into `Bytes` — acceptable for v1 (files ≤ 5 GiB cap per DB CHECK); streaming via `AsyncRead` is a future enhancement |
| Content-Disposition filename encoding | Avoided by using literal `"download"` sentinel |

## Acceptance criteria

* `GET /v1/files/{file_id}/download` → 200 + correct bytes + correct Content-Type
* Returns 404 when file is soft-deleted
* Returns 404 when file does not exist
* Returns 403 when `Action::FilesRead` is denied (Guest role or below)
* Returns 503 when `object_store` is not configured
* Returns 404 when RLS blocks cross-group attempt (Rule 10 satisfied)
* `cargo clippy -- -D warnings` clean
* CI green

## Cross-references

* Plan 0088 (GAR-555) — files list + DELETE slice 1 (pattern reference)
* Plan 0090 (GAR-559) — GET single file (DB lookup pattern)
* Plan 0047 (GAR-395 slice 3) — ObjectStore usage in gateway
* ROADMAP.md §3.4 "Arquivos" — `GET /v1/files/{file_id}:download`
* ADR 0004 — object storage (presigned URL deferred to follow-up)

## Estimativa

0.5 / 1 / 1.5 horas (baixo / provável / alto).
