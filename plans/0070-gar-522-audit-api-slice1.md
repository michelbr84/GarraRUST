# Plan 0070 — GAR-522: REST /v1 audit slice 1 (`GET /v1/groups/{group_id}/audit`)

**Status:** Em execução
**Autor:** Claude Sonnet 4.6 (garra-routine 2026-05-05, America/New_York)
**Data:** 2026-05-05 (America/New_York)
**Issue:** [GAR-522](https://linear.app/chatgpt25/issue/GAR-522)
**Branch:** `routine/202605052222-audit-api-slice1`
**Epic:** `epic:ws-api`
**Parent:** GAR-396

---

## §1 Goal

Land `GET /v1/groups/{group_id}/audit` — a cursor-paginated endpoint exposing
`audit_events` for group owners (ROADMAP §3.4 "Auditoria", LGPD art. 8 §5).

Single endpoint this slice:

- `GET /v1/groups/{group_id}/audit?cursor=<uuid>&limit=<n>&action=<str>&resource_type=<str>`

## §2 Architecture

`audit_events` has **FORCE RLS** via two migrations:

- Migration 007: `CREATE POLICY audit_events_group_or_self … USING (group_id = current_setting('app.current_group_id')::uuid OR actor_user_id = current_setting('app.current_user_id')::uuid)`
- Migration 013: adds `WITH CHECK` to the same policy

The handler must SET LOCAL both `app.current_user_id` **and** `app.current_group_id` using
the parameterized `set_config` approach (plan 0056 — no `format!()` interpolation).

Cross-group isolation: if the path `group_id` ≠ `principal.group_id`, return 404
(don't reveal existence of foreign groups).

No audit event is emitted for reads to avoid circular audit noise.

## §3 Tech stack

- Axum 0.8 + `RestV1FullState` (same as memory.rs, tasks.rs)
- `sqlx::query_as` with `sqlx::types::Json<serde_json::Value>` for the `metadata` jsonb column
- `garraia_auth::{Action, Principal, can}`
- `utoipa::path` for OpenAPI registration
- Integration test in `crates/garraia-gateway/tests/rest_v1_audit.rs`

## §4 Design invariants

1. **Authz gate first**: `can(&principal, Action::ExportGroup)` → 403 if false. Owner-only
   per migration 002 seed (`admin` has `export.self` only, NOT `export.group`).
2. **Cross-group → 404**: `path_group_id ≠ principal.group_id` → 404 (not 403).
3. **SET LOCAL both vars**: `app.current_user_id` + `app.current_group_id` via parameterized
   `set_config` inside the transaction, before any SELECT.
4. **No audit on read**: reading audit events does NOT itself emit an audit event.
5. **Cursor keyset**: `(created_at DESC, id DESC)` — `id` is UUID v4 (random), serves as
   tiebreaker only; the subquery lookup pattern from memory.rs applies here.
6. **`ip` exposed as text**: `ip::text` (inet → string). Owner is entitled to forensic data.
7. **`metadata` as jsonb**: exposed as-is (already PII-safe by audit_workspace_event invariant).
8. **Optional filters**: `?action=<str>` and `?resource_type=<str>` are validated to be
   non-empty if provided; no allow-list (action strings are internal enums from `WorkspaceAuditAction`).

## §5 Validações pré-plano

- [x] `audit_events` table exists with FORCE RLS (migration 007 + 013). Index `audit_events_group_created_idx` on `(group_id, created_at DESC) WHERE group_id IS NOT NULL` supports this query efficiently.
- [x] `Action::ExportGroup` is in the enum (action.rs:41).
- [x] `can()` central table maps `owner` → `ExportGroup` = true; `admin` → false.
- [x] `set_config` parameterized protocol shipped in plan 0056 — reuse exact pattern from memory.rs.
- [x] `RestV1FullState` wired with `app_pool` (mode 1 real, modes 2-3 fail-soft 503).
- [x] No new migration needed.

## §6 Out of scope

- `?actor_user_id=` filter (slice 2).
- `GET /v1/audit/me` personal audit log (slice 2, `ExportSelf`).
- Export download ZIP/CSV — GAR-400 (epic:compliance).
- Write endpoints (audit events are immutable by design).
- `user_events` (login/logout, `group_id IS NULL`) — deferred to slice 2.

## §7 Rollback

This PR adds only new files (`audit.rs`, `rest_v1_audit.rs` test) plus route
wiring in `mod.rs` and `openapi.rs`. Rolling back = revert those additions.
No schema change. No data migration.

## §8 Open questions

- None blocking.

## §9 File structure

```text
crates/garraia-gateway/src/rest_v1/
  audit.rs                    ← NEW (T1-T3)
  mod.rs                      ← MODIFIED: add `pub mod audit;` + route wiring (T4)
  openapi.rs                  ← MODIFIED: register AuditEventSummary + list_audit path (T5)
crates/garraia-gateway/tests/
  rest_v1_audit.rs            ← NEW integration test (T6)
plans/
  0070-gar-522-audit-api-slice1.md   ← this file
  README.md                   ← MODIFIED: add row 0070
ROADMAP.md                    ← MODIFIED: check [x] audit endpoint in §3.4
```

## §10 Tasks (M1)

- [ ] **T1** — `audit.rs`: DTOs (`AuditEventSummary`, `ListAuditResponse`, `ListAuditQuery`) + 4 unit tests (validate query params)
- [ ] **T2** — `audit.rs`: `list_audit` handler — RLS context, authz gate, cursor query (no filter), map rows, next_cursor
- [ ] **T3** — `audit.rs`: add optional `action` + `resource_type` filter branches to the query
- [ ] **T4** — `mod.rs`: `pub mod audit;` + route `GET /v1/groups/:group_id/audit` wired into mode-1 router
- [ ] **T5** — `openapi.rs`: register `AuditEventSummary`, `ListAuditResponse`, `list_audit` path (200/401/403/404)
- [ ] **T6** — `rest_v1_audit.rs`: 6 integration scenarios (A1–A6): happy path, cursor pagination, action filter, cross-group 404, member 403, no JWT 401
- [ ] **T7** — `cargo fmt --all` + `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` green
- [ ] **T8** — ROADMAP §3.4 audit checkbox + plans/README.md row 0070

## §11 Risk register

| Risk | Mitigation |
|------|-----------|
| `ip::text` cast fails on NULL | `ip` column is nullable; `SELECT ip::text` returns NULL if ip IS NULL — handled by `Option<String>` |
| `metadata` jsonb deserialization | Use `serde_json::Value` via `sqlx::types::Json` — flexible, no schema assumption |
| Cursor lookup race (row deleted between pages) | Subquery returns no rows → treated as "cursor not found" → 400 Bad Request |
| Cross-group data leak via RLS bypass | Double-checked: path_group_id == principal.group_id enforced app-layer + RLS enforced db-layer |

## §12 Acceptance criteria

- `GET /v1/groups/{group_id}/audit` returns 200 with a list of `AuditEventSummary` items for an owner after at least one audit event exists (seeded by other endpoints in the integration harness).
- Cursor pagination: first page returns up to `limit` items with `next_cursor`; second page returns items older than the cursor.
- `?action=member.role_changed` returns only matching events.
- Cross-group attempt → 404.
- Member role → 403.
- No JWT → 401.
- `cargo clippy -- -D warnings` and `cargo fmt --check` green.
- CI 16+ checks green.

## §13 Cross-references

- ROADMAP §3.4 "Auditoria": `GET /v1/groups/{group_id}/audit?cursor=...`
- migration 002 (`audit_events` table + `export.group` permission seed)
- migration 007 + 013 (RLS FORCE + WITH CHECK for `audit_events`)
- plan 0056 (parameterized `set_config` — no `format!()`)
- plan 0062 (memory GET — cursor pagination pattern reference)
- GAR-400 (compliance export — downstream consumer of this data)

## §14 Estimativa

| Cenário | LOC | Tempo |
|---------|-----|-------|
| Optimista | ~280 | 1.5h |
| Provável  | ~340 | 2h   |
| Pessimista| ~420 | 3h   |
