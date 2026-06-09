# Plan 0295 — GAR-832: Health Run 105 (2026-06-09 ~08:45 ET)

**Status:** Done
**Linear:** GAR-832
**Branch:** `health/202606091245-run105-status-note`
**Previous run:** GAR-831 / plan 0293 (run 104, ~04:46 ET 2026-06-09)

---

## Summary

Autonomous health & security routine — run 105.
Priority **(i)** — all surfaces clean, no actionable security work found.

## Housekeeping Completed This Run

- PR #701 (`ci(mutants): GAR-825`) squash-merged as `6060116` — 3-way shard + --features test-support in mutants.yml
- PR #698 (`7f0f8e6`): health run 104 status note / GAR-831 — merged prior to this run
- No open health/ or routine/ PRs at scan time

## Scan Results

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI success on main `6060116` (2026-06-09T12:37Z) |
| Malware (cargo/npm) | ✅ none | cargo-deny CI job success |
| Dependabot PRs | ✅ none open | 0 open Dependabot PRs |
| Dependabot alerts | ⚠️ 1 moderate allowlisted | rsa RUSTSEC-2023-0071, expiry 2026-07-31 |
| cargo-audit | ✅ pass | CI Security — cargo audit success |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 + RUSTSEC-2024-0429 + RUSTSEC-2026-0173 suppressed |
| CodeQL | ✅ pass | Analyze (rust) + Analyze (javascript-typescript) + Analyze (actions) all success |
| CI on main (`6060116`) | ✅ green | CI + Quality Ratchet + CodeQL + Security all success |

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
- Systemic mutation fix — GAR-825 (Done 2026-06-09)
- CodeQL ledger re-audit — GAR-491, due 2026-08-01
