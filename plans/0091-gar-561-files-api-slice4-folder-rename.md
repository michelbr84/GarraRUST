# Plan 0091 — Files REST API slice 4: PATCH folder rename

**GAR-561** · epic:ws-api · Fase 3.4 "Arquivos"
**Branch:** `feat/gar-561-files-api-slice4-folder-rename`
**Date:** 2026-05-09 (America/New_York)

---

## Goal

Land slice 4 of the Files REST surface: a single endpoint that lets a member rename a folder inside their group.

- `PATCH /v1/groups/{group_id}/folders/{folder_id}` — body `{ "name": "..." }` → 200 + updated `FolderSummary`

Schema (`folders`) and FORCE RLS are already live (migration 003, GAR-387). Authz primitive `Action::FilesWrite` exists and is granted to Owner/Admin/Member (GAR-391c can() matrix, lines 38/63/106 of `garraia-auth/src/can.rs`). This slice adds:

1. One new `WorkspaceAuditAction::FolderRenamed` variant (= `"folder.renamed"`).
2. One handler `patch_folder` in `crates/garraia-gateway/src/rest_v1/files.rs` (extends, does not duplicate, the slice 1–3 module).
3. One `.patch(...)` chained on the existing `/v1/groups/{group_id}/folders/{folder_id}` route in 3 router-build modes.
4. One utoipa entry in `openapi.rs`.
5. One integration test file `tests/rest_v1_folders_patch.rs`, single `#[tokio::test]` bundling 8 scenarios.

**Rescope note.** GAR-561 was originally created with a wider declared scope (POST + PATCH + DELETE folder CRUD). This session ships PATCH only; folder POST + DELETE move to a follow-up issue (GAR-562 or sibling).

---

## Architecture

```
PATCH /v1/groups/{group_id}/folders/{folder_id}
  → RestV1FullState → AppPool → garraia_app role
  → require_group_id(principal) → 400 if X-Group-Id missing
  → check_group_match(path_group_id, principal.group_id) → 403 mismatch
  → can(principal, Action::FilesWrite) → 403 if denied
  → validate_folder_name: trim → 1..=200 chars, reject '/', NUL → 400
  → BEGIN
    → SET LOCAL app.current_user_id / app.current_group_id (set_config)
    → UPDATE folders SET name = $1, updated_at = now()
        WHERE id = $2 AND group_id = $3 AND deleted_at IS NULL
        RETURNING id, name, parent_id, created_by, created_by_label,
                  created_at, updated_at
    → 0 rows → return 404 (rolls back tx)
    → on UniqueViolation (SQLSTATE 23505 from
        folders_unique_name_per_parent_idx) → return 409 Conflict
        with PII-safe detail (no echo of the conflicting name)
    → audit_workspace_event(FolderRenamed,
        metadata = { folder_id, group_id, name_len })
  → COMMIT
  ← 200 + FolderSummary (updated)
```

`name_len` is computed in characters (`name.chars().count()`) so multi-byte UTF-8 names report a stable count regardless of byte width. Pattern matches the `name_len` already emitted by `FileRenamed` (plan 0089). `folder_id` is duplicated alongside `resource_id` in the metadata for log-search ergonomics — consumers can filter on `metadata.folder_id` without parsing `resource_id`.

---

## Tech stack

- **Rust / Axum 0.8** — `patch` route, `Path`, `Json`, `State`, `FromRequestParts`
- **sqlx 0.8** — parameterized `query_as` over `Postgres`
- **garraia-auth** — `Principal`, `can()`, `Action::FilesWrite`, `WorkspaceAuditAction::FolderRenamed` (NEW), `audit_workspace_event`
- **utoipa** — `#[utoipa::path]` annotation for OpenAPI 3.1
- **integration test** — same `Harness::get().await` + `seed_user_with_group` pattern as `tests/rest_v1_files_patch.rs`

---

## Design invariants

