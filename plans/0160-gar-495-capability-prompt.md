# Plan 0160 — GAR-495: Capability Prompt Nativo

**Linear issue:** [GAR-495](https://linear.app/chatgpt25/issue/GAR-495) — "Capability prompt nativo" (Backlog → In Progress). Labels: `epic:maxpower`. Project: epic [GAR-492](https://linear.app/chatgpt25/issue/GAR-492).

**Status:** ⏳ In Progress — 2026-05-21 (Florida).

## Goal

Build a provider-agnostic runtime capability prompt generator for `garra max-power`.
Assembles a human-readable description of available LLM providers, built-in tools,
configured channels, and active MCP servers from the loaded `AppConfig`.

Printed as a banner when `garra max-power` is invoked, giving any LLM operator
(human or AI) immediate situational awareness of what the runtime exposes.

## Architecture

1. **New module `crates/garraia-cli/src/capability_prompt.rs`** (~230 LOC):
   - `ProviderInfo { name, provider_type, model }` — derived from `AppConfig.llm`
   - `CapabilitySnapshot { providers, builtin_tools, channels, mcp_servers }` — all `Vec<String>` / `Vec<ProviderInfo>`
   - `BUILTIN_TOOLS: &[(&str, &str)]` — static slice of (name, description) pairs matching the tools in `garraia-agents::tools`
   - `build_snapshot(config: &AppConfig) -> CapabilitySnapshot` — pure function, no I/O
   - `render_prompt(snap: &CapabilitySnapshot) -> String` — formats into multi-section text
   - Unit tests covering ≥ 3 provider configurations (acceptance criterion from ROADMAP §1.2.1)

2. **Modify `crates/garraia-cli/src/max_power.rs`**:
   - `run(goal, mode, config)` — add `config: &garraia_config::AppConfig` param
   - Print capability summary (provider count + tool count) before routing

3. **Modify `crates/garraia-cli/src/main.rs`**:
   - Pass `&config` to `max_power::run(goal, mode, &config)`

## Tech stack

`garraia-config::AppConfig` (already a dep of garraia-cli). No new deps. No DB. No
network calls. Pure data transformation.

## Design invariants

1. **No I/O in `build_snapshot`** — accepts `&AppConfig`, returns snapshot synchronously.
   Tests don't need async or tempfiles.
2. **No PII** — provider names, model names, channel names are config keys (not user data).
3. **Fail-soft** — empty maps produce empty sections; prompt always renders without panic.
4. **Provider-agnostic** — the prompt is plain text readable by any LLM regardless of which
   provider is active. No provider-specific formatting.
5. **Static tool list** — built-in tool names are hardcoded constants mirroring the tool
   modules in `garraia-agents/src/tools/`. Dynamic tool discovery (via AgentRuntime) is
   deferred to GAR-499 (Agent team MVP).

## Out of scope

- Dynamic tool discovery from a running gateway (GAR-499)
- MCP tool schemas (GAR-499)
- Persisting the prompt to disk
- Colourised output (follow-up UX task)

## Rollback

Pure additive change. If reverted, `max_power::run` loses the capability summary line;
routing and handoff are unaffected.

## File structure

```
crates/garraia-cli/src/
  capability_prompt.rs    ← new
  max_power.rs            ← modified (run signature + summary print)
  main.rs                 ← modified (pass &config to max_power::run)
plans/
  0160-gar-495-capability-prompt.md  ← this file
plans/README.md           ← add row 0160
```

## Tasks

- [x] T1 — Write `capability_prompt.rs`: types + `build_snapshot` + `render_prompt` + unit tests
- [x] T2 — Update `max_power.rs`: accept `&AppConfig`, print capability summary
- [x] T3 — Update `main.rs`: pass `&config`
- [x] T4 — `cargo check -p garraia` + `cargo clippy -p garraia` green
- [x] T5 — `cargo test -p garraia` green
- [x] T6 — Commit + push
- [x] T7 — PR opened + CI green
- [x] T8 — Squash merge + bookkeeping (ROADMAP §7 + plans/README)

## Acceptance criteria

- `garra max-power` (no args) prints capability summary with provider/tool/channel counts.
- `garra max-power --goal "fix bug X"` still routes to `systematic-debugging`.
- Unit tests cover: (a) Anthropic-only config, (b) OpenAI + Ollama config, (c) empty config.
- `cargo check --workspace` and `cargo clippy --workspace -- -D warnings` remain green.

## Risk register

| Risk | Mitigation |
|------|-----------|
| main.rs refactor breaks other Commands | Minimal change — only MaxPower arm passes config |
| Static tool list goes stale | Tracked via comment referencing `garraia-agents/src/tools/` |

## Estimativa

< 1 hour. ~280 LOC total (new + modified).

## Cross-references

- plan 0153 (GAR-494 max-power skeleton — predecessor)
- plan 0154 (GAR-497 bash safety gate — prerequisite done)
- ROADMAP §1.2.1 §7 item 5
- Epic GAR-492
