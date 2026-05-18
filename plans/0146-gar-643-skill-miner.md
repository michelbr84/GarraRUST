# Plan 0146 — GAR-643: Skill Miner — detect repeatable patterns in session logs

**Status:** 🚧 In Progress (2026-05-18)
**Issue:** [GAR-643](https://linear.app/chatgpt25/issue/GAR-643) (sub-issue 2/10 of [GAR-641](https://linear.app/chatgpt25/issue/GAR-641))
**Branch:** `routine/202605180623-gar-643-skill-miner`
**Epic parent:** Fase 1.4 — Garra Learning Agent (`epic:learning-agent`)

---

## Goal

Implement `garraia-learning::miner` — the Skill Miner component (2/10 of the Learning
Agent epic). Reads structured session logs from `~/.garra/sessions/*.json`, detects
command sequences that repeat ≥ N times across sessions, and emits candidate skill files
to `~/.garra/skills/_candidates/` with PII-redacted content.

Unblocks the Mine → Generate → Validate → Promote pipeline.

---

## Architecture

```text
crates/garraia-learning/src/miner.rs   (replace stub)
  SessionRecord                         JSON shape for ~/.garra/sessions/<id>.json
  CommandEntry                          single command + exit_code + stdout/stderr
  MineOptions                           sessions_dir / candidates_dir / threshold
  MinedPattern                          detected pattern with count + slug + hash
  pub fn mine(opts: &MineOptions)       main entry point → Vec<MinedPattern>
  pub fn mine_from_log(path)            existing stub → now returns empty vec
  fn load_all_sessions(dir)             read all *.json from dir
  fn load_session(path)                 parse single session JSON
  pub fn normalize_cmd(cmd)             PII-free, arg-generalized command string
  fn find_patterns(sessions, threshold) sliding-window pair detection
  pub fn redact(text)                   replace emails/tokens/paths in body
  fn compute_hash(seq)                  FNV-1a hex8 content hash
  fn derive_slug(seq)                   action-word slug from normalized cmds
  fn write_candidate_if_new(dir, pat)   idempotent YAML+Markdown writer

crates/garraia-learning/Cargo.toml     add serde_json = { workspace = true }
```

---

## Tech stack

- Rust edition 2024 (existing crate)
- `serde_json` (workspace) — parse session JSON files
- `serde` + `serde_yaml` (workspace) — serialize candidate frontmatter
- `garraia-common::Error`/`Result` — error propagation
- `std::collections::HashMap` / `std::collections::HashSet` — pattern counting
- No external hashing crate — FNV-1a implemented inline (~10 LOC)

---

## Design invariants

- **No `unwrap()` in production paths.** All I/O errors propagate via `?`.
- **Idempotency**: candidate filename = `mined-<slug>-<hash8>.md`. Re-running with the
  same sessions produces the same filenames → no duplicates.
- **PII redaction** applied to candidate body before writing:
  - Email-shaped tokens → `<EMAIL>`
  - Long alphanumeric tokens ≥ 32 chars → `<TOKEN>`
  - Absolute home paths (`/home/<user>/`, `/Users/<user>/`) → `<HOMEPATH>`
- **Threshold default = 3.** Configurable via `MineOptions::threshold`.
- **Graceful degradation**: if `sessions_dir` does not exist, returns `Ok(vec![])`.
  Malformed session files are skipped with a `tracing::warn!` — one bad file
  does not abort the entire run.
- **No async** in this module. Session mining is a local FS operation; sync I/O is
  sufficient and simpler to test.

---

## Validações pré-plano

- [x] `garraia-learning` crate exists with stub `miner.rs` returning `Err(Error::Other(...))`.
- [x] `serde_json` is a workspace dependency (`Cargo.toml` line 50).
- [x] `garraia-common::Error` has `Other(String)` variant for wrapping I/O errors.
- [x] `serde_yaml` in workspace deps for candidate frontmatter serialization.
- [x] GAR-643 Linear issue in Backlog (no duplicate `mine` implementation in main).
- [x] `cargo check -p garraia-learning` is green on current main.

---

## Out of scope

- LLM-assisted drafting (GAR-644 — Skill Generator).
- Skill promotion to active registry (GAR-645, GAR-649).
- Embedding-based retrieval (GAR-646 — prereq: Fase 2.1).
- `garra skills mine` CLI subcommand wiring in `garraia-cli` (deferred to CLI slice).
- Reading from `.garra-estado.md` as session source (opt-in transcripts only for now).
- Window sizes > 2 (pair-based detection is sufficient for acceptance criteria; larger
  windows deferred to a future slice once data volume justifies it).

---

## Rollback

The only code change is in `crates/garraia-learning/src/miner.rs` and
`crates/garraia-learning/Cargo.toml`. Reverting the PR restores the stub.
No database schema changes. No new files in `crates/garraia-workspace/`.

---

## §12 Open questions

| # | Question | Resolution |
|---|---|---|
| 1 | Exact session JSON schema — is there an existing producer? | No existing producer in main. Schema is defined by THIS plan as the normative format. |
| 2 | Should the miner handle sessions with 0 or 1 commands? | Yes — skip them during pair extraction (no pairs possible). |
| 3 | Should pairs from the SAME session count multiple times? | No — each session contributes at most 1 occurrence per pair (set-based counting per session). |
| 4 | Candidate body format — full Markdown or YAML-only? | YAML frontmatter + Markdown body with a `## Commands` section listing the normalized sequence. |
| 5 | Should `serde_json` be added to `[workspace.dependencies]` or only to the crate? | It is already in `[workspace.dependencies]` (Cargo.toml:50). |

---

## File structure

```
crates/garraia-learning/
├── Cargo.toml                    + serde_json dep
└── src/
    └── miner.rs                  full implementation (~280 LOC)
plans/
├── README.md                     add row for 0145 + 0146
└── 0146-gar-643-skill-miner.md   this file
```

---

## M1 Tasks

### T1 — Add serde_json dep + cargo check

- [ ] Add `serde_json = { workspace = true }` to `crates/garraia-learning/Cargo.toml`.
- [ ] `cargo check -p garraia-learning` green.

### T2 — Define types: SessionRecord, CommandEntry, MineOptions, MinedPattern

- [ ] Define `CommandEntry { cmd, exit_code, stdout, stderr }` with serde defaults.
- [ ] Define `SessionRecord { session_id, intent, task_family, commands, created_at }`.
- [ ] Define `MineOptions { sessions_dir, candidates_dir, threshold }`.
- [ ] Define `MinedPattern { normalized_sequence, occurrence_count, slug, content_hash }`.
- [ ] `cargo check -p garraia-learning` green.

### T3 — Implement normalize_cmd() with unit tests (RED → GREEN)

- [ ] Write failing tests: integer stripping, `--delete <branch>` normalization.
- [ ] Implement `normalize_cmd()`.
- [ ] `cargo test -p garraia-learning -- normalize` green.

### T4 — Implement redact() with unit tests (RED → GREEN)

- [ ] Write failing tests: email redaction, long-token redaction, home-path redaction.
- [ ] Implement `redact()` (same patterns as `safety.rs` but replaces instead of denies).
- [ ] `cargo test -p garraia-learning -- redact` green.

### T5 — Implement find_patterns() + write_candidate_if_new() with fixture tests (RED → GREEN)

- [ ] Write failing test: 10 sessions fixture, 3 with the `gh pr merge / git push --delete` pair.
- [ ] Implement `find_patterns()` using sliding-window pair detection.
- [ ] Implement `compute_hash()` (FNV-1a inline).
- [ ] Implement `derive_slug()`.
- [ ] Implement `write_candidate_if_new()` with YAML frontmatter + Markdown body.
- [ ] Implement idempotency test (run twice → same files, no duplicates).
- [ ] `cargo test -p garraia-learning -- miner` green.

### T6 — Implement mine() + mine_from_log() + integration test

- [ ] Implement `load_session()` + `load_all_sessions()`.
- [ ] Implement `mine()` orchestrator.
- [ ] Replace `mine_from_log()` stub with no-op returning `Ok(vec![])`.
- [ ] Integration test using `tempdir` with real JSON files.
- [ ] `cargo test -p garraia-learning` green (all 17 safety tests + new miner tests).

### T7 — cargo clippy + cargo fmt

- [ ] `cargo clippy -p garraia-learning -- -D warnings` green.
- [ ] `cargo fmt -p garraia-learning --check` green.

### T8 — Update plans/README.md + ROADMAP bookkeeping

- [ ] Add plan 0145 row to `plans/README.md` (GAR-372, PR #396, merged).
- [ ] Add plan 0146 row to `plans/README.md`.
- [ ] Mark GAR-643 In Progress in Linear.
- [ ] Update ROADMAP §1.5 when plan merges.

---

## Risk register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Session JSON schema mismatch with future producer | Low | Low | Schema is normative; producer adapts to it. |
| FNV-1a hash collisions on small candidate sets | Very low | Low | 64-bit hash → negligible collision probability in practice. |
| `serde_yaml` serialization format changes | Very low | Low | Integration test verifies the output can be reparsed. |

---

## Acceptance criteria

- [ ] `cargo test -p garraia-learning` green (≥17 safety tests + ≥8 new miner tests).
- [ ] Fixture test: 10 sessions, 3 with `gh pr merge <N> --squash --delete-branch` +
      `git push origin --delete <BRANCH>` → emits exactly 1 candidate in `candidates_dir`.
- [ ] Threshold test: with threshold=4, the above fixture emits 0 candidates.
- [ ] PII test: session with `user@example.com` in stdout → candidate body contains
      `<EMAIL>` not the raw email.
- [ ] Idempotency: running `mine()` twice on the same sessions produces the same files
      (no duplicates, same content).
- [ ] Malformed JSON session: skipped with warn, other sessions processed normally.
- [ ] `cargo clippy -p garraia-learning -- -D warnings` green.

---

## Cross-references

- **GAR-641** — Learning Agent epic (parent)
- **GAR-642** — Architecture scaffold (plan 0144, merged PR #393) — provides types + `safety.rs`
- **GAR-644** — Skill Generator (next sub-issue) — consumes candidate files written by this plan
- **Plan 0144** — scaffold reference for crate structure + type conventions
- **CLAUDE.md §Convenções de código** — no `unwrap()`, no SQL concat, no PII in logs

---

## Estimativa

Low: 1 day | Provável: 2 days | Alta: 3 days
