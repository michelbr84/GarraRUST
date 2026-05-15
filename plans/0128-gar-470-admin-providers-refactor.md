# Plan 0128 — GAR-470: Q9.b — Extract admin/providers.rs from admin/handlers.rs

**Epic:** GAR-430 (Quality Gates Phase 3.6)
**Parent:** GAR-439 (Q9 Refactor admin/handlers.rs)
**Issue:** [GAR-470](https://linear.app/chatgpt25/issue/GAR-470)
**Branch:** `routine/202505150020-admin-providers-extract`
**Status:** ✅ Merged 2026-05-14 (Florida) via PR #349 (`eacbf9b`)

---

## Goal

Extract the "Phase 3: Providers Console" section (~347 LOC) from
`crates/garraia-gateway/src/admin/handlers.rs` into a new dedicated module
`crates/garraia-gateway/src/admin/providers.rs`. Zero behavior change.

This is the first of the Q9.b–Q9.g sub-slices; it reduces `admin/handlers.rs`
from 3240 LOC toward the 600–800 LOC target.

---

## Architecture

```
crates/garraia-gateway/src/admin/
  handlers.rs      — 3240 → ~2900 LOC after extraction; re-exports providers::*
  providers.rs     — NEW: ~347 LOC provider handler family
  shared.rs        — AdminState, derive_encryption_key (unchanged, Q9.a)
  mod.rs           — add `pub mod providers;`
  routes.rs        — unchanged (paths via `handlers::*` re-export still resolve)
```

The pattern mirrors Q9.a: new module holds the implementation; `handlers.rs`
re-exports everything so `routes.rs` callers (`handlers::admin_list_providers`,
etc.) remain unchanged.

---

## Tech Stack

- Rust, Axum 0.8, no new deps
- `admin::shared::AdminState` as handler state
- `admin::middleware::{AuthenticatedAdmin, extract_ip}` for auth/IP
- `admin::rbac::{Action, Resource, check_permission}` for RBAC

---

## Design Invariants

1. **Zero behavior change** — pure mechanical move of code.
2. **Re-export compatibility** — `handlers.rs` gains `pub use super::providers::*`
   so that `routes.rs` paths (e.g. `handlers::admin_list_providers`) continue to compile.
3. **No new public API surface** — all extracted items were already public.
4. **NEVER `unwrap()` in production code** — not introduced by this PR.

---

## Functions Extracted (lines 1255–1601 of handlers.rs)

| Symbol | Type | LOC |
|--------|------|-----|
| `admin_list_providers` | handler | ~75 |
| `UpdateProviderSettingsRequest` | struct | ~9 |
| `update_provider_settings` | handler | ~40 |
| `provider_health` | handler | ~40 |
| `enable_provider` | handler | ~33 |
| `disable_provider` | handler | ~33 |
| `provider_failover` | handler | ~27 |
| `list_provider_overrides` | handler | ~28 |
| `SetProviderOverrideRequest` | struct | ~4 |
| `set_provider_override` | handler | ~32 |

Total: ~321 LOC of extracted code + module doc comment.

---

## Validation Commands

```bash
cargo fmt --check --all
cargo check -p garraia-gateway --locked
cargo clippy -p garraia-gateway --all-targets -- -D warnings
cargo test -p garraia-gateway
```

---

## Out of Scope

- No changes to routes.rs handler references
- No changes to provider logic or RBAC rules
- No other Q9 slices (9.c through 9.g deferred to future plans)
- No new tests (refactor-only; existing tests remain the behavioral contract)

---

## Rollback

`git revert` the single commit or close the PR without merging.

---

## Milestones

### M1 — Create providers.rs + wire mod.rs

- [x] Create `admin/providers.rs` with extracted code
- [x] Add `pub mod providers;` to `admin/mod.rs`
- [x] Replace extracted code in `handlers.rs` with `pub use super::providers::*;`
- [x] `cargo check -p garraia-gateway` green

### M2 — Lint + Test

- [x] `cargo fmt --check --all` green
- [x] `cargo clippy -p garraia-gateway --all-targets -- -D warnings` green
- [x] `cargo test -p garraia-gateway` green

### M3 — Commit + Push + PR

- [x] Commit: `refactor(admin): Q9.b — extract admin/providers.rs (GAR-470)`
- [x] Push branch
- [x] Open PR

---

## Acceptance Criteria

- [ ] `admin/providers.rs` exists with all provider handlers
- [ ] `admin/handlers.rs` has `pub use super::providers::*` in place of extracted code
- [ ] `cargo check --workspace --exclude garraia-desktop` passes
- [ ] CI 100% green (all workflow checks)
- [ ] GAR-470 marked Done in Linear

---

## Risk Register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Import path conflicts | Low | Follows Q9.a pattern exactly |
| Broken re-export chain | Low | `routes.rs` uses `handlers::*` which re-exports |
| Clippy complaining about unused imports | Low | Only import what providers.rs needs |

---

## Cross-References

- Plan 0127: curl|sh PR-B (separate track, blocked on PR #348)
- GAR-439 Q9 parent (Done — Q9.a delivered)
- GAR-430 EPIC (In Progress)
- PR #90 (Q9.a pattern reference)

---

## Estimativa

0.5 / 1 / 2 hours.
