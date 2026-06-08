# Plan 0283 — GAR-820: GET /v1/groups/{group_id}/files/{file_id}/versions/{version}

**Status:** In Progress  
**Linear:** [GAR-820](https://linear.app/chatgpt25/issue/GAR-820)  
**Branch:** `routine/202606080700-get-file-version`  
**Date:** 2026-06-08 (America/New_York)

---

## Goal

Add `GET /v1/groups/{group_id}/files/{file_id}/versions/{version}` — fetch a single file version
by its integer version number. Closes the single-item gap in the file versions CRUD:

| Method | Path | Plan | Status |
|--------|------|------|--------|
| POST | `.../versions` | 0094 / GAR-567 | ✅ Done |
| GET  | `.../versions` | 0095 / GAR-569 | ✅ Done |
| GET  | `.../versions/{version}` | **0283 / GAR-820** | 🚧 This plan |

---

## Schema context

`file_versions` table (migration 003):

```sql
PK: (file_id uuid, version int)
Columns: group_id, object_key, etag, checksum_sha256, integrity_hmac,
         size_bytes, mime_type, created_by, created_by_label, created_at
```

`{version}` is a positive integer — not a UUID.

---

## Implementation

### 1. `garraia-auth/src/audit_workspace.rs`

Add `FileVersionRead` variant after `FileVersionsListed`:

```rust
/// Emitted when GET .../versions/{version} returns successfully (plan 0283, GAR-820).
/// resource_type = "files", resource_id = "{file_id}".
/// Metadata: { file_id, group_id, version: i32 } — PII-safe.
FileVersionRead,
```

String mapping: `"file.version.read"`.

### 2. `garraia-gateway/src/rest_v1/files.rs`

New `get_file_version` handler (inserted before `#[cfg(test)]`):

- `Path<(Uuid, Uuid, i32)>` for `(path_group_id, file_id, version)`
- `require_group_id` + cross-group check + `can(FilesRead)`
- `set_rls_context` → single `fetch_optional` on `file_versions WHERE file_id=$1 AND group_id=$2 AND version=$3`
- 404 if `None` — covers file-not-found, version-not-found, cross-group (no existence leak)
- Audit `FileVersionRead` with `{ file_id, group_id, version }`
- Returns `Json<FileVersionSummary>` — reuses existing type

### 3. `garraia-gateway/src/rest_v1/mod.rs`

Add route in all 3 branches:

```
/v1/groups/{group_id}/files/{file_id}/versions/{version}  →  get(files::get_file_version)
```

(fail-soft and no-auth stubs use `unconfigured_handler`)

### 4. `garraia-gateway/src/rest_v1/openapi.rs`

Add `super::files::get_file_version` to `paths(...)` list.
No new schema — response reuses `FileVersionSummary`.

---

## Unit tests (6)

1. `file_version_summary_serializes_all_fields` — all 7 fields present
2. `file_version_summary_nil_created_by_is_none` — `Option<Uuid>` None path
3. `file_version_summary_created_at_utc_z` — `created_at` serializes with trailing `Z`
4. `file_version_summary_nil_uuid_round_trips` — Uuid::nil() preserved
5. `file_version_summary_version_integer_preserved` — `"version":42` in JSON
6. `file_version_summary_large_size_bytes_preserved` — 5 GiB i64 intact

---

## Acceptance

- `cargo check -p garraia-gateway` ✅
- `cargo clippy --workspace --tests --exclude garraia-desktop --no-deps -- -D warnings` ✅ (0 warnings)
- `cargo test -p garraia-gateway --lib` ✅ 667 tests pass
- ROADMAP §3.4 updated ✅
- `plans/README.md` row 0283 added ✅