1. **RLS dual-GUC**: SET LOCAL `app.current_user_id` AND `app.current_group_id` via `set_config($_, $1, true)` before the UPDATE (plan 0056 / 0088 / 0089 pattern reused via the `set_rls_context` already present in `files.rs`).
2. **Group-ID cross-check**: path `{group_id}` must equal `principal.group_id`; mismatch → 403 (uses `check_group_match` already in `files.rs`).
3. **No PII in audit metadata**: carry `folder_id`, `group_id`, and `name_len` — never the raw `name` (mirrors `FileRenamed` and the family-wide pattern from `ChatCreated`/`MemberCreated`).
4. **Soft-deleted folders not renameable**: WHERE `deleted_at IS NULL` → 0 rows → 404. We do NOT distinguish "not in group" vs "deleted" vs "never existed" — RLS already filters cross-group, the explicit `group_id = $3` clause is belt-and-suspenders, and 404 is the same for all three cases (plan 0088 §"Out of scope" Q1 precedent).
5. **Validation in app layer**: `name` 1..=200 chars (matches DB CHECK on `folders.name` at `migrations/003_files_and_folders.sql:59` — note: 200, not 500 like `files.name`), trimmed, rejecting `/` and NUL byte. The DB CHECK is the safety net; the app-layer 400 gives callers a precise error message instead of a generic `23514 check_violation`.
6. **23505 → 409 Conflict.** `folders_unique_name_per_parent_idx UNIQUE (group_id, COALESCE(parent_id, nil_uuid), name) WHERE deleted_at IS NULL` (migration 003:88-90) means a rename to a sibling-already-using name raises Postgres `23505`. Plan 0089 file rename did NOT need this branch (files have no name UNIQUE). **Folder rename MUST catch and translate to 409 Conflict** with PII-safe body (no echo of conflicting name) — leaking 5xx for a user-recoverable conflict is wrong UX. This is the only handler-shape delta vs plan 0089.
7. **No partial fields in this slice**: body MUST contain `name`. Other fields (`parent_id` for moves, `settings`) are out of scope and not deserialized — extra JSON keys are silently ignored by `serde::Deserialize` default behavior, but only `name` is read.

---

## Validações pré-plano

- [x] `Action::FilesWrite` exists and is in the `can()` matrix for Owner/Admin/Member (`crates/garraia-auth/src/can.rs:38,67,106`)
- [x] `set_rls_context` and `check_group_match` helpers already present in `files.rs` (slice 1)
- [x] `audit_workspace_event` is the public API for inserting `audit_events` rows
- [x] `folders.name` DB CHECK is `length(name) BETWEEN 1 AND 200` (`crates/garraia-workspace/migrations/003_files_and_folders.sql:59`)
- [x] `folders` has `updated_at` column and FORCE RLS via `app.current_group_id`
- [x] `folders_unique_name_per_parent_idx` covers (group_id, COALESCE(parent_id, nil_uuid), name) where deleted_at IS NULL (`migrations/003_files_and_folders.sql:88-90`)
- [x] `RestError::Conflict(String)` already maps to 409 (`rest_v1/problem.rs:50,95,112`)
- [x] 23505 mapping pattern verified against existing handlers (`groups.rs:755`, `invites.rs:269`, `messages.rs:614`)
- [x] No existing `rest_v1_folders*` integration test — bootstrap a fresh `tests/rest_v1_folders_patch.rs`

---

## Out of scope

- Create folder (`POST /v1/groups/{group_id}/folders`) — needs same-group `parent_id` validation + cycle protection + initial-name UNIQUE handling. Move to GAR-562 / slice 5.
- Soft-delete folder (`DELETE /v1/groups/{group_id}/folders/{folder_id}`) — needs cascade semantics (subfolders? files inside?). Move to GAR-562 / slice 5.
- Move folder (`parent_id` mutation) — needs cross-group destination validation + cycle protection. Slice 6+.
- Cascading soft-delete of children files / subfolders.
- Hard delete / restore from trash (deferred indefinitely).

---

## Rollback

This slice is handler + audit-enum + 1 test file + 1 doc file. Rolling back =
revert the squash commit; no DB migration changes. The `FolderRenamed`
enum variant is additive — removing it later only matters if any
audit consumer dispatched on it (none today).

---

## File structure

