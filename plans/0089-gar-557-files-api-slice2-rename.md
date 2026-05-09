# Plan 0089 — Files REST API slice 2: PATCH rename

**GAR-557** · epic:ws-api · Fase 3.4 "Arquivos"
**Branch:** `feat/gar-557-files-api-slice2-rename`
**Date:** 2026-05-09 (America/New_York)

---

## Goal

Land the second slice of the Files REST surface: a single endpoint that lets a member rename a file inside their group.

- `PATCH /v1/groups/{group_id}/files/{file_id}` — body `{ "name": "..." }` → 200 + updated `FileSummary`

Schema (`files`) and FORCE RLS are already live (migration 003, GAR-387). Authz primitive `Action::FilesWrite` exists and is granted to Owner/Admin/Member (GAR-391c can() matrix, lines 141/168/193 of `garraia-auth/src/can.rs`). This slice adds:

1. One new `WorkspaceAuditAction::FileRenamed` variant (= `"file.renamed"`).
2. One handler `patch_file` in `crates/garraia-gateway/src/rest_v1/files.rs` (extends, does not duplicate, the slice 1 module).
3. One route in 3 router-build modes (handlers / unconfigured fail-soft / no-auth stub).
4. One utoipa entry in `openapi.rs`.
5. One integration test file `tests/rest_v1_files_patch.rs`, single `#[tokio::test]` bundling 7 scenarios.

---

## Architecture

```
PATCH /v1/groups/{group_id}/files/{file_id}
  → RestV1FullState → AppPool → garraia_app role
  → require_group_id(principal) → 400 if X-Group-Id missing
  → check_group_match(path_group_id, principal.group_id) → 403 mismatch
  → can(principal, Action::FilesWrite) → 403 if denied
  → validate name: trim → 1..=500 chars, reject '/', NUL → 400
  → BEGIN
    → SET LOCAL app.current_user_id / app.current_group_id (set_config)
    → UPDATE files SET name = $1, updated_at = now()
        WHERE id = $2 AND group_id = $3 AND deleted_at IS NULL
        RETURNING id, name, mime_type, size_bytes, current_version,
                  total_versions, folder_id, created_by,
                  created_by_label, created_at, updated_at
    → 0 rows → return 404 (rolls back tx)
    → audit_workspace_event(FileRenamed, metadata = { name_len, group_id })
  → COMMIT
  ← 200 + FileSummary (updated)
```

`name_len` is computed in characters (`name.chars().count()`) so multi-byte
UTF-8 names report a stable count regardless of byte width. Pattern matches the
`name_len` already emitted by `FileDeleted` (plan 0088 line 504).

---

## Tech stack

- **Rust / Axum 0.8** — `patch` route, `Path`, `Json`, `State`, `FromRequestParts`
- **sqlx 0.8** — parameterized `query_as` over `Postgres`
- **garraia-auth** — `Principal`, `can()`, `Action::FilesWrite`, `WorkspaceAuditAction::FileRenamed` (NEW), `audit_workspace_event`
- **utoipa** — `#[utoipa::path]` annotation for OpenAPI 3.1
- **integration test** — same `Harness::get().await` + `seed_user_with_group` pattern as `tests/rest_v1_chats.rs`

---

## Design invariants

1. **RLS dual-GUC**: SET LOCAL `app.current_user_id` AND `app.current_group_id` via `set_config($_, $1, true)` before the UPDATE (plan 0056 / 0088 pattern reused via `set_rls_context` already present in `files.rs:173`).
2. **Group-ID cross-check**: path `{group_id}` must equal `principal.group_id`; mismatch → 403 (uses `check_group_match` already in `files.rs:201`).
3. **No PII in audit metadata**: carry `name_len` and `group_id` — never the raw `name` (mirrors `FileDeleted` and the family-wide pattern from `ChatCreated`/`MemberCreated`).
4. **Soft-deleted files not renameable**: WHERE `deleted_at IS NULL` → 0 rows → 404. We do NOT distinguish "not in group" vs "deleted" vs "never existed" — RLS already filters cross-group, the explicit `group_id = $3` clause is belt-and-suspenders, and 404 is the same for all three cases (plan 0088 §"Out of scope" Q1 precedent).
5. **Validation in app layer**: `name` 1..=500 chars (matches DB CHECK on `files.name`), trimmed, rejecting `/` and NUL byte. The DB CHECK is the safety net; the app-layer 400 gives callers a precise error message instead of a generic `23514 check_violation`.
6. **No partial fields in this slice**: body MUST contain `name`. Other fields (folder_id, settings) are out of scope and not deserialized — extra JSON keys are silently ignored by `serde::Deserialize` default behavior, but only `name` is read.

