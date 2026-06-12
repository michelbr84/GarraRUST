# Plan 0324 — GAR-865: Health Run 123 (2026-06-12 ~20:45 ET): All Surfaces Clean

> **Status:** Done (priority i — status note only)
> **Linear:** [GAR-865](https://linear.app/chatgpt25/issue/GAR-865)
> **Branch:** `health/202606122045-run123-status-note`
> **Routine type:** Health & Security (complementary to roadmap routine at xH:15)

## Goal

Autonomous health & security scan run 123. All surfaces clean — no actionable security work found. Filing status note per protocol priority (i).

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

## Scan results (2026-06-12 ~20:45 ET / 00:45 UTC 2026-06-13)

**Main commit scanned:** `fa3715b` (2026-06-12T20:46Z) — docs(tracking): GAR-863

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI Secret Scan job success on `fa3715b` |
| Malware (cargo/npm) | ✅ none | cargo-deny CI job success (`advisories ok, bans ok, licenses ok, sources ok`) |
| Dependabot PRs | ✅ none open | 0 open Dependabot PRs |
| Dependabot security alerts | ⚠️ 1 moderate (RUSTSEC-2023-0071), allowlisted | rsa 0.9.10 — Marvin Attack timing sidechannel. HS256-only invariant holds. Allowlisted, expiry 2026-07-31. No first_patched_version available. |
| Security Audit (cargo-audit) | ✅ pass | CI Security Audit success on `fa3715b`; 18 unmaintained warnings (GTK/unic/proc-macro-error2), 0 vulnerabilities, 0 unsound |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 + RUSTSEC-2024-0429 + 18 unmaintained suppressed |
| CodeQL | ✅ pass | Analyze (rust) + Analyze (js-ts) success on `fa3715b` |
| Quality Ratchet | ✅ pass | CI success |
| CI on main (`fa3715b`) | ✅ green | All 20 jobs success |
| Workflow failures (last 7d) | ✅ none | No failures on main in last 7 days |
| Open PRs | 1 routine/ skipped | PR #738 `routine/202606121915-get-chat-member` — not touched (roadmap routine territory) |

## Pre-flight housekeeping

PR #737 (`health/202606121645-run122-tracking`, GAR-863 tracking) was open and fully green → squash-merged as `fa3715b` at start of this run.

## Tasks

- [x] T1 — Scan all security surfaces (CI logs, workflow runs, audit.toml, deny.toml)
- [x] T2 — Check Linear for existing open health/security issues (no run-123 duplicate found)
- [x] T3 — Determine priority: (i) — no actionable work
- [x] T4 — Create Linear issue GAR-865
- [x] T5 — Write plan 0324
- [x] T6 — Update `docs/security/dependabot-status.md`
- [x] T7 — Update `plans/README.md`
- [x] T8 — Commit, push, open PR, merge

## Risk register

| Risk | Likelihood | Mitigation |
|---|---|---|
| New RUSTSEC advisory published between runs | Low | cargo-audit CI gate catches at next push |
| rsa RUSTSEC-2023-0071 patch lands | Low | Dependabot auto-opens PR; next run picks it up |

## Acceptance criteria

- `dependabot-status.md` updated with run 123 entry.
- `plans/README.md` row added for plan 0324.
- PR merged to main with all CI green.
- GAR-865 marked Done.

## Cross-references

- Previous run: GAR-863 (run 122, plan 0323, PR #736)
- Security backlog: GAR-456 (rsa), GAR-513 (glib), GAR-491 (CodeQL ledger)
- ROADMAP.md §1.5 — security baseline

## Estimativa

< 15 min (doc-only).
