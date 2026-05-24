# Plan 0176 — GAR-493: GarraMaxPower ADR 0011

## Goal

Record the architectural decision for **GarraMaxPower** (the Garra native agent-advanced
mode, epic [GAR-492](https://linear.app/chatgpt25/issue/GAR-492)) in
`docs/adr/0011-garra-max-power.md` now that all eight sub-issues have been merged to main.

ADR 0009 is taken by the Garra Glass design system (plan 0116 / GAR-607), so the
GarraMaxPower ADR is numbered **0011** (following the learning-agent ADR 0010).

## Architecture

Docs-only PR: one new ADR file + one-row update to `docs/adr/README.md` +
ROADMAP §1.2.1 cross-link correction (0009 → 0011).

## Tech stack

Markdown; no code changes.

## Design invariants

- ADR is **Accepted** (all implementation sub-issues already merged).
- ROADMAP references updated from `0009` to `0011` to match the real file path.
- No new code; `cargo check --workspace` stays green by definition.

## Validações pré-plano

- [x] `docs/adr/0009-web-console-design-system.md` exists — 0009 is taken.
- [x] `docs/adr/0010-garra-learning-agent.md` exists — 0010 is taken.
- [x] `docs/adr/0011-*.md` does not exist — 0011 is free.
- [x] All GAR-492 sub-issues (GAR-494–GAR-501 + GAR-498 + GAR-499) are Done.

## Out of scope

- Implementation of any `garra max-power` subcommand or crate change.
- Changes to config, secrets, or CI workflows.

## Rollback

N/A (docs only; revert the commit if content is wrong).

## §12 Open questions

None.

## File Structure

```
docs/adr/0011-garra-max-power.md       ← new
docs/adr/README.md                      ← +1 row
ROADMAP.md                              ← §1.2.1 reference 0009→0011
plans/README.md                         ← +1 row for this plan
```

## M1 — Tasks

- [x] T1: Write `docs/adr/0011-garra-max-power.md` (Accepted).
- [x] T2: Add row to `docs/adr/README.md` index.
- [x] T3: Fix ROADMAP.md §1.2.1 ADR reference from 0009 to 0011.
- [x] T4: Add row to `plans/README.md` for plan 0176.
- [x] T5: Single commit `docs(adr): GAR-492 — ADR 0011 GarraMaxPower nativo`.
- [x] T6: Push + open PR + wait CI green + squash-merge.
- [x] T7: Mark GAR-493 Done in Linear.

## Risk register

| Risk | Mitigation |
|------|------------|
| ADR number collision | Verified: 0011 is free |
| ROADMAP has more than one `0009` reference for MaxPower | Fix all via grep |

## Acceptance criteria

- `docs/adr/0011-garra-max-power.md` merged to main, status Accepted.
- `docs/adr/README.md` lists row 0011.
- `cargo check --workspace` green (no code change).
- GAR-493 Done in Linear.

## Cross-references

- Epic: [GAR-492](https://linear.app/chatgpt25/issue/GAR-492)
- Sub-issues merged: GAR-494, GAR-497, GAR-501, GAR-500, GAR-495, GAR-496, GAR-498, GAR-499
- ADR 0009 (taken): `docs/adr/0009-web-console-design-system.md`
- ADR 0010: `docs/adr/0010-garra-learning-agent.md`
- ROADMAP §1.2.1 GarraMaxPower

## Estimativa

~30 min (docs only).
