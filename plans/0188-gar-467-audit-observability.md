# Plan 0188 — GAR-467 Q6.5: Mutation Testing — audit_event observability coverage

## Goal

Kill the surviving mutations in `garraia-auth/src/internal.rs` related to
`audit_login()` calls in `verify_credential_with_ctx`. The mutants exploit the
fact that existing tests do not assert (a) **exactly 1 row** per terminal and
(b) that `ip` is populated for every non-argon2id terminal. A new test covers
the **NULL stored_hash** path which was missing entirely.

## Architecture

Tests-only PR. No production code changes. All additions land in
`crates/garraia-auth/tests/verify_internal.rs`.

Terminals of `verify_credential_with_ctx`:

| # | Branch | AuditAction | Existing assertions | Gap |
|---|--------|-------------|---------------------|-----|
| T1 | Argon2id success | `LoginSuccess` | last_audit_for + user_id + ip + count (via PBKDF2 test) | add count=1 |
| T2 | PBKDF2 lazy upgrade | `PasswordHashUpgraded` + `LoginSuccess` | count_audit_action=1 each | complete |
| T3 | Wrong password | `LoginFailureWrongPassword` | last_audit_for + user_id | add count=1 + ip |
| T4 | User not found | `LoginFailureUserNotFound` | last_audit_for + null user_id | add count=1 + ip |
| T5 | Suspended | `LoginFailureAccountNotActive` | last_audit_for + user_id | add count=1 + ip |
| T6 | Deleted | `LoginFailureAccountNotActive` | last_audit_for + user_id | add count=1 + ip |
| T7 | NULL stored_hash | `LoginFailureUnknownHash` | **missing** | new test |
| T8 | Unknown hash prefix | `LoginFailureUnknownHash` | last_audit_for + user_id | add count=1 + ip |

## Tech stack

- Rust async integration tests with `tokio::test`
- `testcontainers` + `pgvector/pgvector:pg16`
- `garraia_workspace::Workspace` for migrations
- `garraia_auth::{InternalProvider, LoginPool, LoginConfig, RequestCtx}`

## Design invariants

- NO production code changes.
- Each terminal asserts `count_audit_action(...) == 1` to kill "extra row" mutants.
- Each terminal asserts `row.ip.is_some()` when `RequestCtx::ip` is provided.
- NULL stored_hash test uses `seed_user(admin, email, None, "active")` — valid user but no hash.
- NULL stored_hash must return `Err(AuthError::UnknownHashFormat)` AND commit the audit row.

## Validações pré-plano

- `cargo check -p garraia-auth` → 0 errors (no prod changes)
- `cargo test -p garraia-auth verify_internal` locally (needs testcontainers runtime)

## Out of scope

- Mutation testing run itself (triggered separately by cargo-mutants CI job)
- Changes to any production module
- New terminals not in `verify_credential_with_ctx`

## Rollback

Revert the `verify_internal.rs` changes. No schema changes, no production risk.

## File structure

```
crates/garraia-auth/tests/verify_internal.rs   — augmented (count + ip + null-hash test)
plans/0188-gar-467-audit-observability.md      — this file
plans/README.md                                — row 0188 added
```

## M1 tasks

- [x] T1 — Add count=1 assertion to `argon2id_happy_path_emits_login_success_audit`
- [x] T2 — Add count=1 + ip assertions to `wrong_password_returns_none_with_failure_audit`
- [x] T3 — Add count=1 + ip assertions to `user_not_found_returns_none_with_null_actor_audit`
- [x] T4 — Add count=1 + ip assertions to `suspended_account_returns_none_with_account_audit`
- [x] T5 — Add count=1 + ip assertions to `deleted_account_takes_same_path_as_suspended`
- [x] T6 — Add count=1 + ip assertions to `unknown_hash_format_returns_err_with_audit`
- [x] T7 — New test: `null_stored_hash_emits_unknown_hash_audit`
- [x] T8 — Update plans/README.md row

## Risk register

| Risk | Mitigation |
|------|-----------|
| testcontainers flakiness in CI | Already proven stable by existing 10 tests |
| NULL hash INSERT rejected by DB constraint | `password_hash` is nullable per migration 001 schema |

## Acceptance criteria

- All 11 tests in `verify_internal.rs` pass in CI (was 10 before this PR)
- Each terminal asserts count=1 for its respective `AuditAction`
- NULL hash path tested: `Err(UnknownHashFormat)` + audit row committed
- PR ≤ 400 LOC (tests-only)
- CI 100% green

## Cross-references

- GAR-467 Linear issue
- GAR-436 (mutation testing baseline epic)
- GAR-430 (Quality Gates Phase 3.6 epic)
- `plans/0012-gar-391c-extractor-and-wiring.md` (verify path design)

## Estimativa

2h — tests-only, no schema changes, no prod code.
