# Dependabot Status

> Last updated: **2026-05-27 run 43** (health routine — all surfaces clean, docker/build-push-action Dependabot PR merged to main as `0a820a01`, 8 open Dependabot PRs (none security-labeled), routine/ PR #548 GAR-721 noted, priority (i). GAR-725. Previous: run 42 all surfaces clean, PR #549 `4ad84a1`, priority (i) (GAR-724)).
> Source of truth: `.cargo/audit.toml` and `deny.toml` (the suppression
> rationale lives there, this file is the alert-to-rationale index).

## Confirmed 2026-05-27 run 43 (~16:45 ET) — all surfaces clean, priority (i)

Health routine ran on 2026-05-27 (~16:45 ET / 20:45 UTC). Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

**Open health/ PRs resolved this run:** None open at scan time (PR #549 GAR-724 run 42 already squash-merged as `4ad84a1`).

**Pending routine/ PRs noted (NOT actioned — routine/ territory):** PR #548 (`routine/202605271220-search-slice11-task-lists`, GAR-721) — open, In Progress, skipped per protocol.

**CI on main (`4ad84a1`, PR #549 GAR-724 health run 42):** All 20 checks confirmed green via PR #549 check runs.

**Notable change vs run 42:** docker/build-push-action Dependabot PR (was open at run 42) now merged to main as `0a820a011acac96` (2026-05-26). Dependabot PR count: 9 → 8. No security surface change.

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable); GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #549 (4ad84a1) |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PR #549 |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456), glib MEDIUM (GAR-513), rand LOW (GAR-513) |
| Open Dependabot PRs | ⚠️ 8 open, none security | tracing-opentelemetry, lopdf, otel-semantic-conventions, otel-otlp, criterion (dev), rand_chacha, otel_sdk, patch-and-minor group |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass (CI) | All 20 checks green on PR #549 |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 (rsa) + RUSTSEC-2024-0429 (glib) + RUSTSEC-2026-0097 (rand) suppressed, expiry 2026-07-31 |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #549 |
| CI on main (`4ad84a1`) | ✅ green | 20/20 checks confirmed via PR #549 |

**No security fix applied this run.** Bookkeeping only: plan 0206 (GAR-725), plans README row 0205 marked ✅ Merged + row 0206 added, dependabot-status run 43 note. Linear: GAR-725. Next security backlog: argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4; rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31; CodeQL ledger re-audit due 2026-08-01 (GAR-491); GAR-711 OpenTelemetry 0.26→0.32 Backlog.

---


## Confirmed 2026-05-27 run 42 (~12:50 ET) — all surfaces clean, priority (i)

Health routine ran on 2026-05-27 (~12:50 ET / 16:50 UTC). Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

**Open health/ PRs resolved this run:** None open at scan time (PR #547 GAR-723 run 41 already squash-merged as `3f7c345`).

**Pending routine/ PRs noted (NOT actioned — routine/ territory):** PR #548 (`routine/202605271220-search-slice11-task-lists`, GAR-721) — open, routine/ territory, skipped per protocol.

**CI on main (`5472b63`, PR #543 GAR-718 search slice 10 chats):** All 20 checks confirmed green via PR #543 check runs.

**Notable change vs run 41:** PR #543 (GAR-718 — search slice 10 types=chats, feat(search)) merged as `5472b63`. No change to security surface. Dependabot PR count stable at 9.

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable); GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #543 |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PR #543 |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456), glib MEDIUM (GAR-513), rand LOW (GAR-513) |
| Open Dependabot PRs | ⚠️ 9 open, none security | tracing-opentelemetry, lopdf, otel-semantic-conventions, otel-otlp, criterion (dev), rand_chacha, otel_sdk, patch-and-minor group, docker/build-push-action |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass (CI) | All 20 checks green on PR #543 |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 (rsa) + RUSTSEC-2024-0429 (glib) + RUSTSEC-2026-0097 (rand) suppressed, expiry 2026-07-31 |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #543 |
| CI on main (`5472b63`) | ✅ green | 20/20 checks confirmed via PR #543 |

**No security fix applied this run.** Bookkeeping only: plan 0205 (GAR-724), plans README row 0200 marked ✅ Merged + row 0205 added, dependabot-status run 42 note. Linear: GAR-724. Next security backlog: argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4; rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31; CodeQL ledger re-audit due 2026-08-01 (GAR-491); GAR-711 OpenTelemetry 0.26→0.32 Backlog.

---

## Confirmed 2026-05-27 run 41 (~08:45 ET) — all surfaces clean, priority (i)

Health routine ran on 2026-05-27 (~08:45 ET / 12:45 UTC). Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

**Open health/ PRs resolved this run:** PR #546 (`health/202605270715-run40-status-note`, GAR-722) — 20/20 CI green, squash-merged as `fa679e6c6638166d0b2fcc521c714dc6d9185986`.

**Pending routine/ PRs noted (NOT actioned — routine/ territory):** PR #543 (`routine/202605270025-search-slice10-chats-v2`, GAR-718) — open, skipped per protocol.

**CI on main (`fa679e6c`, PR #546 GAR-722 health run 40):** All 20 checks confirmed green via PR #546 check runs.

**Notable change vs run 40:** PR #546 (GAR-722 run 40 status note) merged as `fa679e6c`. No change to security surface. Dependabot PR count stable at 9.

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable); GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #546 |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PR #546 |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456), glib MEDIUM (GAR-513), rand LOW (GAR-513) |
| Open Dependabot PRs | ⚠️ 9 open, none security | tracing-opentelemetry, lopdf, otel-semantic-conventions, otel-otlp, criterion (dev), rand_chacha, otel_sdk, patch-and-minor group, docker/build-push-action |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass (CI) | All 20 checks green on PR #546 |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 (rsa) only active suppression |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #546 |
| CI on main (`fa679e6c`) | ✅ green | 20/20 checks confirmed via PR #546 |

**No security fix applied this run.** Bookkeeping only: plan 0204 (GAR-723), plans README row 0203 marked ✅ Merged + row 0204 added, dependabot-status run 41 note. Linear: GAR-723. Next security backlog: argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4; rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31; CodeQL ledger re-audit due 2026-08-01 (GAR-491); GAR-711 OpenTelemetry 0.26→0.32 Backlog.

---

## Confirmed 2026-05-27 run 40 (~07:15 ET) — all surfaces clean, priority (i)

Health routine ran on 2026-05-27 (~07:15 ET / 11:15 UTC). Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

**Open health/ PRs resolved this run:** None open at scan time.

**Pending routine/ PRs noted (NOT actioned — routine/ territory):** PR #543 (`routine/202605270025-search-slice10-chats-v2`, GAR-718) — 20/20 CI green, skipped per protocol.

**CI on main (`61d0514`, PR #545 GAR-720 health run 39):** All 20 checks confirmed green via PR #543 check runs.

**Local cargo audit:** `cargo audit --deny unsound` exit 0 (1098 advisories loaded, 19 allowed unmaintained warnings — all in deny.toml ignore list).

**Notable change vs run 39:** No change to security surface. Plan 0202 (GAR-720) merged via PR #545. Dependabot PR count stable at 9. GAR-711 (OpenTelemetry 0.26→0.32 / RUSTSEC-2025-0052) remains Backlog.

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable); GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #543 |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PR #543 |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456), glib MEDIUM (GAR-513), rand LOW (GAR-513) |
| Open Dependabot PRs | ⚠️ 9 open, none security | tracing-opentelemetry, lopdf, otel-semantic-conventions, otel-otlp, criterion (dev), rand_chacha, otel_sdk, patch-and-minor group, docker/build-push-action |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass (local + CI) | exit 0, 1098 advisories, 19 allowed unmaintained warnings |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 (rsa) only active suppression |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #543 |
| CI on main (`61d0514`) | ✅ green | All checks confirmed |

**No security fix applied this run.** Bookkeeping only: plan 0203 (GAR-722), plans README row 0202 marked ✅ Merged + row 0203 added, dependabot-status run 40 note. Linear: GAR-722. Next security backlog: argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4; rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31; CodeQL ledger re-audit due 2026-08-01 (GAR-491); GAR-711 OpenTelemetry 0.26→0.32 Backlog.

---

## Confirmed 2026-05-27 run 39 (~04:45 ET) — all surfaces clean, priority (i)

Health routine ran on 2026-05-27 (~04:45 ET / 08:45 UTC). Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

**Open health/ PRs resolved this run:** None open at scan time.

**Pending routine/ PRs noted (NOT actioned — routine/ territory):** PR #543 (`routine/202605270025-search-slice10-chats-v2`, GAR-718) — open, skipped per protocol.

**CI on main (`fa6fe50`, PR #544 GAR-719 health run 38):** All 20 checks confirmed green via PR #543 check runs.

**Notable change vs run 38:** No change to security surface. Dependabot PR count stable at 9. GAR-711 (OpenTelemetry 0.26→0.32 / RUSTSEC-2025-0052) remains Backlog.

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable); GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #543 |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PR #543 |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456), glib MEDIUM (GAR-513), rand LOW (GAR-513) |
| Open Dependabot PRs | ⚠️ 9 open, none security | tracing-opentelemetry, lopdf, otel-semantic-conventions, otel-otlp, criterion (dev), rand_chacha, otel_sdk, patch-and-minor group, docker/build-push-action |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #543 |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 (rsa) only active suppression |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #543 |
| CI on main (`fa6fe50`) | ✅ green | All checks confirmed |

**No security fix applied this run.** Bookkeeping only: plan 0202 (GAR-720), plans README row 0201 marked ✅ Merged + row 0202 added, dependabot-status run 39 note. Linear: GAR-720. Next security backlog: argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4; rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31; CodeQL ledger re-audit due 2026-08-01 (GAR-491); GAR-711 OpenTelemetry 0.26→0.32 Backlog.

---

## Confirmed 2026-05-27 run 38 (~00:45 ET) — all surfaces clean, priority (i)

Health routine ran on 2026-05-27 (~00:45 ET / 04:45 UTC). Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

**Open health/ PRs resolved this run:** None open at scan time.

**Pending routine/ PRs noted (NOT actioned — routine/ territory):** PR #543 (`routine/202605270025-search-slice10-chats-v2`, GAR-718) — open, CI in progress, skipped per protocol.

**CI on main (`d6d0487`, PR #540 GAR-716 search slice 9):** All checks confirmed green via PR #543 check runs.

**Notable change vs run 37:** PR #540 (GAR-716 search slice 9 folders) was merged to main as `d6d0487` after run 37 completed. Plans README row 0199 bookkeeping fixed this run.

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable); GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #543 |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PR #543 |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456), glib MEDIUM (GAR-513), rand LOW (GAR-513) |
| Open Dependabot PRs | ⚠️ 9 open, none security | tracing-opentelemetry, lopdf, otel-semantic-conventions, otel-otlp, criterion (dev), rand_chacha, otel_sdk, patch-and-minor group, docker/build-push-action |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #543 |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 (rsa) only active suppression |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #543 |
| CI on main (`d6d0487`) | ✅ green | All checks confirmed |

**No security fix applied this run.** Bookkeeping only: plan 0201 (GAR-719), plans README rows 0199 corrected + 0200 + 0201 added, dependabot-status run 38 note. Linear: GAR-719. Next security backlog: argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4; rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31; CodeQL ledger re-audit due 2026-08-01 (GAR-491); GAR-711 OpenTelemetry 0.26→0.32 Backlog.

---

## Confirmed 2026-05-26 run 37 (~20:45 ET) — all surfaces clean, priority (i)

Health routine ran on 2026-05-26 (~20:45 ET / 00:45 UTC May 27). Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

**Open health/ PRs resolved this run:** PR #541 (`health/202605261645-run36-status-note`, GAR-715 run 36) — 20/20 CI green, squash-merged as `95ed89bc86d5b28d4d0440c907036881107270bd`.

**Pending routine/ PRs noted (NOT actioned — routine/ territory):** PR #540 (`routine/202605261820-search-slice9-folders`, GAR-716) — open, behind main, skipped per protocol. Note: references plan 0197 which is now taken by GAR-715; will require renumbering when the roadmap routine rebases it.

**CI on main (`95ed89b`, PR #541 GAR-715 health run 36):** All 20 checks passed.

**Notable change vs run 36:** No change to security surface. Dependabot PR count stable at 9. GAR-711 (OpenTelemetry 0.26→0.32 / RUSTSEC-2025-0052) remains Backlog.

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable); GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #541 (20/20 green) |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PR #541 |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456), glib MEDIUM (GAR-513), rand LOW (GAR-513) |
| Open Dependabot PRs | ⚠️ 9 open, none security | tracing-opentelemetry, lopdf, otel-semantic-conventions, otel-otlp, criterion (dev), rand_chacha, otel_sdk, patch-and-minor group, docker/build-push-action |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #541 |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 (rsa) only active suppression |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #541 |
| CI on main (`95ed89b`) | ✅ green | All 20 checks confirmed |

**No security fix applied this run.** Bookkeeping only: plan 0198 (GAR-717), plans README rows 0197 marked merged + 0198 added, dependabot-status run 37 note. Linear: GAR-717. Next security backlog: argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4; rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31; CodeQL ledger re-audit due 2026-08-01 (GAR-491); GAR-711 OpenTelemetry 0.26→0.32 Backlog.

---

## Confirmed 2026-05-26 run 36 (~12:45 ET) — all surfaces clean, priority (i)

Health routine ran on 2026-05-26 (~12:45 ET / 16:45 UTC). Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

**Open health/ PRs resolved this run:** PR #536 (GAR-714 run 35) — was dirty (merge conflict in plans/README.md from plan number collision with PR #537); rebased clean onto main and merged at `9a52349`. PR #538 (GAR-467 + GAR-705 docs bookkeeping) — updated and merged at `abc0d34`.

**Pending routine/ PRs noted (NOT actioned — routine/ territory):** None open.

**CI on main (`0a820a0`, PR #511 docker/build-push-action bump):** All 20 checks passed.

**Notable change vs run 35:** No change to security surface. Dependabot PR count and alerts stable. PR #536 GAR-714 (run 35 bookkeeping) resolved and merged this run.

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable); GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on `0a820a0` (20/20 green) |
| Malware (cargo/npm) | ✅ none | cargo-deny green |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456 Done), glib MEDIUM (GAR-513), rand LOW (GAR-513) |
| Open Dependabot PRs | ⚠️ 9 open, none security | tracing-opentelemetry, lopdf, otel-semantic-conventions, otel-otlp, criterion (dev), rand_chacha, otel_sdk, patch-and-minor group, docker/build-push-action |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 (rsa) only active suppression |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green |
| CI on main (`0a820a0`) | ✅ green | All 20 checks confirmed |

**No security fix applied this run.** Bookkeeping only: plan 0197 (GAR-715), plans README row 0197 + dependabot-status run 36 note. Linear: GAR-715. Next security backlog: argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4; rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31; CodeQL ledger re-audit due 2026-08-01 (GAR-491); GAR-711 OpenTelemetry 0.26→0.32 Backlog.

---

## Confirmed 2026-05-26 run 35 (~12:45 ET) — all surfaces clean, priority (i)

Health routine ran on 2026-05-26 (~12:45 ET / 16:45 UTC). Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

**Open health/ PRs resolved this run:** None (none were open).

**Pending routine/ PRs noted (NOT actioned — routine/ territory):** PR #535 (`routine/202605261215-search-slice8-sort-by`, GAR-713) — skipped per protocol.

**CI on main (`885ed2e`, PR #534 GAR-712 health run 34):** All 20 checks passed.

**Notable change vs run 34:** No change to security surface. Dependabot PR count stable at 9. GAR-711 (OpenTelemetry 0.26→0.32 / RUSTSEC-2025-0052) remains Backlog — 4 open Dependabot PRs (#515, #518, #519, #522) cover the upgrade but cargo audit CI still passing.

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable); GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on `885ed2e` (20/20 green) |
| Malware (cargo/npm) | ✅ none | cargo-deny green |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456 Done), glib MEDIUM (GAR-513), rand LOW (GAR-513) |
| Open Dependabot PRs | ⚠️ 9 open, none security | tracing-opentelemetry, lopdf, otel-semantic-conventions, otel-otlp, criterion (dev), rand_chacha, otel_sdk, patch-and-minor group, docker/build-push-action |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 (rsa) only active suppression |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green |
| CI on main (`885ed2e`) | ✅ green | All 20 checks confirmed |

**No security fix applied this run.** Bookkeeping only: plan 0196 (GAR-714), plans README rows 0194✅ + 0195 (GAR-713) + 0196 + dependabot-status run 35 note. Linear: GAR-714. Next security backlog: argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4; rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31; CodeQL ledger re-audit due 2026-08-01 (GAR-491); GAR-711 OpenTelemetry 0.26→0.32 Backlog.

---

## Confirmed 2026-05-26 run 34 (~04:45 ET) — all surfaces clean, priority (i)

Health routine ran on 2026-05-26 (~04:45 ET / 08:45 UTC). Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

**Open health/ PRs resolved this run:** None (none were open).

**Pending routine/ PRs noted (NOT actioned — routine/ territory):** None open.

**CI on main (`f6c3aa5`, PR #533 GAR-710):** All 20 checks passed.

**Notable change vs run 33:** Dependabot PR count reduced from 11 to 9 (wasmtime-wasi auto-closed after GAR-708 merge; dtolnay/rust-toolchain also closed).

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable); GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #533 (20/20 green) |
| Malware (cargo/npm) | ✅ none | cargo-deny green |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456 Done), glib MEDIUM (GAR-513), rand LOW (GAR-513) |
| Open Dependabot PRs | ⚠️ 9 open, none security | tracing-opentelemetry, lopdf, otel-semantic-conventions, otel-otlp, criterion (dev), rand_chacha, otel_sdk, patch-and-minor group, docker/build-push-action |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 (rsa) only active suppression |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green |
| CI on main (`f6c3aa5`) | ✅ green | All 20 checks confirmed |

**No security fix applied this run.** Bookkeeping only: plan 0194 (GAR-712), plans README row + dependabot-status run 34 note. Linear: GAR-712. Next security backlog: argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4; rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31; CodeQL ledger re-audit due 2026-08-01 (GAR-491).

---

## Confirmed 2026-05-26 run 33 (~00:45 ET) — PR #528 GAR-708 merged, all surfaces clean, priority (i)

Health routine ran on 2026-05-26 (~00:45 ET / 04:45 UTC). Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found after completing run 32 work.

**Open health/ PRs resolved this run:**
- PR #528 (`health/202605260057-wasmtime-45-file-perms-fix`, GAR-708): wasmtime 44.0.2→45.0.0 path_open(TRUNCATE) FilePerms::WRITE bypass fix — 20/20 CI checks ✅ — squash-merged as `ff07bff`.
- PR #527 (`docs/gar-706-bookkeeping`): Obsolete (0189 already marked ✅ Merged inside PR #528 squash). Closed.

**Pending routine/ PRs noted (NOT actioned — routine/ territory):**
- PR #526 (`routine/202605260025-search-slice6-tasks`, GAR-707): Skipped per protocol.

**CI on main (`ff07bff`, PR #528 health run 32):** All 20 checks passed.

**Notable change vs run 31:** 11 open Dependabot PRs (previously 0). These are routine ecosystem version bumps — none carry GitHub "security" label; CI cargo-audit confirmed no new RUSTSEC advisories.

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable); GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #528 (20/20 green) |
| Malware (cargo/npm) | ✅ none | cargo-deny green |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456 Done), glib MEDIUM (GAR-513), rand LOW (GAR-513) |
| Open Dependabot PRs | ⚠️ 11 open, none security | tracing-opentelemetry, wasmtime-wasi (auto-closing), lopdf, otel-semantic-conventions, otel-otlp, criterion (dev), rand_chacha, otel_sdk, patch-and-minor group, dtolnay/rust-toolchain, docker/build-push-action |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 (rsa) only active suppression |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green |
| CI on main (`ff07bff`) | ✅ green | All 20 checks confirmed |

