# Plan 0134 — GAR-634: Unblock tokio 1.52.3 via nix 0.31.3 + process-wrap 9.1.0

**Status:** ✅ Merged  
**Linear:** [GAR-634](https://linear.app/chatgpt25/issue/GAR-634)  
**Branch:** `health/202605160900-tokio-unblock`  
**Author:** health-routine (2026-05-16)

---

## Goal

Unblock `tokio` from `1.50.0` → `1.52.3` by resolving the transitive `libc`
version conflict introduced by `nix v0.31.1` (via `rmcp → process-wrap`).

## Architecture

`tokio 1.52.3` requires `mio 1.2.0` which requires `libc ^0.2.183`.  
`nix 0.31.1` pins `libc =0.2.180` (exact pin — irreconcilable with `^0.2.183`).  
`nix 0.31.3` updates the exact pin to `libc =0.2.186`, which satisfies
`^0.2.183` — conflict resolved.

Dependency chain:
```
garraia-cli → rmcp 1.6.0 → process-wrap 9.0.3 → nix 0.31.1 → libc =0.2.180
                                                 ↓ fix
                          → process-wrap 9.1.0 → nix 0.31.3 → libc =0.2.186
tokio 1.52.3 → mio 1.2.0 → libc ^0.2.183  ✅ satisfied by libc 0.2.186
```

## Tech stack

- Rust workspace (Cargo.lock — lockfile-only change)
- No Cargo.toml changes required

## Design invariants

- Lockfile-only — no API changes, no source code changes
- All three updates (nix, process-wrap, tokio) applied atomically in one commit
- `cargo check --workspace --exclude garraia-desktop` passes ✅

## Out of scope

- Upgrading nix past the 0.31.x series
- Changing rmcp version
- Any source code refactoring

## Rollback

`git revert <commit>` restores prior lockfile. No schema changes.

## Open questions

None — fix is deterministic and verified locally.

## File structure

```
Cargo.lock   — 7 packages updated (libc, nix, mio, process-wrap,
                socket2, tokio, tokio-macros)
```

## M1 Tasks

- [x] T1: Identify root cause (nix 0.31.1 exact-pins libc =0.2.180)
- [x] T2: Verify fix (nix 0.31.3 uses libc =0.2.186, satisfies ^0.2.183)
- [x] T3: Apply `cargo update -p nix -p process-wrap`
- [x] T4: Apply `cargo update -p tokio --precise 1.52.3`
- [x] T5: `cargo check --workspace --exclude garraia-desktop` → ✅
- [x] T6: Commit + push health/ branch
- [x] T7: Open PR, wait for CI green, merge
- [x] T8: Mark GAR-634 Done in Linear, update dependabot-status.md

## Risk register

| Risk | Likelihood | Mitigation |
|---|---|---|
| tokio 1.52.3 introduces breaking API changes | Low | Lockfile-only — no source change; CI catches regressions |
| mio 1.2.0 changes socket behavior | Low | Upstream semver guarantees compat; E2E tests in CI |
| nix 0.31.3 changes Unix API surface | Very low | process-wrap dependency — not used directly in GarraRUST code |

## Acceptance criteria

- `cargo update -p tokio --precise 1.52.3` succeeds (unblocked by nix 0.31.3)
- `cargo check --workspace --exclude garraia-desktop` exits 0
- All CI checks green (Format, Clippy, Test×3, Build, MSRV, cargo-deny, Security Audit)
- GAR-634 marked Done in Linear

## Cross-references

- GAR-634 (tokio upgrade blocker — Backlog → Done)
- PR #366 (security sweep that identified the blocker)
- health/202605160900-tokio-unblock (branch)

## Estimativa

~15 min (lockfile-only, low complexity)
