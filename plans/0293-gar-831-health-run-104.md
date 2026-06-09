# Plan 0293 — GAR-831: Health Run 104 (2026-06-09 ~04:46 ET)

**Status:** Done
**Linear:** GAR-831
**Branch:** `health/202506090846-run104-status-note`
**Previous run:** GAR-830 / plan 0292 (run 103, ~07:15 ET 2026-06-09)

---

## Summary

Autonomous health & security routine — run 104.
Priority **(i)** — all surfaces clean, no actionable security work found.

## Housekeeping Completed This Run

- PR #697 (`claude/focused-cray-cu03h1`): squash-merged as `dd96289` — health run 103 status note / GAR-830
- PR #695 (`docs/mark-plan-0288-gar-827-done`): merged — marks plan 0288 / GAR-827 Done in plans/README.md
- PR #696 (`routine/202506091430-get-chat-member`): skipped (routine/ prefix — roadmap routine territory)

## Scan Results

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI success on main `dd96289` (2026-06-09T08:47Z) |
| Malware (cargo/npm) | ✅ none | cargo-deny CI success |
| Dependabot PRs | ✅ none open | 0 open Dependabot PRs |
| Dependabot alerts | ⚠️ 1 moderate allowlisted | rsa RUSTSEC-2023-0071, expiry 2026-07-31 |
| cargo-audit | ✅ pass | 0 vulnerabilities; 18 unmaintained warnings (all tracked) |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 + RUSTSEC-2024-0429 + RUSTSEC-2026-0173 suppressed |
| CodeQL | ✅ pass | Analyze (rust) + Analyze (javascript-typescript) + Analyze (actions) all success |
| CI on main (`dd96289`) | ✅ green | CI + Quality Ratchet + CodeQL all success |

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
- Systemic mutation fix — GAR-825 (Backlog)
- CodeQL ledger re-audit — GAR-491, due 2026-08-01