```
crates/garraia-auth/src/
  audit_workspace.rs                 ← ADD FolderRenamed variant + as_str arm + 2 unit-test entries
crates/garraia-gateway/src/rest_v1/
  files.rs                           ← ADD PatchFolderRequest DTO + ERR_FOLDER_* consts + validate_folder_name + patch_folder handler + 8 unit tests
  mod.rs                             ← chain .patch(files::patch_folder) on /v1/groups/{group_id}/folders/{folder_id} in 3 build modes
  openapi.rs                         ← ADD PatchFolderRequest import + super::files::patch_folder path + PatchFolderRequest schema entry
crates/garraia-gateway/tests/
  rest_v1_folders_patch.rs           ← NEW — single #[tokio::test] with 8 scenarios
plans/
  0091-gar-561-files-api-slice4-folder-rename.md   ← NEW (this file)
  README.md                          ← + row 0091
```

No migrations.

---

## Integration scenarios

Bundled into one `#[tokio::test] async fn v1_folders_patch_scenarios()` to avoid the sqlx runtime-teardown race (plan 0016 M3 commit `4f8be37`).

| # | Description | Expected |
|---|-------------|----------|
| F1 | Owner renames live folder (happy path) | 200 + name updated + audit row `folder.renamed` with `{folder_id, group_id, name_len}` and NO `name` key |
| F2 | Owner renames soft-deleted folder | 404; DB name preserved |
| F3 | Owner renames folder_id that does not exist | 404 |
| F4 | Empty name (after trim) | 400 |
| F5 | Name exceeding 200 chars | 400 (boundary: 200 chars OK, 201 fails) |
| F6 | Name containing `/` | 400 |
| F7 | Path `group_id` ≠ principal group_id | 403 |
| F8 | Rename collides with sibling under same parent (UNIQUE) | 409 Conflict; DB name preserved; body MUST NOT echo the conflicting name |

`401 missing bearer` is covered by router-level middleware (already validated in `rest_v1_chats.rs` C4); not duplicated here. NUL-byte rejection is exercised by `validate_folder_name_rejects_nul_byte` unit test in `files::tests`.

---

## Verification

End-to-end:

1. `cargo fmt --check`
2. `cargo check -p garraia-auth -p garraia-gateway`
3. `cargo check -p garraia-gateway --tests --features garraia-gateway/test-helpers`
4. `cargo clippy -p garraia-auth -p garraia-gateway --tests --features garraia-gateway/test-helpers --no-deps -- -D warnings`
5. `cargo test -p garraia-auth audit_workspace` (3 unit tests including new `FolderRenamed.as_str` assertion + `distinct_strings` entry)
6. `cargo test -p garraia-gateway --lib rest_v1::files::tests` (existing 18 + new 8 = 26 unit tests)
7. `cargo test -p garraia-gateway --test rest_v1_folders_patch --features garraia-gateway/test-helpers` (8 scenarios, requires Postgres harness)
8. `cargo audit --no-fetch` — 22 warnings, 0 errors (unchanged baseline)
9. `cargo deny check` — advisories ok, bans ok, licenses ok, sources ok

---

## Regras absolutas

| Regra | Aplicação |
|-------|-----------|
| Zero unwrap em prod | handler usa `?` propagation; tests podem usar `.expect("…")` |
| Só queries parametrizadas | `sqlx::query_as` com `.bind` |
| PII out of audit | metadata só carrega `folder_id`, `group_id`, `name_len` (usize) — nunca o raw name |
| RLS forçada em todas leituras de tenant | `set_rls_context` antes do UPDATE |
| Documentar por que não é apenas RLS | §Design invariants 4 explica o belt-and-suspenders WHERE |
| 23505 nunca vira 5xx | §Design invariants 6 + handler match arm explícito |

---

## Próximos slices possíveis (não-bloqueantes)

- **slice 5** — `POST /v1/groups/{group_id}/folders` (create folder) + `DELETE /v1/groups/{group_id}/folders/{folder_id}` (soft-delete folder). GAR-562.
- **slice 6** — `POST /v1/groups/{group_id}/folders/{folder_id}/move` (move between parents).
- **slice 7** — version mgmt (`POST /v1/files/{file_id}/versions`, `GET versions`, `download`).
