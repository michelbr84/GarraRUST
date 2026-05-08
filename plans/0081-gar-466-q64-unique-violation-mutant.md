# Plan 0081 — GAR-466 / Q6.4: kill `is_unique_violation` constant-true mutant

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:test-driven-development` to drive this slice. Tests-only PR, no production code change.

**Linear issue:** [GAR-466](https://linear.app/chatgpt25/issue/GAR-466) — "Q6.4: Mutation Testing — unique-violation error path" (Backlog → In Progress, Medium). Labels: `epic:quality-gates`. Parent: [GAR-436](https://linear.app/chatgpt25/issue/GAR-436) (Mutation Testing baseline). Epic: [GAR-430](https://linear.app/chatgpt25/issue/GAR-430).

**Status:** ⏳ Draft — branched off `main@401b980` on 2026-05-07 22:56 ET (UTC `202605080256`).

**Goal:** Kill the `is_unique_violation -> true` constant mutant at `crates/garraia-auth/src/internal.rs:430` (mutant #2 / #5 in `docs/mutation-baseline-2026-04.md`) by adding a negative-case unit test that exercises non-`Database` `sqlx::Error` variants. The pre-existing integration test `signup_duplicate_email` (`tests/signup_flow.rs:236`) already kills mutants that flip the function's positive return — but a constant-`true` mutant survives because the positive case has the same observable outcome (`AuthError::DuplicateEmail`). A negative-case test closes the gap.

**Architecture:**

1. Add `#[cfg(test)] mod tests {}` at the bottom of `crates/garraia-auth/src/internal.rs` with two short unit tests over the private `is_unique_violation` helper:
   - `is_unique_violation_false_on_pool_timeout` — constructs `sqlx::Error::PoolTimedOut` (unit variant, freely constructible) and asserts `!is_unique_violation(&err)`.
   - `is_unique_violation_false_on_configuration` — constructs `sqlx::Error::Configuration("not a db error".into())` and asserts `!is_unique_violation(&err)`.
2. Both tests fail under the `-> true` constant mutant (assertions become `!true`) and pass under the original implementation.
3. No production code changes. No new dependencies. No schema impact. Inline `mod tests` is the existing convention for unit tests in this crate (see `hashing.rs`, `jwt.rs`).

**Tech stack:** `sqlx 0.8` (already in workspace), `#[cfg(test)] mod tests`, idiomatic Rust unit tests. No async runtime needed (synchronous fn under test).

---

## Design invariants (non-negotiable)

1. **Tests-only PR.** Zero production-code mutation. The mutant is killed purely by adding test coverage; refactoring `is_unique_violation` to be `pub(crate)` is unnecessary because inline `mod tests` has same-module access to private items.
2. **No new test dependency on a real Postgres.** `sqlx::Error::PoolTimedOut` and `sqlx::Error::Configuration` are constructible without I/O — the test runs in microseconds and adds zero CI cost.
3. **PII-safe (regra 6 do `CLAUDE.md`).** No literal email, no PII fixture. The test only inspects error variants.
4. **No `unwrap()` outside tests.** N/A — the unit tests use `assert!`/`assert!(...)` only; no `unwrap`.
5. **Integration tests untouched.** `tests/signup_flow.rs` keeps its existing `signup_duplicate_email` positive-case assertion intact — the new tests are additive, not a replacement.

---

## Validações pré-plano (gate executed in this session)

- ✅ Mutant signature confirmed in `docs/mutation-baseline-2026-04.md:66, 103, 221` — entry #2/#5: `internal.rs:430` `is_unique_violation -> true`.
- ✅ Function location verified — `crates/garraia-auth/src/internal.rs:425-436` (issue body referenced stale `signup_pool.rs:153` location; actual file is `internal.rs`).
- ✅ Variant returned is `AuthError::DuplicateEmail`, not `DuplicateIdentity` (issue body has stale wording — `DuplicateIdentity` does not exist in `error.rs`). Existing positive test asserts the correct variant at `tests/signup_flow.rs:236`.
- ✅ `sqlx::Error::PoolTimedOut` is a unit variant — directly constructible in tests (sqlx 0.8 source: `sqlx-core/src/error.rs`).
- ✅ `sqlx::Error::Configuration(String)` is constructible from a `&str` via `Into<Box<dyn StdError + Send + Sync>>`.
- ✅ Inline `#[cfg(test)] mod tests {}` pattern used elsewhere in this crate — see `hashing.rs:284`, `jwt.rs` (multiple), `audit_workspace.rs:120`.
- ✅ Existing integration test `signup_duplicate_email` covers the positive case (`Err(AuthError::DuplicateEmail)` on second signup). Negative case (non-Database error → not classified as unique-violation) is the missing coverage.

