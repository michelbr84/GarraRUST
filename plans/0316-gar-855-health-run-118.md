# Plan 0316 — GAR-855: Health run 118 status note (2026-06-11 ~12:45 ET)

**Type:** Security health routine — status note (priority i)
**Linear:** [GAR-855](https://linear.app/chatgpt25/issue/GAR-855)
**Date:** 2026-06-11 ~12:45 ET (Florida local time)
**Run number:** 118

## Result

**Priority (i) — no actionable security work found.** All surfaces clean.

## Housekeeping

PR #726 (`health/202606110705-run117-status-note` / GAR-854) squash-merged as `6575f88` — health run 117.
PR #725 (`routine/202606111221-doc-blocks-get` / GAR-853) merged as `7f13deb` — skipped per protocol (routine/ prefix).
All CI checks green on main `6575f88` (2026-06-11T13:34Z).

## Scan Scope

- GitHub Actions CI on main (last 20 runs via MCP API)
- GitHub Dependabot alerts and PRs (0 open confirmed — 59 total, all closed)
- Security surfaces: cargo-audit, cargo-deny, CodeQL, gitleaks, GitHub secret scanning
- dependabot-status.md, codeql-suppressions.md, Linear security issues
- All 15 CI jobs on latest main run confirmed success

## Transient CI Note

Run 27348484450 (2026-06-11T12:59Z) showed `conclusion: failure` for "Test (windows-latest)" — root cause: step 17 "Post Cache cargo registry" (a GitHub Actions post-job artifact upload step) failed, not the actual test step (step 9 "Run tests" passed). Subsequent run 27350603184 at 13:34Z succeeded on all platforms including Windows. Not actionable.

## Advisory Table (carried from run 117 — no changes)

| Package | Locked Version | Advisory | Status |
|---|---|---|---|
| h2 | 0.4.14 | RUSTSEC-2024-0332 (CONTINUATION flood) | ✅ Safe — patched in ≥0.4.4 |
| h2 | 0.4.14 | RUSTSEC-2024-0003 (resource exhaustion) | ✅ Safe — patched in ≥0.4.2 |
| ring | 0.17.14 | RUSTSEC-2025-0009 / CVE-2025-4432 (AES panic) | ✅ Safe — patched in ≥0.17.12 |
| bytes | 1.11.1 | RUSTSEC-2026-0007 / CVE-2026-25541 (BytesMut overflow) | ✅ Safe — 1.11.1 is the fix |
| rustls | 0.23.40 | RUSTSEC-2024-0399 (Acceptor panic) | ✅ Safe — patched in ≥0.23.18 |
| idna | 1.1.0 | RUSTSEC-2024-0421 / CVE-2024-12224 (Punycode bypass) | ✅ Safe — patched in ≥1.0.0 |
| curve25519-dalek | 4.1.3 | RUSTSEC-2024-0344 (timing variability) | ✅ Safe — 4.1.3 is the fix |
| wasmtime | 45.0.0 | RUSTSEC-2026-0095 / CVE-2026-34987 (Winch sandbox escape) | ✅ Safe — patched in 43.0.1; Winch not used |
| wasmtime | 45.0.0 | RUSTSEC-2026-0149 (WASI path_open TRUNCATE bypass) | ✅ Safe — patched in ≥45.0.0 |
| wasmtime | 45.0.0 | CVE-2026-34944 (f64x2.splat out-of-bounds) | ✅ Safe — patched in 43.0.1 |
| tungstenite | 0.21.0 | RUSTSEC-2023-0065 (DoS large HTTP headers) | ✅ Safe — patched in ≥0.20.1 |
| rsa | 0.9.10 | RUSTSEC-2023-0071 (Marvin Attack timing) | ⚠️ Known moderate — allowlisted GAR-456, expiry 2026-07-31 |
| zip | 3.0.0, 4.6.1 | CVE-2025-29787 (path traversal) | ✅ Safe — patched in ≥2.3.0 |
| glib | 0.18.5 | RUSTSEC-2024-0429 (VariantStrIter unsound) | ⚠️ Known unsound — allowlisted GAR-513, expiry 2026-07-31 |

## CI Status (main `6575f88`, 2026-06-11T13:34Z)

All CI jobs: ✅ success. Includes cargo-audit, cargo-deny, CodeQL (Analyze rust + js-ts), Quality Ratchet, gitleaks.

| Job | Conclusion |
|---|---|
| Format Check | ✅ success |
| Clippy Linting | ✅ success |
| cargo-deny | ✅ success |
| Security Audit | ✅ success |
| Secret Scan (gitleaks) | ✅ success |
| MSRV check (1.93) | ✅ success |
| Test (ubuntu-latest) | ✅ success |
| Test (macos-latest) | ✅ success |
| Test (windows-latest) | ✅ success |
| Build Check | ✅ success |
| Coverage (cargo-llvm-cov) | ✅ success |
| Playwright E2E (MCP UI) | ✅ success |
| E2E Tests | ✅ success |
| Install.sh shellcheck | ✅ success |
| Dependency Review | ⏭ skipped (push event, not PR) |

## No Changes Made

This is a status note only. No lockfile or code changes needed.

## Next Security Backlog

- rsa RUSTSEC-2023-0071 (expiry 2026-07-31) — GAR-456
- glib RUSTSEC-2024-0429 (expiry 2026-07-31) — GAR-513
- CodeQL ledger re-audit due 2026-08-01 — GAR-491
- Monitor HTTP/2 Bomb (CVE-2026-49975) for h2/hyper Rust advisory (none found)
