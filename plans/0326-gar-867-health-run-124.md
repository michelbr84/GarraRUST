# Plan 0326 — GAR-867: Health Run 124 (2026-06-13 ~00:45 ET): All Surfaces Clean

> **Status:** Done (priority i — status note only)
> **Linear:** [GAR-867](https://linear.app/chatgpt25/issue/GAR-867)
> **Branch:** `health/202606130048-run124-status-note`
> **Routine type:** Health & Security (complementary to roadmap routine at xH:15)

## Goal

Autonomous health & security scan run 124. All surfaces clean — no actionable security work found. Filing status note per protocol priority (i).

## Architecture

Status-note-only PR: updates `docs/security/dependabot-status.md` and `plans/README.md`. No code changes.

## Tech stack

Rust (Axum 0.8) + Postgres 16, security toolchain: cargo-audit, cargo-deny, CodeQL, gitleaks.

## Design invariants

- NEVER edit `.quality/baseline.json` manually.
- NEVER add `continue-on-error: true` to workflows.
- health/ branch prefix — never touch routine/ branches.

## Out of scope

No code changes. No dependency bumps. Allowlisted advisories unchanged.

## Rollback

N/A — doc-only PR.

## Scan results (2026-06-13 ~00:45 ET / 04:45 UTC)

**Main commit scanned:** `ba9b2d6` (2026-06-12T21:31Z) — docs(tracking): GAR-865 — mark plan 0324 done

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI Secret Scan job success on `ba9b2d6` (2026-06-12T21:32Z) |
| Malware (cargo/npm) | ✅ none | cargo-deny CI job success (`advisories ok, bans ok, licenses ok, sources ok`) |
| Dependabot PRs | ✅ none open | 0 open Dependabot PRs |
| Dependabot security alerts | ⚠️ 1 moderate (RUSTSEC-2023-0071), allowlisted | rsa 0.9.10 — Marvin Attack timing sidechannel. HS256-only invariant holds. Allowlisted, expiry 2026-07-31. No first_patched_version available. |
| Security Audit (cargo-audit) | ✅ pass | CI Security Audit success on `ba9b2d6` (2026-06-12T21:35Z); 0 vulnerabilities, 0 unsound |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 + RUSTSEC-2024-0429 + unmaintained suppressed |
| CodeQL | ✅ pass | Analyze (rust) + Analyze (js-ts) success on `ba9b2d6` (2026-06-12T21:31Z) |
| Quality Ratchet | ✅ pass | CI success on `ba9b2d6` |
| CI on main (`ba9b2d6`) | ✅ green | All 15 jobs success |
| Workflow failures (last 7d) | ✅ none | No failures on main in last 7 days |
| Open PRs | 2 open | PR #741 (`claude/` cleanup — CI green, branch protection blocked merge); PR #738 `routine/202606121915-get-chat-member` — not touched (roadmap routine territory) |

## Pre-flight housekeeping

PR #741 (`claude/festive-bell-r8l7jr` — branch cleanup): all 20 CI checks green but squash-merge rejected by branch protection (4 required checks enforcement). Left for owner to resolve. Not a health/ PR.

PR #738 (`routine/202606121915-get-chat-member`, GAR-864): roadmap routine territory — skipped per protocol.

## Tasks

- [x] T1 — Scan all security surfaces (CI logs, workflow runs, audit.toml, deny.toml)
- [x] T2 — Check Linear for existing open health/security issues (no run-124 duplicate found)
- [x] T3 — Determine priority: (i) — no actionable work
- [x] T4 — Create Linear issue GAR-867
- [x] T5 — Write plan 0325
- [x] T6 — Update `docs/security/dependabot-status.md`
- [x] T7 — Update `plans/README.md`
- [x] T8 — Commit, push, open PR, merge

## Risk register

| Risk | Likelihood | Mitigation |
|---|---|---|
| New RUSTSEC advisory published between runs | Low | cargo-audit CI gate catches at next push |
| rsa RUSTSEC-2023-0071 patch lands | Low | Dependabot auto-opens PR; next run picks it up |

## Acceptance criteria

- `dependabot-status.md` updated with run 124 entry.
- `plans/README.md` row added for plan 0325.
- PR merged to main with all CI green.
- GAR-867 marked Done.

## Cross-references

- Previous run: GAR-865 (run 123, plan 0324, PR #739)
- Security backlog: GAR-456 (rsa), GAR-513 (glib), GAR-491 (CodeQL ledger)
- ROADMAP.md §1.5 — security baseline

## Estimativa

< 15 min (doc-only).
