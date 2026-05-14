# Plan 0116 — GAR-605: Add `actions` language to CodeQL matrix (close 15 stale Medium alerts)

> Health routine — 2026-05-14 (Florida local time). Branch: `health/202605140048-codeql-actions-language`.

## Goal

Close 15 stale Medium CodeQL alerts (`actions/missing-workflow-permissions`) that have been open since 2026-04-30. The underlying permissions fix landed in PR #322 (`eb4b84a`); the alerts remain open only because the custom `codeql.yml` matrix did not include the `actions` language, so nothing re-evaluates the workflow files.

## Architecture

No runtime code change. Single-file YAML change to `.github/workflows/codeql.yml`: add a third matrix entry `{language: actions, build-mode: none}`. The `Analyze (actions)` job runs buildless; on the next push-to-main CodeQL run it re-evaluates `.github/workflows/*.yml` and the 15 stuck alerts auto-close as `fixed`.

## Tech stack

- GitHub Actions / CodeQL `actions/codeql-action@v4`
- `actions` language analyzer (buildless, no toolchain required)

## Design invariants

- No change to Rust or JS/TS analysis configuration.
- No suppression entries added to `docs/security/codeql-suppressions.md` — these are real fixes (permissions), not dismissals.
- `codeql.yml` matrix stays declarative; new entry mirrors the shape of the existing `rust` and `javascript-typescript` entries.

## Out of scope

- Actual permissions changes (done in PR #322).
- Any new Rust / JS-TS alert triage.
- Dependabot residuals (rsa / glib / rand — all upstream-blocked, expiry 2026-07-31).

## Rollback

Revert the 9-line addition to `codeql.yml`. Alerts revert to open (stale) state.

## Open questions

None — mechanism confirmed: `Analyze (actions)` completed success on PR #323 in 48 seconds (buildless).

## File structure

```
.github/workflows/codeql.yml   — +9 lines (actions matrix entry)
plans/0116-gar-605-codeql-actions-language.md  — this file
plans/README.md                — +1 row
docs/security/dependabot-status.md — updated session snapshot
```

## Tasks

- [x] T1: Create Linear issue GAR-605 (child of GAR-486)
- [x] T2: Add `language: actions, build-mode: none` entry to `codeql.yml` matrix (implemented in PR #323)
- [x] T3: CI green on PR #323 — `Analyze (actions)` success; all other 17 checks success; `Test (windows-latest)` in_progress
- [x] T4: Merge PR #323 (squash) — closes 15 Medium alerts after next CodeQL run on main
- [x] T5: Merge PR #321 (docs bookkeeping — plan 0114 T8) (`c45fcff`)
- [x] T6: Create this plan file (0116)
- [x] T7: Update `docs/security/dependabot-status.md` with 2026-05-14 session snapshot
- [x] T8: Update `plans/README.md` row 0116 + merge health/ PR

## Risk register

| Risk | Likelihood | Mitigation |
|---|---|---|
| `Test (windows-latest)` fails on PR #323 | Low | Fix and push; docs-only CI changes shouldn't flake |
| 15 alerts don't auto-close after merge | Low | CodeQL re-scans on push-to-main; if alerts persist >24h, file follow-up under GAR-605 |
| New `actions` analysis surfaces new alerts in `codeql.yml` | Low | `Analyze (actions)` already ran on PR #323 with no new findings |

## Acceptance criteria

1. PR #323 merged to main (squash).
2. `Analyze (actions)` job present and green in next CodeQL run on main.
3. `gh api repos/michelbr84/GarraRUST/code-scanning/alerts?state=open&severity=medium` returns 0 within 24h of merge.
4. `plans/README.md` row 0116 shows `✅ Merged`.

## Cross-references

- GAR-605 (Linear): https://linear.app/chatgpt25/issue/GAR-605
- GAR-486 (parent umbrella): https://linear.app/chatgpt25/issue/GAR-486
- PR #323: https://github.com/michelbr84/GarraRUST/pull/323
- PR #322 (permissions fix): https://github.com/michelbr84/GarraRUST/pull/322
- `docs/security/codeql-suppressions.md` — suppression ledger

## Estimativa

T-shirt: XS (9 lines of YAML, already implemented). Total calendar time: ~30 min (CI wait).
