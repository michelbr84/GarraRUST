# Plan 0125 — GAR-623: docs(bookkeeping) — plan 0123 / PR #341 self-ref fix

**Linear issue:** [GAR-623](https://linear.app/chatgpt25/issue/GAR-623) — "docs(bookkeeping): plan 0123 / PR #341 self-ref fix — CLAUDE.md + ROADMAP + plans/README" (In Progress, Low). Team: GarraIA-RUST.

**Status:** ✅ Approved 2026-05-14 (Florida).

**Goal:** PR #341 (`e570631`) could not write its own PR number into `CLAUDE.md` — a classic self-referential bootstrapping gap. This plan fixes the three dangling references left by that PR and adds the missing `plans/README.md` rows for the web console delivery series (plans 0118–0123).

---

## Root cause

`e570631` (PR #341, plan 0123) added the full 10-PR delivery log to `CLAUDE.md` but had to leave the last entry as `PR final (E2E + ROADMAP/README/CLAUDE)` because the PR number was unknown at write time. Same omission affected:

- `ROADMAP.md` line 782: plan range listed as `0117-0122` (0123 missing).
- `ROADMAP.md` line 791: PR list ends at `#340` (missing `#341`).
- `plans/README.md`: no rows for plans 0118–0123 (web console PRs 5–10).

---

## Scope

| File | Change |
|------|--------|
| `CLAUDE.md` | `PR final (E2E + ROADMAP/README/CLAUDE)` → `#341 (E2E + ROADMAP/README/CLAUDE)` |
| `ROADMAP.md` | `0117-0122` → `0117-0123`; add `, #341` to PR list |
| `plans/README.md` | Add rows for 0118–0123 |

Administrative: close **GAR-486** in Linear (all sub-issues Done; umbrella stale).

---

## Out of scope

- No Rust code changes.
- No new Playwright tests (already added in PR #341).
- No ROADMAP structural changes (§7 priority order will be updated in a future session when the next Fase 3.4 slice ships).

---

## Tasks

- [x] T1 — Create plan file (this document) and Linear issue GAR-623.
- [x] T2 — Fix `CLAUDE.md`: replace `PR final (...)` with `#341 (...)`.
- [x] T3 — Fix `ROADMAP.md`: add `0123` to plan range; add `#341` to PR list.
- [x] T4 — Fix `plans/README.md`: add rows 0118–0123.
- [x] T5 — Commit, push, open PR.
- [x] T6 — CI green → squash-merge.
- [x] T7 — Close GAR-486 in Linear; mark GAR-623 Done.
- [x] T8 — Update `plans/README.md` with this plan's PR number + commit sha.

---

## Acceptance criteria

- `grep "PR final" CLAUDE.md` returns empty.
- `grep "#341" CLAUDE.md` matches the PR list entry.
- `grep "0117-0123" ROADMAP.md` returns a match.
- `grep "#341" ROADMAP.md` returns a match in the PR list.
- `plans/README.md` contains rows for 0118, 0119, 0120, 0121, 0122, 0123.

---

## Estimativa

≪10 min implementation, ~20 min CI.
