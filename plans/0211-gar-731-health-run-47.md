# Plan 0211 — GAR-731: Health run 47 (2026-05-28 ~03:15 ET)

**Status:** In Progress
**Linear:** [GAR-731](https://linear.app/chatgpt25/issue/GAR-731)
**Priority:** (i) — informational, no actionable security work found
**Date:** 2026-05-28 ~03:15 ET / 07:15 UTC

## Summary

Daily security/dependency health routine — run 47. Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

## Cargo Audit Results

- Tool: `cargo-audit 0.22.1`
- Advisory DB: 1098 advisories loaded
- **Vulnerabilities (errors): 0**
- **Warnings: 19** (all `unmaintained` / informational — unchanged from run 40+, all in deny.toml ignore list)
- Exit code: 0 (clean)

## Unmaintained Warnings (all blocked on upstream, no fix available)

| Package | Version | Advisory | Root cause | Blocked by |
|---|---|---|---|---|
| atk, atk-sys, gdk, gdk-sys, gdkwayland-sys, gdkx11, gdkx11-sys, gtk, gtk-sys, gtk3-macros | 0.18.2 | RUSTSEC-2024-0411–0420 | gtk-rs GTK3 bindings unmaintained | Tauri v2 GTK4 upgrade |
| async-std | 1.13.2 | RUSTSEC-2025-0052 | Discontinued | opentelemetry_sdk upgrade (GAR-711) |
| derivative | 2.2.0 | RUSTSEC-2024-0388 | Unmaintained | poise upgrade (no 0.7.x yet) |
| fxhash | 0.2.1 | RUSTSEC-2025-0057 | Unmaintained | kuchikiki / Tauri upgrade |
| proc-macro-error | 1.0.4 | RUSTSEC-2024-0370 | Unmaintained | glib-macros / Tauri upgrade |
| unic-char-property, unic-char-range, unic-common, unic-ucd-ident, unic-ucd-version | 0.9.0 | RUSTSEC-2025-0075/0080/0081/0098/0100 | Unmaintained | urlpattern / Tauri upgrade |

## Key Security Packages — Verified Clean

| Package | Version | Notes |
|---|---|---|
| wasmtime | 45.0.0 | Above all April 9 2026 advisory patched versions (43.0.1 latest patch) |
| rustls | 0.23.40 | Clean |
| rustls-webpki | 0.103.13 | Above RUSTSEC-2024-0003 patched (0.102.3) |
| h2 | 0.4.14 | Above RUSTSEC-2024-0336 patched (0.4.10) |
| hyper | 1.9.0 | Clean |
| tokio | 1.52.3 | Clean |
| ring | 0.17.14 | Clean |
| openssl / openssl-sys | 0.10.80 / 0.9.116 | Clean |
| rsa | 0.9.10 | Above RUSTSEC-2023-0071 patched (0.9.7) — also in audit.toml suppress list |
| axum | 0.8.9 (+ 0.7.9 transitive via tonic/otel) | Clean |
| jsonwebtoken | 10.4.0 | Clean |

## Open PRs at Run Time

**Routine/ territory (not actioned — protocol):**
- PR #556: `feat(search): GAR-730 — slice 13 types=users` — open, skipped
- PR #552: `feat(search): GAR-726 — slice 12 types=threads` — open, skipped
- PR #555: `docs(plans): GAR-728 — mark plan 0209 complete` — open, skipped

**Dependabot (8 open, none security-labeled):**
- PR #513: patch-and-minor group (7 updates)
- PR #515: opentelemetry_sdk 0.26.0→0.32.0
- PR #516: rand_chacha 0.9.0→0.10.0
- PR #517: criterion 0.5.1→0.8.2
- PR #518: opentelemetry-otlp 0.26.0→0.32.0
- PR #519: opentelemetry-semantic-conventions 0.26.0→0.32.0
- PR #520: lopdf 0.34.0→0.40.0
- PR #522: tracing-opentelemetry 0.32.1→0.33.0

## Dependabot Alerts (unchanged)

3 open, all UPSTREAM-BLOCKED:
- rsa HIGH — GAR-456, suppressed in audit.toml + deny.toml, expiry 2026-07-31
- glib MEDIUM — GAR-513, suppressed, expiry 2026-07-31
- rand LOW — GAR-513, suppressed, expiry 2026-07-31

## Security Backlog (unchanged)

- **GAR-456**: rsa / RUSTSEC-2023-0071 HIGH — suppression expiry 2026-07-31
- **GAR-491**: CodeQL ledger re-audit due 2026-08-01
- **GAR-513**: glib + rand — suppression expiry 2026-07-31
- **GAR-669**: argon2 ≥ 0.6 stable blocks Slices 3–4
- **GAR-711**: OpenTelemetry 0.26→0.32 Backlog

## No Changes Applied

Bookkeeping only. No code, lockfile, or dependency changes. Priority ladder exhausted at (i).