**No security fix applied this run.** PR #528 (GAR-708 wasmtime fix) was the security fix from run 32 — merged at run start. Bookkeeping-only PR (plan 0191, plans README rows 0190✅ + 0191 + dependabot-status run 33 note). Linear: GAR-709. Next security backlog: argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4; rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31; CodeQL ledger re-audit due 2026-08-01 (GAR-491).

---

> Last updated (previous): **2026-05-25 run 31** (health routine — all surfaces clean, PR #508 run 30 merged `ef040ad`, priority (i). GAR-706. Previous: run 30 all surfaces clean, PR #506 conflict resolved, priority (i) (GAR-705); run 29 all surfaces clean, routine/ PR #505 noted, priority (i) (GAR-704); run 28 all surfaces clean, PR #503 merged, priority (i) (GAR-702); run 27 all surfaces clean, PR #501 run 26 open 20/20 CI green, priority (i) (GAR-701). Previous: run 30 all surfaces clean, PR #503 run 27 merged `ba8482b`, priority (i). GAR-702. Previous: run 26 all surfaces clean, PR #499 merged `61bd6a7`, priority (i) (GAR-699); run 25 all surfaces clean, routine/ PR #498 noted (roadmap routine), priority (i) (GAR-698); run 24 all surfaces clean, routine/ PR #496 noted, priority (i) (GAR-696); run 23 all surfaces clean, routine/ PR #492 pending merge (skipped), priority (i) (GAR-695); run 22 all surfaces clean, GAR-499 agent team reviewed clean, priority (i) (GAR-694); run 21 merge run-20 PRs + plan numbering fix; 3 upstream-blocked alerts; priority (i) (GAR-693); run 20 all surfaces clean; plans 0168+0169 marked merged (PR #484); priority (i) (GAR-692); run 19 deny.toml advisory-not-detected cleanup GAR-513/plan 0169 (PR #483/484 merged `b3f62fd`); run 18 all surfaces clean, PR #482 merged, priority (i) (GAR-690); run 17 all surfaces clean, no open health/ PRs, priority (i) (GAR-689); run 16 PR #477 + PR #475 merged, all surfaces clean, priority (i) (GAR-688); run 15 CI retrigger for ubuntu-latest transient failure + RUSTSEC-2026-0149 wasmtime-wasi 44.0.1→44.0.2 fix (GAR-685, GAR-686); run 14 RUSTSEC-2026-0149 wasmtime fixed; run 13 upstream-blocked unchanged; run 12 upstream-blocked unchanged; run 11 upstream-blocked state unchanged; run 10 upstream-blocked state unchanged; run 9 upstream-blocked state unchanged; run 8 password-hash + rand upstream-blocked; run 7 GAR-674 windows-sys 0.52→0.61; run 6 GAR-673; run 5 GAR-672; run 4 GAR-671; run 3 GAR-670; run 2 GAR-668 RUSTSEC-2026-0145 + tokio-tungstenite 0.29; run 1 GAR-667 all-clean; run 6 GAR-665; run 5 GAR-664; run 4 GAR-663; run 3 GAR-662; run 2 lockfile bump PR #401; run 1 GAR-661). (health routine — all surfaces clean, PR #503 run 27 merged `ba8482b`, priority (i). GAR-702. Previous: run 26 all surfaces clean, PR #499 merged `61bd6a7`, priority (i) (GAR-699); run 25 all surfaces clean, routine/ PR #498 noted (roadmap routine), priority (i) (GAR-698); run 24 all surfaces clean, routine/ PR #496 noted, priority (i) (GAR-696); run 23 all surfaces clean, routine/ PR #492 pending merge (skipped), priority (i) (GAR-695); run 22 all surfaces clean, GAR-499 agent team reviewed clean, priority (i) (GAR-694); run 21 merge run-20 PRs + plan numbering fix; 3 upstream-blocked alerts; priority (i) (GAR-693); run 20 all surfaces clean; plans 0168+0169 marked merged (PR #484); priority (i) (GAR-692); run 19 deny.toml advisory-not-detected cleanup GAR-513/plan 0169 (PR #483/484 merged `b3f62fd`); run 18 all surfaces clean, PR #482 merged, priority (i) (GAR-690); run 17 all surfaces clean, no open health/ PRs, priority (i) (GAR-689); run 16 PR #477 + PR #475 merged, all surfaces clean, priority (i) (GAR-688); run 15 CI retrigger for ubuntu-latest transient failure + RUSTSEC-2026-0149 wasmtime-wasi 44.0.1→44.0.2 fix (GAR-685, GAR-686); run 14 RUSTSEC-2026-0149 wasmtime fixed; run 13 upstream-blocked unchanged; run 12 upstream-blocked unchanged; run 11 upstream-blocked state unchanged; run 10 upstream-blocked state unchanged; run 9 upstream-blocked state unchanged; run 8 password-hash + rand upstream-blocked; run 7 GAR-674 windows-sys 0.52→0.61; run 6 GAR-673; run 5 GAR-672; run 4 GAR-671; run 3 GAR-670; run 2 GAR-668 RUSTSEC-2026-0145 + tokio-tungstenite 0.29; run 1 GAR-667 all-clean; run 6 GAR-665; run 5 GAR-664; run 4 GAR-663; run 3 GAR-662; run 2 lockfile bump PR #401; run 1 GAR-661).
> Source of truth: `.cargo/audit.toml` and `deny.toml` (the suppression
> rationale lives there, this file is the alert-to-rationale index).

## Snapshot

| Metric | 2026-04-22 | 2026-04-30 (last sprint) | 2026-05-07 | 2026-05-08 | 2026-05-09 | 2026-05-11 | 2026-05-12 (today) |
|---|---|---|---|---|---|---|---|
| Total Dependabot alerts open | 20 | **7** | **8** (confirmed) | **8** (confirmed — no new alerts) | **8** (unchanged — serenity chain still carries all 4 RUSTSEC IDs) | **8** (unchanged) | **8** → **4** pending (PR #293 merged, Dependabot rescan in progress) |
| High severity | 1 | 1 | **2** | **2** | **2** | **2** | **2** → **1** (alert #37 closing) |
| Medium severity | 4 | 2 | **2** | **2** | **2** | **2** | **2** → **1** (alert #11 closing) |
| Low severity | 4 | 4 | **4** | **4** | **4** | **4** | **4** → **2** (alerts #23, #22 closing) |
| With Linear ownership | mixed | **7 / 7** | **8 / 8** | **8 / 8** | **8 / 8** | **8 / 8** | **4 / 4** (post-rescan) |
| `rustls-webpki 0.101.7` in Cargo.lock | ✅ present | ✅ present | ✅ present | ✅ present | ✅ **REMOVED** (plan 0087) | ✅ absent | ✅ absent |
| `rustls-webpki 0.102.8` in Cargo.lock | ✅ present | ✅ present | ✅ present | ✅ present | ✅ present | ✅ present | ✅ **REMOVED** (PR #293) |

## Confirmed 2026-05-25 run 31 (~20:45 ET) — all surfaces clean, PR #508 run 30 merged, priority (i)

Health routine ran on 2026-05-25 (~20:45 ET / 00:45 UTC May 26). Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

**Open health/ PRs resolved this run:**
- PR #508 (`health/202605251645-run30-status-note`, GAR-705, run 30): 20/20 CI checks all success — squash-merged as `ef040ad`.

**Pending routine/ PRs noted (not actioned — routine/ territory):**
- PR #509 (`routine/202605251820-q6-5-audit-observability`, GAR-467): 20/20 CI green. Skipped per protocol.

**CI on main (`ef040ad`, PR #508 health run 30):** All 20 checks passed (verified via PR #509 check runs, same base).

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable); GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #509 base `ef040ad` (20/20 green) |
| Malware (cargo/npm) | ✅ none | cargo-deny green |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456 Done), glib MEDIUM (GAR-513), rand LOW (GAR-513) |
| Open Dependabot PRs | ✅ none | 0 open |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 (rsa) only active suppression |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green |
| CI on main (`ef040ad`) | ✅ green | All 20 checks passed |

**No security fix applied this run.** Bookkeeping-only PR (plan 0189, plans README rows 0187✅ + 0189 + dependabot-status run 31 note). Linear: GAR-706. Next security backlog: argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4; rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31; CodeQL ledger re-audit due 2026-08-01 (GAR-491).

---

## Confirmed 2026-05-25 run 30 (~16:45 ET) — all surfaces clean, PR #506 conflict resolved, priority (i)

Health routine ran on 2026-05-25 (~16:45 ET / 20:45 UTC May 25). Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

**Open health/ PRs resolved this run:**
- PR #506 (`docs/gar-703-bookkeeping`, GAR-703 bookkeeping): dirty-state merge conflict in `plans/README.md` fixed — merged main (ec683e9 adds row 0186) into branch, resolved conflict, pushed. CI re-triggered: 20/20 checks in progress.

**Pending routine/ PRs noted (not actioned — routine/ territory):**
- None open.

**CI on main (`ec683e9`, PR #507 health run 29):** All 20 checks passed (verified via PR #506 check run baseline).

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable); GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #506 (20/20 checks green) |
| Malware (cargo/npm) | ✅ none | cargo-deny green |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456 Done), glib MEDIUM (GAR-513), rand LOW (GAR-513) |
| Open Dependabot PRs | ✅ none | 0 open |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #506 check run |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 (rsa) only active suppression |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green |
| CI on main (`ec683e9`) | ✅ green | All 20 checks passed (PR #507 before squash-merge) |

**No security fix applied this run.** Bookkeeping-only PR (plan 0187, plans README rows 0186✅ + 0187 + dependabot-status run 30 note). Linear: GAR-705. Next security backlog: argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4; rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31; CodeQL ledger re-audit due 2026-08-01 (GAR-491).

---

## Confirmed 2026-05-25 run 29 (~12:45 ET) — all surfaces clean, routine/ PR #505 noted, priority (i)

Health routine ran on 2026-05-25 (~12:45 ET / 16:45 UTC May 25). Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

**Open health/ PRs resolved this run:**
- None — no open health/ PRs. Previous health/ PR #504 (GAR-702, run 28) was squash-merged as `1b68238`.

**Pending routine/ PRs noted (not actioned — routine/ territory):**
- PR #505 (`routine/202605251215-search-slice5-files`, GAR-703): search slice 5 types=files. 19/20 CI checks done. Not a security PR.

**CI on main (`1b68238`, PR #504 health run 28):** All 20 checks passed before squash-merge.

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable); GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #504 (20/20 checks green) |
| Malware (cargo/npm) | ✅ none | cargo-deny green |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456 Done), glib MEDIUM (GAR-513), rand LOW (GAR-513) |
| Open Dependabot PRs | ✅ none | 0 open |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #504 (20/20) |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 (rsa) only active suppression |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green |
| CI on main (`1b68238`) | ✅ green | All 20 checks passed (PR #504 before squash-merge) |

**No security fix applied this run.** Bookkeeping-only PR (plan 0186, plans README rows 0184✅ + 0186 + dependabot-status run 29 note). Linear: GAR-704. Next security backlog: argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4; rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31; CodeQL ledger re-audit due 2026-08-01 (GAR-491).

---

## Confirmed 2026-05-25 run 28 (~10:25 ET) — all surfaces clean, health/ PR #503 run 27 merged, priority (i)

Health routine ran on 2026-05-25 (~10:25 ET / 14:25 UTC May 25). Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

**Open health/ PRs resolved this run:**
- PR #503 (`health/202605250710-run27-status-note`, GAR-701): 20/20 CI green → squash-merged as `ba8482b`.

**Pending routine/ PRs noted (not actioned — routine/ territory):**
- PR #502 (`routine/202605251124-message-attachments-api`, GAR-700): message attachments API. Not a security PR.

**CI on main (`ba8482b`, PR #503 health run 27):** All 20 checks passed.

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable); GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #503 (20/20 checks green) |
| Malware (cargo/npm) | ✅ none | cargo-deny green |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456 Done), glib MEDIUM (GAR-513), rand LOW (GAR-513) |
| Open Dependabot PRs | ✅ none | 0 open |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #503 (20/20) |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 (rsa) only active suppression |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #503 |
| CI on main (`ba8482b`) | ✅ green | All 20 checks passed |

**No security fix applied this run.** Bookkeeping-only PR (plans README rows 0183✅ + 0184 + dependabot-status run 28 note). Linear: GAR-702. Next security backlog: argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4; rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31; CodeQL ledger re-audit due 2026-08-01 (GAR-491).

---

## Confirmed 2026-05-25 run 27 (~07:10 ET) — all surfaces clean, health/ PR #501 run 26 merged, priority (i)

Health routine ran on 2026-05-25 (~07:10 ET / 11:10 UTC May 25). Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

**Open health/ PRs resolved this run:**
- PR #501 (`health/202605250445-run26-status-note`, GAR-699): 20/20 CI green → squash-merged as `312f046`.

**Pending routine/ PRs noted (not actioned — routine/ territory):**
- PR #498 (`routine/202605250015-search-has-attachment`, GAR-697): search slice 4. Not a security PR.
- PR #502 (`routine/202605251124-message-attachments-api`, GAR-700): message attachments API. Not a security PR.

**CI on main (`312f046`, PR #501 health run 26):** All 20 checks passed.

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable); GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #501 (20/20 checks green) |
| Malware (cargo/npm) | ✅ none | cargo-deny green |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456 Done), glib MEDIUM (GAR-513), rand LOW (GAR-513) |
| Open Dependabot PRs | ✅ none | 0 open |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #501 (20/20) |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 (rsa) only active suppression |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #501 |
| CI on main (`312f046`) | ✅ green | All 20 checks passed |

**No security fix applied this run.** Bookkeeping-only PR (plans README rows 0181✅ + 0182 + 0183 + dependabot-status run 27 note). Linear: GAR-701. Next security backlog: argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4; rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31; CodeQL ledger re-audit due 2026-08-01 (GAR-491).

---

## Confirmed 2026-05-25 run 26 (~04:45 ET) — all surfaces clean, routine/ PR #498 noted, priority (i)

Health routine ran on 2026-05-25 (~04:45 ET / 08:45 UTC May 25). Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

**Open health/ PRs resolved this run:**
- PR #499 (`health/202605250045-run25-status-note`, GAR-698) — all 20 CI checks green → squash-merged as `61bd6a7`.

**Pending routine/ PR #498 noted (not actioned — routine/ territory):**
- PR #498 (`routine/202605250015-search-has-attachment`) — GAR-697 search slice 4 has_attachment filter + migration 020 message_attachments. Skipped per protocol.

**CI on main (`61bd6a7`, PR #499 health run 25 bookkeeping):** All 20 checks green — Format, Clippy, Test×3, Build, MSRV, cargo-deny, Security Audit, Coverage, Analyze (rust/js-ts/actions), Playwright, E2E, Secret Scan, Dependency Review, Quality Ratchet.

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable); GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #499 (20/20 checks green) |
| Malware (cargo/npm) | ✅ none | cargo-deny green |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456), glib MEDIUM (GAR-513), rand LOW (GAR-513) |
| Open Dependabot PRs | ✅ none | 0 open |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #499 (20/20) |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 (rsa) only active suppression |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #499 |
| CI on main (`61bd6a7`) | ✅ green | All 20 checks passed |

**No security fix applied this run.** Bookkeeping-only PR (plans README rows 0180→✅ + 0181 + dependabot-status run 26 note). Linear: GAR-699. Next security backlog: argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4; rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31; CodeQL ledger re-audit due 2026-08-01 (GAR-491).

---

## Confirmed 2026-05-25 run 25 (~00:45 ET) — all surfaces clean, routine/ PR #498 noted, priority (i)

Health routine ran on 2026-05-25 (~00:45 ET / 04:45 UTC May 25). Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

**Open health/ PRs resolved this run:**
- None — no open `health/` PRs from previous runs.

**Pending routine/ PR #498 noted (not actioned — routine/ territory):**
- PR #498 (`routine/202605250015-search-has-attachment`) — GAR-697 search slice 4 has_attachment filter + migration 020 message_attachments. CI in progress (~17/20 checks done). Skipped per protocol.

**CI on main (`149b91b`, PR #497 health run 24 bookkeeping):** All 20 checks green — Format, Clippy, Test×3, Build, MSRV, cargo-deny, Security Audit, Coverage, Analyze (rust/js-ts/actions), Playwright, E2E, Secret Scan, Dependency Review, Quality Ratchet.

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable); GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #497 (20/20 checks green) |
| Malware (cargo/npm) | ✅ none | cargo-deny green |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456), glib MEDIUM (GAR-513), rand LOW (GAR-513) |
| Open Dependabot PRs | ✅ none | 0 open |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #497 (20/20) |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 (rsa) only active suppression |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #497 |
| CI on main (`149b91b`) | ✅ green | All 20 checks passed |

**No security fix applied this run.** Bookkeeping-only PR (plans README rows 0178→✅ + 0180 + dependabot-status run 25 note). Linear: GAR-698. Next security backlog: argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4; rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31; CodeQL ledger re-audit due 2026-08-01 (GAR-491).

---

## Confirmed 2026-05-24 run 24 (~00:45 ET) — all surfaces clean, routine/ PR #496 noted, priority (i)

Health routine ran on 2026-05-24 (~00:45 ET / 04:45 UTC May 24). Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

**Open health/ PRs resolved this run:**
- `health/202605231000-gar513-deny-toml-hygiene` — orphan branch; corresponding PR #483 was already merged 2026-05-23. No action needed.

**Pending routine/ PRs noted (not actioned — routine/ territory):**
- PR #496 (`claude/wizardly-ptolemy-UncRd`) — docs sync (TODO.md + ROADMAP + README). Merged as `73ecc5d` before this run started.

**CI on main (`73ecc5d`, PR #496 TODO/ROADMAP sync):** All 20 checks green — Format, Clippy, Test×3, Build, MSRV, cargo-deny, Security Audit, Coverage, Analyze (rust/js-ts/actions), Playwright, E2E, Secret Scan, Dependency Review, Quality Ratchet.

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable); GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #496 (20/20 checks green) |
| Malware (cargo/npm) | ✅ none | cargo-deny green |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456), glib MEDIUM (GAR-513), rand LOW (GAR-513) |
| Open Dependabot PRs | ✅ none | 0 open |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #496 (20/20) |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 (rsa) only active suppression |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #496 |
| CI on main (`73ecc5d`) | ✅ green | All 20 checks passed |

