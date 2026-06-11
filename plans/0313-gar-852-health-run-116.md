# Plan 0313 — GAR-852: Health run 116 status note (2026-06-11 ~07:05 ET)

**Type:** Security health routine — status note (priority i)
**Linear:** [GAR-852](https://linear.app/chatgpt25/issue/GAR-852)
**Date:** 2026-06-11 ~07:05 ET (Florida local time)
**Run number:** 116

## Result

**Priority (i) — no actionable security work found.** All surfaces clean.

## Housekeeping

PR #722 (`health/202606110445-run115-status-note`) squash-merged as `2a58538` — health run 115 / GAR-849.
PR #723 (`routine/202606111634-doc-pages-version-restore` / GAR-850) open — CI in progress, skipped per protocol (routine/ prefix, not health/).
All CI checks green on main `d8e80e2` (2026-06-11T06:58Z).

## Scan Scope

- GitHub Actions CI on main (last runs via MCP API)
- GitHub Dependabot alerts and PRs (via MCP search: 0 open)
- Security — cargo audit CI, CodeQL CI, Quality Ratchet CI, cargo deny CI, GitHub secret scanning
- Manual RUSTSEC/CVE web research (cargo-audit not available in remote session toolchain)
- dependabot-status.md, Linear security issues
- 1,073 locked crates across 22 workspace members

## Advisory Scan Results

Full manual scan of the RUSTSEC advisory database for all security-relevant packages in Cargo.lock. All advisories from 2024–2026 cross-checked against locked versions.

| Package | Locked Version | Advisory | Status |
|---|---|---|---|
| h2 | 0.4.14 | RUSTSEC-2024-0332 (CONTINUATION flood) | ✅ Safe — patched in ≥0.4.4 |
| h2 | 0.4.14 | RUSTSEC-2024-0003 (resource exhaustion) | ✅ Safe — patched in ≥0.4.2 |
| ring | 0.17.14 | RUSTSEC-2025-0009 / CVE-2025-4432 (AES panic) | ✅ Safe — patched in ≥0.17.12 |
| bytes | 1.11.1 | RUSTSEC-2026-0007 / CVE-2026-25541 (BytesMut overflow) | ✅ Safe — 1.11.1 is the fix |
| rustls | 0.23.40 | RUSTSEC-2024-0399 (Acceptor panic) | ✅ Safe — patched in ≥0.23.18 |
| idna | 1.1.0 | RUSTSEC-2024-0421 / CVE-2024-12224 (Punycode bypass) | ✅ Safe — patched in ≥1.0.0 |
| curve25519-dalek | 4.1.3 | RUSTSEC-2024-0344 (timing variability) | ✅ Safe — 4.1.3 is the fix |
| wasmtime | 45.0.0 | RUSTSEC-2026-0095 / CVE-2026-34987 (Winch sandbox escape) | ✅ Safe — patched in 43.0.1; Winch not used (Cranelift default) |
| wasmtime | 45.0.0 | RUSTSEC-2026-0149 (WASI path_open TRUNCATE bypass) | ✅ Safe — patched in ≥45.0.0 |
| wasmtime | 45.0.0 | CVE-2026-34944 (f64x2.splat out-of-bounds) | ✅ Safe — patched in 43.0.1 |
| wasmtime | 45.0.0 | RUSTSEC-2026-0006 / CVE-2026-24116 (f64.copysign x86-64 AVX) | ✅ Safe — fixed pre-42.x; 45.0.0 post-fix |
| tungstenite | 0.21.0 | RUSTSEC-2023-0065 (DoS large HTTP headers) | ✅ Safe — patched in ≥0.20.1; pulled via serenity 0.12.5 |
| rsa | 0.9.10 | RUSTSEC-2023-0071 (Marvin Attack timing) | ⚠️ Known moderate — allowlisted GAR-456, expiry 2026-07-31 |
| zip | 3.0.0, 4.6.1 | CVE-2025-29787 (path traversal) | ✅ Safe — patched in ≥2.3.0 |
| glib | 0.18.5 | RUSTSEC-2024-0429 (VariantStrIter unsound) | ⚠️ Known unsound — allowlisted GAR-513, expiry 2026-07-31 |

### Not in lockfile (verified)
`rkyv`, `fast_id_map`, `matrix-sdk-base`, `number_prefix`, `lettre`, `openssl-src`

## HTTP/2 Bomb (CVE-2026-49975) — Investigated, Not Applicable

Disclosed 2026-06-02. Combines HPACK amplification + Slowloris flow-control stalling. Affects nginx, Apache httpd, IIS, Envoy, Cloudflare Pingora. No RUSTSEC advisory found for `h2` or `hyper` Rust crates. GarraRUST uses `h2 0.4.14` as embedded Hyper/Axum implementation — distinct implementation from the affected web servers. Monitoring: check `rustsec.org/packages/h2.html` in future runs.

## Toolchain

- Rust 1.94.1 (2026-03-25) — Patched for CVE-2026-33056 (tar-rs symlink, fixed in 1.94.1) ✅
- CVE-2026-5223 (Cargo symlink tarball, fixed in 1.96.0) — only affects third-party registries; crates.io not affected → low risk

## wasmtime 45.0.0 → 45.0.1 (not urgent)

wasmtime 45.0.1 released 2026-06-05. Fixes WASIp2 zero-delay timer regression. GarraRUST uses WASIp1 (`p1` module) exclusively. Not a security fix — low priority hygiene update, deferred.

## Key Dependency Notes

- `serenity 0.12.5` (Discord client in `garraia-channels`) pulls in `tokio-tungstenite 0.21.0` + `secrecy 0.8.0` — both at safe versions for known advisories
- `utoipa-swagger-ui 9.0.2` pulls in `zip 3.0.0` — safe (CVE-2025-29787 patched in ≥2.3.0)
- `native-tls 0.2.18` pulls in system `openssl` — no RUSTSEC advisories found for native-tls 0.2.x
- 1,073 locked packages; many multi-version (hashbrown 5 versions, windows-sys 6 versions) — expected from large workspace

## CI Status (main `d8e80e2`, 2026-06-11T06:58Z)

All 20 CI jobs: ✅ success. Includes cargo-audit, cargo-deny, CodeQL, Quality Ratchet, gitleaks.

## No Changes Made

This is a status note only. No lockfile or code changes needed.

## Next Security Backlog

- rsa RUSTSEC-2023-0071 (expiry 2026-07-31) — GAR-456
- glib RUSTSEC-2024-0429 (expiry 2026-07-31) — GAR-513
- CodeQL ledger re-audit due 2026-08-01 — GAR-491
- Monitor HTTP/2 Bomb (CVE-2026-49975) for h2/hyper Rust advisory (none yet)
