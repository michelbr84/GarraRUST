# Plan 0113 — GAR-601: Bump aws-actions/configure-aws-credentials v4 → v6

**Status:** ✅ Merged  
**Branch:** `health/202605131257-aws-actions-v6`  
**PR:** [#313](https://github.com/michelbr84/GarraRUST/pull/313)  
**Commit:** `4374623`  
**Linear:** [GAR-601](https://linear.app/chatgpt25/issue/GAR-601)  
**Triggered by:** Health routine 2026-05-13, priority (h) — GitHub Actions Node20 deprecation deadline (2026-06-02)

---

## Goal

Upgrade `aws-actions/configure-aws-credentials` from `@v4` (Node20) to `@v6` (Node24) in `.github/workflows/deploy.yml` before the GitHub-forced Node20→Node24 switch on 2026-06-02. Closes Dependabot PR #281.

## Architecture

Single 1-line change in one workflow file. No Rust code, no Cargo files, no schema.

## Tech stack

- GitHub Actions YAML

## Design invariants

- Only `deploy.yml` changes
- Inputs used (`aws-access-key-id`, `aws-secret-access-key`, `aws-region`) are identical in v4/v5/v6
- No boolean inputs used — v5 breaking change (boolean input handling) does not affect us
- v6 requires GitHub runner ≥ v2.327.1 (GitHub-hosted runners are always current)

## Out of scope

- `thiserror`, `notify`, `toml`, `dialoguer` Dependabot PRs (#284, #288, #290, #292) — deferred to next health run
- GAR-482 (dependency-review-action, gitleaks-action) — separate tracking issue

## Rollback

Revert the 1-line change (v6 → v4). No data migration needed.

## Open questions

None.

## File structure

```
.github/workflows/deploy.yml   # line 91: @v4 → @v6
plans/0113-gar-601-aws-actions-v6.md  # this file
plans/README.md                # new row
```

## M1 — Single task: bump aws-actions version

- [x] Edit `deploy.yml` line 91: `aws-actions/configure-aws-credentials@v4` → `@v6`
- [x] Verify no other workflow files reference the old version
- [x] Commit, push, open PR (#313)
- [x] CI green (all 18 checks)
- [x] Squash-merge, mark GAR-601 Done

## Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| v6 runner requirement not met | Very Low | GitHub-hosted runners auto-update |
| Boolean input breakage (v5) | None | We don't use boolean inputs |
| Other workflow files missed | Low | grep to verify after change |

## Acceptance criteria

1. `aws-actions/configure-aws-credentials@v4` absent from all workflow files on main
2. All CI checks green on the health/ PR
3. GAR-601 moved to Done in Linear
4. Dependabot PR #281 auto-closes after merge

## Cross-references

- Dependabot PR #281 (the automated bump this ports)
- GAR-481 (Done — official actions/* Node24 migration, 2026-04-29)
- GAR-482 (Backlog — dependency-review-action + gitleaks-action tracking)
- GitHub deadline notice: 2026-06-02 forced Node20→Node24 switch

## Estimativa

< 5 min implementation. CI runtime ~15–20 min.