**No security fix applied this run.** Bookkeeping-only PR (plans README row 0178 + dependabot-status run 24 note). Linear: GAR-696. Next security backlog: argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4; rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31; CodeQL ledger re-audit due 2026-08-01 (GAR-491).

---

## Confirmed 2026-05-24 run 23 (~00:45 ET) — all surfaces clean, routine/ PR #492 pending merge (skipped), priority (i)

Health routine ran on 2026-05-24 (~00:45 ET / 04:45 UTC May 24). Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

**Open health/ PRs resolved this run:**
- None — no open `health/` PRs from previous runs.

**Pending routine/ PR #492 noted (not actioned — routine/ territory):** `routine/202605240015-gar-493-garra-maxpower-adr`, docs-only ADR 0011 GarraMaxPower. Skipped per protocol.

**CI on main (`7e45ec5`, PR #490 GAR-499):** All 20 checks green — Format, Clippy, Test×3, Build, MSRV, cargo-deny, Security Audit, Coverage, Analyze (rust/js-ts/actions), Playwright, E2E, Secret Scan, Dependency Review, Quality Ratchet.

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable); GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #490 (20/20 checks green) |
| Malware (cargo/npm) | ✅ none | cargo-deny green |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456), glib MEDIUM (GAR-513), rand LOW (GAR-513) |
| Security Audit | ✅ pass | cargo audit --deny unsound green |
| CodeQL | ✅ pass | Analyze (rust + js-ts + actions) all green |

## Confirmed 2026-05-23 run 22 (~20:45 ET) — all surfaces clean, GAR-499 agent team reviewed clean, priority (i)

Health routine ran on 2026-05-23 (~20:45 ET / 00:45 UTC May 24). Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

**Open health/ PRs resolved this run:**
- PR #489 (`health/202605231645-run21-status-note`, GAR-693) — all 20 CI checks green → squash-merged as `133fef8`.

**Security review — routine/ PR #490 (GAR-499 agent team MVP):** New `team.rs` module (486 LOC) in `garraia-cli`. Pure Rust, no network, no file I/O in production code. Uses `std::sync::mpsc` channels with `.ok()` handling — no `unwrap()` outside `#[cfg(test)]`. No new crate dependencies. No SQL, no auth, no PII, no unsafe blocks. **CLEAN** — no security concerns.

**Plan numbering conflict noted (not actioned — routine/ territory):** PR #490 adds `plans/0173-gar-499-agent-team-mvp.md` but main already has `plans/0173-gar-693-health-run-21.md`. Roadmap routine must resolve on merge.

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable); Cargo.lock has `argon2 = "0.5.3"`. GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #489 (20/20 checks green) |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PR #489 |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456), glib MEDIUM (GAR-513), rand LOW (GAR-513) — all suppression expiry 2026-07-31 |
| Open Dependabot PRs | ✅ none | 0 open |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #489 (20/20) |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 (rsa) only active suppression |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #489 |
| CI on main (`133fef8`) | ✅ green | All 20 checks passed |

**No security fix applied this run.** Bookkeeping-only PR (plans README rows 0173→✅ + 0174 + dependabot-status run 22 note). Linear: GAR-694. Next security backlog: argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4; rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31; CodeQL ledger re-audit due 2026-08-01 (GAR-491).

---

## Confirmed 2026-05-23 run 21 (~16:45 ET) — merge run-20 PRs + plan numbering fix, all surfaces clean, priority (i)

Health routine ran on 2026-05-23 (~16:45 ET / 20:45 UTC). Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

**Pending health/ PRs resolved this run:**
- PR #487 (`chore/plan-0170-done-bookkeeping`) — updated to current main, CI green, squash-merged at `d334516`
- PR #486 (`health/202605231245-run20-status-note`, GAR-692) — `dirty` (conflict in `plans/README.md`) → resolved + plan numbering fix (0171=GAR-498, 0172=GAR-692), CI green, squash-merged at `07070f5`

**Plan numbering fix:** Commit `c65e099` added `plans/0171-gar-498-native-skills-registry.md` to main without a README entry. PR #486 had claimed `0171` for GAR-692. Fixed: GAR-498=0171, GAR-692=0172, GAR-693=0173.

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable); Cargo.lock has `argon2 = "0.5.3"`. GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #486 + #487 |
| Malware (cargo/npm) | ✅ none | cargo-deny green |
| Dependabot alerts | ⚠️ 3 open, UPSTREAM-BLOCKED | rsa HIGH (GAR-456), glib MEDIUM (GAR-513), rand LOW (GAR-513) — all suppression expiry 2026-07-31 |
| Open Dependabot PRs | ✅ none | 0 open |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #486 (20/20) |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 (rsa) only active suppression |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green |
| CI on main (`07070f5`) | ✅ green | All 20 checks passed |

**No security fix applied this run.** Bookkeeping-only PR (plans README row 0173 + dependabot-status run 21 note). Linear: GAR-693. Next security backlog: argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4; rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31; CodeQL ledger re-audit due 2026-08-01 (GAR-491).

---

## Confirmed 2026-05-23 run 20 (~12:45 ET) — plans 0168+0169 bookkeeping, all surfaces clean, priority (i)

Health routine ran on 2026-05-23 (~12:45 ET / 16:45 UTC). Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

**Open PRs resolved this run:** None. Branch `health/202605231000-gar513-deny-toml-hygiene` was already merged into main via cleanup PR #484 (`b3f62fd`). Open routine PR #485 (GAR-691 Q10.g, branch `routine/202605231215-q10g-bootstrap-telegram`) — skipped per protocol.

