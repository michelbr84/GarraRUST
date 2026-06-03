# Plan 0259 — GAR-784 + GAR-785: lopdf 0.34→0.40 + rand_chacha 0.9→0.10 compat fixes

**Status:** Done — merged 2026-06-03 via PR #634 (`fa8e3c0`)
**Linear:** [GAR-784](https://linear.app/chatgpt25/issue/GAR-784) + [GAR-785](https://linear.app/chatgpt25/issue/GAR-785)
**Branch:** `routine/202506031615-deps-lopdf-randchacha-compat`
**Date:** 2026-06-03 (America/New_York)

## Goal

Fix two blocked Dependabot PRs (#628 + #629) by adapting call sites in
`garraia-media` and `garraia-workspace` to the new APIs, closing both in a
single clean PR.

## Root causes

### lopdf 0.34 → 0.40 (`garraia-media`, PR #629, GAR-785)

`lopdf 0.40` renamed `Object::as_string()` (returns `Result<&str, _>`) to
`Object::as_str()` (returns `Result<&[u8], _>`). Call sites in `pdf.rs` used
`as_string().ok().map(|s| s.to_string())`. After the bump, `as_str()` returns
`&[u8]` so the `to_string()` call fails to compile.

Fix: replace `.and_then(|v| v.as_str().ok()).map(|s| s.to_string())` with
`.and_then(|v| v.as_str().ok()).map(|b| String::from_utf8_lossy(b).into_owned())`
across all 7 metadata fields. `from_utf8_lossy` handles both UTF-8 and
Latin-1 PDF strings gracefully.

### rand_chacha 0.9 → 0.10 (`garraia-workspace`, PR #628, GAR-784)

`rand_chacha 0.10` uses `rand_core 0.10` while `rand 0.9` uses `rand_core 0.9`.
These are incompatible trait versions. The `migration_smoke.rs` test's
`unit_vector` helper used `use rand::{Rng, SeedableRng}` (rand_core 0.9) but
then called `ChaCha8Rng::seed_from_u64(seed)` and `rng.random_range(...)` which
require rand_core 0.10 traits. Multiple `rand_core` versions in the dep graph
caused a type mismatch.

Fix:
- Remove `rand = "0.9"` dev-dep (was only used in `unit_vector`).
- Bump `rand_chacha = "0.9"` → `"0.10"`.
- In `unit_vector`, switch to `use rand_chacha::rand_core::{Rng, SeedableRng}`
  and generate floats via `rng.next_u64()` (no dependency on `rand::Rng`).

## Design invariants

1. No production code changed — both fixes are in lib/test code only.
2. `from_utf8_lossy` ensures metadata extraction never panics on non-UTF-8 PDFs.
3. `unit_vector` output changes numerically (different generation method) but
   the smoke test does not check specific vector values — only that the DB
   insert/query round-trips correctly.

## Out of scope

- Full lopdf 0.40 API audit (other lopdf methods in `pdf.rs`).
- Migrating `rand` to 0.10 across the workspace.
- Closing PR #628 / #629 manually (superseded by this PR).

## File Structure

| File | Change |
|------|--------|
| `crates/garraia-media/Cargo.toml` | `lopdf = "0.34"` → `"0.40"` |
| `crates/garraia-media/src/pdf.rs` | `as_str().ok().map(to_string)` → `as_str().ok().map(from_utf8_lossy)` (7 sites) |
| `crates/garraia-workspace/Cargo.toml` | `rand = "0.9"` removed; `rand_chacha = "0.9"` → `"0.10"` |
| `crates/garraia-workspace/tests/migration_smoke.rs` | `use rand::{Rng, SeedableRng}` → `rand_chacha::rand_core::{Rng, SeedableRng}`; `random_range` → `next_u64` |
| `plans/0258-gar-783-invite-decline.md` | Status updated to Done (PR #632 merged) |
| `plans/README.md` | Rows 0258 updated + 0259 added |
| `ROADMAP.md` | Plan 0258 / GAR-783 marked ✅ |

## M1 Tasks

- [x] T1: lopdf version bump + `as_str` + `from_utf8_lossy` in `pdf.rs`
- [x] T2: `rand_chacha` bump + `rand` removal + `unit_vector` fix in `migration_smoke.rs`
- [x] T3: Docs — bookkeeping for plan 0258 (Done) + plan 0259 row

## Acceptance criteria

- `cargo check -p garraia-media -p garraia-workspace` clean.
- `cargo clippy -p garraia-media -p garraia-workspace --tests --no-deps -- -D warnings` clean.
- CI green on this PR.

## Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| `as_str()` returns bytes-only (not lossy-decoded) for non-ASCII PDFs | Low | `from_utf8_lossy` replaces invalid UTF-8 sequences with `�` — acceptable for metadata |
| `unit_vector` output changes break snapshot tests | Low | No snapshot assertions on vector values; smoke test checks round-trip, not specific values |

## Cross-references

- Dependabot PR #628 (rand_chacha): superseded by this PR
- Dependabot PR #629 (lopdf): superseded by this PR
- GAR-784, GAR-785 — the two Linear issues tracking these fixes