---

## Out of scope (rejected explicitly)

- Refactoring `is_unique_violation` to `pub(crate)` — unnecessary; inline tests have private-item access.
- Refactoring the function signature or behavior — production semantics are correct; only test coverage is missing.
- Re-running the full mutation testing workflow on this branch — `mutants.yml` is schedule-only + `workflow_dispatch`, never on PR path. The next scheduled run will pick up the new tests automatically. Manual `workflow_dispatch` against `garraia-auth` after merge is optional follow-up, not a merge gate.
- Killing the other mutants from baseline (`audit_workspace.rs:156` GAR-467, `jwt.rs:33/63/81/179` GAR-465/468, `app_pool.rs:203/218` GAR-464/468, `storage_redacted.rs:71` GAR-464, `types.rs:47` GAR-468, `signup_pool.rs:153 Debug` GAR-468) — each has its own Linear issue and can be tackled in subsequent routine slices.
- Updating `docs/mutation-baseline-2026-04.md` to remove this mutant from the open list — the doc is a frozen snapshot of the baseline run; the next mutation run produces a new doc. No edit here.

---

## Rollback plan

Single commit, additive only. `git revert <sha>` removes the new `mod tests` block from `internal.rs` cleanly. Zero migration, zero new dependency, zero touched route, zero schema change. Worst-case revert is one `git revert`.

---

## §12 Open questions (pre-start)

1. **Should the new tests live inline in `internal.rs` or in a separate `tests/internal_unique_violation.rs` integration test file?** → **Decision:** inline. The function is private (no `pub(crate)` needed), inline access keeps the patch minimal, and existing crate convention favors inline `#[cfg(test)] mod tests` for unit tests of private helpers (see `hashing.rs`, `jwt.rs`).
2. **Should we also test the positive case (Database/23505 → true) inline as a unit test?** → **Decision:** no. The positive case is already covered end-to-end by `signup_duplicate_email` integration test, which exercises a real Postgres `unique_violation` and asserts the resulting `AuthError::DuplicateEmail`. Adding an inline unit test that fabricates a `sqlx::Error::Database` requires constructing a `PgDatabaseError` (private opaque type) — high friction for marginal value.
3. **Could a non-Database `sqlx::Error` variant other than `PoolTimedOut` / `Configuration` be more representative?** → **Decision:** these two are sufficient. `PoolTimedOut` represents transient infrastructure failure; `Configuration` represents init-time misconfiguration. Both are common-enough non-DB error variants that survive the function's `match` arms. Adding more (e.g., `Io`, `Tls`, `Protocol`) would not improve mutation coverage — the mutant only needs ONE non-Database test to die.

---

## File structure

| Path | Change | LOC |
|---|---|---|
| `crates/garraia-auth/src/internal.rs` | Append `#[cfg(test)] mod tests { ... }` | +~25 |
| `plans/0081-gar-466-q64-unique-violation-mutant.md` | New | +~120 |
| `plans/README.md` | Add row 0081 | +1 |

Total impact: ~150 LOC across 3 files, of which ~25 LOC is the actual test code.

---

## Tasks (M1)

