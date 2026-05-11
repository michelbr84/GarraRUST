# Plan 0099 — GAR-577: Files REST API slice 9 — POST /v1/groups/{group_id}/files

## §1 Goal

Add `POST /v1/groups/{group_id}/files` — the missing file-creation endpoint. Slices 1-8 (plans 0088-0095) built every operation on existing files (list, rename, get, download, soft-delete, new version, list versions, folder CRUD) but left the initial file+v1 creation gap unfilled. This slice closes it.

## §2 Architecture

Reuses the same two-phase commit pattern as `post_new_version` (plan 0094):

1. Validate MIME type from `Content-Type` header (fail-fast 415).
2. Validate `X-File-Name` header (1-500 chars, no control chars, no leading/trailing whitespace).
3. Optionally validate `X-Folder-Id` header (folder must exist in group and not be soft-deleted).
4. Read body bytes with operator cap (`storage.max_patch_bytes`).
5. Compute SHA-256 of body.
6. Open Postgres transaction + set RLS context (same `set_rls_context` helper).
7. Resolve `created_by_label` from `users.display_name` (same pattern as `create_folder`).
8. `ObjectStore::put` (blob-first — runs **before** COMMIT).
9. INSERT into `files` (id, group_id, folder_id, name, current_version=1, total_versions=1, size_bytes, mime_type, created_by, created_by_label).
10. INSERT into `file_versions` (file_id, group_id, version=1, object_key, etag, checksum_sha256, integrity_hmac, size_bytes, mime_type, created_by, created_by_label).
11. Emit `WorkspaceAuditAction::FileCreated` audit event (new variant).
12. COMMIT → 201 + `FileCreatedResponse`.

Object key scheme: `groups/{group_id}/files/{file_id}/v1/{version_uuid}` (consistent with `post_new_version`'s `groups/{group_id}/files/{file_id}/v{N}/{version_uuid}`).

## §3 Tech stack

- Rust stable, Axum 0.8, sqlx 0.8, garraia-storage ObjectStore trait.
- No new dependencies; no schema migration (files/file_versions already exist — migration 003).
- Feature: `garraia-gateway/test-helpers` for integration test harness.

## §4 Design invariants

- **Two-phase ordering**: ObjectStore PUT before Postgres COMMIT (orphaned blob on rollback is acceptable per ADR 0004 §5.3.1).
- **No `unwrap()`** outside tests.
- **No SQL string concat**: all queries use `sqlx::query()` with `bind()`.
- **No PII in audit metadata**: carry `name_len` not `name` in audit payload.
- **MIME allow-list**: delegate to `garraia_storage::mime_allowlist::is_mime_allowed`.
- **Cross-group guard**: `path_group_id` must equal `principal.group_id` (same as all other file endpoints).
- **RLS context**: set both `app.current_user_id` AND `app.current_group_id` via parameterized `set_config` (plan 0056 pattern).

## §5 Validações pré-plano

- [x] `files` schema: `name NOT NULL CHECK (length(name) BETWEEN 1 AND 500)`, `folder_id` nullable with compound FK → `folders(id, group_id)` (MATCH SIMPLE, so NULL is valid for root files). Verified in migration 003.
- [x] `file_versions` schema: compound FK `(file_id, group_id) REFERENCES files(id, group_id)` + `object_key UNIQUE`. Verified in migration 003.
- [x] `WorkspaceAuditAction` enum in `garraia-auth/src/audit_workspace.rs` already has `FileDeleted`, `FileRenamed`, `FileVersionCreated` — adding `FileCreated` follows the same pattern.
- [x] `create_folder` handler (plan 0092) demonstrates the `display_name` lookup pattern.
- [x] Route `/v1/groups/{group_id}/files` only has `get()` — no `post()` — confirmed in `mod.rs` line 412.

## §6 Out of scope

- Presigned URL initUpload/completeUpload pattern.
- tus resumable upload (already shipped, GAR-395 / plan 0047).
- Virus scanning hook (feature flag `av-clamav`).
- Folder-name deduplication within a group (not required by schema).

## §7 Rollback

No schema migration → rollback = revert the PR. No data loss risk since no migration is included.

## §8 File structure (changes)

```
crates/garraia-auth/src/audit_workspace.rs       — add FileCreated variant + as_str() + test
crates/garraia-gateway/src/rest_v1/files.rs      — add FileCreatedResponse struct, validate_file_name(), create_file() handler
crates/garraia-gateway/src/rest_v1/mod.rs        — add .post(files::create_file) to route + fail-soft branch
tests/rest_v1_files_create.rs (new)              — integration tests
plans/0099-gar-577-files-api-slice9-create.md    — this file
plans/README.md                                  — new row
```

## §9 M1 tasks

- [ ] T1: Add `FileCreated` variant to `WorkspaceAuditAction` (audit_workspace.rs) — include `as_str()` arm + unit test assertion.
- [ ] T2: Add `FileCreatedResponse` struct + `validate_file_name()` helper in `files.rs`.
- [ ] T3: Implement `create_file()` handler in `files.rs`.
- [ ] T4: Register route in `mod.rs` (full + fail-soft branches).
- [ ] T5: Write integration test file `tests/rest_v1_files_create.rs` (TDD: write tests first, verify red, then fix with T3).
- [ ] T6: `cargo check -p garraia-gateway -p garraia-auth` green.
- [ ] T7: `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` green.
- [ ] T8: Update `plans/README.md` + ROADMAP.md checkboxes.

## §10 Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Compound FK insertion order (files before file_versions) | Low | INSERT files first (get file_id) then INSERT file_versions referencing it |
| `object_key` collision (UUID-based, statistically impossible) | Negligible | UNIQUE index on `file_versions.object_key` returns 409 on conflict |
| Body read OOM on large files | Low | `to_bytes(body, cap)` with operator cap (same as `post_new_version`) |
| Orphaned blob on Postgres rollback | Accepted | Per ADR 0004 §5.3.1; future maintenance job reclaims |

## §11 Acceptance criteria

- `POST /v1/groups/{group_id}/files` returns 201 with file_id + version=1.
- MIME not in allow-list → 415.
- Body exceeds cap → 413.
- Missing `X-File-Name` → 400.
- Invalid name (empty, >500, control char) → 400.
- Bad `X-Folder-Id` (not found / soft-deleted in group) → 400.
- Cross-group `path_group_id` mismatch → 403.
- No storage configured → 503.
- Integration test: newly created file queryable via `GET /v1/groups/{group_id}/files/{file_id}`.
- Audit event `file.created` present in `audit_events` table after happy path.
- All CI checks green (Format, Clippy, Test×3, Build, MSRV, cargo-deny, Security Audit, Coverage, CodeQL, Playwright, E2E, Secret Scan, Dependency Review).

## §12 Open questions

None — all design decisions resolved by the existing `post_new_version` + `create_folder` patterns.

## §13 Cross-references

- Plan 0088 (GAR-555): files slice 1 (list + delete).
- Plan 0094 (GAR-567): files slice 7 (new version — pattern reused here).
- Plan 0092 (GAR-562): folder POST (creator_label resolution pattern reused here).
- ADR 0004: object storage design.

## §14 Estimativa

- Implementação: ~3h
- LOC: ~250 Rust + ~150 test
- CI wall-time: ~25 min
