# Plan 0340 — GAR-881 — GET /v1/me/audit — personal audit trail

**Status:** In Progress  
**Issue:** [GAR-881](https://linear.app/chatgpt25/issue/GAR-881)  
**Branch:** `routine/202506141215-me-audit`  
**Estimate:** 2h

---

## Goal

Add `GET /v1/me/audit` — cursor-paginated personal audit log exposing the
caller's own user-scoped audit events: login, logout, signup, password.changed,
api_key.{created,revoked,updated}, session.{revoked,all_revoked}.

This completes the self-service security visibility story started in plan 0327
(sessions) → 0331 (API keys) → 0335 (password change). Users can now
review their own security events without contacting an admin.

---

## Architecture

- No new migration — `audit_events` table (migration 002) already exists with
  index `audit_events_actor_created_idx ON audit_events(actor_user_id, created_at DESC)`.
- Query filter: `actor_user_id = $caller AND group_id = $nil_uuid`. Personal
  events are stored with `group_id = '00000000-0000-0000-0000-000000000000'`
  (nil UUID) by convention (see plans 0327, 0331, 0335).
- RLS: SET LOCAL `app.current_user_id = $caller`, `app.current_group_id = $nil_uuid`.
  Branch 2 of `audit_events_group_or_self` policy (migration 013) allows the
  actor to see their own events. Explicit `AND group_id = nil` in WHERE gives
  defense-in-depth — no cross-group leak even if RLS were misconfigured.
- Cursor pagination: keyset on `(created_at DESC, id DESC)`, identical to the
  group audit endpoint (audit.rs, plan 0070).
- No `X-Group-Id` header required — user-scoped endpoint.

---

## Tech stack

- Rust, Axum 0.8, sqlx (QueryBuilder), utoipa
- File: `crates/garraia-gateway/src/rest_v1/me.rs` (new handler + DTOs)
- Files touched: `mod.rs` (route wiring), `openapi.rs` (schema registration)

---

## Design invariants

- **No PII in response**: `actor_label` NOT returned (caller already knows
  their name); `ip` NOT returned (could enable self-correlation attacks). Only
  `id`, `action`, `resource_type`, `resource_id`, `metadata`, `created_at`.
- **No group events leak**: explicit `AND group_id = $nil_uuid` in SELECT.
- **No audit event emitted for reads** (invariant: no circular noise).
- Consistent with audit.rs: same query builder pattern, same cursor shape.

---

## Out of scope

- Cross-group audit (needs admin/BYPASSRLS role — GAR-391d, Fase 3.4).
- `ip` field (browser clients can retrieve their IP from other means).
- `action` filter is supported; `resource_type` filter is not (personal events
  have a small known set of actions, filtering by type adds little value).

---

## Rollback

Revert the me.rs + mod.rs + openapi.rs changes. No migration to roll back.

---

## File structure

```
crates/garraia-gateway/src/rest_v1/
  me.rs          ← add ListMyAuditQuery, PersonalAuditEventSummary,
                    MyAuditResponse, list_my_audit handler + 6 tests
  mod.rs         ← wire route in all 3 branches
  openapi.rs     ← register handler + schemas
ROADMAP.md       ← mark GET /v1/me/audit ✅ in §3.4
plans/README.md  ← add plan row
```

---

## M1 tasks

- [ ] T1 — DTOs + handler in me.rs
- [ ] T2 — Route wiring in mod.rs (all 3 branches)
- [ ] T3 — OpenAPI registration
- [ ] T4 — ROADMAP + plans/README.md update
- [ ] T5 — `cargo clippy` clean + `cargo test -p garraia-gateway`

---

## Acceptance criteria

- `GET /v1/me/audit` returns 200 with `events` array and optional `next_cursor`.
- Events are filtered to `actor_user_id = caller` and `group_id = nil-uuid`.
- Cursor pagination works: second request with `cursor=<id>` returns next page.
- `action` query param filters by action string.
- 401 when JWT missing/invalid.
- `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean.
- CI ≥20/20 green.

---

## Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| RLS branch 2 allows cross-group leak | Low | Explicit `AND group_id = nil` in WHERE; defense-in-depth |
| `metadata` contains PII | Low | Existing handlers emit only `name_len`, not raw names |

---

## Cross-references

- Plan 0070 / GAR-522 — `GET /v1/groups/{group_id}/audit` (group audit)
- Plan 0327 / GAR-866 — `GET /v1/me/sessions`
- Plan 0331 / GAR-871 — `POST/GET /v1/me/api-keys`
- Plan 0335 / GAR-876 — `PATCH /v1/me/password`
- Migration 002 — `audit_events` table + `actor_created_idx`
- Migration 013 — `audit_events_group_or_self` WITH CHECK explicit
