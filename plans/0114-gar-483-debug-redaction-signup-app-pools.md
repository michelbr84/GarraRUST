# Plan 0114 — GAR-483: Q6.6.b Debug-Redaction Tests for SignupPool + AppPool

## Goal

Kill 2 missed Debug-redaction mutants in `crates/garraia-auth` identified in
cargo-mutants run [25307117776](https://github.com/michelbr84/GarraRUST/actions/runs/25307117776)
(2026-05-04):

| File:Line | Mutant target |
|-----------|--------------|
| `signup_pool.rs:~153` | `Debug for SignupPool` → `Ok(Default::default())` (empty output) |
| `app_pool.rs:~218` | `Debug for AppPool` → `Ok(Default::default())` (empty output) |

Both mutations replace the `finish()` call in the Debug formatter with a no-op,
producing an empty string. No existing test exercises `format!("{:?}", pool)` on
a real `SignupPool` or `AppPool` instance.

## Architecture

Tests-only change. No production code modified. Adds one new integration test
file `crates/garraia-auth/tests/debug_redaction_pools.rs` with two
testcontainer-backed tests, each spinning up a fresh pgvector/pg16 container,
applying migrations to provision the dedicated roles, promoting the target role
to LOGIN, constructing the pool via its real constructor, and asserting the Debug
output.

Pattern follows `app_pool_role_guard.rs` exactly (no `mod common` dependency).

## Tech stack

- `testcontainers 0.27` + `testcontainers-modules 0.15` (already in dev-deps)
- `garraia-workspace::Workspace` + `WorkspaceConfig` for migrate_on_start
- `sqlx::PgPool` for admin query to promote roles
- `tokio::test` (single-threaded flavor is sufficient)

## Design invariants

- No production code changes — tests-only PR.
- Both tests are `#[tokio::test]` (no `test-support` feature needed since they
  construct pools through the public constructor, not via `raw()`).
- Each test is self-contained (no shared harness) to avoid hidden ordering deps.
- Debug assertions check for:
  - Presence of the redaction marker (e.g. `"<PgPool[garraia_app]>"`).
  - Absence of any credential fragment from the connection URL.

## Validações pré-plano

- [x] `signup_pool.rs:~153` mutant confirmed not covered by any existing test.
- [x] `app_pool.rs:~218` mutant confirmed not covered by any existing test.
- [x] `testcontainers` + `testcontainers-modules` already in dev-deps.
- [x] `garraia-workspace` already in dev-deps (provides migrate_on_start).
- [x] Pattern validated against `app_pool_role_guard.rs` (same crate).

## Out of scope

- Killing other missed mutants from the same run (covered by sibling GAR-4xx issues).
- Changes to production code.
- Mutants in crates other than `garraia-auth`.

## Rollback

Revert the new test file. No migration or production code to roll back.

## File structure

```
crates/garraia-auth/tests/
  debug_redaction_pools.rs   ← NEW: 2 integration tests
```

## Tasks (M1)

- [x] T1: Write `app_pool_debug_does_not_expose_credentials` test (red → green)
- [x] T2: Write `signup_pool_debug_does_not_expose_credentials` test (red → green)
- [x] T3: `cargo test -p garraia-auth --test debug_redaction_pools` green locally
- [x] T4: `cargo clippy --workspace --tests --exclude garraia-desktop --no-deps -- -D warnings` green
- [ ] T5: PR opened, CI green
- [ ] T6: PR merged
- [ ] T7: Linear GAR-483 marked Done
- [ ] T8: plans/README.md row updated + plans/0114 checkboxes flipped

## Risk register

| Risk | Mitigation |
|------|-----------|
| Container startup slow in CI | Already proven by `app_pool_role_guard.rs` in same crate |
| garraia_app / garraia_signup roles not created by migrations | Verified: migrations 008+010 create them |
| Role promotion requires superuser | Admin pool (`postgres:postgres`) has superuser in testcontainer |

## Acceptance criteria

- `cargo-mutants` on `signup_pool.rs` + `app_pool.rs` shows 0 missed mutants for the Debug impls.
- `format!("{:?}", app_pool)` contains `"<PgPool[garraia_app]>"` and not the connection URL.
- `format!("{:?}", signup_pool)` contains `"<PgPool[garraia_signup]>"` and not the connection URL.

## Cross-references

- Parent issue: [GAR-436](https://linear.app/chatgpt25/issue/GAR-436) (mutation testing epic)
- This issue: [GAR-483](https://linear.app/chatgpt25/issue/GAR-483)
- Sibling: plans/0112 (GAR-465 Q6.3 session TTL boundary)
- Pattern: `crates/garraia-auth/tests/app_pool_role_guard.rs`

## Estimativa

- Esforço: 0.5 / 1 / 2 horas
- LOC: ~100 (tests only)
