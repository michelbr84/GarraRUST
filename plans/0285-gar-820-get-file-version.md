# Plan 0283 — GAR-820: GET /v1/groups/{group_id}/files/{file_id}/versions/{version}

**Issue:** [GAR-820](https://linear.app/chatgpt25/issue/GAR-820)
**Branch:** `claude/serene-fermat-DuTeY`
**Date:** 2026-06-08 (Florida ET)

## Goal

Add `GET /v1/groups/{group_id}/files/{file_id}/versions/{version}` — fetch a
single file version by its positive integer version number. Complements the
existing list endpoint (GAR-569) and new-version endpoint (GAR-567).

## Files changed

| File | Change |
|------|--------|
| `crates/garraia-auth/src/audit_workspace.rs` | Add `FileVersionRead` variant + `as_str` arm + test list entry |
| `crates/garraia-gateway/src/rest_v1/files.rs` | Add `get_file_version` handler (~90 LOC) + 6 unit tests |
| `crates/garraia-gateway/src/rest_v1/mod.rs` | Wire `GET .../versions/{version}` in all 3 router branches |
| `ROADMAP.md` | Tick GET single version ✅; tick tus ✅; tick §3.6 Busca ✅ |
| `plans/README.md` | Add row 0283 |

## Implementation

### Handler pattern

Follows `list_file_versions` exactly:
1. `require_group_id` → 400 if missing
2. `path_group_id != group_id` → 403
3. `!can(&principal, FilesRead)` → 403
4. Begin tx + `set_rls_context`
5. `SELECT 1 FROM files WHERE id=$1 AND deleted_at IS NULL` → 404 if absent
6. `SELECT version, size_bytes, mime_type, checksum_sha256, created_by,
   created_by_label, created_at FROM file_versions WHERE file_id=$1 AND
   group_id=$2 AND version=$3` → 404 if absent
7. Audit `FileVersionRead` in same tx
8. Commit → 200 `Json(FileVersionSummary)`

### Cross-group isolation

`group_id` column guard in the query + FORCE RLS `set_config` double-locks.
Callers in a different group receive 404 (no existence leak).

### Audit action

`WorkspaceAuditAction::FileVersionRead` → `"file.version.read"`.
Metadata: `{ file_id, group_id, version }` — PII-safe (no filename/MIME).

## Tasks

- [x] Add `FileVersionRead` to `audit_workspace.rs`
- [x] Add `get_file_version` handler to `files.rs`
- [x] Wire route in all 3 router branches in `mod.rs`
- [x] 6 unit tests (all fields preserved, nil created_by, UTC created_at,
      nil UUID round-trip, version integer preserved, large size_bytes)
- [x] ROADMAP.md updated (GET single version ✅, tus ✅, §3.6 Busca ✅)
- [x] plans/README.md row 0283 added
- [ ] CI green
