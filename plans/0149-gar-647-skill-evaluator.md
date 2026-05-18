# Plan 0149 — GAR-647: Skill Evaluator — objective metrics + EMA score update

**Status:** 🚧 In Progress (2026-05-18)
**Issue:** [GAR-647](https://linear.app/chatgpt25/issue/GAR-647) (sub-issue 5/10 of [GAR-641](https://linear.app/chatgpt25/issue/GAR-641))
**Branch:** `feat/202605181616-gar-647-skill-evaluator`
**Epic parent:** Fase 1.4 — Garra Learning Agent (`epic:learning-agent`)

---

## Goal

Implement `garraia-learning::evaluator` — the Skill Evaluator component (5/10 of the
Learning Agent epic). Replaces the 2-line stub with a full evaluation pipeline.

Takes objective signals from skill execution (exit code, test counts, diff stats,
log scan) and updates the skill's EMA score. Triggers deprecation when the score
drops below threshold or when anti-flap fires.

---

## Architecture

```text
crates/garraia-learning/src/evaluator.rs   (replace stub)
  EvalSignals struct                       exit_code, tests_passed/failed,
                                           lines/files_changed, log_has_errors,
                                           log_has_panics, latency_ms
  EvalOutcome enum                         Pass | Fail { reason } | Deprecated { reason }
  EvalResult struct                        new_score, fail_count, deprecated, outcome
  EmaConfig struct                         alpha (0.3), deprecate_threshold (0.3),
                                           anti_flap_threshold (3)
  pub fn evaluate(skill, signals, config)  main entry point — mutates skill, returns EvalResult
  fn compute_raw_score(signals)            0.0..=1.0 weighted rubric
  fn update_ema(current, raw, alpha)       α·raw + (1-α)·current
  fn is_failure(signals)                   exit != 0 || panics || tests_failed > 0
```

Scoring rubric (weights sum to 1.0):
- exit_code == 0: +0.40
- tests_failed == 0 && tests_passed > 0: +0.30 (else +0.10 if no tests)
- !log_has_errors && !log_has_panics: +0.20
- files_changed > 0: +0.10

---

## Tech stack

- Rust edition 2024 (existing crate)
- `garraia_common::Error`/`Result` — error propagation
- `Skill::ANTI_FLAP_THRESHOLD` — reused constant (3)
- `Skill::MIN_PROMOTE_SCORE` — reused constant (0.5)

No Cargo.toml changes.

---

## Design invariants

- **No `unwrap()` in production paths.**
- **`fail_count` resets to 0 on success** (anti-flap: only consecutive failures count).
- **`deprecated` flag set but file not deleted** — preserves history; Registry handles persistence.
- **Skill mutation is atomic within the call** — both `score` and `fail_count` update together.
- **EMA alpha = 0.3 default** — recent signals have 30% weight; avoids over-reacting to noise.
- **Deprecation threshold = 0.3** — skills below 30% quality are retired.

---

## Validações pré-plano

- [x] `garraia-learning` crate exists with stub `evaluator.rs`.
- [x] `LearningSkillFrontmatter.score`, `.fail_count`, `.deprecated` all `pub`.
- [x] `Skill::ANTI_FLAP_THRESHOLD = 3` and `Skill::MIN_PROMOTE_SCORE = 0.5` in `lib.rs`.
- [x] No external crate deps needed.
- [x] `cargo check -p garraia-learning` green.

---

## Out of scope

- Actual CI check polling via `gh` API (deferred to Auto-Updater GAR-648).
- Writing evaluation results to disk / registry (caller's responsibility).
- Score history file `.garra/skills/_history/<name>.json` (GAR-650 Versioning).
- Web UI display (GAR-651).

---

## M1 Tasks

### T1 — Define EvalSignals + EvalOutcome + EvalResult + EmaConfig

- [ ] All structs/enums defined and `pub`.
- [ ] `EmaConfig::default()` with alpha=0.3, deprecate_threshold=0.3, anti_flap=3.
- [ ] `cargo check -p garraia-learning` green.

### T2 — Implement compute_raw_score + update_ema (RED → GREEN)

- [ ] Tests: exit_code=0, all tests pass, no errors → score ~1.0.
- [ ] Tests: exit_code=1 → score ~0.0.
- [ ] Implement helpers.

### T3 — Implement evaluate (RED → GREEN)

- [ ] Test: success → score increases, fail_count=0.
- [ ] Test: failure → score decreases, fail_count increments.
- [ ] Test: 3 consecutive failures → deprecated=true.
- [ ] Test: score < 0.3 after EMA → deprecated=true.
- [ ] Test: success after failure resets fail_count.
- [ ] Implement `pub fn evaluate`.
- [ ] `cargo test -p garraia-learning evaluator` green.

### T4 — clippy + workspace check

- [ ] `cargo clippy --workspace … -D warnings` clean.

### T5 — Update plans/README.md

- [ ] Add row 0149.

---

## Acceptance criteria

- [ ] `cargo test -p garraia-learning -- evaluator` — all evaluator tests pass.
- [ ] Success signals → score EMA-updated upward, fail_count reset to 0.
- [ ] 3 consecutive failures → `EvalResult.deprecated = true`, skill.frontmatter.deprecated = true.
- [ ] Score below 0.3 → deprecated.
- [ ] `cargo clippy … -D warnings` clean.
- [ ] CI green.

---

## Cross-references

- Plan 0148 (GAR-645, Skill Registry) — `promote`/`deprecate` consume EvalResult.
- Plan 0144 (GAR-642, scaffold) — `Skill` types + constants.
- GAR-641 (epic) — 5/10 of the Learning Agent.
- ADR 0010 — Accepted learning architecture.

---

## Estimativa

0.5 / 1 / 1.5 days. ~180 LOC production, ~120 LOC tests.
