# Plan 0124 — GAR-620: bump `metrics 0.24.5` (yanked) → `0.24.6`

> Health routine — 2026-05-14 (America/New_York)
> Branch: `health/202605141250-metrics-yanked-0246`
> Linear: [GAR-620](https://linear.app/chatgpt25/team/GAR)

## Goal

Remove `metrics 0.24.5` (yanked from crates.io) from `Cargo.lock` by
bumping to `0.24.6`, the latest non-yanked patch in the `0.24.x` line.

## Architecture

Lockfile-only patch — no `Cargo.toml` edits. The `metrics` crate is a
transitive dependency of `garraia-telemetry` and `garraia-gateway`
(via `metrics-exporter-prometheus`). The `0.24.x` API surface
(`counter!`, `gauge!`, `histogram!` macros) is stable across this bump.

## Tech stack

- `cargo update -p "metrics@0.24.5"` — resolves to `0.24.6`
- `Cargo.lock` — lockfile update only
- `docs/security/dependabot-status.md` — session snapshot update

## Design invariants

- No `Cargo.toml` version pins changed.
- No RUSTSEC advisory IDs added or removed (`audit.toml`/`deny.toml` unchanged).
- `deny.toml` `yanked = "warn"` — the removed entry reduces the warning count from 22 → 21.
- All existing `[advisories]` ignore entries remain valid.

## Out of scope

- No other dependency bumps in this plan.
- The 3 remaining Dependabot alerts (rsa/GAR-456, glib/GAR-513, rand/GAR-513) are upstream-blocked — not addressed here.
- Migration of `garraia-telemetry` to a major version of `metrics` (e.g., 0.23 → 0.24) — already happened in earlier batches.

## Rollback

`cargo update -p "metrics@0.24.6" --precise 0.24.5` — but 0.24.5 is yanked,
so effective rollback requires `cargo update -p "metrics@0.24.6" --precise 0.24.4`.
In practice, this plan has zero application-level risk.

## Security surfaces scanned (2026-05-14)

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #336 head |
| Malware (cargo/npm graph) | ✅ none | No malware advisories in cargo graph |
| Dependabot alerts | ✅ 3 open, upstream-blocked | rsa/GAR-456, glib/GAR-513, rand/GAR-513 — expiry 2026-07-31 |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | 21 warnings (↓ from 22 post-fix) |
| cargo-deny | ✅ pass | `advisories ok, bans ok, licenses ok, sources ok` |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All green on PR #336 + main |
| CI on main (31fb678) | ✅ green | All checks passed |

## Open questions

1. Is `metrics 0.24.5` yanked due to a bug or a publish error? — The yanked message typically
   says "incorrect publish" or similar. Not a security advisory; treat as maintenance.
2. Will `metrics 0.24.6` be re-yanked? — Monitor next health routine run.

## File structure

```text
Cargo.lock                              ← metrics 0.24.5 → 0.24.6
docs/security/dependabot-status.md     ← session snapshot update
plans/0124-gar-620-metrics-yanked-0246.md  ← this file
plans/README.md                         ← new row for plan 0124
```

## M1 — Implement

- [x] T1: Confirm `metrics 0.24.5` is yanked (PR #336 description, cargo audit output)
- [x] T2: `cargo update -p "metrics@0.24.5"` → resolves to `0.24.6` (done in PR #336)
- [x] T3: CI green on PR #336: Security Audit ✅, cargo-deny ✅, Dependency Review ✅,
          Secret Scan ✅, Analyze (rust) ✅, Analyze (js-ts) ✅, Analyze (actions) ✅,
          E2E Tests ✅, Format Check ✅, Clippy ✅, MSRV ✅, Test (macOS) ✅
- [x] T4: Update `docs/security/dependabot-status.md` — session snapshot
- [x] T5: Add plan row to `plans/README.md`
- [x] T6: Merge PR #336 — squash-merged as `adbe00af` (all 19/19 CI checks ✅)
- [x] T7: Confirmed — `metrics 0.24.5` no longer in `Cargo.lock` on main; `cargo audit` warning count 22→21

## Risk register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| `metrics 0.24.6` API drift | Very Low | Low | API surface is stable across 0.24.x patch line |
| CI flake masking real issue | Low | Medium | All security gates are blocking; health routine re-runs |
| Double-bump in next routine | Low | Low | metrics 0.24.6 will appear in Cargo.lock; no further action needed unless re-yanked |

## Acceptance criteria

- [x] `metrics 0.24.5` no longer appears in `Cargo.lock` on `main`.
- [x] `cargo audit` on main shows 21 allowed warnings (not 22).
- [x] All CI checks green on merged main (19/19 ✅).
- [x] `docs/security/dependabot-status.md` updated.
- [x] `plans/README.md` row added for 0124.

## Cross-references

- GAR-620 (this issue)
- PR #336: `fix(deps): bump metrics 0.24.5 (yanked) → 0.24.6`
- `docs/security/dependabot-status.md` — session snapshot
- Prior health routine: plan 0116 (GAR-605, CodeQL actions language)

## Estimativa

- T1–T5: ~30 min (documentation + scan)
- T6: CI wait ~30–45 min (in progress)
- Total: ~1h
