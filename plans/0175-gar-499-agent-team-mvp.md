# Plan 0175 — GAR-499: Agent Team MVP

## Goal

Add the **Agent Team MVP** for GarraMaxPower: three cooperating agents
(`OrchestratorAgent`, `ExecutorAgent`, `ReviewerAgent`) that communicate via
typed `std::sync::mpsc` channels and drive the six-phase workflow
`Brainstorm → Spec → Plan → Execute → Review → Finish` using the
`NativeSkillRegistry` from GAR-498.

Wire the team into `garra max-power --goal <text>` so the pipeline actually
executes instead of printing a placeholder.

## Architecture

```
OrchestratorAgent
    │   sends PhaseTask via orch→exec channel
    ▼
ExecutorAgent  (runs NativeSkill from garraia-skills registry)
    │   sends ExecMsg::Completed via exec→rev channel
    ▼
ReviewerAgent  (validates SkillRunOutput: non-empty summary + next_steps)
    │   sends ReviewMsg back via rev→orch channel
    ▼
OrchestratorAgent  (records PhaseResult, continues or halts)
```

All three agents run synchronously in one thread in this MVP.  The channel
seams make a future async upgrade transparent.

## Tech Stack

- `crates/garraia-cli/src/team.rs` (new, ~250 LOC)
- `crates/garraia-cli/src/max_power.rs` (wire `AgentTeam::run`)
- `crates/garraia-cli/src/main.rs` (`mod team;`)
- No new crate dependencies — `garraia-skills` is already in `garraia-cli`'s
  `Cargo.toml`.

## Design Invariants

- **No network in MVP** — `NativeSkill::run()` is pure Rust, no LLM calls.
- **No `unwrap()` outside tests** — all channel ops use `.ok()` / `match`.
- **All commands pass `safety_gate`** — already enforced in `NativeSkillRegistry`.
- **`AgentTeam::run()` is infallible** — it returns `TeamSummary` regardless;
  individual phase failures are recorded in `PhaseResult.decision`.
- **`TeamPhase::Finish`** is appended only when all prior phases `Accepted`.

## Out of Scope

- Async execution or `tokio::sync::mpsc` (deferred to post-MVP).
- LLM-backed executor (GAR-492 Agent Team Phase 2).
- Retry loop on `NeedsRevision` (future slice).
- Web UI integration (covered by GAR-651 already shipped).

## Rollback

Remove `mod team;` from `main.rs` and revert `max_power.rs` to the
placeholder println — single commit, zero schema changes.

## Tasks

### T1 — Create `crates/garraia-cli/src/team.rs`

- [ ] Types: `TeamPhase` (Copy enum, 6 variants), `TeamRole` (Copy, 3 variants),
      `ReviewDecision` (Clone, 3 variants), `PhaseResult`, `TeamSummary`.
- [ ] Internal message structs: `PhaseTask`, `ExecMsg`, `ReviewMsg`.
- [ ] `WORKFLOW` const mapping `TeamPhase → skill name`.
- [ ] `ExecutorAgent<'a>` with `process(task_rx, reply_tx)`.
- [ ] `ReviewerAgent` with `review(output) -> ReviewDecision` (pure fn) and
      `process(fwd_rx, decision_tx)`.
- [ ] `AgentTeam` struct + `new()` + `Default` + `run(goal) -> TeamSummary`.
- [ ] `#[cfg(test)] mod tests` — ≥ 12 cases.

### T2 — Wire into `max_power.rs`

- [ ] Import `crate::team::AgentTeam`.
- [ ] In `route_goal()`: replace placeholder with `AgentTeam::new().run(goal)`.
- [ ] Add `print_team_summary(summary: &TeamSummary)` helper.

### T3 — Declare module in `main.rs`

- [ ] Add `mod team;` alongside existing `mod` declarations.

### T4 — Docs + tracking

- [ ] Update `plans/README.md` row 0172 → ✅ Merged, add 0175 row.
- [ ] Mark GAR-499 In Progress in Linear.

## Acceptance Criteria

- `cargo test -p garraia-cli` green including ≥ 12 new team tests.
- `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean.
- `garra max-power --goal "fix the login bug"` prints phase-by-phase output with Brainstorm through Finish.
- `garra max-power --goal "add OAuth"` routes to brainstorm and runs the full pipeline.
- All CI checks pass on the PR.

## Risk Register

| Risk | Mitigation |
|------|-----------|
| `try_recv` race on same-thread channel | Not a race — send happens before recv; buffered channel guarantees ordering. |
| `WORKFLOW` const with non-`Copy` type | `TeamPhase` derives `Copy`; no issue. |
| Clippy `dead_code` on internal structs | Annotate with `#[allow(dead_code)]` if needed, or keep structs pub within module. |

## Cross-References

- GAR-498 (native skill registry, plan 0171) — prerequisite ✅ Done
- GAR-492 epic (GarraMaxPower) — parent epic
- GAR-494 (`garra max-power` skeleton) ✅ Done
- ROADMAP §7 priority 5: GAR-498 + GAR-499

## Estimativa

1 session, ~250 LOC new, ~30 LOC edited, ≥ 12 tests.
