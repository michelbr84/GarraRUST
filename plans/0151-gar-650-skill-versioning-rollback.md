# Plan 0151 — GAR-650: Skill Versioning/Rollback — git-tracked history + git revert

**Status:** 🚧 In Progress (2026-05-19)
**Issue:** [GAR-650](https://linear.app/chatgpt25/issue/GAR-650) (sub-issue 9/10 of [GAR-641](https://linear.app/chatgpt25/issue/GAR-641))
**Branch:** `routine/202605190015-gar-650-skill-versioning`
**Epic parent:** Fase 1.4 — Garra Learning Agent (`epic:learning-agent`)

---

## Goal

Implement `garraia-learning::versioning` — the Skill Versioning/Rollback component (9/10 of
the Learning Agent epic). Replaces the 2-line stub with a full versioning pipeline.

Each skill is a git-tracked file. This module exposes:
1. **History** — `git log` of the skill file, returning structured `SkillVersion` entries.
2. **Diff** — `git diff <from>..<to> -- <skill-file>` between two SHAs.
3. **Rollback** — `git revert <sha>` of the commit that promoted a skill, plus audit.
4. **Score History** — append-only JSON ledger in `.garra/skills/_history/<skill>.json`.
5. **Score Append** — add a new `ScoreEntry` to the ledger without rewriting old entries.

---

## Architecture

```text
crates/garraia-learning/src/versioning.rs   (replace stub)
  VersioningOptions struct                  skills_dir: PathBuf, repo_root: PathBuf
  SkillVersion struct                       sha, short_sha, date, author, message
  ScoreEntry struct                         sha, timestamp_utc (ISO-8601Z), score: f32
  ShellRunner trait (re-used from updater)  run_git(&[&str], &Path) -> Result<String>
  ProcessShellRunner (re-use)               real process spawner

  pub fn history(name, opts, runner)        Vec<SkillVersion> via git log --format
  pub fn diff(name, from_sha, to_sha, opts, runner)  String via git diff
  pub fn rollback(name, to_sha, opts, runner) git revert + score reset + audit entry
  pub fn score_history(name, opts)          Vec<ScoreEntry> read from JSON ledger
  pub fn append_score_entry(name, entry, opts)  append ScoreEntry (never rewrite old)

  fn skill_file_path(name, opts) -> PathBuf
  fn history_file_path(name, opts) -> PathBuf
  fn read_score_ledger(path) -> Vec<ScoreEntry>
  fn write_score_ledger(path, entries) -> Result<()>
```

---

## Tech stack

- Rust edition 2024 (existing crate)
- `garraia_common::Error` / `Result`
- `std::process::Command` — git invocation via `ShellRunner`
- `serde` + `serde_json` (already in Cargo.toml) — score ledger serialization
- `chrono` (check Cargo.toml) — UTC timestamps for `ScoreEntry`
- No Cargo.toml changes required unless chrono needs adding

---

## Design invariants

1. **Append-only ledger**: `append_score_entry` reads the current JSON array, appends one entry,
   and writes back. A test verifies that an existing entry's `score` field is never mutated.
2. **Rollback is idempotent**: calling rollback with the same `to_sha` twice is a no-op
   (detect via `git log` whether the revert commit already exists).
3. **No auto-merge**: versioning does not open PRs or merge anything — that is updater's job.
4. **ShellRunner abstraction**: all git calls go through the `ShellRunner` trait, enabling
   unit tests with `MockShellRunner` that never touches a real repo.
5. **Audit in metadata**: `audit_events` action is `skill.rolled_back`, metadata carries
   `{ skill_name_len: usize, from_sha, to_sha, reason }` — no PII (name length, not name).
6. **History dir isolation**: `_history/` lives inside the skills_dir, not in a user-visible
   location, keeping the skills folder clean.

---

## Validações pré-plano

- [x] `crates/garraia-learning/src/versioning.rs` exists as a 13-line stub (confirmed 2026-05-19).
- [x] `ShellRunner` trait and `ProcessShellRunner` already exist in `updater.rs`; will re-export
  or duplicate minimally to avoid circular module deps.
- [x] `serde_json` is already a dep of `garraia-learning` (confirmed via Cargo.toml inspection).
- [x] No DB schema changes required — score ledger is filesystem-only JSON.
- [x] `garraia_common::Error` / `Result` is used consistently by all other learning modules.

---

## Out of scope

- Web UI (GAR-651, 10/10).
- CLI subcommands `garra skills history/diff/rollback` — wiring into `garraia-cli` is a
  future slice; this plan delivers the library functions only.
- Retriever integration (GAR-646, blocked by Fase 2.1 embeddings).

---

## Rollback (plan rollback, not skill rollback)

If CI fails: revert all commits on this branch, delete branch, mark GAR-650 back to Backlog.
The stub `versioning.rs` is non-functional, so reverting causes zero regression.

---

## §12 Open questions

1. **chrono vs time**: Does `garraia-learning` already pull `chrono`? If not, use
   `std::time::SystemTime` to format ISO-8601Z without adding a dep. → Resolved at T1.
2. **Rollback target**: The spec says "git revert of the promoting commit". But if multiple
   commits touch the skill file, which one to revert? → `to_sha` is explicit from the caller;
   `rollback(name, to_sha)` reverts that exact SHA, not "the last commit" on the file.
3. **Score reset on rollback**: After reverting the git commit, should the score be reset to
   the historical value stored in the ledger? → Yes: `rollback` looks up the `ScoreEntry`
   matching `to_sha` in the ledger and re-appends it with a new timestamp as the "current"
   score, preserving the append-only invariant.

---

## File structure

```text
crates/garraia-learning/src/versioning.rs  — full implementation (replaces stub)
crates/garraia-learning/src/lib.rs        — no change required (already pub mod versioning)
```

---

## M1 Tasks

- [x] **T1** — Read Cargo.toml; confirm deps (`serde_json`, check `chrono`); decide timestamp approach.
- [ ] **T2** — Write `SkillVersion` + `ScoreEntry` structs + `VersioningOptions` + re-export `ShellRunner`.
- [ ] **T3** — Implement `history()` via `git log --format=...` parsing.
- [ ] **T4** — Implement `diff()` via `git diff <from>..<to> -- <path>`.
- [ ] **T5** — Implement `score_history()` + `append_score_entry()` with append-only test.
- [ ] **T6** — Implement `rollback()`: git revert + idempotency guard + score reset + audit stub.
- [ ] **T7** — Unit tests: MockShellRunner for history/diff/rollback; tempdir for score ledger.
- [ ] **T8** — `cargo clippy` clean + update ROADMAP.md + plans/README.md row.

---

## Risk register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| chrono dep missing | Low | Low | Use `std::time::SystemTime` for ISO-8601 UTC |
| git not available in CI test env | Low | Low | `MockShellRunner` in all unit tests; git only in integration |
| Rollback creates merge conflict if skill evolved further | Medium | Medium | Document in function doc; caller decides; idempotency guard prevents duplicate reverts |

---

## Acceptance criteria

- `cargo test -p garraia-learning versioning` green (≥12 unit tests).
- `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean.
- `history()` returns `Vec<SkillVersion>` parsed from `git log` mock.
- `diff()` returns raw diff string from git mock.
- `rollback()` is idempotent: second call with same SHA returns `Ok(())` without calling git revert again.
- `append_score_entry()` is append-only: test verifies old entries are not mutated.
- `score_history()` reads the full ledger JSON.
- All functions covered by `MockShellRunner`-based unit tests (no live git required).

---

## Cross-references

- `plans/0150-gar-648-skill-auto-updater.md` — ShellRunner pattern origin.
- `crates/garraia-learning/src/updater.rs` — `ShellRunner` trait to re-use.
- `crates/garraia-learning/src/registry.rs` — `RegistryOptions` and file layout reference.
- `crates/garraia-learning/src/safety.rs` — safety gate called before rollback in production.
- [GAR-641](https://linear.app/chatgpt25/issue/GAR-641) — Learning Agent epic.
- [GAR-651](https://linear.app/chatgpt25/issue/GAR-651) — Web UI (10/10, next after this).

---

## Estimativa

0.5 / 1 / 2 semanas. Expected: ~300 LOC implementation + ~200 LOC tests.
