# Plan 0144 — GAR-642: Learning Agent Architecture — Scaffold `garraia-learning` + ADR 0010 Accepted

**Status:** ✅ Done (merged 2026-05-18, PR #393)
**Issue:** [GAR-642](https://linear.app/chatgpt25/issue/GAR-642) (sub-issue 1/10 of [GAR-641](https://linear.app/chatgpt25/issue/GAR-641))
**Branch:** `routine/202605180030-gar-642-learning-scaffold`
**Epic parent:** Fase 1.4 — Garra Learning Agent (`epic:learning-agent`)

---

## Goal

Deliver the **architecture slice** of the Garra Learning Agent epic:

1. Promote ADR 0010 (`docs/adr/0010-garra-learning-agent.md`) from **Proposed → Accepted**.
2. Create crate `crates/garraia-learning/` with stub modules + **working Safety Gate** (`safety.rs`).
3. `cargo check -p garraia-learning` green.
4. Unit tests for Safety Gate covering all `SafetyDenial` variants.
5. Add `garraia-learning` to workspace.

Unblocks issues GAR-643..GAR-651 (all other sub-issues of the Learning Agent epic).

---

## Architecture

```text
crates/garraia-learning/
├── Cargo.toml
└── src/
    ├── lib.rs           — public facade + shared types (Skill, LearningSkillFrontmatter, SkillSource, SkillScope)
    ├── miner.rs         — STUB: session log pattern miner
    ├── generator.rs     — STUB: LLM-assisted skill drafter
    ├── registry.rs      — STUB: dual-scope skill store (~/.garra/ + .garra/)
    ├── retriever.rs     — STUB: embedding-based skill lookup (Fase 2.1 prereq)
    ├── evaluator.rs     — STUB: objective metric collector (exit/tests/CI/diff)
    ├── updater.rs       — STUB: PR branch proposer
    ├── versioning.rs    — STUB: git-tracked history wrapper
    ├── safety.rs        — WORKING: hard-wall denylist + score + anti-flap + PII
    └── skill_override.rs — STUB: CLI/UI approve/reject/lock/delete API
```

The `Skill` type is distinct from `garraia-skills::SkillDefinition` — it adds
Learning Agent-specific metadata (score, source, scope, fail_count, locked,
critical_paths_touched).

---

## Tech stack

- Rust edition 2024
- `thiserror = "2"` (workspace dep) for `SafetyDenial`
- `serde` + `serde_yaml` (workspace) for frontmatter
- `garraia-common` for `Error`/`Result` base types
- `garraia-skills` (workspace) for `SkillDefinition` interop

---

## Design invariants

- **Safety Gate is a hard wall**: `safety::gate()` is called before ANY skill promotion. No bypass flag, no `unsafe`.
- `override` is a Rust reserved keyword — module is named `skill_override.rs`, exposed as `pub mod skill_override`.
- All stub functions return `Err(Error::Other("not yet implemented".into()))` — no `todo!()` in production paths (test-only stubs use `unimplemented!()` is also ok but we prefer Err).
- No `unwrap()` in any production code path.
- PII check does NOT use external regex crate in this slice — uses simple character scanning to avoid adding a new workspace dep.

---

## Validações pré-plano

- [x] `garraia-skills` crate exists at `crates/garraia-skills/` with `SkillFrontmatter`, `SkillDefinition`, `SkillScanner`, `SkillInstaller`.
- [x] `thiserror = "2"` in workspace deps.
- [x] `serde_yaml = "0.9"` in workspace deps.
- [x] `garraia-common::Error` has `Skill(String)` and `Other(String)` variants.
- [x] ADR 0010 exists at `docs/adr/0010-garra-learning-agent.md` (Status: Proposed).
- [x] GAR-642 Linear issue In Progress.
- [x] No open PRs blocking this work.

---

## Out of scope

- Full implementation of Miner, Generator, Registry, Retriever, Evaluator, Updater, Versioning, SkillOverride — those are GAR-643..GAR-651.
- `garra skills` CLI subcommands — GAR-645 (Skill Registry) + CLI slice.
- Web UI tabs — GAR-651.
- Postgres/pgvector storage — skill storage is filesystem-only in v1.
- Integration with `garraia-agents::AgentRuntime` beyond exporting `Skill` + Safety Gate publicly.

---

## Rollback

`git revert` the merge commit. The scaffold adds only a new crate with no callers; removing it is zero-risk.

---

## §12 Open questions

1. **Embeddings API for Retriever** — `garraia-embeddings` (GAR-372, Fase 2.1) does not exist yet. Retriever stub returns empty `Vec<Skill>`. This is documented and expected.
2. **`SkillFrontmatter` extension** — Should `LearningSkillFrontmatter` extend `SkillFrontmatter` via composition or be separate? Decision: separate struct in `garraia-learning::lib`. Avoids coupling the parser crate to Learning-specific fields.
3. **Safety Gate shared with GarraMaxPower** — ADR 0010 says the denylist should be shared with `garraia-tools::safety_gate`. For this slice, the denylist is implemented in `garraia-learning::safety` and exported via `pub use`. GarraMaxPower (GAR-497) will import from here.

---

## File structure

```
crates/garraia-learning/
  Cargo.toml
  src/
    lib.rs
    miner.rs
    generator.rs
    registry.rs
    retriever.rs
    evaluator.rs
    updater.rs
    versioning.rs
    safety.rs
    skill_override.rs
```

Workspace: `Cargo.toml` → add `"crates/garraia-learning"` to `members`.
Docs: `docs/adr/0010-garra-learning-agent.md` → Status: Accepted.
Plans: `plans/README.md` → add row for 0144.

---

## M1 Tasks

- [x] T1: Create `plans/0144-gar-642-learning-agent-scaffold.md` (this file).
- [x] T2: Create `crates/garraia-learning/Cargo.toml`.
- [x] T3: Create `crates/garraia-learning/src/lib.rs` (Skill type + facade exports).
- [x] T4: Create `crates/garraia-learning/src/safety.rs` (working Safety Gate + 17 tests).
- [x] T5: Create stub modules (miner, generator, registry, retriever, evaluator, updater, versioning, skill_override).
- [x] T6: Add `crates/garraia-learning` to workspace `Cargo.toml`.
- [x] T7: `cargo check -p garraia-learning` green. `cargo test -p garraia-learning` 17/17 pass.
- [x] T8: Promote ADR 0010 Status → Accepted.
- [x] T9: Update `plans/README.md` with this plan row.
- [x] T10: Update `ROADMAP.md` §1.5 + §7 to reflect ADR 0010 Accepted + GAR-642 Done.
- [ ] T11: Commit, push, open PR, wait for CI green, merge.

---

## Risk register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Rust edition 2024 breaks stub syntax | Low | Medium | Use `edition.workspace = true`; check with `cargo check` before commit |
| `override` keyword collision | High | Low | Module named `skill_override.rs`, not `override.rs` |
| Safety Gate incomplete denylist | Medium | High | Table-driven tests; ADR lists minimum patterns |
| Retriever stub breaks callers | Low | Low | No callers yet; scaffold only |

---

## Acceptance criteria

- [x] `cargo check -p garraia-learning` exits 0.
- [ ] `cargo test -p garraia-learning` shows Safety Gate tests passing (at minimum: DangerousCommand, CriticalPath, ScoreTooLow).
- [ ] ADR 0010 status is "Accepted" in `docs/adr/0010-garra-learning-agent.md`.
- [ ] `crates/garraia-learning` is a workspace member.
- [ ] CI workflow (Format + Clippy + Test×3 + Build + MSRV) is green.

---

## Cross-references

- ADR 0010: `docs/adr/0010-garra-learning-agent.md`
- Epic plan: `plans/0138-gar-learning-agent-epic.md`
- Parent epic: [GAR-641](https://linear.app/chatgpt25/issue/GAR-641)
- This issue: [GAR-642](https://linear.app/chatgpt25/issue/GAR-642)
- Next issue: [GAR-643](https://linear.app/chatgpt25/issue/GAR-643) Skill Miner

---

## Estimativa

**LOC:** ~350 (safety.rs ~200 incl. tests, lib.rs ~80, stubs ~70)
**Time:** 2-3 hours implementation + CI wait
