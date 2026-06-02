# Plan 0256 — GAR-778: Health Run 76 — Restore Node.js 24 action versions

## Goal

Restore `actions/checkout@v6` and `actions/setup-node@v6` across all 8 GitHub Actions
workflow files, reversing the regression introduced by PR #561 (feat/search: GAR-733)
which overwrote all workflow files using old `@v4` templates.

## Architecture

Pure YAML mechanical bump — no Rust code, no Cargo.toml, no migrations.

## Tech stack

GitHub Actions YAML. Affects 8 workflow files.

## Design invariants

- Only `actions/checkout` and `actions/setup-node` are in scope.
- `github/codeql-action/init@v4` and `analyze@v4` are **NOT** changed here
  (tracked by GAR-502, different deadline).
- `gitleaks-action@v2` and `dependency-review-action@v5` are **NOT** changed here
  (tracked by GAR-482 — no upstream v5 available yet for those).
- `dtolnay/rust-toolchain`, `docker/*`, `aws-actions/*`, `EmbarkStudios/*` — out of scope.

## Out of scope

- Any Rust compilation changes.
- Any test additions (YAML-only changes don't require TDD regression tests).
- `codeql.yml` `codeql-action` versions (GAR-502).

## Rollback

`git revert <commit>` — mechanical YAML change, trivially reversible.

## Open questions

None.

## File structure

```
.github/workflows/
  ci.yml             — checkout@v4→v6 (13×), setup-node@v4→v6 (1×)
  deploy.yml         — checkout@v4→v6 (2×)
  release.yml        — checkout@v4→v6 (6×)
  mutants.yml        — checkout@v4→v6 (1×)
  cargo-audit.yml    — checkout@v4→v6 (1×)
  quality-ratchet.yml — checkout@v4→v6 (1×)
  codeql.yml         — checkout@v4→v6 (1×) — codeql-action stays @v4
  branch-cleanup.yml — checkout@v4→v6 (1×)
plans/
  0256-gar-778-health-run-76-restore-node24-checkout-v6.md  (this file)
plans/
  README.md  — row 0256 added
```

## M1 tasks

- [x] T1: Upgrade `actions/checkout@v4` → `@v6` in ci.yml (13 occurrences)
- [x] T2: Upgrade `actions/setup-node@v4` → `@v6` in ci.yml (1 occurrence)
- [x] T3: Upgrade `actions/checkout@v4` → `@v6` in deploy.yml (2 occurrences)
- [x] T4: Upgrade `actions/checkout@v4` → `@v6` in release.yml (6 occurrences)
- [x] T5: Upgrade `actions/checkout@v4` → `@v6` in mutants.yml (1 occurrence)
- [x] T6: Upgrade `actions/checkout@v4` → `@v6` in cargo-audit.yml (1 occurrence)
- [x] T7: Upgrade `actions/checkout@v4` → `@v6` in quality-ratchet.yml (1 occurrence)
- [x] T8: Upgrade `actions/checkout@v4` → `@v6` in codeql.yml (1 occurrence)
- [x] T9: Upgrade `actions/checkout@v4` → `@v6` in branch-cleanup.yml (1 occurrence)
- [x] T10: Update plans/README.md row 0256
- [x] T11: Commit + push + open PR

## Risk register

| Risk | Likelihood | Mitigation |
|------|------------|-----------|
| checkout@v6 API incompatibility | Very low | Same interface as v4/v5; confirmed in GAR-481 PR #95 |
| setup-node@v6 cache change | Very low | Cache config unchanged; we pass `cache: 'npm'` explicitly |
| Any workflow breakage | Very low | YAML-only, no application logic |

## Acceptance criteria

- [x] `grep -r "checkout@v4\|setup-node@v4" .github/workflows/` returns zero hits
- [x] All CI checks pass on the PR
- [x] No Node.js 20 deprecation warning in any CI run on this branch

## Cross-references

- GAR-778 (this issue): https://linear.app/chatgpt25/issue/GAR-778
- GAR-481 (original fix, Done): https://linear.app/chatgpt25/issue/GAR-481
- GAR-482 (third-party, Backlog): https://linear.app/chatgpt25/issue/GAR-482
- GAR-502 (codeql-action v4→v5, Backlog): https://linear.app/chatgpt25/issue/GAR-502
- PR #95 (GAR-481 original fix): https://github.com/michelbr84/GarraRUST/pull/95
- PR #561 (GAR-733 feat/search, regression source): https://github.com/michelbr84/GarraRUST/pull/561

## Estimativa

~5 minutes implementation (mechanical YAML-only). CI ~30 min.
