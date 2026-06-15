# Plan 0347 — GAR-891 Q6.15: Kill 4 missed mutants in password.rs + audit_workspace.rs

**Linear:** [GAR-891](https://linear.app/chatgpt25/issue/GAR-891)
**Branch:** `health/202606151300-q615-mutant-password-audit-coverage`
**Health run:** 2026-06-15 ~08:45 ET (UTC 12:45)

---

## Goal

Kill the 4 missed mutants reported by the `Mutation Testing — garraia-auth (pilot)` scheduled
workflow run #11 (2026-06-15T10:49 UTC, run id 27541142471). All 4 mutants are in
security-critical paths: password change, LGPD anonymization, and workspace audit emission.

---

## Missed mutants

| Mutant | File | Location | Mutation | Shard |
|--------|------|----------|----------|-------|
| M1 | `crates/garraia-auth/src/audit_workspace.rs` | 897:5 | `replace audit_workspace_event → Ok(())` | 0 |
| M2 | `crates/garraia-auth/src/password.rs` | 119:5 | `replace anonymize_identity → Ok(())` | 0 |
| M3 | `crates/garraia-auth/src/password.rs` | 75:58 | `replace \|\| with && in change_password` | 1 |
| M4 | `crates/garraia-auth/src/password.rs` | 81:8 | `delete ! in change_password` | 2 |

---

## Root cause

`password.rs` (plan 0335 / GAR-876 + plan 0345 / GAR-888) only has unit tests for the
`PasswordChangeOutcome` enum and `anon_login` format — not the DB operations themselves.
`audit_workspace_event` in `audit_workspace.rs:897` only has unit tests for enum string
stability — no test verifies the INSERT actually executes.

---

## Architecture / invariants

- All new tests use the shared `Harness` from `tests/common/harness.rs` (one container per
  binary; isolation via unique UUIDs per test).
- `change_password` and `anonymize_identity` run via `LoginPool` (BYPASSRLS `garraia_login`).
- `audit_workspace_event` takes a `Transaction<'_, Postgres>` on `app_pool` (`garraia_app`)
  with `SET LOCAL app.current_user_id` and `SET LOCAL app.current_group_id` pre-set per
  the caller contract in `audit_workspace.rs:871-883`.
- Verification of INSERT into `audit_events` goes via the admin URL (bypasses RLS).
- No schema changes, no migration.

---

## Out of scope

- Other missed mutants from previous runs (all closed in GAR-824, GAR-825).
- Test coverage for `change_password` success path's DB update verification (update confirmed
  indirectly by the `PasswordChangeOutcome::Success` return; direct query deferred).
- `password.rs`'s `IdentityNotFound` path (already covered by the unit tests pattern).

---

## Rollback

PR not merged → delete branch. No DB migration to roll back.

---

## File structure

```
crates/garraia-auth/
  Cargo.toml                            ← add 2 [[test]] sections
  tests/
    password_change.rs                  ← NEW — kills M2, M3, M4
    audit_workspace_event_integration.rs ← NEW — kills M1
plans/
  0347-gar-891-q615-mutant-password-audit-coverage.md  ← this file
  README.md                             ← row added
```

---

## Tasks

- [x] T1 — Create Linear issue GAR-891
- [x] T2 — Create plan file `plans/0347-...md`
- [ ] T3 — Implement `tests/password_change.rs` (kills M2, M3, M4)
- [ ] T4 — Implement `tests/audit_workspace_event_integration.rs` (kills M1)
- [ ] T5 — Add `[[test]]` sections to `Cargo.toml`
- [ ] T6 — Update `plans/README.md`
- [ ] T7 — Push + open PR + wait for CI green
- [ ] T8 — Squash-merge; mark GAR-891 Done

---

## Risk register

| Risk | Mitigation |
|------|------------|
| Shared harness container not booted for new test binaries | Each binary boots its own container via `OnceCell` in `Harness::get()` |
| `login` column nullable in `user_identities` — NULL before anonymize | Test asserts `IS NOT NULL` + starts_with("anon-") after call |
| PBKDF2-SHA256 prefix assertion changes with `pbkdf2` crate update | Hard assertion on `$pbkdf2-sha256$` prefix in test; CI catches drift |
| SET LOCAL GUC not propagating to RLS policy | Integration test commits and queries via admin pool; failure surfaces as assertion error |

---

## Acceptance criteria

- `cargo test -p garraia-auth --features test-support` passes (all 4 new tests green)
- `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean
- PR CI: all 16+ checks green
- Next Monday mutation run: `password.rs` and `audit_workspace.rs` report 0 missed mutants

---

## Cross-references

- Run: https://github.com/michelbr84/GarraRUST/actions/runs/27541142471
- GAR-436 (epic parent)
- GAR-824 (Q6.13 — previous mutation fix)
- GAR-825 (Q6.14 — systemic `--features test-support` fix)
- plan 0335 (GAR-876 — `change_password` implementation)
- plan 0345 (GAR-888 — `anonymize_identity` implementation)

---

## Estimativa

< 2h wall-clock (no schema changes, pure test additions).
