# Plan 0306 — GAR-844: garra-routine-trigger.yml retry on transient 401

## Goal

Fix `garra-routine-trigger.yml` to survive transient GitHub API 401 errors on
`gh issue list`, which caused a false-positive CI failure on main at 16:07 UTC
2026-06-10 (run ID 27289189001). The same workflow succeeded at 10:31 UTC the
same day — this is a transient auth blip, not a structural permissions problem.

## Architecture

The workflow is infrastructure-only (creates/comments on GitHub issues). It
does not touch Rust code, migrations, or any security surface. The fix is
confined to `.github/workflows/garra-routine-trigger.yml` — one step, add a
retry wrapper.

## Tech stack

- GitHub Actions YAML
- `gh` CLI (already used in the workflow)
- Bash `for` loop with `sleep` for backoff

## Design invariants

- No Rust/Flutter code changed.
- No permissions widened.
- Idempotency of the workflow is preserved (dedup by label still works).
- Retry only wraps the `gh issue list` read; write operations (label create,
  issue create, comment) are already idempotent or use `|| true`.

## Out of scope

- Fixing the root cause of transient 401s (GitHub infrastructure).
- Adding retry to the other `gh` calls (they tolerate failure or are idempotent).
- Changing the cron schedule.

## Rollback

`git revert` the single commit — workflow falls back to original behavior.

## File structure

```
.github/workflows/garra-routine-trigger.yml   ← only change
plans/0306-gar-844-garra-routine-trigger-retry.md  ← this file
plans/README.md                                ← add 0306 row
```

## M1: Add retry to "Find any open tracking issue" step

- [ ] Wrap `gh issue list` in a retry loop (3 attempts, 5s linear backoff)
- [ ] Ensure the loop propagates failure only after all retries are exhausted
- [ ] Verify the `existing` / `skip` outputs still emit correctly on success
- [ ] `cargo fmt` / `cargo clippy` N/A (YAML-only change)
- [ ] Commit `fix(ci): GAR-844 — add retry on gh issue list in garra-routine-trigger.yml`

## M2: Plan bookkeeping

- [ ] Add 0306 row to plans/README.md
- [ ] Commit `docs(plans): add plan 0306 for GAR-844`

## Risk register

| Risk | Mitigation |
|---|---|
| Retry loop loops forever | Hard cap at 3 iterations |
| `sleep 5` blocks the runner too long | Total max delay 10s — well under 5-min job timeout |
| Retry masks a real permissions regression | On persistent failure (all 3 retries), exits non-zero — CI still fails |

## Acceptance criteria

- `garra-routine-trigger.yml` retries `gh issue list` up to 3×, 5s apart
- Workflow deduplicates correctly on success (existing issue found → comment)
- Workflow creates new issue correctly (no existing issue → create)
- CI green on the PR (Format, Clippy, Test×3, Build, MSRV, etc.)

## Cross-references

- Linear: [GAR-844](https://linear.app/chatgpt25/issue/GAR-844)
- Failed run: https://github.com/michelbr84/GarraRUST/actions/runs/27289189001
- Workflow: `.github/workflows/garra-routine-trigger.yml`
- Previous run 110 plan: [0302](0302-gar-841-health-run-110.md)

## Estimativa

< 30 min. Single YAML file change + plan bookkeeping.
