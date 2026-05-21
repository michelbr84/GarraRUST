# Plan 0161 — GAR-496: Repo Workflow Seguro

**Linear issue:** [GAR-496](https://linear.app/chatgpt25/issue/GAR-496) — "Repo workflow seguro" (Backlog → In Progress). Labels: `epic:maxpower`. Project: epic [GAR-492](https://linear.app/chatgpt25/issue/GAR-492).

**Status:** ✅ Merged 2026-05-21 via PR #455 (`1b7f04c`) — Done.

## Goal

Implement safe git/gh repo workflow wrappers for the GarraMaxPower pipeline (ROADMAP §1.2.1).
Provides a `RepoWorkflow<R>` struct that wraps git and `gh` CLI operations with pre-flight
safety checks:

- Refuses to push to protected branches (`main`, `master`, `release/*`).
- Requires a clean working tree before creating feature branches.
- Reports current branch and tree status so the operator knows whether it's safe to proceed.

## Architecture

**New module `crates/garraia-cli/src/repo_workflow.rs`** (~280 LOC):

1. `WorkflowError` enum — `ProtectedBranch { branch }`, `DirtyWorkingTree { summary }`,
   `CommandFailed { cmd, stderr }`, `ParseError(String)`.
2. `trait GitRunner: Send + Sync` — `run(root: &Path, args: &[&str]) -> Result<String, WorkflowError>`.
3. `struct ProcessRunner` — production impl via `std::process::Command`.
4. `fn is_protected_branch(branch: &str) -> bool` — checks against `PROTECTED_PATTERNS`.
5. `struct RepoWorkflow<R: GitRunner>` with methods:
   - `current_branch(&self) -> Result<String, WorkflowError>` — `git rev-parse --abbrev-ref HEAD`
   - `is_clean(&self) -> Result<bool, WorkflowError>` — `git status --porcelain`
   - `create_branch(&self, name: &str) -> Result<(), WorkflowError>` — dirty-tree pre-check then `git checkout -b`
   - `push_branch(&self, branch: &str) -> Result<(), WorkflowError>` — protected-branch pre-check then `git push -u origin`
   - `open_pr(&self, title, body, base) -> Result<String, WorkflowError>` — `gh pr create`
6. **Unit tests** via `MockRunner` struct with a map of `(args_key -> Result<String, WorkflowError>)`.
   12 unit tests covering all pre-flight guards.

**Modify `crates/garraia-cli/src/max_power.rs`**:
- Replace the `[GAR-496..GAR-501] not yet implemented` placeholder in `route_goal` with a
  real preflight summary: detect git root, call `current_branch()` + `is_clean()`, print
  the result with appropriate warnings. Silently no-ops when not in a git repo.

**Modify `crates/garraia-cli/src/main.rs`**:
- Add `mod repo_workflow;`

## Tech stack

- `std::process::Command` for process execution — no new deps.
- `garraia_common::safety_gate` already guards destructive shell ops; this module adds
  structured git semantics on top.

## Design invariants

1. **Zero new deps** — `repo_workflow` uses only `std::process::Command` + `std::path::Path`.
2. **Protected patterns are constants** — `PROTECTED_PATTERNS: &[&str]` compiled in; not
   configurable at runtime (security boundary, not a user preference).
3. **No force push** — no `push_branch` variant accepts `--force`; force push is handled
   exclusively by `garraia_common::safety_gate`.
4. **Fail-open in non-git directories** — `route_goal` catches `CommandFailed` from
   `current_branch()` and skips the preflight block (user may be in `/tmp` or similar).
5. **No PII in errors** — `WorkflowError::CommandFailed.stderr` is already from git/gh stderr,
   which may contain branch names (not user data).

## Out of scope

- Cloning repositories (GAR-498/499 full pipeline)
- PR templates or label management
- Handling multiple remotes (assumes `origin`)
- `git merge` / `rebase` orchestration

## Rollback

Pure additive change. Removing `repo_workflow.rs` and the `mod` declaration restores
the original state with no behavior change elsewhere.

## File structure

```
crates/garraia-cli/src/
  repo_workflow.rs          ← new (~280 LOC)
  max_power.rs              ← modified (replace placeholder with preflight summary)
  main.rs                   ← modified (add mod repo_workflow)
plans/
  0161-gar-496-repo-workflow.md  ← this file
plans/README.md             ← add row 0161, mark 0160 merged
ROADMAP.md                  ← §7 mark GAR-495 Done, add GAR-496 In Progress entry
```

## Tasks

- [x] T1 — Write `repo_workflow.rs`: `WorkflowError` + `GitRunner` trait + `ProcessRunner` + `is_protected_branch` + `RepoWorkflow` + `MockRunner` + 12 unit tests
- [x] T2 — Update `max_power.rs`: replace placeholder with preflight summary call
- [x] T3 — Update `main.rs`: `mod repo_workflow;`
- [x] T4 — `cargo check -p garraia` + `cargo clippy -p garraia` green
- [x] T5 — `cargo test -p garraia` green
- [x] T6 — Commit + push (per-task commits)
- [x] T7 — PR opened + CI green
- [x] T8 — Squash merge + bookkeeping (ROADMAP §7 + plans/README)

## Acceptance criteria

- `is_protected_branch("main")` → `true`; `is_protected_branch("feat/foo")` → `false`.
- `push_branch("main")` → `Err(WorkflowError::ProtectedBranch { .. })`.
- `create_branch("feat/foo")` on dirty tree → `Err(WorkflowError::DirtyWorkingTree { .. })`.
- `garra max-power --goal "fix bug X"` prints preflight branch + clean status.
- `cargo check --workspace` and `cargo clippy --workspace -- -D warnings` remain green.

## Risk register

| Risk | Mitigation |
|------|-----------|
| Not in a git repo at startup | `route_goal` catches `CommandFailed` and skips preflight silently |
| `gh` not installed | `open_pr` returns `CommandFailed`; no panic |
| Branch name injection | `git checkout -b` does not spawn a shell; args are passed as separate strings |

## Estimativa

< 2 hours. ~310 LOC total (new + modified).

## Cross-references

- plan 0153 (GAR-494 max-power skeleton)
- plan 0154 (GAR-497 bash safety gate — related safety layer)
- plan 0160 (GAR-495 capability prompt — predecessor)
- ROADMAP §1.2.1 §7 item 5
- Epic GAR-492
