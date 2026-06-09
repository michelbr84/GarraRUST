# Plan 0296 — GAR-833: Health Run 106 (2026-06-09 ~12:45 ET)

**Status:** Done
**Linear:** GAR-833
**Branch:** `health/202606091645-run106-status-note`
**Previous run:** GAR-832 / plan 0295 (run 105, ~08:45 ET 2026-06-09)

---

## Summary

Autonomous health & security routine — run 106.
Priority **(i)** — all surfaces clean, no actionable security work found.

## Housekeeping Completed This Run

- No open health/ or routine/ PRs at scan time
- PR #703 (`docs(plans): mark plan 0295 / GAR-832 Done`) squash-merged as `0c59ae2` — health run 105 tracking
- PR #701 (`ci(mutants): GAR-825`) squash-merged as `6060116` — previously noted

## Scan Results

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI success on main `0c59ae2` (2026-06-09T13:39Z) |
| Malware (cargo/npm) | ✅ none | cargo-deny CI job success |
| Dependabot PRs | ✅ none open | 0 open Dependabot PRs |
| Dependabot alerts | ⚠️ 1 moderate allowlisted | rsa RUSTSEC-2023-0071, expiry 2026-07-31 |
| cargo-audit | ✅ pass | CI Security — cargo audit success |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 + RUSTSEC-2024-0429 + RUSTSEC-2026-0173 suppressed |
| CodeQL | ✅ pass | Analyze (rust) + Analyze (javascript-typescript) + Analyze (actions) all success |
| CI on main (`0c59ae2`) | ✅ green | CI + Quality Ratchet + CodeQL + Security all success |

## Workflow Run Status (last 20 on main)

All 20 most recent workflow runs concluded as **success**. No failures, timeouts, or cancellations in the last 7 days.

Latest run: CI + CodeQL + Quality Ratchet @ 2026-06-09T13:39Z on `0c59ae2`.

## rsa Dependency Chain

```
rsa v0.9.10
└── jsonwebtoken v10.4.0
    ├── garraia-auth v0.2.1
    └── garraia-gateway v0.2.1
```

Security invariant: GarraRUST uses HS256 only — rsa code path not reachable.
RUSTSEC-2023-0071 allowlisted in audit.toml (expiry 2026-07-31).

## Unmaintained Warnings (18, all suppressed in deny.toml)

- gtk-rs GTK3 (10 IDs, RUSTSEC-2024-0411..0420) — GAR-430, expiry 2026-07-31
- derivative RUSTSEC-2024-0388 — transitive via poise/serenity
- proc-macro-error RUSTSEC-2024-0370 — transitive, no maintained alt
- proc-macro-error2 RUSTSEC-2026-0173 — GAR-817 (Done 2026-06-08), expiry 2026-07-31
- unic-* (5 IDs, RUSTSEC-2025-0075/0080/0081/0098/0100) — GAR-430, expiry 2026-07-31

## Next Security Backlog

- rsa RUSTSEC-2023-0071 — jsonwebtoken path, no upstream fix, expiry 2026-07-31
- glib RUSTSEC-2024-0429 — GAR-513 (In Progress), audit.toml-only, expiry 2026-07-31
- proc-macro-error2 RUSTSEC-2026-0173 — GAR-817 (Done), deny.toml suppress, expiry 2026-07-31
- CodeQL ledger re-audit — GAR-491, due 2026-08-01

## Open Branches (non-main)

Stale health/ branches present (no open PRs — previously merged):
- `health/202606061247-run85-status-note`
- `health/202606072047-run93-status-note`
- `health/202606082047-run100-status-note`

Stale routine/ branches (do not touch — roadmap routine territory):
- `routine/202506051820-get-thread`
- `routine/202506060630-get-task-label`
- `routine/202506091220-gar-825-mutants-shard-test-support`
- `routine/202506091430-get-chat-member`
- `routine/202606071900-get-thread-messages`
- `routine/202606080700-get-file-version`

Other stale branches: `claude/serene-fermat-5SxhY`, `claude/wizardly-ptolemy-nsabU`,
`docs/gar-824-mark-done`, `docs/gar-828-mark-done`, `docs/mark-0283-done`,
`docs/mark-plan-0274-done`, `docs/mark-plan-0293-gar-831-done`, `test-connectivity-probe`.

## Acceptance Criteria

- [x] Linear issue GAR-833 filed (automation, health-routine, epic:sec-harden)
- [x] Plan 0296 created
- [x] plans/README.md updated
- [x] docs/security/dependabot-status.md updated
- [x] PR merged to main with green CI
