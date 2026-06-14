# Plan 0335 — GAR-876: PATCH /v1/me/password — change own password

## Goal

Add `PATCH /v1/me/password` so an authenticated user can change their own password
in the REST API. Closes the self-service credential gap left after the API keys,
sessions, and profile endpoints were delivered.

## Architecture

```
PATCH /v1/me/password
  → patch_my_password (me.rs)
    → garraia_auth::change_password(login_pool, user_id, current_pw, new_pw)
        → login_pool.pool() BYPASSRLS
        → SELECT id, password_hash FROM user_identities WHERE user_id = $1 AND provider = 'internal' FOR NO KEY UPDATE
        → verify_argon2id / verify_pbkdf2 (dual-verify)
        → hash_argon2id(new_password)
        → UPDATE user_identities SET password_hash = $1, hash_upgraded_at = now() WHERE id = $2
        → COMMIT
    ← PasswordChangeOutcome::{Success | WrongPassword | IdentityNotFound}
    → audit_workspace_event(app_pool_tx, PasswordChanged, user_id, nil_uuid)
    ← 204 No Content
```

## Tech stack

- Rust · Axum 0.8 · sqlx · garraia-auth (login_pool, hashing, audit_workspace)
- No migration needed — `user_identities.hash_upgraded_at` already exists (migration 009)

## Design invariants

1. `login_pool` (BYPASSRLS, `garraia_login` role) is the only pool allowed to touch
   `user_identities.password_hash`. NEVER use `app_pool` (CLAUDE.md Rule 12).
2. `current_password` and `new_password` become `SecretString` immediately after
   JSON deserialization — never logged, never cloned beyond what's needed.
3. Both `IdentityNotFound` and `WrongPassword` map to 403 (anti-enumeration).
4. Audit event uses `app_pool` (FORCE RLS with `app.current_group_id = nil_uuid`),
   consistent with sessions and API keys patterns (plan 0327/0331).
5. `new_password` length: 8–1024 chars (matches signup convention from GAR-391c).

## Out of scope

- Email change endpoint (separate epic)
- Password reset / forgot-password flow (unauthenticated, needs email delivery)
- Multi-factor authentication
- Session invalidation on password change (may be added later via follow-up)

## Rollback

Revert this PR. No migration involved.

## File structure

```
crates/garraia-auth/src/
  audit_workspace.rs   — add PasswordChanged variant + "password.changed"
  password.rs          — NEW: PasswordChangeOutcome + change_password()
  lib.rs               — pub mod password; pub use password::{...}
crates/garraia-gateway/src/rest_v1/
  me.rs                — PatchMyPasswordRequest + patch_my_password handler + tests
  mod.rs               — register PATCH /v1/me/password route + unconfigured stubs
ROADMAP.md             — add [x] PATCH /v1/me/password line
plans/README.md        — add row for plan 0335
```

## Tasks

### M1 — garraia-auth: PasswordChanged audit variant
- [x] Add `PasswordChanged` doc comment + variant to `WorkspaceAuditAction`
- [x] Add `"password.changed"` arm in `as_str()`
- [x] Add assertion to `workspace_audit_action_as_str_stable` test

### M2 — garraia-auth: `change_password` function
- [x] New `crates/garraia-auth/src/password.rs` with `PasswordChangeOutcome` + `change_password`
- [x] Export from `lib.rs`
- [x] ≥ 4 unit tests (outcome variants, hash-format dispatch)

### M3 — garraia-gateway: handler + route
- [x] `PatchMyPasswordRequest` struct + `patch_my_password` handler in `me.rs`
- [x] Register `.patch(me::patch_my_password)` in full + unconfigured routers in `mod.rs`
- [x] ≥ 4 unit tests in me.rs

### M4 — docs
- [x] Add `[x] PATCH /v1/me/password` to ROADMAP.md §3.4
- [x] Add row 0335 to plans/README.md

## Risk register

| Risk | Mitigation |
|---|---|
| Two independent transactions (login_pool + app_pool) not atomic | Accepted: password update and audit are independent by design (same pattern as session/api-key flows). Worst case: hash updated but audit missing on crash. |
| Timing difference between WrongPassword and IdentityNotFound | Both paths skip any expensive crypto — same latency; both return 403 so attacker can't distinguish. |

## Acceptance criteria

- `PATCH /v1/me/password` with valid JWT + correct current password + valid new password → 204
- Same endpoint with wrong current_password → 403
- `audit_events.action = 'password.changed'` row created on success
- `cargo check -p garraia-auth -p garraia-gateway` clean
- `cargo clippy --workspace --tests --exclude garraia-desktop --no-deps -- -D warnings` clean
- ≥ 8 unit tests total

## Cross-references

- GAR-391b — verify_credential, hash_argon2id (foundation)
- GAR-866 / plan 0327 — sessions pattern (login_pool + app_pool audit)
- GAR-871 / plan 0331 — API keys pattern (nil_uuid group_id in audit)
- CLAUDE.md Rule 12 — never read user_identities via app_pool

## Estimativa

- 1-2h implementation, ~350 LOC total (auth: ~120 LOC, gateway: ~200 LOC, tests: ~80 LOC)
