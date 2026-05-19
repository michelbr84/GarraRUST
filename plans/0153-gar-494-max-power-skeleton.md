# Plan 0153 — GAR-494: `garra max-power` skeleton CLI subcommand

**Issue:** GAR-494 (GarraMaxPower epic GAR-492)
**Branch:** `routine/202605190910-gar-494-max-power`
**Date:** 2026-05-19
**Scope:** `crates/garraia-cli` only — no gateway, no DB, no auth changes.

## Goal

Add `garra max-power` to `garraia-cli` as a skeleton that:
- Prints a banner + numbered pipeline menu when invoked without `--goal`
- Routes a `--goal` text to the correct GarraMaxPower workflow by keyword matching
- Emits `route: <workflow>` + a rationale line
- `--help` exits 0 with clap-generated usage

The full state machine (brainstorm → spec → plan → execute loop) is NOT implemented here — that is GAR-495..GAR-501.

## Architecture

New file: `crates/garraia-cli/src/max_power.rs`

Sync-only (no async needed for skeleton). Function `pub fn run(goal: Option<String>, mode: String)` called directly from `async_main` dispatch without `.await`.

### Keyword routing table

| Keywords | Route |
|---|---|
| bug, fix, crash, error, broken, panic, regression | `systematic-debugging` |
| feature, add, implement, build, create, new | `brainstorm` |
| refactor, clean, extract, rename, simplify, restructure | `refactor-module` |
| test, coverage, spec, unit, integration | `tdd-loop` |
| docs, document, readme, explain, describe | `generate-docs` |
| review, audit, check, inspect, analyse, analyze | `code-review` |
| (no match) | `brainstorm` (default) |

### Mode flag

`--mode new|existing|auto` (default `auto`). The skeleton prints the mode and notes that state machine resume is not yet implemented.

## File structure

```
crates/garraia-cli/src/
  max_power.rs     ← new
  main.rs          ← add mod + Commands::MaxPower + dispatch arm
```

## Tasks

- [x] T1: Create `max_power.rs` with `run()`, banner, menu, routing, tests
- [x] T2: Edit `main.rs` — `mod max_power`, `Commands::MaxPower`, dispatch arm
- [x] T3: `cargo check -p garraia-cli` + `cargo test -p garraia-cli`
- [x] T4: Commit + push + PR

## Acceptance criteria

- `garra max-power --help` → exit 0, prints usage
- `garra max-power` (no args) → prints banner + numbered pipeline menu
- `garra max-power --goal "fix bug X"` → prints `route: systematic-debugging` + rationale
- `garra max-power --goal "add feature Y"` → prints `route: brainstorm`
- `garra max-power --goal "refactor module Z"` → prints `route: refactor-module`
- All unit tests in `max_power.rs` pass

## Out of scope

- Actual state machine / pipeline execution (GAR-495+)
- Postgres / auth / gateway changes
- MCP wiring for max-power

## Risk register

| Risk | Mitigation |
|---|---|
| clap value_parser for --mode rejects unexpected values | Use `value_parser = ["new", "existing", "auto"]` — same pattern as Glob |

## Cross-references

- GAR-492: GarraMaxPower epic
- GAR-494: This issue
- CLAUDE.md: no unwrap in production, conventional commits
