# Plan 0174 — GAR-694: Health Run 22 (2026-05-23 ~20:45 ET)

## Goal

Document health & security run 22 — all surfaces clean, priority (i) status note.
Bookkeeping: mark plan 0173 (GAR-693 health run 21) as merged, add this run's row.

## Security Scan Results

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #489 (20/20 checks green) |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PR #489 |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456), glib MEDIUM (GAR-513), rand LOW (GAR-513) — suppression expiry 2026-07-31 |
| Open Dependabot PRs | ✅ none | 0 open |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #489 (20/20) |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 (rsa) only active suppression |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #489 |
| CI on main (`133fef8`) | ✅ green | All 20 checks passed |

## Actions Taken

1. **Merged PR #489** (`health/202605231645-run21-status-note`, GAR-693) — all 20 CI checks green → squash-merged as `133fef8`.
2. **Full security scan** — priority ladder exhausted at (i).
3. **Security review of routine/ PR #490** (GAR-499 agent team MVP):
   - Pure Rust, no network, no file I/O in production code
   - `std::sync::mpsc` channels with `.ok()` handling, no `unwrap()` outside tests
   - No new crate dependencies, no SQL, no auth, no PII, no unsafe blocks
   - **CLEAN** — no security concerns
4. **Plan numbering conflict noted** (not actioned — routine/ territory): PR #490 adds `plans/0173-gar-499-agent-team-mvp.md` but main already has `plans/0173-gar-693-health-run-21.md`. Roadmap routine must resolve on merge.

## Priority Ladder

- (a) Secret scanning active/unverified: **none**
- (b) Malware: **none**
- (c) Critical Dependabot with fix: **none**
- (d) High Dependabot with fix: rsa — **upstream-blocked** (RUSTSEC-2023-0071, expiry 2026-07-31)
- (e) Critical CodeQL: **none**
- (f) High CodeQL: **none**
- (g) CI failure on main <24h: **✅ green**
- (h) Medium Dependabot/CodeQL: glib + rand — **upstream-blocked**
- **(i) No actionable work → status note only**

## Next Security Backlog

- argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4 (password-hash + rand)
- rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31
- CodeQL ledger re-audit due 2026-08-01 (GAR-491)

## Cross-References

- GAR-693 (health run 21, PR #489, `133fef8`)
- GAR-692 (health run 20, PR #486, `07070f5`)
- GAR-456 (rsa HIGH — RUSTSEC-2023-0071, upstream-blocked)
- GAR-513 (glib+rand — upstream-blocked, expiry 2026-07-31)
- GAR-491 (CodeQL ledger re-audit, due 2026-08-01)
