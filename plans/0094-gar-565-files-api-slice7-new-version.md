# Plan 0094 — Files REST API slice 7: POST /v1/files/{id}/versions (new file version)

**Issue:** GAR-567
**Epic:** GAR-WS-API (Fase 3.4)
**Branch:** `routine/202605100800-files-new-version`
**Created:** 2026-05-10 (America/New_York)

---

## 1. Goal

Add `POST /v1/groups/{group_id}/files/{file_id}/versions` — a synchronous endpoint that accepts raw bytes in the request body and creates a new content version for an existing file. Closes the `[ ] POST /v1/files/{file_id}:newVersion` ROADMAP item (§3.4 Arquivos).

This completes the core CRUD surface for individual files in Fase 3.4. Future slices can add version listing, rollback, and tus-linked versioning.

---

## 2. Architecture

```
Client ──POST /v1/groups/{gid}/files/{fid}/versions──► garraia-gateway
         headers: Content-Type, Content-Length
         body: raw bytes (≤ max_patch_bytes)

Handler (files.rs):
  1. Auth (Principal JWT + FilesWrite + group_id match)
  2. Read Content-Type → mime_type; validate MIME allow-list
  3. Read body bytes (capped)
  4. Compute SHA-256 hex of body
  5. Build object_key = "groups/{gid}/files/{fid}/v{N+1}/{uuid}"
  6. ObjectStore::put(key, bytes, PutOptions { hmac_secret, content_type })
     → ObjectMetadata { etag_sha256, integrity_hmac, size_bytes }
  7. DB transaction (AppPool, FORCE RLS):
     a. SELECT files FOR UPDATE → current_version, check deleted_at IS NULL
     b. INSERT file_versions (version = current_version + 1)
     c. UPDATE files SET current_version=N+1, total_versions=total_versions+1,
                         size_bytes=?, mime_type=?, updated_at=now()
     d. INSERT audit_events (file.version.created, PII-safe)
  8. Return 201 + FileVersionResponse
```

**Two-phase ordering:** object store write happens BEFORE the DB transaction commits. If the DB commit fails after the object write, the orphaned blob is cleaned up by a future maintenance job (same rationale as plan 0044 §5.3.1). If the object store write fails, the DB transaction never starts → no orphan.

---

## 3. Tech stack

- **Language:** Rust (stable 1.92+)
- **Web:** Axum 0.8 (`State`, `Path`, `TypedHeader`, axum `Bytes` extractor)
- **DB:** `sqlx` 0.8 with Postgres 16 (AppPool, FORCE RLS via `set_config`)
- **Storage:** `garraia_storage::ObjectStore::put` (LocalFs in dev/CI, S3 in prod)
- **Hashing:** `sha2` crate (already in workspace) for SHA-256
- **MIME:** `garraia_storage::mime_allowlist::is_mime_allowed`
- **OpenAPI:** `utoipa::path` annotation + `openapi.rs` registration

---

## 4. Design invariants

1. **PII-safe audit:** metadata carries `size_bytes`, `new_version`, `group_id`, `file_id` — no raw filename or MIME type.
2. **NO `object_key` in HTTP responses** — internal storage detail.
3. **Cross-group Rule 10:** `path_group_id == principal.group_id`; RLS provides defense-in-depth.
4. **MIME allow-list enforced before bytes are read** to fail fast on 415.
5. **Fail-closed on missing staging:** 503 when `upload_staging` is `None`.
6. **Atomic DB update:** `INSERT file_versions` + `UPDATE files` in one transaction.
7. **`files_current_le_total` CHECK** preserved: `total_versions = total_versions + 1` before `current_version = new_version` satisfies `current_version <= total_versions` at all times.

---

## 5. Validações pré-plano

- [x] `file_versions` schema reviewed (migration 003) — compound PK `(file_id, version)`, FK `(file_id, group_id) → files(id, group_id)`.
- [x] `files` schema reviewed — `current_version`, `total_versions`, `size_bytes`, `mime_type`, `updated_at` are mutable columns.
- [x] `ObjectStore::put` interface confirmed — returns `ObjectMetadata { etag_sha256, integrity_hmac, size_bytes }`.
- [x] MIME allow-list function confirmed: `garraia_storage::mime_allowlist::is_mime_allowed(&str) -> bool`.
- [x] HMAC secret available via `upload_staging.hmac_secret` — same as tus upload commit.
- [x] `sha256_hex_of` helper already in `uploads.rs` (module-private) — will duplicate as `files`-local fn.
- [x] `Action::FilesWrite` exists in `can.rs` and is granted to Owner/Admin/Member.
- [x] Route `/v1/groups/{group_id}/files/{file_id}/versions` does not conflict with existing routes.