---

## Validações pré-plano

- [x] `Action::FilesWrite` exists and is in the `can()` matrix for Owner/Admin/Member (`crates/garraia-auth/src/can.rs:141,168,193`)
- [x] `set_rls_context` and `check_group_match` helpers already present in `files.rs` (slice 1)
- [x] `audit_workspace_event` is the public API for inserting `audit_events` rows
- [x] `files.name` DB CHECK is `length(name) BETWEEN 1 AND 500` (`crates/garraia-workspace/migrations/003_files_and_folders.sql:104`)
- [x] `files` has `updated_at` column and FORCE RLS via `app.current_group_id`
- [x] No existing `rest_v1_files*` integration test — bootstrap a fresh `tests/rest_v1_files_patch.rs`

---

## Out of scope

- Move between folders (`folder_id` mutation) — needs same-group destination validation + cycle protection (slice 3+)
- MIME / settings overrides (slice 3+)
- Hard delete / restore from trash (deferred indefinitely)
- Folder rename — analogous endpoint will land in a sibling slice

---

## Rollback

This slice is handler + audit-enum + 1 test file. Rolling back =
revert the squash commit; no DB migration changes. The `FileRenamed`
enum variant is additive — removing it later only matters if any
audit consumer dispatched on it (none today).

---

## File structure

```
crates/garraia-auth/src/
  audit_workspace.rs      ← ADD FileRenamed variant + as_str arm + as_str unit test
crates/garraia-gateway/src/rest_v1/
  files.rs                ← ADD PatchFileRequest DTO + patch_file handler
  mod.rs                  ← ADD .route(..., patch(files::patch_file)) in handler-build mode + 2 stubs
  openapi.rs              ← ADD super::files::patch_file path
crates/garraia-gateway/tests/
  rest_v1_files_patch.rs  ← NEW — single #[tokio::test] with 7 scenarios
plans/
  0089-gar-557-files-api-slice2-rename.md  ← NEW (this file)
```

No migrations.

---

## Integration scenarios

Bundled into one `#[tokio::test] async fn v1_files_patch_scenarios()` to avoid
the sqlx runtime-teardown race (plan 0016 M3 commit `4f8be37`).

| # | Description | Expected |
|---|-------------|----------|
| F1 | Owner renames live file (happy path) | 200 + name updated + audit row `file.renamed` with `name_len` |
| F2 | Owner renames soft-deleted file | 404 |
| F3 | Owner renames file_id that does not exist | 404 |
| F4 | Empty name (after trim) | 400 |
| F5 | Name exceeding 500 chars | 400 |
| F6 | Name containing `/` | 400 |
| F7 | Path `group_id` ≠ principal group_id | 403 |

`401 missing bearer` is covered by router-level middleware (already validated
in `rest_v1_chats.rs` C4); not duplicated here.

---

## Verification

End-to-end:

1. `cargo fmt --check`
2. `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings`
3. `cargo test -p garraia-auth audit_workspace` (existing audit unit tests + new `FileRenamed.as_str` assertion)
4. `cargo test -p garraia-gateway --test rest_v1_files_patch` (7 scenarios, requires Postgres harness)
5. CI: 18/18 checks green (unchanged set from PR #235/237)
6. `cargo audit --no-fetch` — 22 warnings, 0 errors (unchanged)

---

## Regras absolutas

| Regra | Aplicação |
|-------|-----------|
| Zero unwrap em prod | handler usa `?` propagation; tests podem usar `.expect("…")` |
| Só queries parametrizadas | `sqlx::query_as` com `.bind` |
| PII out of audit | metadata só carrega `name_len: usize` + `group_id: Uuid` |
| RLS forçada em todas leituras de tenant | `set_rls_context` antes do UPDATE |
| Documentar por que não é apenas RLS | §Design invariants 4 explica o belt-and-suspenders WHERE |

---

## Próximos slices possíveis (não-bloqueantes)

- **slice 3** — `PATCH /v1/groups/{group_id}/folders/{folder_id}` (rename folder)
- **slice 4** — `POST /v1/groups/{group_id}/folders` (create folder)
- **slice 5** — `POST /v1/groups/{group_id}/files/{file_id}:move` (move between folders)
- **slice 6** — version mgmt (`POST /v1/files/{file_id}/versions`, `GET versions`, `download`)
