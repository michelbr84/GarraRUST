# Plan 0158 — GAR-669 Slice 1: rand_chacha 0.9 + rand 0.9 co-bump (garraia-workspace dev-deps)

**Status:** ✅ Done — merged PR #446 (`d9f811ac`) 2026-05-20  
**Branch:** `health/202005200845-rand-chacha-0.9`  
**Linear:** [GAR-669](https://linear.app/chatgpt25/issue/GAR-669)  
**Health routine run:** 7 (2026-05-20 ~08:45 ET)

---

## Goal

Merge Dependabot PR #423 (`rand_chacha` 0.3.1 → 0.9.0) by co-bumping `rand` in
`garraia-workspace` dev-deps from 0.8 to 0.9, then adapting the one API call-site
(`gen_range` → `random_range`). No production code touched — dev-deps only.

## Architecture

The only use of `rand` + `rand_chacha` in the entire workspace (outside of the
workspace-level `rand = "0.9"` dep) is in:

- `crates/garraia-workspace/Cargo.toml` dev-deps
- `crates/garraia-workspace/tests/migration_smoke.rs` — one `fn unit_vector(seed)`
  helper that generates deterministic 768-d vectors for pgvector integration tests

## Why PR #423 Failed

Dependabot bumped only `rand_chacha` (0.3 → 0.9) but left `rand` at 0.8. This created
a rand_core version conflict:

| crate | version | rand_core |
|---|---|---|
| rand | 0.8.6 | 0.6.4 |
| rand_chacha | 0.9.0 | 0.9.5 |

The test code imports `rand::SeedableRng` (from rand_core 0.6) but calls
`ChaCha8Rng::seed_from_u64` which is implemented via rand_core 0.9's `SeedableRng` —
type mismatch → E0599.

## Fix

1. **`garraia-workspace/Cargo.toml`** — co-bump dev-deps:
   - `rand = "0.8"` → `rand = "0.9"`
   - `rand_chacha = "0.3"` → `rand_chacha = "0.9"`
   Both use rand_core 0.9.5 → conflict resolved.

2. **`garraia-workspace/tests/migration_smoke.rs`** — adapt renamed API:
   - `rng.gen_range(-1.0..1.0)` → `rng.random_range(-1.0..1.0)`
   (`gen_range` was renamed to `random_range` in rand 0.9)

## Tech Stack

- Rust `garraia-workspace` crate, dev-deps only
- `rand 0.9.3` + `rand_chacha 0.9.0` (already in Cargo.lock via workspace deps)
- No production code, no migrations, no auth path touched

## Design Invariants

- Blast radius: `garraia-workspace` dev-deps only — zero production surface
- `rand_chacha::ChaCha8Rng` is used for **deterministic** vector generation
  (bit-for-bit stable via ChaCha8 algorithm, not StdRng — contract preserved)
- The API used (`seed_from_u64`, `random_range`, `ChaCha8Rng`) is identical in
  rand 0.8 and rand 0.9 semantically; only the method name changes

## Out of Scope

- PR #422 (windows-sys 0.52 → 0.61) — different fix, windows-only bin path
- PR #424 (rand 0.8 → 0.10) — larger breaking changes (Rng → RngExt, etc.)
- PR #430 (password-hash 0.5 → 0.6) — auth-critical, needs security-auditor review

## Rollback

Revert the two Cargo.toml lines and one migration_smoke.rs line. Cargo.lock reverts
automatically on next `cargo update` run.

## Files Changed

```
crates/garraia-workspace/Cargo.toml          — rand 0.8→0.9, rand_chacha 0.3→0.9
crates/garraia-workspace/tests/migration_smoke.rs — gen_range→random_range
Cargo.lock                                   — garraia-workspace dep entries updated
plans/0157-gar-669-rand-chacha-0.9-dev-dep-co-bump.md (this file)
plans/README.md
```

## Tasks

- [x] T1: Identify root cause of PR #423 CI failures (rand_core version conflict)
- [x] T2: Prove `Rng`, `SeedableRng`, `random_range` all exist in rand 0.9
      (confirmed via garraia-security using rand 0.9 + green CI on PR #444)
- [x] T3: Edit `garraia-workspace/Cargo.toml` dev-deps
- [x] T4: Edit `migration_smoke.rs` — gen_range → random_range
- [x] T5: `cargo check -p garraia-workspace --tests` → Finished with 0 errors
- [x] T6: `cargo clippy -p garraia-workspace --tests --no-deps -- -D warnings` → clean
- [x] T7: Cargo.lock updated (rand 0.8.6 → 0.9.3, rand_chacha 0.3.1 → 0.9.0 for garraia-workspace)
- [x] T8: Plan + README committed on health branch
- [x] T9: PR opened, CI polled until green (20/20 checks green)
- [x] T10: Squash-merge (`d9f811ac`), Linear GAR-669 updated (Done)

## Risk Register

| Risk | Likelihood | Mitigation |
|---|---|---|
| rand 0.9 removes another API used in tests | Low | cargo check passed locally |
| Clippy -D warnings catches deprecation | Low | clippy ran clean locally |
| ChaCha8 output changes between 0.3 and 0.9 | None | algorithm guaranteed stable |

## Acceptance Criteria

- All 20 CI checks green on the health PR
- Dependabot PR #423 can be closed (our fix supersedes it with a complete, working bump)
- GAR-669 updated to reflect Slice 1 complete

## Cross-references

- Dependabot PR #423: rand_chacha 0.3.1 → 0.9.0 (superseded)
- GAR-669: parent tracking issue for all 4 Dependabot bumps
- PR #444 (bookkeeping, 20/20 green): proof that rand 0.9 API works in workspace

## Estimativa

0.5h implementation, 1h CI wait.
