# Plan 0148 — GAR-645: Skill Registry — dual-scope persist, list, promote, deprecate

**Status:** 🚧 In Progress (2026-05-18)
**Issue:** [GAR-645](https://linear.app/chatgpt25/issue/GAR-645) (sub-issue 4/10 of [GAR-641](https://linear.app/chatgpt25/issue/GAR-641))
**Branch:** `feat/202605181446-gar-645-skill-registry`
**Epic parent:** Fase 1.4 — Garra Learning Agent (`epic:learning-agent`)

---

## Goal

Implement `garraia-learning::registry` — the Skill Registry component (4/10 of the
Learning Agent epic). Replaces the 4-stub-function stub with a full dual-scope skill
store: persist, list, get, promote, deprecate, and list candidates.

Connects the Mine → Generate → **Registry** → Promote pipeline.

---

## Architecture

```text
crates/garraia-learning/src/registry.rs   (replace stub)
  RegistryOptions struct                  global_dir + project_dir
  LockGuard struct                        RAII lock-file in _locks/
  pub fn list_skills(opts, scope)         scan *.md in scope dir(s), skip _* prefixed
  pub fn get_skill(opts, name, scope)     find by name in scope dir(s)
  pub fn promote(skill, opts)             write to scope dir with lock-file guard
  pub fn deprecate(opts, name, scope)     set deprecated=true in frontmatter, rewrite
  pub fn list_candidates(dir)             find mined-*.md, parse via generator::load_candidate_file
  fn read_skill_file(path)               parse YAML frontmatter + body
  fn write_skill_file(skill, path)       serialize frontmatter + body
  fn skill_path(dir, name)               dir/<name>.md
  fn list_skills_in_dir(dir)             scan non-hidden *.md, deserialize each
  fn acquire_lock(locks_dir, name)       OpenOptions::create_new (atomic on Unix)
  fn scope_dir(opts, scope)              pick global_dir or project_dir

crates/garraia-learning/src/lib.rs
  LearningSkillFrontmatter               + deprecated: bool (#[serde(default)])
```

No Cargo.toml changes — `serde_yaml`, `tempfile` already present.

---

## Tech stack

- Rust edition 2024 (existing crate)
- `serde_yaml` (workspace) — frontmatter serialization
- `garraia_common::Error`/`Result` — error propagation
- `generator::load_candidate_file` — reused for list_candidates
- `std::fs::OpenOptions::create_new` — atomic lock acquisition

---

## Design invariants

- **No `unwrap()` in production paths.**
- **Lock-file in `_locks/`** ensures concurrent `promote` calls don't corrupt files.
- **`_*` prefixed files/dirs skipped** by `list_skills_in_dir` (reserved for `_locks/`, `_candidates/`, `_deprecated/`).
- **`deprecated` field** added to `LearningSkillFrontmatter` with `#[serde(default)]` — backwards-compatible.
- **`promote` is idempotent** on name: overwrites existing file after acquiring lock.
- **`list_candidates`** delegates to `generator::load_candidate_file`; non-parseable files are skipped with `tracing::warn!`.

---

## Validações pré-plano

- [x] `garraia-learning` crate exists with stub `registry.rs`.
- [x] `serde_yaml` + `tempfile` are available.
- [x] `garraia_common::Error::Io` is `#[from] std::io::Error`.
- [x] `generator::load_candidate_file` is `pub`.
- [x] `LearningSkillFrontmatter`, `SkillScope`, `Skill` are all `pub` in `lib.rs`.
- [x] `cargo check -p garraia-learning` is green on main.

---

## Out of scope

- CLI `garra skills *` wiring (GAR-650 Human Override slice).
- `delete_skill` (GAR-650).
- `lock`/`unlock` skill (GAR-650).
- Embedding-based retrieval (GAR-646 — prereq: Fase 2.1).
- Auto-updater PR flow (GAR-648).
- Git-backed versioning (GAR-650).
- Score update / EMA tracking (GAR-647 Evaluator).

---

## M1 Tasks

### T1 — Add `deprecated` to LearningSkillFrontmatter

- [ ] `pub deprecated: bool` with `#[serde(default)]` in `lib.rs`.
- [ ] `cargo check -p garraia-learning` green.

### T2 — Define RegistryOptions + LockGuard

- [ ] `pub struct RegistryOptions { global_dir, project_dir }`
- [ ] `impl RegistryOptions { pub fn new(…), pub fn default_with_cwd(cwd) }`
- [ ] `struct LockGuard(PathBuf)` + `Drop` impl.
- [ ] `fn acquire_lock(locks_dir, name) -> Result<LockGuard>`.

### T3 — Implement read/write helpers (RED → GREEN)

- [ ] `fn read_skill_file(path) -> Result<Skill>` — split on `---` delimiter.
- [ ] `fn write_skill_file(skill, path) -> Result<()>` — serialize frontmatter + body.
- [ ] Unit tests: round-trip write → read preserves all fields.

### T4 — Implement list_skills + get_skill (RED → GREEN)

- [ ] `fn list_skills_in_dir(dir) -> Result<Vec<Skill>>` — skip `_*`.
- [ ] `pub fn list_skills(opts, scope) -> Result<Vec<Skill>>`.
- [ ] `pub fn get_skill(opts, name, scope) -> Result<Option<Skill>>`.
- [ ] Tests: empty dir → `[]`; file present → returns skill; wrong scope → None.

### T5 — Implement promote + deprecate (RED → GREEN)

- [ ] `pub fn promote(skill, opts) -> Result<PathBuf>` — mkdir + lock + write.
- [ ] `pub fn deprecate(opts, name, scope) -> Result<bool>` — read → set flag → write.
- [ ] Tests: promote writes file; deprecate sets `deprecated=true`; unknown name returns `false`.

### T6 — Implement list_candidates

- [ ] `pub fn list_candidates(candidates_dir) -> Result<Vec<Candidate>>`.
- [ ] Test: write a `mined-*.md` fixture → list returns Candidate.

### T7 — clippy + workspace check

- [ ] `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean.

### T8 — Update plans/README.md

- [ ] Add row 0148.

---

## Acceptance criteria

- [ ] `cargo test -p garraia-learning -- registry` — all registry tests pass.
- [ ] `promote` writes `<name>.md` with valid YAML frontmatter in the correct scope dir.
- [ ] `list_skills` returns skills from both scopes when `scope = None`.
- [ ] `deprecate` sets `deprecated: true` and rewrites the file.
- [ ] `list_candidates` round-trips a mined candidate file into `Candidate`.
- [ ] `cargo clippy … -D warnings` clean.
- [ ] CI green.

---

## File structure

```
crates/garraia-learning/
└── src/
    ├── lib.rs          + deprecated field on LearningSkillFrontmatter
    └── registry.rs     full implementation (~320 LOC, replaces 28-line stub)
plans/
├── README.md           add row 0148
└── 0148-gar-645-skill-registry.md   this file
```

---

## Cross-references

- Plan 0147 (GAR-644, Skill Generator) — `generate()` output is the `Skill` promoted here.
- Plan 0146 (GAR-643, Skill Miner) — `load_candidate_file` reused by `list_candidates`.
- Plan 0144 (GAR-642, Learning Agent scaffold) — `Skill` + `LearningSkillFrontmatter` types.
- GAR-641 (epic parent) — 4/10 of the Learning Agent.
- ADR 0010 — Accepted learning architecture.

---

## Estimativa

0.5 / 1 / 2 days. ~320 LOC production, ~150 LOC tests.
