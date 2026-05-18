# Plan 0147 — GAR-644: Skill Generator — LLM-assisted skill drafting

**Status:** 🚧 In Progress (2026-05-18)
**Issue:** [GAR-644](https://linear.app/chatgpt25/issue/GAR-644) (sub-issue 3/10 of [GAR-641](https://linear.app/chatgpt25/issue/GAR-641))
**Branch:** `routine/202605181219-gar-644-skill-generator`
**Epic parent:** Fase 1.4 — Garra Learning Agent (`epic:learning-agent`)

---

## Goal

Implement `garraia-learning::generator` — the Skill Generator component (3/10 of the
Learning Agent epic). Takes a `Candidate` produced by the Skill Miner and generates a
polished Markdown skill document via an LLM provider (provider-agnostic via trait).

Unblocks the Mine → **Generate** → Validate → Promote pipeline.

---

## Architecture

```text
crates/garraia-learning/src/generator.rs   (replace stub)
  SkillDraftProvider trait                  sync, Send+Sync, mockable
  Candidate struct                          in-memory representation of mined candidate
  GenerateOptions struct                    existing_skill_names for dedup
  pub fn generate(candidate, provider, opts) main entry point → Skill
  pub fn load_candidate_file(path)          parse mined-*.md file into Candidate
  fn build_prompt(candidate)               fills SKILL_DRAFT_PROMPT template
  fn parse_llm_response(raw, slug)          extracts frontmatter + body from LLM output
  fn to_kebab(s)                           any string → kebab-case
  fn make_unique_name(base, existing)      append -v2 / -v3 on collision
  fn apply_pii_redaction(skill) -> Skill   delegates to miner::redact()

crates/garraia-learning/src/generator.rs   SKILL_DRAFT_PROMPT const (embedded template)
```

No Cargo.toml changes needed — `serde_yaml` is already a workspace dependency.

---

## Tech stack

- Rust edition 2024 (existing crate)
- `serde_yaml` (workspace) — parse candidate frontmatter + serialize generated frontmatter
- `garraia-common::Error`/`Result` — error propagation
- `miner::redact()` — PII redaction reused from Skill Miner

---

## Design invariants

- **No `unwrap()` in production paths.** All I/O and parse errors propagate via `?`.
- **Provider-agnostic**: `dyn SkillDraftProvider` with no dependency on `garraia-agents`.
  The sync trait can be bridged to async at the call site via `tokio::task::block_in_place`
  when wired into the gateway.
- **PII redaction applied automatically** to the generated body via `miner::redact()`.
- **Name deduplication**: if `name` collides with `existing_skill_names`, append `-v2`, `-v3`, …
- **Kebab-case names only**: non-conforming LLM output is normalized via `to_kebab()`.
- **Graceful parse fallback**: if the LLM returns a body without valid YAML frontmatter,
  construct a minimal `Skill` using the candidate slug as name rather than returning `Err`.
- **Prompt is a `const` string** embedded in `generator.rs` — no runtime file-read.

---

## Validações pré-plano

- [x] `garraia-learning` crate exists with stub `generator.rs`.
- [x] `serde_yaml` is a workspace dependency (used in `miner.rs`).
- [x] `garraia-common::Error::Other(String)` variant available.
- [x] `miner::redact()` is `pub` and takes `&str` → `String` (confirmed in miner.rs:166).
- [x] `LearningSkillFrontmatter`, `SkillSource`, `SkillScope` are all `pub` in `lib.rs`.
- [x] GAR-644 in Backlog (no duplicate generator implementation in main).
- [x] `cargo check -p garraia-learning` is green on current main.

---

## Out of scope

- CLI `garra skills generate` wiring in `garraia-cli` (deferred to CLI slice).
- Real LLM integration tests against OpenRouter/OpenAI/Ollama (feature-gated stub is sufficient).
- Skill promotion to active registry (GAR-645, GAR-649).
- Embedding-based retrieval (GAR-646 — prereq: Fase 2.1).
- Auto-updater for existing skills (GAR-648).

---

## Rollback

Only `crates/garraia-learning/src/generator.rs` changes. Reverting the PR restores the
one-liner stub. No database schema changes. No new crates.

---

## §12 Open questions

| # | Question | Resolution |
|---|---|---|
| 1 | Should the LLM provider trait be async? | No for this slice. Sync trait is simpler to test and can be bridged async at call site. |
| 2 | What if LLM returns invalid YAML frontmatter? | Fallback: construct minimal Skill with slug as name + raw body. Log a `tracing::warn!`. |
| 3 | Should PII in generated text return `Err` or be silently redacted? | Silently redact (apply `miner::redact()`); return the cleaned Skill. |
| 4 | Do we need a `prompts/` directory? | No — prompt is a `const` embedded in `generator.rs`. Avoids runtime file-read. |
| 5 | Integration tests against real LLMs? | Feature-gated under `#[cfg(feature = "integration-tests")]`; not enabled in CI. |

---

## File structure

```
crates/garraia-learning/
└── src/
    └── generator.rs          full implementation (~260 LOC, replaces 7-line stub)
plans/
├── README.md                 add row for 0147
└── 0147-gar-644-skill-generator.md   this file
```

---

## M1 Tasks

### T1 — Define SkillDraftProvider trait + Candidate + GenerateOptions

- [ ] `pub trait SkillDraftProvider: Send + Sync { fn draft(&self, prompt: &str) -> Result<String>; }`
- [ ] `pub struct Candidate { slug, normalized_commands, occurrence_count }`
- [ ] `pub struct GenerateOptions { existing_skill_names: Vec<String> }`
- [ ] `cargo check -p garraia-learning` green.

### T2 — Implement build_prompt + SKILL_DRAFT_PROMPT const

- [ ] Write prompt template as `const SKILL_DRAFT_PROMPT: &str`.
- [ ] `fn build_prompt(candidate: &Candidate) -> String` fills template.
- [ ] Unit test: prompt contains slug + occurrence_count + command list.

### T3 — Implement parse_llm_response (RED → GREEN)

- [ ] Write failing test: response with valid frontmatter → `Skill` with correct name/body.
- [ ] Write failing test: response without `---` delimiters → fallback to slug-named Skill.
- [ ] Implement `parse_llm_response(raw: &str, fallback_slug: &str) -> Result<Skill>`.
- [ ] `cargo test -p garraia-learning generator` green.

### T4 — Implement to_kebab + make_unique_name (RED → GREEN)

- [ ] Write failing test: `to_kebab("Cleanup Merged Branches!")` → `"cleanup-merged-branches"`.
- [ ] Write failing test: `make_unique_name("foo", &["foo", "foo-v2"])` → `"foo-v3"`.
- [ ] Implement both helpers.
- [ ] `cargo test -p garraia-learning generator` green.

### T5 — Implement generate + apply_pii_redaction (RED → GREEN)

- [ ] Write failing test with `MockDraftProvider` returning valid skill YAML + body.
- [ ] Write failing test: PII in body → redacted automatically.
- [ ] Write failing test: name collision → `-v2` suffix.
- [ ] Implement `generate()` + `apply_pii_redaction()`.
- [ ] `cargo test -p garraia-learning generator` green.

### T6 — Implement load_candidate_file

- [ ] Write failing test: write a `mined-*.md` fixture file → parse into `Candidate`.
- [ ] Implement `pub fn load_candidate_file(path: &Path) -> Result<Candidate>`.
- [ ] `cargo test -p garraia-learning generator` green.

### T7 — clippy + workspace check

- [ ] `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean.

### T8 — Update plans/README.md + ROADMAP §7

- [ ] Add row 0147 to `plans/README.md`.
- [ ] Update ROADMAP §7: mark GAR-643 item with note that GAR-644 is next.

---

## Risk register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|-----------|
| LLM output format varies widely | High | Medium | Robust fallback in `parse_llm_response` |
| `serde_yaml` parses YAML 1.1 (booleans "yes"/"no") | Low | Low | Use `serde_yaml::from_str` which follows YAML 1.2 |
| Clippy warns on `dyn SkillDraftProvider` pattern | Low | Low | Use `+ ?Sized` if needed |

---

## Acceptance criteria

- [ ] `cargo test -p garraia-learning -- --test-output immediate 2>&1 | grep -E "test .*(generator|generate|kebab|unique|pii|prompt|parse)" | grep ok` — all generator tests pass.
- [ ] `generate()` with `MockDraftProvider` returning valid YAML → `Skill` with correct fields.
- [ ] `generate()` with body containing email → body is PII-redacted.
- [ ] `generate()` with `existing_skill_names: vec!["foo".into()]` + LLM producing `name: foo` → result is `name: foo-v2`.
- [ ] `load_candidate_file()` round-trips a miner-written candidate file into `Candidate`.
- [ ] `cargo clippy --workspace … -D warnings` clean.
- [ ] CI (Format + Clippy + Test) green.

---

## Cross-references

- Plan 0146 (GAR-643, Skill Miner) — previous slice; `miner::redact()` is reused here.
- Plan 0144 (GAR-642, Learning Agent scaffold) — `Skill` + `LearningSkillFrontmatter` types.
- Plan 0145 (GAR-372, embeddings scaffold) — Skill Retriever (GAR-646) will consume.
- GAR-641 (épico parent) — 3/10 of the Learning Agent.
- ADR 0010 — Accepted learning architecture.

---

## Estimativa

0.5 / 1 / 2 days. ~260 LOC production, ~130 LOC tests.
