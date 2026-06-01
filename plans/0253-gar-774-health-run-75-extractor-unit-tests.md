# Plan 0253 — GAR-774: Health run 75 — Q6.11 extractor unit tests

**Date:** 2026-06-01 ~20:45 ET
**Linear:** [GAR-774](https://linear.app/chatgpt25/issue/GAR-774)
**Branch:** `health/202506012045-q611-extractor-unit-tests`
**Priority ladder result:** (g) — CI failure on main within 24h (Mutation Testing pilot run #9)

---

## Goal

Fix the "Mutation Testing — garraia-auth (pilot)" CI failure from 2026-06-01T10:11Z
(`35 missed, 103 caught, 38 unviable, 3 timeouts`) by adding unit tests that kill
the 4 security-critical extractor mutations without requiring testcontainers.

## Root Cause

`mutants.yml` runs `cargo mutants --package garraia-auth` without `--features test-support`.
All 8 integration test binaries gated by `required-features = ["test-support"]` in
`garraia-auth/Cargo.toml` are never compiled or run during mutation testing. This makes every
mutation in code exercised only by testcontainer-backed tests appear as MISSED.

The immediate fix (this plan) adds unit tests directly in `src/extractor.rs`
(`#[cfg(test)]`) so the 4 most critical security-bypass mutations are caught without
any testcontainer or workflow change.

A follow-up issue should fix the root cause: add `--features test-support` to `mutants.yml`
with appropriate timeout/sharding to cover sessions, internal, and pool mutations too.

## Architecture

- **No new files** — unit tests go in `src/extractor.rs` as `#[cfg(test)] mod tests { ... }`
- **No dependency changes** — uses only types already imported in `extractor.rs`
- **No feature flag** — unit tests in `src/` always compile in any cargo invocation

## Design Invariants

- Tests kill mutations by asserting the OPPOSITE of what the mutation produces
- `unauth()` mutation produces `(StatusCode::default(), "")` — assert status == 401, msg non-empty
- `forbid()` mutation produces `(StatusCode::default(), "")` — assert status == 403, msg non-empty
- `RequirePermission::check → Ok(())` — negative path must panic via `.expect_err()`
- `require_permission → Ok(())` — same via `.expect_err()`

## Out of Scope

- Sessions mutations (`:81`, `:115`, `:136`, `:147`, `:158`) — require testcontainer DB
- Internal mutations (`verify_credential`, `find_by_provider_sub`) — require testcontainer DB
- Pool mutations (`LoginPool::from_dedicated_config`, etc.) — require testcontainer DB
- Root cause systemic fix (`--features test-support` in `mutants.yml`) — separate issue

## Rollback

Remove the `#[cfg(test)] mod tests` block from `src/extractor.rs`. No schema or API impact.

## Tasks

- [x] T1: Write plan 0253 (this file)
- [ ] T2: Add 4 unit tests to `src/extractor.rs`
- [ ] T3: `cargo test -p garraia-auth --no-default-features` passes
- [ ] T4: Push + open PR
- [ ] T5: CI green (≥16 checks all success)
- [ ] T6: Squash-merge
- [ ] T7: Mark GAR-774 Done
- [ ] T8: Update plans/README.md with merge SHA + PR number

## Acceptance Criteria

- `cargo test -p garraia-auth` exits 0
- PR has all CI checks green
- The 4 extractor mutations are documented as expected-CAUGHT in GAR-774

## Mutations Targeted

| Mutation | File:Line | What It Bypasses | Killing Test |
|----------|-----------|-----------------|--------------|
| `unauth() → (Default::default(), "")` | extractor.rs:28 | 401 response on unauthenticated request | `unauth_returns_unauthorized_status` |
| `forbid() → (Default::default(), "")` | extractor.rs:33 | 403 response on forbidden request | `forbid_returns_forbidden_status` |
| `RequirePermission::check → Ok(())` | extractor.rs:120 | All RBAC permission checks | `require_permission_check_denies_insufficient_role` |
| `require_permission → Ok(())` | extractor.rs:134 | All permission guards | `require_permission_free_fn_denies_insufficient_role` |

## Cross-References

- Parent epic: [GAR-436](https://linear.app/chatgpt25/issue/GAR-436) Q6: cargo-mutants piloto
- Mutation run: https://github.com/michelbr84/GarraRUST/actions/runs/26748641134
- Existing integration tests (not running in mutation CI): `tests/extractor.rs` (gated by `test-support`)
- Plans with related fixes: 0081 (Q6.1 security bypass), 0085 (Q6.3 TTL), 0093 (Q6.6 Debug)

## Estimativa

0.5h implementation + CI wait
