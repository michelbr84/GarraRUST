# Plan 0254 — GAR-775: Q6.12 — kill `is_unique_violation` positive-case mutations + fix broken `mutants::skip` on `redact_urls`

**Date:** 2026-06-02 ~00:46 ET
**Linear:** [GAR-775](https://linear.app/chatgpt25/issue/GAR-775)
**Branch:** `health/202506020046-q612-mutation-unit-tests`
**Priority ladder result:** (g) — CI failure on main within 24h (Mutation Testing pilot run #9, failed 2026-06-01T10:11Z)

---

## Goal

Continue reducing surviving mutations in the garraia-auth crate after GAR-774 (Q6.11) killed the 4 extractor security-bypass mutations. This plan addresses 5 more mutations that are reachable without testcontainers:

- **3 mutations in `storage_redacted.rs::redact_urls`** (1 MISSED + 2 TIMEOUT) — caused by a broken `#[cfg_attr(any(), mutants::skip)]` attribute. `any()` always evaluates to `false`, so `mutants::skip` is never activated despite the existing doc comment clearly documenting intent to skip.
- **2 mutations in `internal.rs::is_unique_violation`** (2 MISSED) — the `is_unique_violation → false` constant mutant and `== → !=` mutant survive because no test exercises the `true` code path (Database/23505 error).

## Root Cause

### storage_redacted.rs
`#[cfg_attr(any(), mutants::skip)]` is a dead attribute. `any()` takes no arguments and evaluates to the literal `false` predicate, so the `mutants::skip` inner attribute is never emitted. The correct cfg key that cargo-mutants adds when running is `mutating`, giving `#[cfg_attr(mutating, mutants::skip)]`.

The mutations inside `redact_urls` cause either:
- `< → <=` at line 112: boundary change that tests don't catch because the function's end-to-end behavior is covered by integration-style tests not run by the mutation runner
- `+= → -=` and `+= → *=`: infinite loops (the 3 TIMEOUT mutations)

The existing doc comment already accepts that skipping `redact_urls` is the right trade-off.

### internal.rs
`is_unique_violation` has tests for non-Database errors (pool timeout, configuration) but none that pass a `sqlx::Error::Database` with SQLSTATE code `"23505"`. Without a test that asserts `true` for a 23505 error:
- `is_unique_violation → false` survives (constant false is observationally identical)
- `== → !=` in the code check survives (returns false for 23505, true for everything else)

## Architecture

Pure unit tests + attribute fix; no testcontainers, no schema changes, no integration test changes.

## Tech Stack

- Rust / sqlx 0.8 (`sqlx::error::DatabaseError` trait)
- cargo-mutants `--cfg mutating` convention
- `#[cfg(test)]` mock struct implementing `sqlx::error::DatabaseError`

## Design Invariants

- No real database needed — `FakeDatabaseError` is a compile-time-only mock
- `mutants::skip` on `redact_urls` does not change production behavior
- No `#[allow(dead_code)]` or suppression annotations in production paths

## Out of Scope

- sessions.rs, internal.rs verify_credential mutations — require testcontainers (follow-up: add `--features test-support` to `mutants.yml` with sharding)
- audit.rs, audit_workspace.rs mutations
- login_pool.rs, signup_pool.rs, app_pool.rs `Debug::fmt` mutations (need live PgPool)

## Rollback

Both changes are additive (new tests) or semantic-preserving (attribute fix). Revert is `git revert`.

---

## Tasks

### M1 — Fix `#[cfg_attr(any(), mutants::skip)]` in `storage_redacted.rs`
- [x] Change `any()` → `mutating` in the `cfg_attr` on `redact_urls`
- [x] `cargo check -p garraia-auth` passes

### M2 — Add `FakeDatabaseError` + 2 unit tests in `internal.rs`
- [x] Add `struct FakeDatabaseError` implementing `sqlx::error::DatabaseError` inside `#[cfg(test)]`
- [x] Add `is_unique_violation_true_on_23505` test
- [x] Add `is_unique_violation_false_on_other_db_error_code` test
- [x] `cargo test -p garraia-auth` passes

### M3 — Bookkeeping
- [x] Update `plans/README.md`: mark plan 0253 as merged (PR #616, `67700a0`)
- [x] Add plan 0254 entry to `plans/README.md`

---

## Risk Register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| `sqlx::error::DatabaseError` trait has changed in sqlx 0.8 | Low | Compile check with `cargo check -p garraia-auth` |
| `mutants::skip` with `--cfg mutating` not recognized by cargo-mutants | Low | Documented convention in cargo-mutants docs; confirmed in PR #616's extractor test pattern |
| TIMEOUT mutations become MISSED after skip fix | N/A | They are SKIPPED (not generated), so won't appear in either category |

## Acceptance Criteria

- `cargo test -p garraia-auth` green
- `cargo clippy -p garraia-auth --no-deps -- -D warnings` clean
- CI green on PR
- Next Monday mutation run (June 9) shows ≤ 26 missed, 0 timeout

## Cross-References

- Plan 0253 (GAR-774, Q6.11): extractor unit tests, PR #616
- GAR-436: Q6 — Mutation Testing epic (parent)
- `.github/workflows/mutants.yml`: cargo-mutants pilot workflow
- `crates/garraia-auth/src/storage_redacted.rs`: `redact_urls` function
- `crates/garraia-auth/src/internal.rs`: `is_unique_violation` function + tests

## Estimativa

~30 min (2 file edits + tests + CI wait)
