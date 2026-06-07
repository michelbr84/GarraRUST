# Plan 0272 — GAR-809: Health run 87 (2026-06-07 ~20:45 ET) — all surfaces clean, priority (i)

**Status:** ✅ Done  
**Linear:** [GAR-809](https://linear.app/chatgpt25/issue/GAR-809)  
**Branch:** `health/202606080045-run87-status-note`  
**Routine type:** Health & security scan (autonomous)  
**Run:** 87 (~20:45 ET / 00:45 UTC, 2026-06-08)  
**Previous run:** [GAR-807](https://linear.app/chatgpt25/issue/GAR-807) / plan 0270 (run 86, ~16:46 ET Jun 6)

---

## Goal

Autonomous health & security scan run 87. Scan all 4 security surfaces, apply priority ladder, merge any pending health/ PRs, file status note if priority (i).

## Architecture / Tech stack

No code changes. Status-note-only run (priority (i)).

## Design invariants

- Never push to main directly
- Never expose secret values in commits or PR bodies
- Health branch prefix `health/` — never touches `routine/` PRs

## Out of scope

- Feature development
- Roadmap items (covered by the roadmap routine at xH:15)

## Rollback

N/A — docs-only commit.

---

## Housekeeping completed

- No pending health/ PRs at run start (PR #657 / health run 86 already merged as `a4d365f`)

---

## Security Surface Scan

| Surface | Evidence | Result |
|---------|----------|--------|
| Secret scanning (gitleaks) | CI Secret Scan job: success on all recent main runs | ✅ clean |
| Malware (cargo/npm graph) | cargo-deny CI job: success on all recent main runs | ✅ clean |
| Dependabot alerts | 2 upstream-blocked advisories: rsa HIGH (GAR-456 / RUSTSEC-2023-0071), glib MEDIUM (GAR-513 / RUSTSEC-2024-0429); no first_patched_version available; expiry 2026-07-31 | ⚠️ unchanged, deferred |
| Open Dependabot PRs | 0 open health/ PRs; routine/ PR #659 (GAR-808) skipped per protocol | ✅ none |
| CodeQL — Analyze (rust) | CI job success on main (run 27074525516 + 27074075619 + 27073493247) | ✅ clean |
| CodeQL — Analyze (javascript-typescript) | CI job success on main (same runs) | ✅ clean |
| CodeQL — Analyze (actions) | CI job success on main (same runs) | ✅ clean |
| Security Audit (cargo audit) | Security Audit CI job success on all recent runs; 0 vulnerabilities | ✅ clean |
| CI on main (7ace764) | All checks green — CI run 27074525517 + CodeQL run 27074525516 + Quality Ratchet run 27074525522 all success | ✅ green |

---

## Priority Ladder

- (a) Secret scanning active/unverified: **none** → skip
- (b) Malware: **none** → skip
- (c) Critical Dependabot with patch: **none** (no critical alerts) → skip
- (d) High Dependabot with patch: rsa HIGH — no first_patched_version (upstream-blocked, GAR-456) → skip
- (e) Critical CodeQL: **none** → skip
- (f) High CodeQL: **none** → skip
- (g) CI failure on main < 24h: **none** (all green) → skip
- (h) Medium Dependabot/CodeQL with low blast radius: glib MEDIUM — no patch (upstream-blocked, GAR-513) → skip
- **(i) None of the above → file status note and exit cleanly** ✓

---

## Tasks

- [x] T1: `git fetch origin main && git pull --ff-only`
- [x] T2: Check pending health/ PRs — none open (PR #657 already merged)
- [x] T3: Scan CI on main (all green — CI/CodeQL/Quality Ratchet success in last 24h)
- [x] T4: Scan secret scanning (gitleaks CI pass on main)
- [x] T5: Scan Dependabot alerts (2 open, all upstream-blocked — unchanged from run 86)
- [x] T6: Scan CodeQL / code scanning (all 3 Analyze jobs green on main)
- [x] T7: Check open health/ PRs (none open)
- [x] T8: Check open routine/ PRs (PR #659 `routine/202606070016-get-task-label-assignments` GAR-808 — skipped per protocol)
- [x] T9: Apply priority ladder → (i) — all clean
- [x] T10: File Linear status note (GAR-809) + write plan 0272 + update plans/README.md + update dependabot-status.md

---

## Risk register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|-----------|
| Dependabot alert (rsa HIGH) unpatched | Ongoing | Medium | Tracked GAR-456, suppressed audit.toml + deny.toml, expiry 2026-07-31 |
| glib MEDIUM upstream-blocked | Ongoing | Low | Tracked GAR-513, allowlisted in audit.toml, expiry 2026-07-31 |

---

## Acceptance criteria

- [x] All security surfaces scanned
- [x] Priority ladder applied
- [x] Linear GAR-809 filed and marked Done
- [x] plans/README.md updated
- [x] Plan file committed on `health/202606080045-run87-status-note`

---

## Cross-references

- GAR-456: rsa HIGH (RUSTSEC-2023-0071) — upstream-blocked carve-out
- GAR-513: glib MEDIUM (RUSTSEC-2024-0429) — upstream-blocked carve-out
- GAR-807: previous health run (86, ~16:46 ET Jun 6)
- Plan 0270: health run 86
- `docs/security/dependabot-status.md`: owner map

## Estimativa

~10 min (scan + housekeeping + docs)