---

## 6. Out of scope

- Listing all versions of a file (`GET /v1/groups/{group_id}/files/{file_id}/versions`)
- Rolling back to a previous version
- Tus-linked versioning (large files via `/v1/uploads` → new version)
- `Content-Encoding` / compression handling
- Range requests / partial uploads

---

## 7. Rollback

- The new handler is additive (no schema changes, no migration). If reverted, callers get 404 on the route — no data corruption.
- Objects already written to the ObjectStore are orphaned but recoverable (maintenance job).

---

## 8. File structure

```
crates/garraia-auth/src/audit_workspace.rs   ← add FileVersionCreated variant + as_str + test
crates/garraia-gateway/src/rest_v1/files.rs  ← add post_new_version handler + FileVersionResponse DTO
crates/garraia-gateway/src/rest_v1/mod.rs    ← register route + openapi schemas
crates/garraia-gateway/src/rest_v1/openapi.rs← register path
crates/garraia-gateway/tests/rest_v1_files_new_version.rs ← integration tests NV1–NV8
plans/0094-gar-565-files-api-slice7-new-version.md ← this file
plans/README.md ← new row
ROADMAP.md ← tick [ ] POST /v1/files/{file_id}:newVersion
```

---

## 9. Tasks (M1)

- [ ] **T1** — `audit_workspace.rs`: add `FileVersionCreated` variant, `as_str` → `"file.version.created"`, unit test.
- [ ] **T2** — `tests/rest_v1_files_new_version.rs`: write integration tests NV1–NV8 (red).
- [ ] **T3** — `files.rs`: implement `post_new_version` handler + `FileVersionResponse` DTO + local `sha256_hex_of` + `validate_mime_type` helper.
- [ ] **T4** — `mod.rs` + `openapi.rs`: register route + OpenAPI schemas; `cargo check -p garraia-gateway --features test-helpers` green.
- [ ] **T5** — `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean; ROADMAP + plans/README updated; commit.

---

## 10. Test plan (NV1–NV8)

| ID | Scenario | Expected |
|----|----------|----------|
| NV1 | Happy path — valid bytes + Content-Type | 201, `version = 2`, files row updated |
| NV2 | File not found (random UUID) | 404 |
| NV3 | Soft-deleted file | 404 |
| NV4 | Cross-group attempt (RLS) | 404 |
| NV5 | Principal lacks `FilesWrite` (Child role) | 403 |
| NV6 | Missing `X-Group-Id` header | 400 |
| NV7 | Missing `Content-Type` header (treated as octet-stream, MIME not in list) | 415 |
| NV8 | Object store not configured | 503 |

---

## 11. Risk register

| Risk | Mitigation |
|------|------------|
| Orphaned blob if DB commit fails after object write | Same risk as tus; future cleanup job; acceptable per plan 0044 §5.3.1 |
| Body buffering into RAM for large files | Cap enforced (max_patch_bytes = 100 MiB); large files should use tus |
| MIME allow-list bypass via content-sniffing | Validation is on declared Content-Type; content-sniffing out of scope |

---

## 12. Acceptance criteria

1. `POST /v1/groups/{group_id}/files/{file_id}/versions` returns 201 + `FileVersionResponse`.
2. `files.current_version` increments; `files.total_versions` increments; `files.size_bytes` + `files.mime_type` updated.
3. New `file_versions` row exists with `version = old_current + 1`.
4. All NV1–NV8 scenarios pass.
5. `cargo clippy --workspace ... -D warnings` clean.
6. CI green (all 18 checks).

---

## 13. Cross-references

- Plans 0088–0093: Files API slices 1–6
- Plan 0044: uploads two-phase commit pattern (object write before DB)
- ADR 0004: object storage key schema
- ROADMAP §3.4 Arquivos: `[ ] POST /v1/files/{file_id}:newVersion`

---

## 14. Estimativa

- T1: 15 min
- T2: 45 min
- T3: 60 min
- T4: 20 min
- T5: 20 min
- **Total:** ~2.5 h (provável)