- [ ] **T0** — Register plan 0081 in `plans/README.md` index.
- [ ] **T1 (RED)** — Add inline `#[cfg(test)] mod tests {}` block to `crates/garraia-auth/src/internal.rs` with both negative-case tests. Verify the tests pass against the current production implementation: `cargo test -p garraia-auth --lib internal::tests`. Verify they would fail under a hypothetical `-> true` constant mutant by **mentally tracing the assertion**: `assert!(!is_unique_violation(&PoolTimedOut))` → original returns `false` → `!false == true` → pass; mutant returns `true` → `!true == false` → fail. (No literal mutant injection in CI; the mutation testing workflow runs on a schedule, not per-PR.)
- [ ] **T2 (GREEN)** — Confirm `cargo clippy -p garraia-auth --tests --no-deps -- -D warnings` clean.
- [ ] **T3 (CI)** — Push, open PR `tests(auth): GAR-466 — kill is_unique_violation constant-true mutant (Q6.4)`, drive CI to ≥16 actual workflow checks green.

---

## Risk register

| Risk | Likelihood | Mitigation |
|---|---|---|
| `sqlx::Error::Configuration(String)` signature drifts in a future sqlx bump | Low | Crate is pinned at 0.8.x via workspace `Cargo.toml`; tests will fail at compile time on signature change, not silently. |
| Inline `mod tests` block confuses cargo-mutants viability scoring | Very low | cargo-mutants explicitly walks `#[cfg(test)]` modules in the same crate as the SUT. The pilot already tests against inline modules in other files of `garraia-auth`. |
| The next scheduled mutation run still reports the mutant as `missed` despite the new tests | Low | If observed, file a follow-up issue: the test file may not be in the test target cargo-mutants invokes; verify via `cargo test --workspace -p garraia-auth -- internal::tests` runs the new test names. Defer fix to next cycle — does not gate this PR's merge (mutants.yml is non-blocking by design per `mutants.yml:25-27`). |

---

## Acceptance criteria

- [ ] `crates/garraia-auth/src/internal.rs` contains a `#[cfg(test)] mod tests {}` block with at least 2 tests: `is_unique_violation_false_on_pool_timeout` and `is_unique_violation_false_on_configuration`.
- [ ] `cargo test -p garraia-auth --lib internal::tests` passes both tests locally and in CI.
- [ ] `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` is clean.
- [ ] PR CI shows ≥ 16 actual workflow checks (Format, Clippy, Test×3, Build, MSRV, cargo-deny, Security Audit, Coverage, Analyze rust, Analyze js-ts, Playwright, E2E, Secret Scan, Dependency Review) all `success`.
- [ ] PR title format: `tests(auth): GAR-466 — kill is_unique_violation constant-true mutant (Q6.4)`.
- [ ] Linear issue GAR-466 transitioned to `Done` post-merge with comment linking the merge commit + PR number.
- [ ] `plans/0081-...md` row updated in `plans/README.md` (T8 of plan).

---

## Cross-references

- **Mutation baseline doc:** [`docs/mutation-baseline-2026-04.md`](../docs/mutation-baseline-2026-04.md) §"Mutantes missed" line 66 / §"Achados materiais" line 103 / §"Distribuição final por GAR" line 221.
- **Sibling plans (Q6 mutation triage):** plan 0049 (GAR-463 Q6.1 — verify-bypass), plan 0050 follow-ups (GAR-468 Q6.6 — Debug skip), GAR-505 (Q6.10 — closed 2026-05-04 covering 6 NEW mutants). This plan closes one of the 4 remnants explicitly listed in `mutation-baseline-2026-04.md:340-344`.
- **Function under test:** `crates/garraia-auth/src/internal.rs:425-436` (`is_unique_violation`) — sole call sites at `:388` and `:410` inside `signup_user`.
- **Existing positive-case integration test:** `crates/garraia-auth/tests/signup_flow.rs:225-238` (`signup_duplicate_email`).

---

## Estimativa

**1 task, ~30 minutes.**

- T0 (README.md row): 2 min.
- T1 (write 2 unit tests): 10 min.
- T2 (clippy + local cargo test): 5 min.
- T3 (push + PR + CI watch): 15 min depending on CI runner queue.

Tiny scope intentionally — first slice in a fresh routine session, demonstrates the routine pipeline end-to-end without taking on a multi-hour adaptation slice.