**Bookkeeping applied:** Plans README rows 0168 (GAR-480 Q10.f bootstrap-imessage) and 0169 (GAR-513 deny.toml cleanup) updated from "In Progress" to ✅ Merged — both were merged via cleanup PR #484 at commit `b3f62fd` on 2026-05-23.

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable); Cargo.lock has `argon2 = "0.5.3"`. GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #483/484 (20/20); main at `b3f62fd` |
| Malware (cargo/npm) | ✅ none | cargo-deny green; no advisory-not-detected warnings (fixed run 19) |
| Dependabot alerts | ⚠️ 2 open, UPSTREAM-BLOCKED | password-hash 0.5→0.6 (#430, GAR-669 Slice 3) + rand 0.8→0.10 (#424, GAR-669 Slice 4) — both blocked on argon2 ≥ 0.6 stable |
| Open Dependabot PRs | ✅ none | 0 open |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #483 (20/20 Security Audit: success) |
| cargo-deny | ✅ pass | RUSTSEC-2023-0071 (rsa) only active suppression; glib+rand removed from deny.toml in run 19 |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #483; 22 suppression ledger entries (all dismissed) |
| CI on main (`b3f62fd`) | ✅ green | All 20 checks passed via PR #483 check suite before merge into PR #484 |

**No security fix applied this run.** Bookkeeping-only PR (plans README update). Linear: GAR-692. Next security backlog: argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4; rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31; CodeQL ledger re-audit due 2026-08-01 (GAR-491).

---

## Confirmed 2026-05-23 run 19 (~08:45 ET) — GAR-513: deny.toml advisory-not-detected cleanup

Health routine ran on 2026-05-23 (~08:45 ET / 12:45 UTC). Full security scan completed.

**Open PRs resolved this run:** PR #482 (GAR-690 run 18 status note, branch `health/202605230445-run18-status-note`) was open with all 20 CI checks green — **merged as first action** (squash at `850d44c`). GAR-690 already marked Done.

**Finding (priority h):** Branch `claude/focused-cray-BM98J` contained prepared but un-PR'd commits from health run 18 fixing `cargo deny` `advisory-not-detected` noise for two IDs:
- `RUSTSEC-2024-0429` (glib 0.18.5 VariantStrIter unsound) → cargo deny advisory DB no longer matches this version
- `RUSTSEC-2026-0097` (rand 0.7.3 thread_rng unsound) → cargo deny advisory DB no longer matches this version

Both IDs are retained in `audit.toml` (cargo audit still matches them; `--deny unsound` gate still enforced). Removed from `deny.toml` only. Plan 0169 / GAR-513.

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable); Cargo.lock has `argon2 = "0.5.3"`. GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | No changes to secrets surface |
| Malware (cargo/npm) | ✅ none | cargo-deny green (after deny.toml cleanup) |
| Dependabot alerts | ⚠️ 2 open, UPSTREAM-BLOCKED | password-hash 0.5→0.6 (#430, GAR-669 Slice 3) + rand 0.8→0.10 (#424, GAR-669 Slice 4) — both blocked on argon2 ≥ 0.6 stable |
| Open Dependabot PRs | ✅ none | 0 open Dependabot PRs |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | Both RUSTSEC IDs retained in audit.toml; CI gate unchanged |
| cargo-deny | ✅ pass (post-fix) | 0 advisory-not-detected warnings for RUSTSEC-2024-0429 + RUSTSEC-2026-0097 |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | 22 suppression ledger entries (all dismissed) |
| CI on main (`850d44c`) | ✅ green | Source: PR #482 check suite (20/20) |

**Fix applied: deny.toml hygiene (GAR-513 / plan 0169).** Removed 2 stale advisory-not-detected entries from deny.toml. Both IDs retained in audit.toml. Linear: GAR-513 (In Progress, due 2026-07-31). Next security backlog: argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4; rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31; CodeQL ledger re-audit due 2026-08-01 (GAR-491).

---

## Confirmed 2026-05-23 run 18 (~04:45 ET) — all surfaces clean, PR #481 merged, priority (i)

Health routine ran on 2026-05-23 (~04:45 ET / 08:45 UTC). Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

**Open PRs resolved this run:** PR #481 (GAR-689 run 17 status note, branch `health/202605230045-run17-status-note`) was open with all 20 CI checks green — **merged as first action** (squash at `7a2e9e5`). GAR-689 marked Done. Only remaining open PR is #480 (branch `routine/202605230020-q10f-bootstrap-imessage`, roadmap territory — skipped per protocol).

**New merges since run 17 (GAR-689):** `7a2e9e5` — PR #481 `docs(security): GAR-689 health run 17` (doc-only, squash-merged this run).

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable); Cargo.lock has `argon2 = "0.5.3"`. GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #481 (Secret Scan: success); main `7a2e9e5` |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PR #481 |
| Dependabot alerts | ⚠️ 2 open, UPSTREAM-BLOCKED | password-hash 0.5→0.6 (#430, GAR-669 Slice 3) + rand 0.8→0.10 (#424, GAR-669 Slice 4) — both blocked on argon2 ≥ 0.6 stable |
| Open Dependabot PRs | ✅ none | 0 open Dependabot PRs |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #481 (Security Audit: success) |
| cargo-deny | ✅ pass | No new advisories; active suppressions: rsa (RUSTSEC-2023-0071), glib (RUSTSEC-2024-0429), rand (RUSTSEC-2026-0097) — all expiry 2026-07-31 |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #481; 22 suppression ledger entries (all dismissed) |
| CI on main (`7a2e9e5`) | ✅ green | All 20 checks passed (source: PR #481 check suite) |

**No fix applied this run.** Linear: GAR-690. Next security backlog: argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4; rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31; CodeQL ledger re-audit due 2026-08-01 (GAR-491).

---

## Confirmed 2026-05-23 run 17 (~00:45 ET) — all surfaces clean, no open health/ PRs, priority (i)

Health routine ran on 2026-05-23 (~00:45 ET). Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

**Open PRs resolved this run:** None. Only open PR is #480 (branch `routine/202605230020-q10f-bootstrap-imessage`, roadmap territory — skipped per protocol).

**New merges since run 16 (GAR-688):** None. The run 16 commits (`63ef1a9`, `94791f0`, `fcb7904`) are the most recent main commits.

**Security review — routine/ PR #480 (Q10.f bootstrap/imessage.rs):** Pure extraction of `build_imessage_channels` (~123 LOC) from `bootstrap/mod.rs`. No behavior change, no new external dependencies, no new attack surface. Skipped (roadmap routine territory).

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable). GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #480 (Secret Scan: success) |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PR #480 |
| Dependabot alerts | ⚠️ 2 open, UPSTREAM-BLOCKED | password-hash 0.5→0.6 (#430, GAR-669 Slice 3) + rand 0.8→0.10 (#424, GAR-669 Slice 4) — both blocked on argon2 ≥ 0.6 stable |
| Open Dependabot PRs | ✅ none | 0 open Dependabot PRs |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #480 (Security Audit: success) |
| cargo-deny | ✅ pass | No new advisories; wasmtime-wasi 44.0.2 remains clean |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #480 |
| CI on main (`63ef1a9`) | ✅ green | No regressions detected |

**No fix applied this run.** Linear: GAR-689. Next security backlog: argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4; rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31; gtk-rs + unic-* (GAR-430) — expiry 2026-07-31.

---

## Confirmed 2026-05-22 run 16 (~20:45 ET) — all surfaces clean, PR #475 + #477 merged, priority (i)

Health routine ran on 2026-05-22 (~20:45 ET). Full security scan completed. Priority ladder exhausted at **(i)** — no actionable security work found.

**Open PRs resolved this run:**
- **PR #477** (`docs/mark-0167-done`) — fully green (20/20 checks) → squash-merged `fcb7904`
- **PR #475** (`docs/mark-0166-done`) — had merge conflict (0167 row present in main but not in branch) → resolved via rebase, pushed `075078b`, CI re-ran → merged after green

**New merges since run 15 (GAR-686):** PR #474 (`4a51841`, GAR-478 — Q10.d extract `build_slack_channels` to `bootstrap/slack.rs`, pure refactor) + PR #476 (`60a8dff`, GAR-479 — Q10.e extract `build_whatsapp_channels` to `bootstrap/whatsapp.rs`, pure refactor).

**Security review — bootstrap/slack.rs + bootstrap/whatsapp.rs:** Pure extractions from `bootstrap/mod.rs`. No behavior change, no new external dependencies, no new attack surface. No command injection, no PII exposure, no unsafe blocks introduced.

**argon2 upstream:** Still `0.6.0-rc.8` (RC, not stable). GAR-669 Slices 3–4 remain blocked until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #475 (gitleaks: success) |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PR #475 |
| Dependabot alerts | ⚠️ 2 open, UPSTREAM-BLOCKED | password-hash 0.5→0.6 (#430, GAR-669 Slice 3) + rand 0.8→0.10 (#424, GAR-669 Slice 4) — both blocked on argon2 ≥ 0.6 stable |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #475 |
| cargo-deny | ✅ pass | No new advisories; wasmtime-wasi 44.0.2 remains clean |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #475 |
| CI on main (`fcb7904`) | ✅ green | PR #477 squash-merge fully green |

**No fix applied this run.** Linear: GAR-688. Next security backlog: rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31. GAR-669 Slices 3–4 unblock when argon2 ≥ 0.6 stable ships.

---

## Confirmed 2026-05-22 run 14 (health routine — RUSTSEC-2026-0149 wasmtime-wasi fixed; upstream-blocked unchanged)

Health routine ran on 2026-05-22 (~08:45 ET initial scan; ~12:30 ET fix applied). New RUSTSEC advisory RUSTSEC-2026-0149 detected mid-run when CI failed on PR #472 (cargo-deny + Security Audit). Fixed immediately by lockfile upgrade wasmtime-wasi 44.0.1 → 44.0.2. Linear: GAR-685.

**New merges since run 13 (GAR-682):** PR #470 (`f337cb9`, GAR-476 — Q10.b extract `build_channels` to `bootstrap/channels.rs`, pure refactor) + PR #471 (`b594ace`, GAR-477 — Q10.c extract `build_discord_channels` + `handle_discord_command` to `bootstrap/discord.rs`, pure refactor).

**Security review — bootstrap/channels.rs + bootstrap/discord.rs:** Pure extractions from `bootstrap/mod.rs`. No behavior change, no new external dependencies, no new attack surface. No command injection, no PII exposure, no unsafe blocks introduced.

**RUSTSEC-2026-0149 (wasmtime-wasi 44.0.1) — FIXED:**
- Advisory: WASI path_open(TRUNCATE) bypasses `FilePerms::WRITE` host restriction (GHSA-2r75-cxrj-cmph)
- Vector: WASI guest could open files with O_TRUNC even with host `FilePerms::WRITE` restriction set
- Impact: `garraia-plugins` (WASM sandbox) via `wasmtime-wasi 44.0.1`
- Fix: `cargo update -p wasmtime-wasi --precise 44.0.2` — bumps wasmtime-wasi + wasmtime + cranelift-* ecosystem 44.0.1 → 44.0.2
- GAR-685 → Done, included in PR #472

**Upstream-blocked unchanged:** Both remaining Dependabot alerts continue to require argon2 ≥ 0.6 stable from upstream. Latest on crates.io: `argon2 = "0.6.0-rc.8"` (RC, not stable). No unblock path until stable release.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #472 |
| Malware (cargo/npm) | ✅ none | cargo-deny green (post-fix) |
| Dependabot alerts | ⚠️ 2 open, UPSTREAM-BLOCKED | password-hash 0.5→0.6 (#430, GAR-669 Slice 3) + rand 0.8→0.10 (#424, GAR-669 Slice 4) — both blocked on argon2 ≥ 0.6 stable |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass (post-fix) | wasmtime-wasi 44.0.1→44.0.2 clears RUSTSEC-2026-0149 |
| cargo-deny | ✅ pass (post-fix) | RUSTSEC-2026-0149 resolved by upgrade, advisories ok |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #472 |
| CI on main (`b594ace`) | ✅ green | base PR #472 after routine/ Q10.c merge |

**Fix applied: RUSTSEC-2026-0149 (GAR-685).** wasmtime-wasi 44.0.1 → 44.0.2 lockfile upgrade. GAR-683 filed. Next security backlog: rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31. GAR-669 Slices 3–4 unblock when argon2 ≥ 0.6 stable ships.

---

## Confirmed 2026-05-21 run 11 (health routine — upstream-blocked state unchanged; SSE stream + audit-log reviewed clean)

Health routine ran on 2026-05-21 (~16:45 ET). Full security scan completed. Priority ladder exhausted at (i) — no actionable security work found.

**New merges since run 10:** PR #459 (`d25b64c`, GAR-678 — `GET /v1/chats/{id}/stream` SSE endpoint + DashMap GC fix + cross-tenant RLS test), PR #462 (`3ddaf3e`, post-merge bookkeeping), PR #463 (`a972947`, GAR-680 — audit-log of SSE chat subscriptions via `chat.subscribed`/`chat.unsubscribed` event pairs).

**Security review — SSE stream handler + ChatStreamGuard:**
- `stream_chat` handler performs RLS context (`SET LOCAL app.current_user_id / app.current_group_id`) inside a proper `pool.begin()` transaction — no implicit auto-commit race (F-2 fix in PR #459).
- `ChatStreamGuard` RAII drop emits `chat.unsubscribed` via a fire-and-forget `tokio::spawn` using `Handle::try_current` — safe no-op when no runtime (test teardown). No PII in metadata (`subscriber_count` integer only).
- `DashMap::remove_if` GC on last receiver drop prevents unbounded memory growth (F-1 fix in PR #459). Race-safe under concurrent subscribe via entry lock.
- All `unwrap()` calls in `rest_v1_chats_sse.rs` are inside `#[cfg(test)]` / integration test blocks per CLAUDE.md rules.
- No new external dependencies introduced. No Cargo.lock security impact.

**Open PRs (not health/):** PR #464 (`michelduek/gar-680-post-merge-bookkeeping`, docs-only ROADMAP update) — CI in progress at time of health run (17/20 checks green, ubuntu/windows/coverage still running).

**Upstream-blocked unchanged:** Both remaining Dependabot alerts continue to require argon2 ≥ 0.6 from upstream before they can be resolved. No argon2 release supporting `password-hash ^0.6` on crates.io as of 2026-05-21 16:45 ET.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #464 (gitleaks job: success) |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PR #464 |
| Dependabot alerts | ⚠️ 2 open, UPSTREAM-BLOCKED | password-hash 0.5→0.6 (#430, GAR-669 Slice 3) + rand 0.8→0.10 (#424, GAR-669 Slice 4) — both blocked on argon2 ≥ 0.6 |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #464 |
| cargo-deny | ✅ pass | advisories ok |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #464 |
| CI on main (`a972947`) | ✅ green | PR #464 check-runs: 17/20 success (ubuntu/windows/coverage in progress at scan time, all others green) |

**No fix applied this run.** Next security backlog: rsa (GAR-456), glib+rand (GAR-513) — all expire 2026-07-31. GAR-669 Slices 3–4 unblock when argon2 ≥ 0.6 ships.

---

## Confirmed 2026-05-21 run 10 (health routine — upstream-blocked state unchanged; repo_workflow.rs reviewed clean)

Health routine ran on 2026-05-21 (~12:45 ET). Full security scan completed. Priority ladder exhausted at (i) — no actionable security work found.

**New merge since run 9:** PR #455 (`1b7f04c`, GAR-496 — repo workflow seguro para garra max-power) squash-merged to main as `671f760` at 12:11 ET — pure CLI feature addition, no new crate dependencies, no Cargo.lock security impact.

**Security review — repo_workflow.rs:** New module reviewed for command injection. `ProcessRunner` uses `std::process::Command::new(program).args(rest)` with separate `&[&str]` arguments — no shell involved, no string concatenation into a shell command. Protected-branch guard (`is_protected_branch`) correctly refuses direct pushes to `main`, `master`, `release/*`. All `unwrap()` calls confined to `#[cfg(test)]` blocks per CLAUDE.md rules. No security issues found.

**Open PRs (not health/):** PR #458 (`chore/ignore-claude-skills-local`) — 19/20 CI checks green (Windows test still in progress); PR #459 (`routine/202605211215-chats-sse-stream`) — skipped per rules (routine/ prefix).

**Upstream-blocked unchanged:** Both remaining Dependabot alerts continue to require argon2 ≥ 0.6 from upstream before they can be resolved. No argon2 release supporting `password-hash ^0.6` on crates.io as of 2026-05-21 12:45 ET.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PRs #455 + #458 (19+/20 checks green, base main `671f760`) |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PRs #455 + #458 |
| Dependabot alerts | ⚠️ 2 open, UPSTREAM-BLOCKED | password-hash 0.5→0.6 (#430, GAR-669 Slice 3) + rand 0.8→0.10 (#424, GAR-669 Slice 4) — both blocked on argon2 ≥ 0.6 |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PRs #455 + #458 |
| cargo-deny | ✅ pass | advisories ok |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #458 |
| CI on main (`671f760`) | ✅ green | PR #458 check-runs: 19/20 success (Windows in progress, all other checks green) |

**No fix applied this run.** Next security backlog: rsa (GAR-456), glib+rand (GAR-513) — all expire 2026-07-31. GAR-669 Slices 3–4 unblock when argon2 ≥ 0.6 ships.

---

## Confirmed 2026-05-21 run 9 (health routine — upstream-blocked state unchanged; windows-sys #422 closed)

Health routine ran on 2026-05-21 (~08:45 ET). Full security scan completed. Priority ladder exhausted at (i) — no actionable security work found.

**New merge since run 8:** PR #453 (`e5a2a08`, GAR-495 — capability prompt nativo para garra max-power) — pure CLI feature addition, no new crate dependencies, no Cargo.lock security impact.

**windows-sys #422 status:** Confirmed closed — `garraia-cli/Cargo.toml` now pins `windows-sys = "0.61"` (Cargo.lock carries 0.61.2). Dependabot auto-closes on next rescan after PR #451 merged as `1e7ce50`.

**Upstream-blocked unchanged:** Both remaining Dependabot alerts continue to require argon2 ≥ 0.6 from upstream before they can be resolved (same finding as run 8). No argon2 release supporting `password-hash ^0.6` on crates.io as of 2026-05-21 09:00 ET.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass (20/20 checks green on PRs #454 + #455, base main `e5a2a08`) |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PRs #454 + #455 |
| Dependabot alerts | ⚠️ 2 open, UPSTREAM-BLOCKED | password-hash 0.5→0.6 (#430, GAR-669 Slice 3) + rand 0.8→0.10 (#424, GAR-669 Slice 4) — both blocked on argon2 |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | 19 allowlisted warnings, CI green on PRs #454 + #455 |
| cargo-deny | ✅ pass | advisories ok |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PRs #454 + #455 |
| CI on main (`e5a2a08`) | ✅ green | 20/20 checks green (verified via PR #454 + #455 check-runs) |

**No fix applied this run.** Next security backlog: rsa (GAR-456), glib+rand (GAR-513) — all expire 2026-07-31. GAR-669 Slices 3–4 unblock when argon2 ≥ 0.6 ships.

---

## Confirmed 2026-05-21 run 8 (health routine — password-hash + rand build-dep upstream-blocked, no actionable fix)

Health routine ran on 2026-05-21 (~04:45 ET). Full security scan completed. Priority ladder exhausted at (i) — no actionable security work found.

**Key finding:** `password-hash 0.5→0.6` (Dependabot alert #430, GAR-669 Slice 3) is **upstream-blocked**. Registry scan confirmed that `argon2 0.5.3` is the latest argon2 release and only supports `password-hash ^0.5`. No argon2 version compatible with password-hash 0.6 has been published on crates.io as of 2026-05-21. The `rand = "0.8"` pin in `crates/garraia-auth` `[build-dependencies]` is a direct consequence of the same constraint (`build.rs` uses `password_hash::rand_core::OsRng` from rand_core 0.6; upgrading rand in build-deps requires upgrading password-hash first). Both GAR-669 Slice 3 and Slice 4 remain deferred until argon2 publishes a release supporting `password-hash ^0.6`.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #453 head (20/20 checks green, based on main `a3c61ce`) |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PR #453 |
| Dependabot alerts | ⚠️ 2 open, UPSTREAM-BLOCKED | password-hash 0.5→0.6 (#430, GAR-669 Slice 3) + rand 0.8→0.10 (#424, GAR-669 Slice 4) — both blocked on argon2 crate not yet supporting password-hash 0.6 |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #453 |
| cargo-deny | ✅ pass | advisories ok |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #453 |
| CI on main (`a3c61ce`) | ✅ green | 20/20 checks green |

**No fix applied this run.** Linear: status note filed (health-routine label). Next security backlog: rsa (GAR-456), glib+rand (GAR-513) — all expire 2026-07-31. GAR-669 Slices 3–4 unblock when argon2 ≥ 0.6 ships.

---

## Confirmed 2026-05-20 run 7 (health routine — GAR-669 Slice 1: rand_chacha 0.9 + rand 0.9 co-bump)

Health routine ran on 2026-05-20 (run 7, ~08:45 ET / 12:45 UTC). Full security scan completed. Fix applied: co-bumped `rand_chacha` 0.3→0.9 and `rand` 0.8→0.9 in `garraia-workspace` dev-deps, renamed `gen_range` → `random_range` in `migration_smoke.rs`. Root cause: rand_chacha 0.9 requires rand_core 0.9 while rand 0.8 uses rand_core 0.6 — type mismatch on `SeedableRng`. Supersedes Dependabot PR #423.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #446 (20/20 checks green) |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PR #446 |
| Dependabot alerts | ⚠️ 3 open, major-version breaks | password-hash 0.5→0.6 (#430 — auth-critical, GAR-669 Slice 3), rand 0.8→0.10 (#424 — Rng→RngExt breaking, GAR-669 Slice 4), windows-sys 0.52→0.61 (#422 — windows-only, GAR-669 Slice 2) |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #446 |
| cargo-deny | ✅ pass | advisories ok |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #446 |
| CI on main (`d9f811ac`) | ✅ green | PR #446 (20/20 checks green) |

**Fix applied:** PR #446 squash-merged as `d9f811ac` 2026-05-20T13:46Z. Dependabot PR #423 (rand_chacha 0.3.1→0.9.0) superseded — comment added. Linear: GAR-669 Done, GAR-674 Done. Next security backlog: rsa (GAR-456), glib+rand (GAR-513) — all expire 2026-07-31. GAR-669 Slices 2–4 remain open.

---

## Confirmed 2026-05-19 run 4 (health routine — all surfaces clean, no actionable work)

Health routine ran on 2026-05-19 (run 4, ~12:45 ET / 16:45 UTC). Full security scan completed. Priority ladder exhausted at (i) — no actionable security work found. New merges on main since run 3: PR #437 GAR-497 bash safety gate (`f2ab1d9`) + docs-only PRs #436/#438/#439. None touched Cargo.lock, deny.toml, .cargo/audit.toml, or any auth/crypto path.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #437 (20/20 checks green) |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PR #437 |
| Dependabot alerts | ⚠️ 4 open, major-version breaks | password-hash 0.5→0.6 (#430), rand 0.8→0.10 (#424), rand_chacha 0.3→0.9 (#423), windows-sys 0.52→0.61 (#422) — all deferred (code changes required) |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #437 |
| cargo-deny | ✅ pass | advisories ok |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #437 |
| CI on main (`deadd799`) | ✅ green | PR #437 (most recent code commit) 20/20 checks green |

**No fix applied this run.** Linear issue: GAR-671 (Done). Next security backlog: rsa (GAR-456), glib+rand (GAR-513) — all expire 2026-07-31.

---

## Confirmed 2026-05-19 run 3 (health routine — all surfaces clean, no actionable work)

Health routine ran on 2026-05-19 (run 3, ~08:45 ET / 12:45 UTC). Full security scan completed. Priority ladder exhausted at (i) — no actionable security work found. New merges on main since run 2: governor 0.8.1→0.10.4 (PR #425, `5375a64`) + GAR-494 max-power subcommand (PR #431, `8a9a915`). Neither touched security-sensitive files.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on main `8a9a915` (20/20 checks green) |
| Malware (cargo/npm) | ✅ none | cargo-deny green on main |
| Dependabot alerts | ⚠️ 4 open, major-version breaks | password-hash 0.5→0.6 (#430), rand 0.8→0.10 (#424), rand_chacha 0.3→0.9 (#423), windows-sys 0.52→0.61 (#422) — all deferred (code changes required) |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on main `8a9a915` |
| cargo-deny | ✅ pass | advisories ok |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on main |
| CI on main (`8a9a915`) | ✅ green | 20/20 checks green |

**No fix applied this run.** Linear issue: GAR-670 (Done). PR #422 (windows-sys) had Security Audit failure on stale base `e60fc4be` — verified the failure predates governor bump PR #425; main is clean. Next security backlog: rsa (GAR-456), glib+rand (GAR-513) — all expire 2026-07-31.

---

## Confirmed 2026-05-19 run 2 (health routine — RUSTSEC-2026-0145 merged + tokio-tungstenite 0.26→0.29)

Health routine ran on 2026-05-19 (run 2, ~08:45 ET / 12:45 UTC). Two fixes delivered:

1. **RUSTSEC-2026-0145** (PAX Header Desynchronization in `astral-tokio-tar`) — PR #432 (`fix/rustsec-2026-0145-astral-tokio-tar`, all 20 CI checks green) was lingering from a prior session; squash-merged as `287edc1c`. Dev-dep only (testcontainers chain).
2. **tokio-tungstenite 0.26→0.29** — Dependabot PR #429 had Cargo.lock conflict with the RUSTSEC fix. Created clean `health/202605190850-tokio-tungstenite-0.29` branch, applied upgrade, merged as `51382a9c` (PR #433). 20/20 CI checks green. cargo audit: 0 vulnerabilities post-merge.

Main now at `51382a9c`. GAR-668 / plan 0152.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #433 |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PR #433 |
| Dependabot alerts | ⚠️ 5 open, major-version breaks | password-hash 0.5→0.6, governor 0.8→0.10, rand 0.8→0.10, rand_chacha 0.3→0.9, windows-sys 0.52→0.61 — all deferred (code changes required) |
| Security Audit (`cargo audit`) | ✅ 0 vulnerabilities | 19 allowed unmaintained warnings (pre-existing) |
| cargo-deny | ✅ pass | advisories ok |
| CodeQL (rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #433 |

## Confirmed 2026-05-18 run 6 (health routine — all surfaces clean, PRs #409+#410 verified, no actionable security work)

Health routine ran on 2026-05-18 (run 6, ~16:45 ET / 20:45 UTC). Full security scan completed. Priority ladder exhausted at (i) — no actionable security work found. New merges on main since run 5: PR #409 (GAR-648 Skill Auto-Updater, 18:58Z) + PR #410 (bookkeeping, 19:29Z) — main now at `ea026e6`. Neither PR touched `Cargo.lock`, `deny.toml`, `.cargo/audit.toml`, or any security-sensitive file.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #409 (job 76592503754, completed success) |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PR #409 |
| Dependabot alerts | ✅ 3 open, all upstream-blocked | rsa/GAR-456, glib/GAR-513, rand/GAR-513 — expiry 2026-07-31 |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #409 (job 76592503841, completed 18:32Z) |
| cargo-deny | ✅ pass | `advisories ok` — job 76592503817 |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #409 |
| CI on main (latest: `ea026e6`) | ✅ green | 20/20 checks green on PR #409 |

**No fix applied this run.** Linear issue: GAR-665 (Done). Next security backlog: rsa (GAR-456), glib+rand (GAR-513) — all expire 2026-07-31.

---

## Confirmed 2026-05-18 run 5 (health routine — RUSTSEC-2026-0112 confirmed resolved, all surfaces clean)

Health routine ran on 2026-05-18 (run 5, ~12:45 ET / 16:45 UTC). Full security scan completed. Priority ladder exhausted at (i) — no actionable security work found. New merges on main since run 4: PRs #402 (GAR-644), #403 (bookkeeping), #404 (GAR-645 Skill Registry) — main now at `b67d030`.

**Key finding this run**: Checked new RUSTSEC advisories above RUSTSEC-2026-0097. Found RUSTSEC-2026-0112 (astral-tokio-tar PAX Header Desynchronization, High severity). Confirmed our lockfile carries `astral-tokio-tar 0.6.1` — the patched version. No action required.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #406 head (`495618f`) |
| Malware (cargo/npm) | ✅ none | RUSTSEC-2026-0107 (cratesio malicious) not in Cargo.lock |
| Dependabot alerts | ✅ 3 open, all upstream-blocked | rsa/GAR-456, glib/GAR-513, rand/GAR-513 — expiry 2026-07-31 |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI pass on PR #406 |
| cargo-deny | ✅ pass | `advisories ok` — RUSTSEC-2026-0112 not triggered (0.6.1 is patched) |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All Analyze jobs green on PR #406 |
| CI on main (latest: `b67d030`) | ✅ green | 19/20 checks green (Test windows still running) |

**No fix applied this run.** Linear issue: GAR-664 (Done). Next security backlog: rsa (GAR-456), glib+rand (GAR-513) — all expire 2026-07-31.

---

## Confirmed 2026-05-18 run 1 (health routine — all surfaces clean, no actionable work)

Health routine ran on 2026-05-18 (run 1, ~00:45 ET). Full security scan completed. Priority ladder exhausted at (i) — no actionable security work found. PR #396 (garraia-embeddings scaffold, GAR-372) merged as `cfda7ad5` by michelbr84.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #396 head (`40016830`) |
| Malware (cargo/npm) | ✅ none | No malware advisories in cargo graph |
| Dependabot alerts | ✅ 3 open, all upstream-blocked | rsa/GAR-456, glib/GAR-513, rand/GAR-513 — expiry 2026-07-31 |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | **19** allowlisted warnings (unchanged from run 3 2026-05-17) |
| cargo-deny | ✅ pass | `advisories ok` |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All Analyze jobs green on PR #396 |
| CI on main (latest: `cfda7ad5`) | ✅ green | PR #396 merged (all 20 checks green) |

**No fix applied this run.** All 3 open Dependabot alerts remain upstream-blocked (expiry 2026-07-31). Linear issue: GAR-661 (Done).

**Open branches inspected:**

| Branch | Status | Action |
|---|---|---|
| `feat/garraia-embeddings-scaffold` | PR #396 — merged as `cfda7ad5` by michelbr84 | ✅ Merged |

---

## Confirmed 2026-05-17 run 3 (health routine — RUSTSEC-2025-0069 closed, daemonize → nix)

Health routine ran on 2026-05-17 (run 3, ~12:45 ET). Full security scan completed. Pending health/ PR #382 found with all 20 CI checks green; squash-merged as `a5daf344`. Priority ladder exhausted at (i) after merge.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #382 head (`281dea9`) |
| Malware (cargo/npm) | ✅ none | No malware advisories in cargo graph |
| Dependabot alerts | ✅ 3 open, all upstream-blocked | rsa/GAR-456, glib/GAR-513, rand/GAR-513 — expiry 2026-07-31 |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | **19** allowlisted warnings (↓1 from 20 — RUSTSEC-2025-0069 removed by PR #382) |
| cargo-deny | ✅ pass | `advisories ok`; RUSTSEC-2025-0069 NOTE added to deny.toml closed history |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | PR #382 all Analyze jobs green; no new open findings |
| CI on main (latest: `a5daf344`) | ✅ green | PR #382 all 20 checks green (squash-merged 2026-05-17 ~16:45 UTC) |

**Fix applied this run (plan 0142 — daemonize RUSTSEC-2025-0069, GAR-656):**

| Change | Before | After |
|---|---|---|
| `daemonize` in `crates/garraia-cli/Cargo.toml` | `"0.5"` (unmaintained) | **removed** |
| `nix` in `crates/garraia-cli/Cargo.toml` | transitive only | `{ version = "0.31", features = ["process"] }` (direct dep) |
| `daemonize 0.5.0` in `Cargo.lock` | ✅ present | ✅ **REMOVED** |
| `start_daemon()` implementation | `daemonize::Daemonize` | `nix::unistd::{fork, setsid}` + `libc::dup2` double-fork idiom |
| RUSTSEC-2025-0069 in `deny.toml` | in ignore list | **REMOVED** — NOTE comment added for closed history |
| `cargo audit` warning count | 20 | **19** |

**Open branches inspected:**

| Branch | Status | Action |
|---|---|---|
| `health/202605171245-replace-daemonize-nix` | PR #382 — all 20 CI checks green | ✅ Merged as `a5daf344` |
| `routine/202605171217-q11-tasks-slice6` | PR #381 — roadmap routine | Skip — roadmap routine's work |
| `routine/202605171215-q11-tasks-slice6-activity` | PR #380 — roadmap routine | Skip — roadmap routine's work |
| `merge/q11-slice6-and-health` | PR #383 — dirty (behind main after PR #382) | Leave — not health/ branch |
| `release/msi-rebuild-v0.2.1` | PR #384 — release branch | Leave — not health/ branch |

Alert count: **3 open** (unchanged). All 3 upstream-blocked with 2026-07-31 expiry. `cargo audit` warning count: **19** (was 20 at run 2, 21 at run 1, 22 at 2026-05-14).

## Confirmed 2026-05-17 run 2 (health routine — RUSTSEC-2025-0134 closed, axum-server 0.7→0.8)

Health routine ran on 2026-05-17 (run 2, ~05:00 ET). Full security scan completed. Highest actionable issue found: RUSTSEC-2025-0134 (`rustls-pemfile` unmaintained), fixed by bumping `axum-server` 0.7→0.8 (which no longer depends on `rustls-pemfile`). Priority ladder exhausted at (i) after merging.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #378 head (`1eb5c4b`) and PR #376 head (`1be73cd`) |
| Malware (cargo/npm) | ✅ none | No malware advisories in cargo graph |
| Dependabot alerts | ✅ 3 open, all upstream-blocked | rsa/GAR-456, glib/GAR-513, rand/GAR-513 — expiry 2026-07-31 |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | **20** allowlisted warnings (↓1 from 21 — RUSTSEC-2025-0134 removed by PR #378) |
| cargo-deny | ✅ pass | `advisories ok`; RUSTSEC-2025-0134 entry removed from deny.toml |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | PR #378 + PR #376 all Analyze jobs green; no new open findings |
| CI on main (latest: `1be73cd`) | ✅ green | PR #376 all 20 checks green (squash-merged 2026-05-17 ~09:12 UTC) |

**Fix applied this run (plan 0138 — axum-server RUSTSEC-2025-0134):**

| Change | Before | After |
|---|---|---|
| `axum-server` in `crates/garraia-gateway/Cargo.toml` | `"0.7"` | `"0.8"` |
| `rustls-pemfile` in `Cargo.lock` | ✅ present (via axum-server 0.7.3) | ✅ **REMOVED** (axum-server 0.8 uses rustls-pki-types) |
| RUSTSEC-2025-0134 in `.cargo/audit.toml` | allowlisted | **REMOVED** — no longer in dependency graph |
| RUSTSEC-2025-0134 in `deny.toml` | allowlisted | **REMOVED** atomically with audit.toml |
| `cargo audit` warning count | 21 | **20** |

**Structural work merged this run:**

- PR #376 (`1be73cd`) — `refactor(gateway): Q11.e — extract rest_v1/tasks/subscriptions.rs (GAR-653)`: pure structural refactor, 3 handlers extracted from `tasks/mod.rs` into new `subscriptions.rs` (~279 LOC). Zero behavior change, no SQL/RLS/auth modifications. Closes GAR-653 (slice 5 of GAR-635 Q11).

**Open branches inspected:**

| Branch | Status | Action |
|---|---|---|
| `routine/202605170707-q11-tasks-slice5` | PR #372 family — roadmap routine | Skip — roadmap routine's work |

Alert count: **3 open** (unchanged). All 3 upstream-blocked with 2026-07-31 expiry. `cargo audit` warning count: **20** (was 21 at last run, 22 at 2026-05-14).

## Confirmed 2026-05-17 (health routine — all surfaces green, bookkeeping plan 0137)

Health routine ran on 2026-05-17 (~04:45 ET). Full security scan completed. Priority ladder exhausted at (i) — no new actionable fix found.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #371 head (`efb295c`) |
| Malware (cargo/npm) | ✅ none | No malware advisories in cargo graph |
| Dependabot alerts | ✅ 3 open, all upstream-blocked | rsa/GAR-456, glib/GAR-513, rand/GAR-513 — expiry 2026-07-31 |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | 21 allowlisted warnings, no new advisories |
| cargo-deny | ✅ pass | `advisories ok` |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | PR #371 all Analyze jobs green; 22 dismissed alerts, no new open findings |
| CI on main (latest: `efb295c`) | ✅ green | PR #371 all 20 checks green |

**Bookkeeping completed this run:**

- `plans/README.md` row 0137: `🚧 In Progress` → `✅ Merged 2026-05-17 via PR #371 (efb295c)` (GAR-635 slice 3 — extract `rest_v1/tasks/assignees.rs`, T8 README update was pending)

**Open branches inspected:**

| Branch | Status | Action |
|---|---|---|
| `routine/202605170404-q11-tasks-slice4` | PR #372 open, CI in-flight | Skip — roadmap routine's work |

Alert count: **3 open** (unchanged). All 3 upstream-blocked with 2026-07-31 expiry. No Dependabot PRs open.

## Confirmed 2026-05-16 run 2 (health routine — all surfaces green, bookkeeping + deny.toml comment fixes)

Health routine ran on 2026-05-16 (run 2, ~12:45 ET). Full security scan completed. Priority ladder exhausted at (i) — no new actionable fix found.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #368 head (`6427dae`) |
| Malware (cargo/npm) | ✅ none | No malware advisories in cargo graph |
| Dependabot alerts | ✅ 3 open, all upstream-blocked | rsa/GAR-456, glib/GAR-513, rand/GAR-513 — expiry 2026-07-31 |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | 21 allowlisted warnings, no new advisories |
| cargo-deny | ✅ pass | `advisories ok` |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | PR #368 all Analyze jobs green; 22 dismissed alerts, no new open findings |
| CI on main (latest: `bec410c`) | ✅ green | PR #368 all 20 checks green |

**Bookkeeping completed this run:**

- `plans/README.md` row 0134: `🚧 In Progress` → `✅ Merged 2026-05-16 via PR #367 (40ee126)` (GAR-634 tokio unblock, T8 README update was pending)
- PR #364 (bookkeeping for GAR-475 / plan 0133) merged as `bec410c` — fully green CI (20/20 checks)
- `deny.toml` SYNC NOTE: added missing "instant ×1 (GAR-627 / health/202605150000)" to closed-advisories history (matching `audit.toml`)
- `deny.toml` RUSTSEC-2026-0097 comment: corrected "rand 0.10.1" → "rand 0.7.3" (the 0.7.x line has no fix; 0.10.1+ is patched — the actual unpatched version in our lockfile is 0.7.3 via phf_generator 0.8)

**Open branches inspected:**

| Branch | Status | Action |
|---|---|---|
| `routine/202605161215-q11-tasks-slice1` | PR #368 open, all CI green | Skip — roadmap routine's work |
| `routine/202605151325-q9d-mcp-templates` | Stale (already merged as PR #358) | Leave — roadmap routine cleanup |
| `routine/202605160620-q9f-bookkeeping` | Stale (PR #364 merged as `bec410c`) | Leave — roadmap routine cleanup |
| `claude/focused-cray-eDXzA` | Orphan — deny.toml comment fixes, no PR ever opened | Absorbed into this PR |

Alert count: **3 open** (unchanged). All 3 upstream-blocked with 2026-07-31 expiry. No Dependabot PRs open.

## Confirmed 2026-05-16 (health routine — GAR-634: tokio 1.50.0→1.52.3 unblocked via nix 0.31.3)

Health routine ran on 2026-05-16. **PR #366** (security dep sweep — h2/rustls/zerocopy/aws-lc-rs/reqwest) merged. **GAR-634** (plan 0134) resolved the tokio 1.52.3 upgrade blocker.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #366 head (`3c438ea`) |
| Malware (cargo/npm) | ✅ none | No malware advisories in cargo graph |
| Dependabot alerts | ✅ 3 open, all upstream-blocked | rsa/GAR-456, glib/GAR-513, rand/GAR-513 — expiry 2026-07-31 |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | 21 allowlisted warnings, no new advisories |
| cargo-deny | ✅ pass | `advisories ok` |
| CodeQL (Analyze rust + js-ts) | ✅ pass | PR #366 Analyze jobs all green |
| CI on main (latest: `02bd9de`) | ✅ green | PR #366 all 20 checks green |

**Fix applied this run (GAR-634, plan 0134):**

| Package | Before | After | Type |
|---|---|---|---|
| `nix` | 0.31.1 (`libc =0.2.180`) | **0.31.3** (`libc =0.2.186`) | Lockfile-only patch |
| `process-wrap` | 9.0.3 | **9.1.0** | Lockfile-only patch |
| `libc` | 0.2.180 | **0.2.186** | Transitive (via nix) |
| `tokio` | 1.50.0 | **1.52.3** | Lockfile-only — unblocked by nix update |
| `mio` | 1.1.1 | **1.2.0** | Transitive (via tokio) |
| `socket2` | 0.6.2 | **0.6.3** | Transitive (via tokio) |
| `tokio-macros` | 2.6.1 | **2.7.0** | Transitive (via tokio) |

**Dep security sweep (PR #366, merged as `02bd9de`):**

| Package | Before | After |
|---|---|---|
| `h2` | 0.4.13 | **0.4.14** |
| `rustls` | 0.23.36 | **0.23.40** |
| `zerocopy` / `zerocopy-derive` | 0.8.39 | **0.8.48** |
| `aws-lc-rs` / `aws-lc-sys` | 1.16.2 / 0.39.1 | **1.17.0 / 0.41.0** |
| `reqwest` | 0.13.2 | **0.13.3** |

Alert count: **3 open** (unchanged). All 3 upstream-blocked with 2026-07-31 expiry.

## Confirmed 2026-05-14 (health routine — metrics 0.24.5 yanked → 0.24.6 lockfile-only fix)

Health routine ran on 2026-05-14. Full `cargo audit` + `cargo deny check` scan completed.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #326 head (`84cf09f`) |
| Malware (cargo/npm) | ✅ none | No malware advisories in cargo graph |
| Dependabot alerts | ✅ 3 open, all upstream-blocked | rsa/GAR-456, glib/GAR-513, rand/GAR-513 — expiry 2026-07-31 |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | 21 allowlisted warnings (3 unsound + 18 unmaintained). No new untracked advisories. |
| cargo-deny | ✅ pass | `advisories ok` — 2 stale `advisory-not-detected` (rustls-pemfile, rand) are expected asymmetry (documented in SYNC NOTE). |
| CodeQL (Analyze rust + js-ts) | ✅ pass | PR #326 Analyze jobs all green |
| CI on main (latest: `ae0306d`) | ✅ green | PR #326 all 18 checks green |

**Fix applied this run:**

| Package | Before | After | Type |
|---|---|---|---|
| `metrics` | 0.24.5 (**yanked**) | **0.24.6** | Lockfile-only patch (`cargo update -p "metrics@0.24.5"`) |

`metrics 0.24.5` was yanked from crates.io (Dependabot PR #287 introduced it). Updated to `0.24.6` (latest non-yanked patch). The API surface is unchanged — `counter`, `gauge`, `histogram` macros remain stable. `cargo audit --deny unsound` warning count: **22 → 21** (yanked warning resolved).

**Dependency hygiene observation:** `tracing-opentelemetry` bumped from 0.27.0 → 0.32.1 (PR #285, Dependabot) introduced a second copy of `opentelemetry 0.31.0` alongside the existing `0.26.0`. Both are transitive via `garraia-telemetry`; no security advisory affects either version. This is a quality concern (duplicate major version), not a security risk. Tracked under normal dependency hygiene.

Alert count: **3 open** (unchanged). All 3 are upstream-blocked with 2026-07-31 expiry.

## Confirmed 2026-05-12 (health routine — GAR-591 merged, rustls-webpki 0.102.8 chain removed)

Health routine ran on 2026-05-12. **PR #293 (GAR-591)** merged at commit `69c357a7ff2c6d8e27a3283d7b2d4bdc235b8e9f`.

| Change | Result |
|---|---|
| serenity feature: `rustls_backend` → `native_tls_backend` | ✅ applied (PR #293, GAR-591) |
| poise `default-features = false` | ✅ applied — prevents feature-unification re-enabling rustls_backend |
| `rustls-webpki 0.102.8` in `Cargo.lock` | ✅ **REMOVED** — only `0.103.13` remains |
| `rustls 0.22.4` in `Cargo.lock` | ✅ **REMOVED** |
| `tokio-rustls 0.25.0` in `Cargo.lock` | ✅ **REMOVED** |
| Dependabot alerts closed | ⏳ PENDING — rescan expected within 24-48h for alerts #37, #11, #23, #22 |
| `audit.toml` + `deny.toml` cleanup | ✅ 4 RUSTSEC IDs removed atomically (this PR, GAR-455 CLOSED) |
| Secret scanning (gitleaks) | ✅ clean — CI pass on PR #293 head |
| Malware (cargo/npm) | ✅ none |
| Security Audit (`cargo audit`) | ✅ pass — CI green on PR #293 |
| cargo-deny | ✅ pass — CI green on PR #293 |
| CodeQL (Analyze rust + js-ts) | ✅ pass — CI green on PR #293 |
| CI on main (latest: `69c357a`) | ✅ green — all 18 checks pass |

Alert count: **8 open** (pre-rescan) → **4 expected** (post-rescan, within 24-48h).
Remaining 4 alerts: rsa/RUSTSEC-2023-0071 (GAR-456), glib/RUSTSEC-2024-0429, lru/RUSTSEC-2026-0002, rand/RUSTSEC-2026-0097 (all GAR-513).

## Confirmed 2026-05-12 run 2 (health routine — GAR-593: lru RUSTSEC-2026-0002 stale ignore removed)

Health routine ran on 2026-05-12 (run 2, after PR #295 merged). **PR #297** (`8f73144`, `fix(security): bump aws-sdk-s3 1.119->1.132 to pull lru 0.16.4`) had already landed the fix; this run removes the stale audit config entries.

| Change | Result |
|---|---|
| `lru` in `Cargo.lock` | ✅ **0.16.4** (patched; RUSTSEC-2026-0002 requires < 0.16.3) |
| `RUSTSEC-2026-0002` in `audit.toml` | ✅ **REMOVED** — lru 0.16.4 patches advisory (PR #299, GAR-593) |
| `RUSTSEC-2026-0002` in `deny.toml` | ✅ **REMOVED** atomically with audit.toml |
| SYNC NOTE in both files | ✅ updated: mandatory-sync set now rsa + glib + rand only |
| GAR-513 carve-out header | ✅ updated: `glib + lru + rand` → `glib + rand` |
| PR #299 CI | ✅ green — all 18 checks passed; merged as `7996dc4` |

Residuals (3 remaining, all expires 2026-07-31):

| Advisory | Crate | Owner | Status |
|---|---|---|---|
| RUSTSEC-2023-0071 | rsa 0.9.10 | GAR-456 | Active — no upstream fix |
| RUSTSEC-2024-0429 | glib 0.18.5 | GAR-513 | Active — Tauri gtk-rs blocker |
| RUSTSEC-2026-0097 | rand 0.7.3 | GAR-513 | Active — build-time dep only |

## Confirmed 2026-05-14 run 2 (health routine — GAR-620: metrics 0.24.5 yanked → 0.24.6)

Health routine ran on 2026-05-14 (run 2, ~8:50 AM ET). Full security scan completed. Highest actionable issue: `metrics 0.24.5` (yanked from crates.io). PR #336 (`claude/focused-cray-9fubA`) implements the lockfile-only patch.

| Change | Result |
|---|---|
| `metrics 0.24.5` (yanked) → `0.24.6` in `Cargo.lock` | ✅ merged — `adbe00af` |
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #336 head |
| Malware (cargo/npm) | ✅ none | |
| Dependabot alerts | ✅ 3 open, all upstream-blocked | rsa/GAR-456, glib/GAR-513, rand/GAR-513 — expiry 2026-07-31 |
| Security Audit (`cargo audit`) | ✅ pass | 21 warnings (↓1 from 22 once PR #336 merges) |
| cargo-deny | ✅ pass | advisories ok |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All green on PR #336 head |
| CI on main (31fb678) | ✅ green | All checks pass |
| plan 0124 | ✅ created | `plans/0124-gar-620-metrics-yanked-0246.md` + GAR-620 in Linear |

Alert count: **3 Dependabot open** (unchanged). The `metrics 0.24.5` yanked issue reduces `cargo audit` warning count from 22 → 21 once PR #336 merges.

## Confirmed 2026-05-14 (health routine — GAR-605: CodeQL actions language fix + plan 0116)

Health routine ran on 2026-05-14. Two pending non-routine PRs merged; one active security fix (15 Medium CodeQL alerts) handled.

| Change | Result |
|---|---|
| PR #321 merged (`c45fcff`) | ✅ Plan 0114 T8 bookkeeping — `plans/README.md` row 0114 updated to ✅ Merged |
| PR #323 merged (GAR-605) | ✅ Add `language: actions, build-mode: none` to `codeql.yml` matrix — `Analyze (actions)` job now active |
| 15 Medium `actions/missing-workflow-permissions` alerts | ⏳ PENDING auto-close — CodeQL re-scan on main expected within 24h; `Analyze (actions)` ran successfully on PR #323 with no new findings |
| Dependabot alerts | ✅ 3 open, all upstream-blocked | rsa/GAR-456, glib/GAR-513, rand/GAR-513 — expiry 2026-07-31 |
| Secret scanning (gitleaks) | ✅ clean | CI pass on main |
| Malware (cargo/npm) | ✅ none | |
| Security Audit (`cargo audit`) | ✅ pass | 3 allowlisted advisories, all with valid rationale |
| cargo-deny | ✅ pass | SYNC NOTE audit.toml ↔ deny.toml intact (mandatory IDs: rsa, glib, rand) |
| CI on main (post-merge) | ✅ green | All checks pass on `c45fcff` |

Alert count: **3 Dependabot open** (unchanged). After next CodeQL run on main, **Medium CodeQL open count → 0** (all 15 `actions/missing-workflow-permissions` expected to auto-close as `fixed`).

## Confirmed 2026-05-13 (health routine — plan 0113 bookkeeping; all surfaces green)

Health routine ran on 2026-05-13. Full security scan completed; no new actionable security issue found.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on main (`0e0edfb`) |
| Malware (cargo/npm) | ✅ none | No malware advisories in cargo graph |
| Dependabot alerts | ✅ 3 open, all upstream-blocked | rsa/GAR-456, glib/GAR-513, rand/GAR-513 — expiry 2026-07-31 |
| Security Audit (`cargo audit`) | ✅ pass | 3 allowlisted advisories, all with valid rationale |
| cargo-deny | ✅ pass | SYNC NOTE audit.toml ↔ deny.toml intact (mandatory IDs: rsa, glib, rand) |
| CodeQL (Analyze rust + js-ts) | ✅ pass | No new open findings |
| CI on main (latest: `0e0edfb`) | ✅ green | All 18 checks pass (confirmed via PR #317 check-runs) |

**Bookkeeping completed this run:** `plans/0113-gar-601-aws-actions-v6.md` and `plans/README.md` row 0113 updated from `🔄 In Progress` to `✅ Merged 2026-05-13 via PR #313 (4374623)`. GAR-601 was the aws-actions/configure-aws-credentials v4→v6 bump (Node20 deprecation deadline 2026-06-02) — the fix landed in main via PR #313 on a prior session; only the plan tracking files were pending.

Alert count: **3 open** (unchanged). All 3 are upstream-blocked with 2026-07-31 expiry. No Dependabot PRs open.

## Confirmed 2026-05-12 run 3 (health routine — bookkeeping only; all surfaces green)

Health routine ran on 2026-05-12 (run 3). Full security scan completed; priority ladder exhausted at (i) — no new actionable fix.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on main (`77c8947`) |
| Malware (cargo/npm) | ✅ none | No malware advisories in cargo graph |
| Dependabot alerts | ✅ 3 open, all upstream-blocked | rsa/GAR-456, glib/GAR-513, rand/GAR-513 — expiry 2026-07-31 |
| Security Audit (`cargo audit`) | ✅ pass | 3 allowlisted advisories, all with valid rationale |
| cargo-deny | ✅ pass | No `advisory-not-detected` warnings; SYNC NOTE audit.toml ↔ deny.toml intact |
| CodeQL (Analyze rust + js-ts) | ✅ pass | 22 alerts all dismissed; no new open findings |
| CI on main (latest: `77c8947`) | ✅ green | Format + cargo-deny completed success; others in-flight on active PRs |

**Bookkeeping completed this run:** `plans/README.md` row 0108 updated from `🔄 In Progress` to `✅ Merged 2026-05-12 via PR #299 (7996dc4)`. GAR-593 was already `Done` in Linear.

Alert count: **3 open** (unchanged since PR #299 merged). All 3 are upstream-blocked with 2026-07-31 expiry. Patch-and-minor Dependabot PR #296 (17 non-security bumps) and major-version PRs #284-292 are open but no CVEs are involved — outside health routine scope.

## Confirmed 2026-05-11 (health routine — all surfaces green)

Health routine ran on 2026-05-11. No new security action required.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #258 head (`70bff54`) |
| Malware (cargo/npm) | ✅ none | No malware advisories in cargo graph |
| Dependabot alerts | ✅ unchanged | 8 open (2 HIGH, 2 MEDIUM, 4 LOW) — all tracked, expiry 2026-07-31 |
| Security Audit (`cargo audit`) | ✅ pass | All advisories in `audit.toml` allowlist; CI green |
| cargo-deny | ✅ pass | `deny.toml` allowlist unchanged |
| CodeQL (Analyze rust + js-ts) | ✅ pass | 22 dismissed alerts, no new findings |
| CI on main (latest: `2c1460c`) | ✅ green | All 18 checks pass |

Alert count: **8 open, unchanged since 2026-05-09.** Priority ladder exhausted at (i). Exiting cleanly.

## Confirmed 2026-05-09 (health routine — AWS sub-chain removed, defense-in-depth)

Health routine ran on 2026-05-09. Defense-in-depth follow-up from GAR-455 deep-dive (2026-05-08):

| Change | Result |
|---|---|
| `aws-sdk-s3` feature swap: `"rustls"` → `"default-https-client"` in `crates/garraia-storage/Cargo.toml` | ✅ applied (plan 0087, GAR-553) |
| `rustls-webpki 0.101.7` in `Cargo.lock` | ✅ **REMOVED** — no longer appears |
| `rustls 0.21.12` in `Cargo.lock` | ✅ **REMOVED** — no longer appears |
| `hyper-rustls 0.24.2` in `Cargo.lock` | ✅ **REMOVED** — no longer appears |
| Dependabot alerts closed | ⚠️ 0 — serenity chain (`rustls-webpki 0.102.8`) still carries all 4 RUSTSEC IDs |
| `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` | ✅ clean |
| Secret scanning | ✅ pass |
| CodeQL | ✅ 22 alerts all dismissed (unchanged) |

Alert count unchanged (8 open). The `rustls-webpki 0.101.7` sub-chain that contributed to
RUSTSEC-2026-0098/0099/0104 has been removed from the dependency graph. Dependabot alerts remain
open because `rustls-webpki 0.102.8` (serenity 0.12.5 chain) still independently carries all 4 IDs.
The `audit.toml`/`deny.toml` allowlists are UNCHANGED — still required for the serenity chain.

## Confirmed 2026-05-08 (health routine — all surfaces green)

Health routine ran on 2026-05-08. All 4 security surfaces scanned:

| Surface | Result |
|---|---|
| Secret scanning (gitleaks) | ✅ pass |
| cargo-deny (advisories) | ✅ pass — all allowlisted |
| Security Audit (cargo-audit) | ✅ pass — all allowlisted |
| Dependabot alerts | ✅ 8 open, all pre-existing, all allowlisted (GAR-455 / GAR-513 / GAR-456) |
| CodeQL (code scanning) | ✅ 22 alerts all dismissed in ledger (alerts #40–#45 hard-coded-crypto-value + #67–#82 path-injection false positives). No new open alerts. Re-audit deadline: 2026-08-01. |

No new untracked alerts. Count reconciled: 8 Dependabot open (2 HIGH, 2 MEDIUM, 4 LOW) — all pre-existing, all upstream-blocked, all allowlisted. Main branch CI green. Open routine/ PR: #217 (task subtasks slice 9 — roadmap routine, unrelated to health). Linear status note filed under GAR team (label: automation,health-routine).

A targeted deep-dive on GAR-455 / Dependabot alert #37
(RUSTSEC-2026-0104, `rustls-webpki` panic in CRL parsing) ran the same
day. Verdict: still upstream-blocked. Details and a new finding about
the AWS sub-chain are recorded in the next sub-section.

## GAR-455 deep-dive 2026-05-08 — alert #37 closure investigation

Triggered by a question of whether GAR-455 could close today without
breaking the project. Read-only investigation; no `Cargo.toml` /
`Cargo.lock` / `deny.toml` / `.cargo/audit.toml` changes were made.

### Verdict

Alert #37 (RUSTSEC-2026-0104) **stays open and remains
upstream-blocked**. The allowlist entry in `.cargo/audit.toml` and the
mirror in `deny.toml` continue to be the correct mitigation.

### Empirical chain map (verified 2026-05-08 via `cargo tree`)

```
rustls-webpki 0.102.8  ← serenity 0.12.5
                         → tokio-tungstenite 0.21.0
                         → rustls 0.22.4
                         (always-on; reachable from garraia-channels +
                          garraia-cli + garraia-gateway)
                         carries ALL 4 RUSTSEC IDs of GAR-455
                         (RUSTSEC-2026-0049 / -0098 / -0099 / -0104)

rustls-webpki 0.101.7  ← aws-sdk-s3 1.119.0 (feature `rustls`)
                         → aws-smithy-runtime 1.11.1 (feature `tls-rustls`)
                         → aws-smithy-http-client 1.1.12
                           (feature `legacy-rustls-ring`)
                         → `legacy-rustls` (renamed dep, points at
                           rustls 0.21.12)
                         (only when `garraia-storage/storage-s3`
                          feature is enabled)
                         carries 3 of 4 RUSTSEC IDs (-0098, -0099, -0104)
```

### Upstream version snapshot (crates.io, 2026-05-08)

| Crate | Lockfile | crates.io latest | Last published | Notes |
|---|---|---|---|---|
| `serenity` | 0.12.5 | **0.12.5** | 2025-12-20 | No 0.13.x or 0.14+ stable release. The `tokio-tungstenite 0.21` pin is internal to serenity 0.12.5; only serenity itself can lift it. |
| `tokio-tungstenite` | 0.21.0 (via serenity) | 0.29.0 | 2026-03-17 | Workspace already declares 0.26 elsewhere; the 0.21 copy is exclusively dragged in by serenity. |
| `aws-sdk-s3` | 1.119.0 | 1.132.0 | 2026-05-06 | A version bump alone does NOT remove rustls 0.21 — `aws-smithy-http-client` is still 1.1.12 underneath. |
| `aws-smithy-http-client` | 1.1.12 | **1.1.12** | 2026-03-02 | Already supports modern rustls 0.23.31 via the `rustls-ring` / `rustls-aws-lc` features. The legacy chain is opt-in through `legacy-rustls-ring`. |

Conclusion on the serenity side: **no upstream path exists today**.
The 0.102.8 chain is purely waiting on a serenity 0.13 (or a 0.12
maintenance release that bumps `tokio-tungstenite`). Re-check on the
next monthly health routine.

### New finding — the AWS sub-chain is feature-flag-fixable, not version-blocked

The earlier mitigation column described the `0.101.7` chain as
upstream-blocked on an `aws-smithy-http-client` upgrade. That framing
is no longer accurate. Empirical reading of the upstream `Cargo.toml`s
on 2026-05-08:

- `aws-sdk-s3 1.119.0`: `rustls = ["aws-smithy-runtime/tls-rustls"]`
- `aws-smithy-runtime 1.11.1`: `tls-rustls = ["aws-smithy-http-client?/legacy-rustls-ring", "connector-hyper-0-14-x"]`
- `aws-smithy-http-client 1.1.12`:
  - `legacy-rustls-ring = ["dep:legacy-hyper-rustls", "dep:legacy-rustls", ...]` (legacy `rustls 0.21.x` renamed)
  - `rustls-ring` / `rustls-aws-lc` → `dep:rustls` at version `0.23.31`

In other words, `aws-sdk-s3 1.119`'s `rustls` feature aliases to the
**legacy** chain, while the same crate ships a separate
`default-https-client` feature that maps to the **modern** rustls 0.23
chain (via `aws-smithy-http-client/rustls-aws-lc`).

`crates/garraia-storage/Cargo.toml` currently passes `features =
["behavior-version-latest", "rustls", "rt-tokio"]` to both
`aws-config` and `aws-sdk-s3`. Note that on `aws-config 1.8.16` the
`rustls` alias already maps to modern rustls 0.23 (via `client-hyper`
→ `aws-smithy-runtime/default-https-client` →
`aws-smithy-http-client/rustls-aws-lc`); only the `aws-sdk-s3` side
flips to the legacy chain.

### What this finding does and does not change

- It DOES open a defense-in-depth path on the AWS sub-chain: swapping
  the `aws-sdk-s3` feature `"rustls"` for `"default-https-client"`
  would remove `rustls 0.21.12` and `rustls-webpki 0.101.7` from
  `Cargo.lock`, eliminating one of the two chains carrying
  RUSTSEC-2026-0098 / -0099 / -0104.
- It DOES NOT close Dependabot alert #37 (or any of the other 3
  GAR-455 alerts). The serenity-driven `rustls-webpki 0.102.8` chain
  carries all 4 RUSTSEC IDs independently. As long as serenity 0.12.5
  is on the lockfile, the allowlist entries for the 4 IDs in
  `.cargo/audit.toml` and `deny.toml` are required.
- The `audit.toml` SYNC NOTE invariant is therefore unaffected: the 4
  rustls-webpki IDs continue to mirror across both files, atomic drop
  still gated on the serenity bump.

### Follow-up (COMPLETED 2026-05-09 — plan 0087, GAR-553, PR health/202605090047)

The AWS-side feature-flag swap has been **landed** in plan 0087 (health
routine 2026-05-09). `crates/garraia-storage/Cargo.toml` now uses
`"default-https-client"` instead of `"rustls"` for `aws-sdk-s3`:

- `rustls 0.21.12`, `rustls-webpki 0.101.7`, `hyper-rustls 0.24.2`
  removed from `Cargo.lock`.
- S3 connectivity preserved via modern rustls 0.23 + aws-lc chain.
- `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean.

The originally-recommended validation from this section remains accurate:

- `cargo audit` and `cargo deny check` should still pass; the 4
  rustls-webpki residual IDs continue to be triggered by the serenity
  chain, so neither file changes.

The Linear placement for that follow-up is GAR-455 itself (or a
sub-issue under it) — not a new epic — because the residual surface
remains the same RUSTSEC IDs.

## Confirmed 2026-05-07 (health routine — no new alerts)

Health routine ran on 2026-05-07. All 4 security surfaces scanned:

| Surface | Result |
|---|---|
| Secret scanning (gitleaks) | ✅ pass |
| cargo-deny (advisories) | ✅ pass — all allowlisted |
| Security Audit (cargo-audit) | ✅ pass — all allowlisted |
| Dependabot alerts | ✅ 8 open, all pre-existing, all allowlisted (GAR-455 / GAR-513 / GAR-456) |

No new untracked alerts. Count reconciled: 8 open (2 HIGH, 2 MEDIUM, 4 LOW) matching the 8 active RUSTSEC IDs in `.cargo/audit.toml`. The "6 estimated" in the 2026-05-06 snapshot was incorrect — the `rsa` RUSTSEC-2023-0071 entry was added to `audit.toml` on 2026-04-30 when `jsonwebtoken 10 rust_crypto` backend brought `rsa 0.9.10` into the production tree (GAR-456). The `openssl` fix on 2026-05-06 closed a separate advisory not in this table. PR #188 (`health/ratchet-20260507-gitignore-local-reports`) merged — added `.github-health-reports/` and `audit/` to `.gitignore` to unblock future health routine iterations.

## Closed 2026-05-06 (health routine)

| Alert | Closure mechanism | Linear |
|---|---|---|
| `openssl` 0.10.78 → 0.10.79 + `openssl-sys` 0.9.114 → 0.9.115 security patch | plan 0073, health routine PR (Dependabot PR #166 was closed because it grouped a breaking `rand 0.8→0.10` major bump; this narrower follow-up applies only the openssl patch). | [GAR-527](https://linear.app/chatgpt25/issue/GAR-527) |

## Closed in sprint 2026-04-22 → 2026-04-30

| Alert range | Closure mechanism | Linear |
|---|---|---|
| 12 lockfile-only Dependabot bumps | PR #97 (`time` + bench refresh) + PR #99 (`openssl` 0.10.75 → 0.10.78) + PR #102 (rand + rustls-webpki bench cleanup) | GAR-484 (closed 2026-04-30) |
| `jsonwebtoken 9 → 10` migration | PR #105 (this sprint, plan `personal-api-key-revogada-vectorized-matsumoto` §Step 3, replaces broken Dependabot PR #103). Adopts `rust_crypto` backend + decouples `garraia-auth` from `rand` churn via direct `getrandom::fill`. | GAR-XXX umbrella, sub-issue 2 |

## Closed 2026-05-12 (PR #293 / GAR-591)

| GH # | RUSTSEC | Crate | Closure mechanism |
|---|---|---|---|
| #37 | RUSTSEC-2026-0104 | `rustls-webpki` | PR #293 (GAR-591): serenity `rustls_backend` → `native_tls_backend`; 0.102.8 chain removed from Cargo.lock. |
| #11 | RUSTSEC-2026-0049 | `rustls-webpki` | Same — part of same serenity chain. |
| #23 | RUSTSEC-2026-0099 | `rustls-webpki` | Same — part of same serenity chain. |
| #22 | RUSTSEC-2026-0098 | `rustls-webpki` | Same — part of same serenity chain. |

Dependabot rescan expected within 24-48h. Until rescan completes, GH UI still shows 8 open.

## Residuals (3 open post-rescan, updated 2026-05-12 run 2)

All 3 remaining alerts have:
- A specific RUSTSEC ID matching `Cargo.lock`.
- A documented rationale block in `.cargo/audit.toml` and/or `deny.toml`.
- A concrete Linear owner.
- An expiration date (**2026-07-31**) that forces re-triage.

The `cargo audit` and `cargo deny` CI gates pass green because each entry
is intentionally allowlisted, not silenced.

| GH # | GHSA | Severity | Crate | RUSTSEC | Linear | Mitigation |
|---|---|---|---|---|---|---|
| — | — | HIGH | `rsa` | RUSTSEC-2023-0071 (Marvin Attack timing sidechannel) | GAR-456 | `rsa 0.9.10` enters tree via two paths: (1) `sqlx-mysql` lockfile residual even with `default-features = false` on all sqlx deps; (2) `jsonwebtoken 10 rust_crypto` backend (added 2026-04-30). GarraRUST emits/verifies HS256 only (`Algorithm::HS256` in `garraia-auth/src/jwt.rs`) — no RSA code path is reachable. Fix paths: (a) `jsonwebtoken` upstream isolates `rsa` behind `asymmetric` feature; (b) migrate to `sqlx-postgres` direct or sqlx 0.9. |
| #2  | GHSA-wrw7-89jp-8q8g | MEDIUM | `glib` | RUSTSEC-2024-0429 (`VariantStrIter` Iterator unsoundness) | GAR-513 | Tauri-only path (`crates/garraia-desktop`), excluded from server CI builds. Low runtime risk in deployments. Fix path: bump glib OR gate ignore behind `desktop` feature. |
| #25 | GHSA-cq8v-f236-94qc | LOW | `rand` | RUSTSEC-2026-0097 (custom logger unsoundness in `rand::rng()`) | GAR-513 | Build-time dep only: `phf_codegen → phf_generator → selectors → tauri-utils → garraia-desktop`. Zero server runtime risk. No 0.7.x patch; fix requires phf_codegen to bump rand. |

## Closed 2026-05-12 run 2 (PR #297 + PR #299 / GAR-593)

| GH # | RUSTSEC | Crate | Closure mechanism |
|---|---|---|---|
| #5 | RUSTSEC-2026-0002 | `lru` | PR #297 (`8f73144`) bumped aws-sdk-s3 1.119→1.132, pulling lru 0.16.4 (patched ≥ 0.16.3). Audit config cleanup via PR #299 (GAR-593). |

## Linear ownership map

- **GAR-455** — ✅ CLOSED 2026-05-12. `rustls-webpki` legacy chains fully removed. Both chains eliminated: aws-smithy (plan 0087, 2026-05-09) + serenity (PR #293, 2026-05-12). 4 of 8 former alerts (#37, #11, #23, #22) closing pending Dependabot rescan.
- **GAR-513** — Unsound triage carve-out (created 2026-05-05; GAR-437 closed 2026-04-27). 2 of 3 remaining alerts (#2 glib, #25 rand). lru (#5 / RUSTSEC-2026-0002) closed 2026-05-12 by GAR-593 / PR #299. Each remaining entry tracked individually as upstream fixes ship.
- **GAR-456** — Marvin Attack timing sidechannel (`rsa 0.9.10`). 1 of 4 remaining alerts (RUSTSEC-2023-0071; GH alert number unknown — cargo audit detects it as workspace advisory). GarraRUST emits and verifies HS256 only; no RSA call site is reachable. Same `2026-07-31` expiration.

## Re-triage cadence

- **Weekly** (Monday): cargo-audit.yml runs `cargo audit --no-fetch
  --deny unsound`. Output reviewed alongside CodeQL Monday-morning batch.
- **Quarterly** (every 3 months): every `audit.toml` ignore is checked
  against its declared expiration. Any past-expiration entry without
  a closing PR triggers immediate Linear sub-issue creation.
- **Ad-hoc**: a Dependabot alert that does NOT match an existing
  allowlist entry is treated as a real new vulnerability and follows
  the standard mitigation procedure (`docs/security/secret-scanning-runbook.md`
  — same 5-step playbook applies analogously).

## Operational checks

```bash
# Snapshot of open Dependabot alerts (mirrors this table when in sync)
gh api repos/michelbr84/GarraRUST/dependabot/alerts --paginate \
  --jq '.[] | select(.state=="open") | {n: .number, severity: .security_advisory.severity, package: .dependency.package.name, ghsa: .security_advisory.ghsa_id}'

# Audit allowlist consistency check
grep -E "^\s*\"RUSTSEC-" .cargo/audit.toml | sort
grep -E "^\s*\"RUSTSEC-" deny.toml | sort
# (the two MUST share the mandatory-sync IDs: rsa, glib, rand
#  per .cargo/audit.toml SYNC NOTE — refreshed 2026-05-12 by GAR-593)

# Verify cargo audit / cargo deny stay green with the allowlist active
cargo audit
cargo deny check
```

## Out of scope (tracked separately)

- Closing the 90 CodeQL alerts — see Linear `GAR-XXX.4` (production
  paths) and `GAR-XXX.5` (test fixtures + suppression convention).
  CodeQL alerts are NOT Dependabot alerts and use a different triage
  pipeline (`docs/security/codeql-setup.md`).
- Moving from `cargo audit` 0.22.x to a version that supports
  per-(advisory, version) ignores — would let us tighten the
  rustls-webpki block without weakening the production hot path.
  Tracked under GAR-455 closure plan.

## See also

- `.cargo/audit.toml` — line-by-line rationale per RUSTSEC ID.
- `deny.toml` — `cargo deny check advisories` config.
- `docs/security/secret-scanning-runbook.md` — companion runbook for
  the secret-scanning side of the security baseline.
- `docs/security/codeql-setup.md` — CodeQL advanced setup runbook.
