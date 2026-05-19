# Plan 0152 — Health Routine Run 2 (2026-05-19): RUSTSEC-2026-0145 merge + tokio-tungstenite 0.29 upgrade

**Epic:** GAR-486 (sec-harden umbrella)
**Linear:** GAR-668
**Branch:** `health/202605190850-tokio-tungstenite-0.29`
**PR:** #433

---

## Goal

Two-part health routine run (2026-05-19, ~08:45 ET / 12:45 UTC, run 2):

1. **Merge** previously-ready RUSTSEC-2026-0145 fix (PR #432 — `fix/rustsec-2026-0145-astral-tokio-tar`, all 20 CI checks green) that was lingering since run 1.
2. **Upgrade** `tokio-tungstenite` workspace dependency from 0.26.2 → 0.29.0 on a clean `health/` branch, superseding the conflicted Dependabot PR #429.

---

## Architecture

No structural changes. Both actions are pure Cargo.lock / Cargo.toml version bumps:

- RUSTSEC-2026-0145: `astral-tokio-tar` 0.6.1 → 0.6.2 (transitive dev-dep via `testcontainers`)
- tokio-tungstenite: workspace dep `0.26` → `0.29` in root `Cargo.toml` + `crates/garraia-gateway/Cargo.toml`

---

## Tech Stack

- Rust workspace (cargo)
- `cargo update -p "tokio-tungstenite@0.26.2" --precise 0.29.0`
- CI: Format / Clippy / Build / Test×3 / MSRV / Security Audit / cargo-deny / Playwright / E2E / CodeQL

---

## Design Invariants

- No breaking API changes at our use-sites (Clippy + all Tests pass locally)
- `cargo audit`: 0 vulnerabilities (19 pre-existing allowed unmaintained warnings)
- No new suppression entries in `.cargo/audit.toml` or `deny.toml`

---

## Out of Scope

- password-hash 0.5 → 0.6 (ecosystem not ready, breaking, deferred — see GAR-667)
- governor 0.8 → 0.10, rand 0.8 → 0.10, rand_chacha 0.3 → 0.9 (major version breaks, code changes required)
- windows-sys 0.52 → 0.61 (Test(windows) failure in Dependabot PR #422, needs investigation)

---

## Rollback

`git revert <squash-sha>` — no DB migrations, no schema changes.

---

## Open Questions

None.

---

## File Structure

```
Cargo.toml                              (tokio-tungstenite "0.26" → "0.29")
crates/garraia-gateway/Cargo.toml       (tokio-tungstenite "0.26" → "0.29")
Cargo.lock                              (removes 0.26.2 + tungstenite 0.26.2, retains 0.29.0)
plans/0152-health-run2-20260519-*.md    (this file)
plans/README.md                         (row added)
docs/security/dependabot-status.md      (run 2 entry added after merge)
```

---

## Tasks

- [x] T1 — Merge PR #432 (RUSTSEC-2026-0145 fix, all CI green, squash sha `287edc1c`)
- [x] T2 — Create `health/202605190850-tokio-tungstenite-0.29` off main
- [x] T3 — Edit Cargo.toml + garraia-gateway/Cargo.toml: 0.26 → 0.29
- [x] T4 — `cargo update -p "tokio-tungstenite@0.26.2" --precise 0.29.0`
- [x] T5 — `cargo check --workspace --exclude garraia-desktop` ✅
- [x] T6 — `cargo clippy ... -D warnings` ✅ (0 warnings)
- [x] T7 — `cargo audit` ✅ (0 vulnerabilities)
- [x] T8 — Commit + push + open PR #433
- [ ] T9 — Wait for all CI checks green on PR #433
- [ ] T10 — Squash-merge PR #433
- [ ] T11 — Update `docs/security/dependabot-status.md` (run 2 entry)
- [ ] T12 — Close superseded Dependabot PR #429 ✅ (already closed)

---

## Risk Register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| tokio-tungstenite 0.27/0.28/0.29 broke a WS API we use | Low | All builds + tests pass locally |
| Security Audit still fails on PR #433 for other advisory | Low | cargo audit local = 0 vulns |
| windows-sys multi-version conflict in Cargo.lock | Low | cargo update resolved cleanly |

---

## Acceptance Criteria

- PR #433 CI: ≥16 actual checks all `success` (Format, Clippy, Build, Test×3, MSRV, cargo-deny, Security Audit, Coverage, CodeQL rust + js-ts, Playwright, E2E, Secret Scan, Dependency Review)
- `cargo audit` on main post-merge: 0 vulnerabilities
- `tokio-tungstenite 0.26.2` absent from Cargo.lock on main

---

## Cross-References

- RUSTSEC-2026-0145: https://rustsec.org/advisories/RUSTSEC-2026-0145
- PR #432 (RUSTSEC fix, merged): squash sha `287edc1c`
- PR #433 (this work): `health/202605190850-tokio-tungstenite-0.29`
- Superseded Dependabot PR #429 (closed)
- GAR-667 (health run 1 today — all-clean status note)
- Plan 0150 (GAR-648, canonical shape reference)

---

## Estimativa

~1h total (most time in CI wait). Routine bookkeeping.
