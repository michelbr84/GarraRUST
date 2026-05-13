# Plan 0112 — GAR-465: Q6.3 Session TTL + Boundary Mutation Coverage

## Goal

Kill 3 missed mutants in `crates/garraia-auth/src/sessions.rs` identified in
cargo-mutants run [25307117776](https://github.com/michelbr84/GarraRUST/actions/runs/25307117776)
(2026-05-04, 90.78% killed):

| Line | Mutant target |
|------|--------------|
| 81 | `now + Duration::days(REFRESH_TTL_DAYS)` — TTL arithmetic |
| 136 | `ct_eq(...).unwrap_u8() == 0` — constant-time compare |
| 147 | `expires_at <= Utc::now()` — expiry boundary inclusive |

## Architecture

Tests-only change. No production code modified. Adds 4 new test functions to
`crates/garraia-auth/tests/sessions_lifecycle.rs` (existing file), each
targeting one observable behaviour killed by the above mutations.

## Tech stack

- `chrono::{DateTime, Duration, Utc}` (already a dep)
- `sqlx::query` admin pool to inject custom `expires_at` values
- `tokio::test(flavor = "multi_thread", worker_threads = 4)` (same as siblings)
- testcontainers pgvector/pg16 (reuses existing `boot()` fixture)

## Design invariants

- No production code changes — tests-only PR.
- `Duration` import added to existing `use chrono::{DateTime, Utc};` line.
- Admin pool used for `UPDATE sessions SET expires_at = ...` injection (same
  pattern as `revoke_is_idempotent_and_persists` uses `SELECT` on admin pool).
- Each test is independent with its own container boot (existing pattern).

## Out of scope

- Race-condition test (`revoked_at` injected between query and verify) — the
  issue lists it as "best-effort"; deferred.
- ct_eq line 136 has its own existing test (`verify_refresh_returns_some_for_valid_token`)
  that should already kill it; new expiry tests are the higher-value additions.

## Rollback

Delete the branch; zero schema/production-code impact.

## M1 tasks

- [x] T1: Create plan 0112 + update plans/README.md
- [x] T2: Add `Duration` to imports and 4 new tests to `sessions_lifecycle.rs`
- [x] T3: `cargo check -p garraia-auth` + `cargo clippy -p garraia-auth --tests` green
- [x] T4: Commit `test(auth): GAR-465 — kill Q6.3 session TTL + expiry boundary mutants`
- [ ] T5: Push + open PR
- [ ] T6: CI green
- [ ] T7: Merge (squash)
- [ ] T8: Mark GAR-465 Done in Linear + update plans/README.md row

## Acceptance criteria

- `sessions_issue_expires_approximately_30_days_from_now` — passes (TTL ≈ 30 days)
- `verify_refresh_returns_none_for_session_expired_1ms_ago` — passes (None)
- `verify_refresh_returns_none_for_session_expired_1s_ago` — passes (None)
- `verify_refresh_returns_some_for_session_expiring_tomorrow` — passes (Some)
- `cargo clippy --tests -p garraia-auth -- -D warnings` clean
- CI 100% green

## Cross-references

- Parent: [GAR-436](https://linear.app/chatgpt25/issue/GAR-436) Q6 cargo-mutants pilot
- Epic: [GAR-430](https://linear.app/chatgpt25/issue/GAR-430) Quality Gates Phase 3.6
- Sibling plans: 0069 (GAR-505 Q6.10), plan 0053 (RUSTSEC triage)

## Estimativa

< 100 LOC of test code. < 30 min implementation.
