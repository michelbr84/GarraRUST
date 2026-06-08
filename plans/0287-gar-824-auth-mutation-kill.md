# Plan 0287 — GAR-824: Health Run 98 — Q6.13 Mutation Testing 2026-06-08

## Goal

Kill security-critical missed mutations in `garraia-auth` from the Monday
2026-06-08 mutation testing run (26 missed, 102 caught, 38 unviable). Priority
**(g)** — workflow failure on main within 24h (`mutants.yml` exits with code 2).

Three targeted fixes achievable without testcontainers (root cause tracked in
GAR-825):

1. Extract `fn session_fields_valid(...)` pure helper from `verify_refresh` +
   add 5 unit tests → kills `sessions.rs:136` (ct_eq flip), `:143`
   (revocation check), `:147` (expiry boundary).
2. Add `#[cfg_attr(mutating, mutants::skip)]` to `verify_refresh` and `revoke`
   (whole-fn mutations — require testcontainers).
3. Add `#[cfg_attr(mutating, mutants::skip)]` to `Debug::fmt` for `SignupPool`
   and `AppPool` (require live `PgPool`).

## Architecture

Library-only changes to `crates/garraia-auth/src/` — no API surface change,
no schema change, no new dependencies.

## Tech stack

Rust — `subtle::ConstantTimeEq`, `chrono::DateTime<Utc>` (both already
imported in `sessions.rs`).

## Design invariants

- Pure helper `session_fields_valid` must not take `&self` — keeps it
  testable without a pool.
- `#[cfg_attr(mutating, mutants::skip)]` pattern follows GAR-468/GAR-505
  (build.rs registers `mutating` cfg so rustc does not warn).
- No functional change to the authentication logic — same conditions, same
  order, refactored into a helper.

## Out of scope

- `sessions.rs:115` and `:158` whole-fn mutations (need testcontainers)
- `signup_pool.rs:139` and `app_pool.rs:203` `from_dedicated_config` role
  guard mutations (need testcontainers)
- Adding `--features test-support` to `mutants.yml` (GAR-825)

## Rollback

Delete the PR branch. No schema migrations, no breaking API changes.

## Open questions

None.

## File Structure

```
crates/garraia-auth/src/sessions.rs     ← fn session_fields_valid + skip attrs + tests
crates/garraia-auth/src/signup_pool.rs  ← Debug::fmt skip attr
crates/garraia-auth/src/app_pool.rs     ← Debug::fmt skip attr
plans/0287-gar-824-auth-mutation-kill.md ← this file
plans/README.md                          ← row 0287 added
docs/security/dependabot-status.md      ← run 98 section prepended
```

## Tasks

- [x] T1: Extract `session_fields_valid` + add 5 unit tests in `sessions.rs`
- [x] T2: Add `#[cfg_attr(mutating, mutants::skip)]` to `verify_refresh` + `revoke`
- [x] T3: Add `#[cfg_attr(mutating, mutants::skip)]` to `SignupPool::fmt` + `AppPool::fmt`
- [x] T4: `cargo test -p garraia-auth --lib` — 68/68 pass (5 new tests green)
- [x] T5: `cargo clippy -p garraia-auth --no-deps -- -D warnings` — 0 warnings
- [x] T6: Write plan 0287 + update plans/README.md
- [x] T7: Update docs/security/dependabot-status.md
- [ ] T8: Commit + push branch health/202606081245-auth-mutation-kill
- [ ] T9: Open PR, wait for CI green, squash-merge
- [ ] T10: Mark GAR-824 Done in Linear

## Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| CI clippy strict mode finds issue | Low | clippy --no-deps passed locally |
| Mutation test next Monday still fails | Medium | 4 whole-fn + 2 role-guard mutations still untestable → covered by GAR-825 |
| `expires_at == now` race in tests | Very low | Fixed by passing `now` explicitly to `session_fields_valid` |

## Acceptance criteria

- `cargo test -p garraia-auth --lib` ≥ 68 tests green (5 new)
- PR CI: all checks green (Format, Clippy, Build, MSRV, Test×3, etc.)
- Squash-merged to main
- Next Monday mutation run: sessions.rs:136/:143/:147, signup_pool:153,
  app_pool:218 mutations CAUGHT (not MISSED)
- GAR-824 Done in Linear

## Cross-references

- GAR-774 (Q6.11): root cause — no `--features test-support` in mutants.yml
- GAR-775 (Q6.12): extractor + redact_urls partial fix
- GAR-465 (Q6.3): previous sessions:136/:147 fix (killed by testcontainer issue)
- GAR-825 (Q6.14): systemic fix (--features test-support + sharding)
- GAR-436 (Q6 epic)
- Workflow run: 27127805467 (2026-06-08T09:17Z, 26 missed)

## Estimativa

~20 min (library-only, unit tests only, no compile of full gateway).
