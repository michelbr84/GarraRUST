# Plan 0343 — DELETE /v1/me — self-service account soft-deletion

**Issue:** [GAR-884](https://linear.app/chatgpt25/issue/GAR-884)
**Parent epic:** [GAR-400](https://linear.app/chatgpt25/issue/GAR-400) — Endpoints de export e delete (direitos do titular)
**Branch:** `routine/202606141815-delete-me`
**Date:** 2026-06-14 (America/New_York)

---

## Goal

Implement `DELETE /v1/me` — the caller soft-deletes their own account (LGPD art. 18 / GDPR art. 17 right to erasure). Sets `users.status = 'deleted'`, revokes all active sessions atomically, emits a compliance audit event. Returns 204 on success, 409 if already deleted.

---

## Architecture

Single transaction:
1. `SELECT status FROM users WHERE id = $1` with `FOR UPDATE` — check current status.
2. If status is already `'deleted'` → 409 Conflict.
3. `UPDATE users SET status = 'deleted' WHERE id = $1`.
4. `UPDATE sessions SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL AND expires_at > now()`.
5. `INSERT INTO audit_events ...` (`WorkspaceAuditAction::AccountSelfDeleted`).
6. COMMIT.

FORCE RLS: `SET LOCAL app.current_user_id` and `SET LOCAL app.current_group_id` (nil-uuid, user-scoped action).

---

## Tech stack

- **Crate:** `garraia-gateway`, `garraia-auth`
- **DB:** Postgres 16 + FORCE RLS (`users` migration 001, status column)
- **Auth:** `Principal` extractor (existing)
- **Audit:** `WorkspaceAuditAction::AccountSelfDeleted` (new variant)

---

## Design invariants

- PII-safe: no email, no display_name in audit metadata — only structural info.
- Atomic: session revocation and status update in same transaction.
- Idempotent: second call → 409 (not 204), preserving the tombstone audit record.
- Hard delete deferred: `users` row is not removed — that belongs to Fase 5.3 retention worker.
- No password required for self-deletion (the caller is already authenticated via JWT).

---

## Out of scope

- `GET /v1/me:export` (data export zip) — separate slice.
- `POST /v1/me:anonymize` — separate slice.
- Hard delete worker (Fase 5.3).
- Cascade effect on group_members / group ownership transfer (separate compliance slice).

---

## Rollback

Standard: `git revert` the squash-merge commit. No schema migration to revert (users.status 'deleted' is pre-existing).

---

## File structure

```
crates/garraia-auth/src/
  audit_workspace.rs        — add AccountSelfDeleted variant + as_str() arm

crates/garraia-gateway/src/rest_v1/
  me.rs                     — add delete_me handler + 6 unit tests
  mod.rs                    — wire DELETE /v1/me in all 3 branches
  openapi.rs                — register path

ROADMAP.md                  — mark DELETE /v1/me [x]
plans/README.md             — add row 0343
plans/0343-gar-884-delete-me.md  — this file
```

---

## M1 Tasks

- [x] Add `WorkspaceAuditAction::AccountSelfDeleted` to `audit_workspace.rs`
- [x] Add `delete_me` handler to `me.rs` with 6+ unit tests
- [x] Wire `DELETE /v1/me` in `mod.rs` (all 3 branches)
- [x] Register OpenAPI path in `openapi.rs`
- [x] Update `ROADMAP.md` §3.4 checklist
- [x] Update `plans/README.md`

---

## Acceptance criteria

- `DELETE /v1/me` returns 204 and sets `users.status = 'deleted'`.
- All caller's active sessions are revoked in the same transaction.
- `audit_events` row emitted with `action = 'account.self_deleted'`, no PII in metadata.
- Second call returns 409 Conflict.
- `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean.
- Unit tests: ≥6 passing.

---

## Risk register

| Risk | Likelihood | Severity | Mitigation |
|------|-----------|----------|-----------|
| RLS blocks users UPDATE | Low | Medium | `app.current_user_id` SET LOCAL satisfies `users_self_manage` policy |
| Double-delete race | Very Low | Low | `SELECT FOR UPDATE` + 409 guard prevents |

---

## Cross-references

- plan 0328 / GAR-869 — DELETE /v1/me/sessions (pattern reference for session revocation)
- plan 0335 / GAR-876 — PATCH /v1/me/password (nil-uuid group_id convention)
- plan 0340 / GAR-881 — GET /v1/me/audit (audit trail — will show this event)
- migration 001 — `users.status CHECK ('active','suspended','deleted')`
