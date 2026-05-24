# ADR 0011 — GarraMaxPower: Native Agent-Advanced Mode

**Status:** Accepted  
**Date:** 2026-05-19 (America/New_York — first sub-issue merged)  
**Updated:** 2026-05-24 (all sub-issues merged; ADR promoted to Accepted)  
**Epic:** [GAR-492](https://linear.app/chatgpt25/issue/GAR-492) — GarraMaxPower  
**Plan:** [0176](../../plans/0176-gar-493-garramaxpower-adr.md)

---

## Context and Problem Statement

GarraIA is a Rust-native, privacy-first AI gateway that powers multiple channels
(Telegram, Discord, mobile, CLI, web). The upstream
[ClaudeMaxPower / Superpowers](https://github.com/obra/superpowers) plugin delivers
advanced agent behaviours (capability snapshots, brainstorm→spec→plan→execute→review
pipeline, skill registry, agent teams, safety gates, handoff / "Auto Dream") for Claude
Code sessions.

**The question**: How should GarraIA expose its own agentic advanced mode?

Two tensions drove the decision:

1. **Portability**: GarraIA must work without the Claude Code CLI, with any LLM
   provider (OpenAI, Anthropic, OpenRouter, Ollama).
2. **Directness**: Copying `.claude/` artefacts into the runtime couples our release
   cycle to an external plugin's schema and update cadence.

---

## Decision Drivers

| Weight | Driver |
|--------|--------|
| ★★★ | No runtime dependency on Claude Code or the Superpowers plugin |
| ★★★ | Executable from `garra` binary on any platform (Linux / macOS / Windows) |
| ★★★ | Safety gates prevent destructive commands from any skill or phase |
| ★★ | Sub-agents communicate via typed Rust channels, not shell pipes |
| ★★ | Handoff / Auto Dream produces a machine-readable TOML state file |
| ★ | Feature parity with ClaudeMaxPower is _not_ a goal — MVP only |

---

## Considered Options

### Option A — Copy `.claude/` artefacts into the Garra runtime

Import `superpowers-config.md`, skill markdown files, and the Superpowers plugin
configuration verbatim and have `garraia-cli` execute them.

**Pros:** Immediate compatibility with skill definitions written for Claude Code.  
**Cons:**
- Runtime dependency on the Claude Code CLI and Superpowers plugin binaries.
- Markdown skill definitions are not type-safe; skill execution is shell round-trips.
- ADR, ROADMAP, and skills are owned by an external repo with its own versioning cadence.
- No reuse of existing `garraia-agents`, `garraia-common`, or `garraia-security` crates.

### Option B — Do nothing; rely on the harness

Keep using the Superpowers plugin exclusively for agent workflows and do not add a
native mode to the `garra` binary.

**Pros:** Zero implementation cost in this phase.  
**Cons:**
- GarraIA users without the Claude Code harness (e.g., server deployments, custom
  integrations) get no agentic advanced mode.
- No dogfooding of `garraia-agents` / `AgentRuntime`.
- Blocks future use-cases like auto-triggered pipelines from channels or API.

### Option C — Native Rust primitives inside the `garra` binary _(chosen)_

Implement GarraMaxPower as a set of Rust crates + a `garra max-power` CLI entry point
that is fully self-contained, provider-agnostic, and safety-gated.

The **six-phase pipeline** (Brainstorm → Spec → Plan → Execute → Review → Finish) is
encoded as typed Rust enums and driven by an `AgentTeam` (Orchestrator + Executor +
Reviewer communicating via `std::sync::mpsc`).

**Pros:**
- Zero external runtime dependencies — compiles and runs on any target that `garraia-cli` supports.
- Skills are first-class Rust structs (`NativeSkillRegistry`), eliminating the markdown-to-shell gap.
- `safety_gate(cmd)` is enforced as a synchronous Rust function in `garraia-common`, not a shell hook.
- `HandoffState` / `.garra-estado.md` TOML is version-controlled alongside the project.
- Reuses `AgentRuntime` and `garraia-agents` providers already in the workspace.

**Cons:**
- Feature surface is smaller than the Superpowers plugin in the short term.
- Skills must be ported manually from markdown to Rust (ongoing).

---

## Decision Outcome

**Chosen option:** C — Native Rust primitives inside the `garra` binary.

The implementation was delivered across eight sub-issues, all merged to `main` by
2026-05-24 (Florida):

| Sub-issue | Deliverable | PR | SHA |
|-----------|-------------|-----|-----|
| [GAR-494](https://linear.app/chatgpt25/issue/GAR-494) | `garra max-power` skeleton + banner | #431 | `8a9a915` |
| [GAR-497](https://linear.app/chatgpt25/issue/GAR-497) | `safety_gate(cmd)` denylist in `garraia-common` | #437 | `f2ab1d9` |
| [GAR-501](https://linear.app/chatgpt25/issue/GAR-501) | `garra verify` pipeline (fmt/clippy/test/flutter/gitleaks) | #441 | `ca9f1fa2` |
| [GAR-500](https://linear.app/chatgpt25/issue/GAR-500) | `HandoffState` + Auto Dream / `.garra-estado.md` | #445 | `f1fb596` |
| [GAR-495](https://linear.app/chatgpt25/issue/GAR-495) | `build_snapshot` capability prompt | #453 | `e5a2a08` |
| [GAR-496](https://linear.app/chatgpt25/issue/GAR-496) | `GitRunner` / `RepoWorkflow` safe branch ops | #455 | `1b7f04c` |
| [GAR-498](https://linear.app/chatgpt25/issue/GAR-498) | `NativeSkillRegistry` (brainstorm, write-spec, write-plan, pre-commit, verify) | direct | `c65e099` |
| [GAR-499](https://linear.app/chatgpt25/issue/GAR-499) | `AgentTeam` (Orchestrator + Executor + Reviewer via mpsc) | #490 | _(pending)_ |

---

## Consequences

### Positive

- `garra max-power --goal "fix X"` runs end-to-end on any machine with a compiled `garraia-cli`
  binary and a valid `ANTHROPIC_API_KEY` (or equivalent).
- `safety_gate` is compile-time enforced: no shell skill can bypass it by running outside the
  Rust process.
- `NativeSkillRegistry` and `AgentTeam` are unit-tested (26 tests across the two modules).
- `HandoffState` TOML persists session context across invocations, enabling resumable runs.

### Negative

- New skills require Rust code; markdown-only authors cannot contribute skills without a PR.
- The six-phase pipeline is synchronous in this MVP; async phases (long LLM calls) require
  a future upgrade to `tokio::sync::mpsc`.

### Neutral

- The Superpowers plugin remains the harness for Claude Code sessions; GarraMaxPower is
  an additive layer that does not replace it.
- Feature parity with ClaudeMaxPower is intentionally deferred — ADR records the current
  MVP scope, not a long-term parity target.

---

## Invariants (Do not violate without a new ADR)

1. `safety_gate(cmd)` in `garraia-common` is called **before** any shell execution by any
   skill or pipeline phase.
2. `AgentTeam::run()` is **infallible** at the API boundary; phase failures are recorded
   in `PhaseResult.decision`, not propagated as panics or `Err`.
3. No `garra max-power` code imports `std::env::var("GARRAIA_JWT_SECRET")` or any secret
   directly — secrets follow `garraia-config::auth` only.
4. `.garra-estado.md` TOML is **always** written via `HandoffState::write_toml()` and
   **never** contains raw secrets (enforced by `RedactedString`).

---

## Links

- ROADMAP §1.2.1: `ROADMAP.md` (search `GarraMaxPower`)
- Epic: [GAR-492](https://linear.app/chatgpt25/issue/GAR-492)
- Plan: [plans/0176-gar-493-garramaxpower-adr.md](../../plans/0176-gar-493-garramaxpower-adr.md)
- Related ADRs: [0010 — Garra Learning Agent](0010-garra-learning-agent.md)
- ClaudeMaxPower / Superpowers reference: <https://github.com/obra/superpowers>
