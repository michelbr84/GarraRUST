# Plan 0302 — GAR-841: Health Run 110 (2026-06-10 ~07:07 ET)

**Status:** Done
**Linear:** GAR-841
**Branch:** `health/202606100707-run110-status-note`
**Previous run:** GAR-839 / plan 0300 (run 109, ~04:45 ET 2026-06-10)

---

## Summary

Autonomous health & security routine — run 110.
Priority **(i)** — all surfaces clean, no actionable security work found.

## Housekeeping Completed This Run

- PR #711 (`health/202606100445-run109-status-note`): squash-merged as `59a13e7` — health run 109 / GAR-839. All CI checks green before merge.
- PR #709 (`routine/202606100620-doc-blocks-crud`) open with routine/ prefix — skipped per protocol.

## New June 2026 Advisories Swept

All RUSTSEC advisories added to the advisory DB in June 2026 were checked against Cargo.lock:

| Advisory | Crate | Date | Impact |
|---|---|---|---|
| RUSTSEC-2026-0173 | proc-macro-error2 | 2026-06-07 | ✅ already in deny.toml (GAR-817) |
| RUSTSEC-2026-0174 | http-types | 2026-06-08 | ✅ not in Cargo.lock |
| RUSTSEC-2026-0172 | diesel | 2026-06-05 | ✅ not in Cargo.lock |
| RUSTSEC-2026-0152..0171 | oneringbuf/russh/metacall/matrix-sdk/pqcrypto/surf/tide/logflux | various | ✅ none in Cargo.lock |
| RUSTSEC-2026-0007 | bytes (integer overflow BytesMut::reserve) | earlier | ✅ bytes 1.11.1 already patched (≥1.11.1 required) |

## Scan Results

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI success on main `ed1093f` (2026-06-10T06:43Z) |
| Malware (cargo/npm) | ✅ none | cargo-deny CI passes |
| Dependabot PRs | ✅ none open | 0 open Dependabot PRs |
| Dependabot alerts | ⚠️ 1 moderate allowlisted | rsa RUSTSEC-2023-0071, expiry 2026-07-31 |
| cargo-audit | ✅ pass | 0 vulnerabilities, 18 allowed unmaintained warnings. `cargo audit --deny unsound` exit 0 |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 + RUSTSEC-2024-0429 + 18 unmaintained suppressed |
| CodeQL | ✅ pass | CI on main `ed1093f` green |
| CI on main | ✅ green | Nightly cargo-audit.yml: success (2026-06-09) |

## Priority Decision

**(i)** — No critical, high, or medium actionable alerts. All known moderate alerts are allowlisted with rationale and expiry dates. No CI failures on main. No open health/ PRs remaining.

## Next Security Backlog

- rsa RUSTSEC-2023-0071 (GAR-456, expiry 2026-07-31) — no `first_patched_version` available upstream (0.10.0-rc.18 still RC)
- RUSTSEC-2024-0429 glib (GAR-513, expiry 2026-07-31) — audit.toml-only residual, desktop-only dep
- CodeQL ledger re-audit due 2026-08-01 (GAR-491)

## Acceptance Criteria

- [x] Status note filed in Linear (GAR-841)
- [x] `docs/security/dependabot-status.md` updated with run 110 results
- [x] `plans/README.md` row added for plan 0302
- [x] PR merged to main with green CI
