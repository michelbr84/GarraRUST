# Plan 0150 — GAR-648: Skill Auto-Updater — diff + branch + PR via gh

**Status:** 🚧 In Progress (2026-05-18)
**Issue:** [GAR-648](https://linear.app/chatgpt25/issue/GAR-648) (sub-issue 7/10 of [GAR-641](https://linear.app/chatgpt25/issue/GAR-641))
**Branch:** `routine/202605181830-gar-648-skill-auto-updater`
**Epic parent:** Fase 1.4 — Garra Learning Agent (`epic:learning-agent`)

---

## Goal

Implement `garraia-learning::updater` — the Skill Auto-Updater component (7/10 of the
Learning Agent epic). Replaces the 2-line stub with a full proposal pipeline.

When the Evaluator detects that a skill's score is falling, the Updater:
1. Bumps the skill version (PATCH/MINOR based on reason heuristic).
2. Creates a canonical git branch `learning/skill-<name>-v<old>-v<new>`.
3. Writes the updated skill file and commits.
4. Opens a PR via `gh pr create` with score history, diff, evidence, and rollback info.
5. **Never** calls `gh pr merge` — human approval is mandatory.
6. Is idempotent: calling twice for the same evidence returns the existing PR URL.

---

## Architecture

```text
crates/garraia-learning/src/updater.rs   (replace stub)
  UpdateEvidence struct                  score_before/after, reason, new_body,
                                         log_excerpt, ci_url
  BumpKind enum                          Patch | Minor
  PullRequestProposal struct             branch, title, body, pr_url
  ShellRunner trait                      run_git(&[&str], &Path) -> Result<String>
                                         run_gh(&[&str], &Path) -> Result<String>
  ProcessShellRunner struct              real process spawner
  pub fn propose_update(skill, evidence) entry point (uses ProcessShellRunner)
  pub fn propose_update_with_runner(…)   injectable runner for tests
  pub fn auto_merge_guard()              returns deterministic Error (no auto-merge)
  pub fn detect_bump_kind(reason)        heuristic: wording/typo/doc → Patch, else Minor
  pub fn bump_version(version, kind)     semver patch/minor increment
  pub fn branch_name(name, old, new)     canonical naming with name sanitization
  fn pr_body(…)                          Markdown body with score, evidence, rollback
  fn assemble_skill_file(…)              frontmatter version bump + new body injection
  fn find_existing_pr(branch, cwd, run)  idempotency via gh pr list
  fn git_root(start)                     walk up dirs to find .git
```

---

## Tech stack

- Rust edition 2024 (existing crate)
- `garraia_common::Error` / `Result` — error propagation
- `std::process::Command` — git + gh invocation (real runner)
- `tempfile` (already in dev-deps) — temp dirs for tests
- `ShellRunner` trait (new, in this file) — mockable for unit tests

No Cargo.toml changes required.

---

## Design invariants

- **No `unwrap()` in production paths.**
- **Never call `gh pr merge`** — `auto_merge_guard()` returns a deterministic error.
- **Idempotent**: `find_existing_pr` checks `gh pr list --head <branch>` before creating.
- **Locked skills are rejected**: `skill.frontmatter.locked == true` → immediate `Err`.
- **No source_path → immediate `Err`**: updater needs a real path to commit.
- **Sanitize branch name**: non-alphanumeric + non-hyphen chars → `-`.
- **PATCH bump** when reason contains: wording, typo, format, style, doc, clarif, minor fix.
- **MINOR bump** for all other reasons (structural step changes).
- **Version passthrough**: if version is unparseable semver, return it unchanged (no panic).
- **PR body always includes**: score before/after, reason, score history link, rollback command.
- **Return to original branch** after committing (best-effort, not fatal if it fails).

---

## Validações pré-plano

- [x] `garraia-learning` crate exists with stub `updater.rs`.
- [x] `LearningSkillFrontmatter.version`, `.locked`, `.score` all `pub`.
- [x] `Skill.source_path: Option<PathBuf>` is `pub`.
- [x] `tempfile = "3"` already in `[dev-dependencies]`.
- [x] `garraia_common::Error` has `Other(String)` and `Io(std::io::Error)` variants.
- [x] No circular dep: updater does not import evaluator or registry.
- [x] `cargo check -p garraia-learning` green.

---

## Out of scope

- Polling GitHub Actions CI for PR checks (tracked separately).
- Safety Gate enforcement before PR creation (GAR-649, 8/10).
- Skill Retriever (GAR-646, blocked on Fase 2.1 embeddings).
- Auto-promotion after approval (GAR-651, 10/10).
- Real `PgVectorStore` or model inference inside the updater.

---

## Rollback

`git revert` the commit on `routine/202605181830-gar-648-skill-auto-updater`.
`updater.rs` stub is trivial to restore (14 lines).

---

## §12 Open questions

| # | Question | Resolution |
|---|----------|------------|
| 1 | Should the updater call `safety_gate` before creating the PR? | GAR-649 owns the Safety Gate integration; updater opens the PR regardless of gate status, gate is called by the promotion step. |
| 2 | How to handle `gh` not installed? | `ProcessShellRunner::run_gh` returns `Err` with clear message; callers handle gracefully. |
| 3 | Should we include full diff in PR body? | No — full diff is too noisy; PR diff tab covers it. Body includes score delta, reason, and log excerpt. |

---

## File structure

```
crates/garraia-learning/src/updater.rs   ← replace 14-line stub (~280 LOC impl + ~170 LOC tests)
plans/0150-gar-648-skill-auto-updater.md ← this file
plans/README.md                          ← new row
```

---

## M1 — Implementation tasks

- [x] T1: Create plan 0150 + update plans/README.md.
- [ ] T2: Implement `updater.rs` — types + ShellRunner + helpers.
- [ ] T3: Implement `propose_update_with_runner` main logic.
- [ ] T4: Write ≥16 unit tests (pure functions + MockShellRunner + temp-dir integration).
- [ ] T5: `cargo check -p garraia-learning` green.
- [ ] T6: `cargo test -p garraia-learning updater` ≥16 tests green.
- [ ] T7: `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean.
- [ ] T8: Commit, push, open PR, wait for CI green.
- [ ] T9: Squash-merge, mark GAR-648 Done, update ROADMAP.md + plans/README.md.

---

## Risk register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| `gh` not installed in CI | High | Low | tests use MockShellRunner; real runner only in prod |
| `git root` walk fails on detached HEAD | Low | Medium | fallback to `start` dir |
| Frontmatter parsing fails (no `---`) | Low | Low | `assemble_skill_file` fallback: prepend new fm |
| Duplicate PR despite idempotency check | Low | Low | `gh pr list` returns empty → create; worst case: two PRs, manual close |

---

## Acceptance criteria

- [ ] Locked skill → `Err("skill '...' is locked")`.
- [ ] No `source_path` → `Err("skill has no source_path")`.
- [ ] `auto_merge_guard()` returns `Error::Other("auto-merge is prohibited...")`.
- [ ] `detect_bump_kind("fix wording")` → `BumpKind::Patch`.
- [ ] `detect_bump_kind("restructure steps")` → `BumpKind::Minor`.
- [ ] `bump_version("1.2.3", Patch)` → `"1.2.4"`.
- [ ] `bump_version("1.2.3", Minor)` → `"1.3.0"`.
- [ ] Branch name for skill "my-skill" v1.0.0→v1.0.1 → `"learning/skill-my-skill-v1.0.0-v1.0.1"`.
- [ ] `propose_update_with_runner` with idempotency mock → existing URL returned without new git calls.
- [ ] Happy-path test: all git/gh calls made in correct order, `PullRequestProposal` returned.
- [ ] `cargo test -p garraia-learning updater` ≥16 tests green.

---

## Cross-references

- GAR-641 (epic) → GAR-642 (scaffold ✅) → GAR-643 (miner ✅) → GAR-644 (generator ✅) → GAR-645 (registry ✅) → GAR-647 (evaluator ✅) → **GAR-648** (updater, this plan)
- GAR-649 (safety gate, 8/10) — will call `safety_gate()` before PR promotion
- GAR-646 (retriever, blocked on embeddings)
- Plan 0149 (evaluator, merged `a79321b`) — defines `EvalResult`, `EvalSignals` consumed by updater

---

## Estimativa

- Baixa: 2h (helpers + mock tests)
- Provável: 3h (full happy-path + idempotency + edge cases)
- Alta: 4h (unexpected `gh` API shape changes)
