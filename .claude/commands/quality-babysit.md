---
description: AI Quality Ratchet babysitting loop — read quality-report.md, fix one regression at a time, push, repeat. PR-1 ships this skill in MANUAL-ONLY mode (read + propose without committing). Auto-loop (up to 5 iterations) is documented but disabled until PR-2/PR-3 when the ratchet is validated.
---

# `/quality-babysit` — AI Quality Ratchet self-correction loop

This skill drives Claude through the **babysitting loop** described in
`plans/0060-quality-ratchet-pr1.md` §7. The loop reads the most recent
`quality-report.md`, identifies **one** regression, applies the minimal fix,
runs local checks, commits, and pushes — repeating up to N=5 iterations until
CI is green again.

**Status (PR-1, 2026-05-05):** This skill ships in **MANUAL-ONLY** mode. Claude
reads the report and **proposes** a fix textually in chat — but does NOT
auto-commit, auto-push, or auto-loop. Auto-loop activation is PR-2/PR-3 work
after the ratchet has been validated as not noisy.

## Modes

- `--manual-only` (default in PR-1): read the latest report, identify one
  regression, propose a textual fix in the chat, STOP. Michel decides whether
  to apply.
- `--auto-loop` (PR-2+, currently disabled): the workflow described in §"Loop"
  below runs end-to-end with the 12 guardrails enforced.

## Loop (auto-loop, future)

For each iteration (up to 5):

1. `gh pr view <PR> --comments | grep -A 200 "<!-- quality-ratchet-comment -->"`
   — extract the most recent quality-ratchet comment from the current PR.
2. Parse the report. Identify **one** regression row.
3. Apply the minimal fix:
   - `max_file_lines` regression → modularize the named file via
     `superpowers:refactor-module`. Never `git rm`.
   - `coverage_pct` regression → add unit tests for the code added in this PR.
     Never remove existing tests.
   - `audit_high` or `audit_critical` regression → bump or replace the named
     dependency. Never `--ignore-yanked` or `cargo install --force` to bypass.
   - `clippy_warnings` regression → fix the new warnings. Never add
     `#[allow(...)]` to silence them.
4. Local check (in scope of the change):
   - `cargo fmt --check`
   - `cargo clippy --workspace --exclude garraia-desktop --all-targets -- -D warnings`
   - `cargo test -p <crate-affected>`
5. Commit with a concise message: `fix(quality): <metric> <one-line>`.
6. `git push`.
7. Wait for CI to re-run via `gh pr checks <PR> --watch`.
8. If CI green AND `quality-report.md` shows no regressions → STOP, report
   victory. If still regressions → loop step 1.

## Guardrails (12, all mandatory)

1. **Maximum 5 iterations** per session.
2. **Maximum 1 regression** corrected per iteration.
3. Commits small and focused.
4. **NEVER update `.quality/baseline.json` automatically** (or even propose to
   in chat). Baseline updates are Michel's call alone.
5. **NEVER deactivate gates.** Removing a workflow step / a `cargo` invocation
   is forbidden.
6. **NEVER add `continue-on-error: true`** anywhere. The Quality Ratchet uses
   the explicit `--mode report-only|enforce` flag on `compare.py` (Michel
   ajuste #1).
7. **NEVER touch branch protection.** Branch protection changes are out of
   scope for any auto-loop. Always require explicit owner approval.
8. **NEVER auto-merge a PR.** Even if CI is green and report shows zero
   regressions, the PR waits for Michel.
9. If the only viable fix would WORSEN another metric → STOP and ask Michel.
10. If the same failure repeats 2 times in a row → STOP and ask Michel
    (signals a structural regression, not cosmetic).
11. If the fix requires a large refactor → invoke `superpowers:writing-plans`
    BEFORE touching any code. Open a child plan, get approval.
12. If the fix touches **security, auth, storage, RLS, secrets, or critical
    CI infrastructure** → invoke `security-auditor` + `code-reviewer` agents
    before pushing.

## Stop conditions (any one triggers STOP)

- All 12 guardrails apply.
- 5 iterations reached.
- A `quality-report.md` shows zero regressions.
- A guardrail is about to be violated (e.g. need to bump baseline).
- CI fails for a non-quality reason (compilation error, flaky test) — the
  babysit loop only handles ratchet regressions, not unrelated CI failures.
- Working tree is dirty before iteration starts (require explicit user
  invocation to handle uncommitted state).

## Required tools

- `gh` CLI authenticated.
- `git` working in the PR's checked-out worktree.
- `cargo` toolchain matching `rust-toolchain` (currently 1.92).
- `python3` or `py` with `pytest` for local script verification.

## Cross-references

- Plan 0060 (PR-1 scope): `plans/0060-quality-ratchet-pr1.md`
- Plan-mãe (filosofia + 12 guardrails): `~/.claude/plans/voc-est-no-projeto-buzzing-volcano.md`
- Workflow: `.github/workflows/quality-ratchet.yml`
- Filosofia: `.quality/README.md`
