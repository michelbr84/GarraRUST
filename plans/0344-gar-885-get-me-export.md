# Plan 0344 — GET /v1/me/export — Personal Data Export (LGPD art. 20 / GDPR arts. 15 & 20)

**Issue:** [GAR-885](https://linear.app/chatgpt25/issue/GAR-885)
**Parent epic:** [GAR-400](https://linear.app/chatgpt25/issue/GAR-400) — Endpoints de export e delete (direitos do titular)
**Branch:** `routine/202606150024-get-me-export`
**Plan slug:** `0344-gar-885-get-me-export`

---

## Goal

Implement `GET /v1/me/export` — LGPD art. 20 / GDPR arts. 15 & 20 right to data portability.
Returns a structured JSON export of the authenticated caller's personal account-level data.

## Scope (Slice 2 of GAR-400)

**Included in this slice (account-level, no cross-group FORCE RLS traversal):**
- `profile` — display_name, email, status, created_at from `users WHERE id = caller_id`
- `sessions` — active sessions (id, device_id, expires_at, created_at)
- `api_keys` — metadata (id, label, scopes, created_at, revoked_at) — hash never returned
- `audit_events` — account-level events (actor_user_id = caller_id AND group_id = nil-uuid, up to 1000)
- `group_memberships` — (group_id, group_name, group_type, role, joined_at) from group_members JOIN groups

**Deferred to Slice 3:**
- Messages, files, memory items, tasks — require per-group FORCE RLS traversal (separate issue)

## Architecture

### Endpoint
`GET /v1/me/export`

### Auth
- `Principal` extractor (Bearer JWT, no `X-Group-Id` required)
- `Action::ExportSelf` capability check — granted to all roles (admin/member/guest)
- Note: ExportSelf does not need group context since account-level data is user-scoped

### Response
- `200 OK`, `Content-Type: application/json`
- `Content-Disposition: attachment; filename="garraia-export-{date}.json"` header
- Body: `ExportMeResponse` (see schema below)

### Database queries
All queries use `app_pool` with SET LOCAL `app.current_user_id = caller_id` and `app.current_group_id = nil_uuid`.
- `users`: `WHERE id = $1` — no FORCE RLS (tenant-root table)
- `sessions`: `WHERE user_id = $1` — no FORCE RLS (tenant-root table)
- `api_keys`: `WHERE user_id = $1` — no FORCE RLS (tenant-root table)
- `audit_events`: `WHERE actor_user_id = $1 AND group_id = $2` (nil_uuid) — NOT FK (design intent)
- `group_members JOIN groups`: `WHERE gm.user_id = $1` — FORCE RLS needs user context

### Schema

```json
{
  "exported_at": "2026-06-15T00:24:00Z",
  "schema_version": "1",
  "profile": {
    "user_id": "...",
    "display_name": "...",
    "email": "...",
    "status": "active",
    "account_created_at": "..."
  },
  "sessions": [
    { "id": "...", "device_id": "...", "expires_at": "...", "created_at": "..." }
  ],
  "api_keys": [
    { "id": "...", "label": "...", "scopes": [], "created_at": "...", "revoked_at": null }
  ],
  "audit_events": [
    { "id": "...", "action": "...", "resource_type": "...", "resource_id": "...", "metadata": {}, "created_at": "..." }
  ],
  "group_memberships": [
    { "group_id": "...", "group_name": "...", "group_type": "...", "role": "...", "joined_at": "..." }
  ]
}
```

## Design Invariants

- **PII**: `email` IS included (right-to-portability requires the data subject to receive their own email). `password_hash` NEVER returned. Raw API key NEVER returned. JWT tokens NEVER returned.
- **No PII in audit metadata**: audit events metadata field carries only what was already stored (e.g., counts, IDs). No re-emission needed.
- **Audit of the export itself**: emit `AccountDataExported` audit event (`account.data_exported`) with metadata `{ "sections": ["profile","sessions","api_keys","audit_events","group_memberships"] }`.
- **Limits**: audit_events capped at 1000 DESC created_at (full export via paginated `GET /v1/me/audit`). Sessions and api_keys: all rows (typically <100).
- **Group membership**: only active memberships returned (`gm.status = 'active'` WHERE applicable).

## Out of Scope

- ZIP packaging (future enhancement)
- Cross-group message/file/memory/task content (slice 3, separate issue)
- Async background export jobs (beyond LGPD art. 20 basic requirement)

## Rollback

PR revert + `git revert <sha>` — no migration needed (no schema change).

## Files Changed

| File | Change |
|------|--------|
| `crates/garraia-auth/src/audit_workspace.rs` | Add `AccountDataExported` variant + `as_str()` arm + test array entry |
| `crates/garraia-gateway/src/rest_v1/me.rs` | Add `export_me` handler + response types + `#[utoipa::path]` + 6 unit tests |
| `crates/garraia-gateway/src/rest_v1/mod.rs` | Wire `GET /v1/me/export` in all 3 routing branches |
| `crates/garraia-gateway/src/rest_v1/openapi.rs` | Register path + import `export_me` + response types |
| `ROADMAP.md` | Add `GET /v1/me/export` ✅ in §3.4 |
| `plans/README.md` | Add row 0344 |
| `plans/0344-gar-885-get-me-export.md` | This file |

## Tasks

### T1 — Add `AccountDataExported` to audit_workspace.rs
- [x] Add variant before closing brace of `WorkspaceAuditAction` enum
- [x] Add `as_str()` arm: `"account.data_exported"`
- [x] Add to test exhaustiveness array

### T2 — Implement `export_me` handler in me.rs
- [x] `ExportMeProfile`, `ExportMeSession`, `ExportMeApiKey`, `ExportMeAuditEvent`, `ExportMeGroupMembership`, `ExportMeResponse` types with `Serialize + ToSchema`
- [x] `#[utoipa::path]` annotation
- [x] Handler: SET LOCAL both vars, 5 queries, audit emit, `axum::response::Response` with headers
- [x] 6 unit tests

### T3 — Wire routing in mod.rs
- [x] Add `.route("/v1/me/export", get(me::export_me))` in all 3 branches (full, auth-stub, no-auth stub)

### T4 — Register in openapi.rs
- [x] Import `export_me` + response types
- [x] Add `super::me::export_me` to paths list

### T5 — Update ROADMAP.md and plans/README.md
- [x] Mark `GET /v1/me/export` ✅ in §3.4 after `DELETE /v1/me` entry
- [x] Add row 0344 to plans/README.md

## Acceptance Criteria

- `cargo check -p garraia-gateway` clean
- `cargo check -p garraia-auth` clean
- `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` — 0 warnings
- 6 unit tests in me.rs pass
- 1 new test in audit_workspace.rs passes
- CI: 20/20 checks green

## Risk Register

| Risk | Mitigation |
|------|-----------|
| `citext` email column returns String not &str | Use `String` in query_as tuple |
| group_members FORCE RLS fails without group context | Use SET LOCAL user_id + nil group_id — group_members policy is user-scoped |
| api_keys.scopes is JSONB | Bind as `serde_json::Value` |
| Content-Disposition header conflicts with JSON middleware | Use `axum::response::Response` builder, not `Json<T>` wrapper |

## Cross-references

- GAR-400 (parent epic) — Endpoints de export e delete
- GAR-884 (predecessor) — DELETE /v1/me (slice 1)
- ROADMAP.md §3.4 — API REST /v1
- CLAUDE.md — Absolute Rule 6: never expose PII in logs
- CLAUDE.md — Absolute Rule 5: never SQL string concat

## Estimativa

~280 LOC — 2h implementação + testes
