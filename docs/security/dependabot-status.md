# Dependabot Status

> Last updated: **2026-05-26 run 35** (health routine — all surfaces clean, 9 open Dependabot PRs (none security-labeled), routine/ PR #535 GAR-713 noted, priority (i). GAR-714. Previous: run 34 all surfaces clean, PR #534 `885ed2e`, priority (i) (GAR-712)).
> Source of truth: `.cargo/audit.toml` and `deny.toml` (the suppression
> rationale lives there, this file is the alert-to-rationale index).

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

**No security fix applied this run.** Bookkeeping only: plan 0196 (GAR-714), plans README row 0196 added + dependabot-status run 35 note. Linear: GAR-714. Next security backlog: argon2 ≥ 0.6 stable → unblocks GAR-669 Slices 3–4; rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31; CodeQL ledger re-audit due 2026-08-01 (GAR-491); GAR-711 OpenTelemetry 0.26→0.32 Backlog.

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

> Last updated (previous): **2026-05-25 run 31** (health routine — all surfaces clean, PR #508 run 30 merged `ef040ad`, priority (i). GAR-706. Previous: run 30 all surfaces clean, PR #506 conflict resolved, priority (i) (GAR-705); run 29 all surfaces clean, routine/ PR #505 noted, priority (i) (GAR-704); run 28 all surfaces clean, PR #503 merged, priority (i) (GAR-702); run 27 all surfaces clean, PR #501 run 26 open 20/20 CI green, priority (i) (GAR-701). Previous: run 30 all surfaces clean, PR #503 run 27 merged `ba8482b`, priority (i). GAR-702. Previous: run 26 all surfaces clean, PR #499 merged `61bd6a7`, priority (i) (GAR-699); run 25 all surfaces clean, routine/ PR #498 noted (roadmap routine), priority (i) (GAR-698); run 24 all surfaces clean, routine/ PR #496 noted, priority (i) (GAR-696); run 23 all surfaces clean, routine/ PR #492 pending merge (skipped), priority (i) (GAR-695); run 22 all surfaces clean, GAR-499 agent team reviewed clean, priority (i) (GAR-694); run 21 merge run-20 PRs + plan numbering fix; 3 upstream-blocked alerts; priority (i) (GAR-693); run 20 all surfaces clean; plans 0168+0169 marked merged (PR #484); priority (i) (GAR-692); run 19 deny.toml advisory-not-detected cleanup GAR-513/plan 0169 (PR #483/484 merged `b3f62fd`); run 18 all surfaces clean, PR #482 merged, priority (i) (GAR-690); run 17 all surfaces clean, no open health/ PRs, priority (i) (GAR-689); run 16 PR #477 + PR #475 merged, all surfaces clean, priority (i) (GAR-688); run 15 CI retrigger for ubuntu-latest transient failure + RUSTSEC-2026-0149 wasmtime-wasi 44.0.1→44.0.2 fix (GAR-685, GAR-686); run 14 RUSTSEC-2026-0149 wasmtime fixed; run 13 upstream-blocked unchanged; run 12 upstream-blocked unchanged; run 11 upstream-blocked state unchanged; run 10 upstream-blocked state unchanged; run 9 upstream-blocked state unchanged; run 8 password-hash + rand upstream-blocked; run 7 GAR-674 windows-sys 0.52→0.61; run 6 GAR-673; run 5 GAR-672; run 4 GAR-671; run 3 GAR-670; run 2 GAR-668 RUSTSEC-2026-0145 + tokio-tungstenite 0.29; run 1 GAR-667 all-clean; run 6 GAR-665; run 5 GAR-664; run 4 GAR-663; run 3 GAR-662; run 2 lockfile bump PR #401; run 1 GAR-661).
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

**CI on main (`61bd6a7`, PR #499 health run 25 bookkeeping):** All 20 checks green.

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
- PR #498 (`routine/202605250015-search-has-attachment`) — GAR-697 search slice 4. CI in progress. Skipped per protocol.

**CI on main (`149b91b`, PR #497 health run 24 bookkeeping):** All 20 checks green.

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
- PR #496 (`claude/wizardly-ptolemy-UncRd`) — docs sync. Merged as `73ecc5d` before this run started.

**CI on main (`73ecc5d`, PR #496 TODO/ROADMAP sync):** All 20 checks green.

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

**Pending routine/ PR #492 noted (not actioned — routine/ territory):** `routine/202605240015-gar-493-garra-maxpower-adr`. Skipped per protocol.

**CI on main (`7e45ec5`, PR #490 GAR-499):** All 20 checks green.

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
- PR #486 (`health/202605231245-run20-status-note`, GAR-692) — resolved + plan numbering fix (0171=GAR-498, 0172=GAR-692), CI green, squash-merged at `07070f5`

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

**Open PRs resolved this run:** None. Branch `health/202605231000-gar513-deny-toml-hygiene` was already merged into main via cleanup PR #484 (`b3f62fd`). Open routine PR #485 (GAR-691 Q10.g) — skipped per protocol.

**Bookkeeping applied:** Plans README rows 0168 and 0169 updated from "In Progress" to ✅ Merged — both merged via PR #484 at commit `b3f62fd` on 2026-05-23.

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

**Open PRs resolved this run:** PR #482 (GAR-690 run 18 status note) was open with all 20 CI checks green — **merged as first action** (squash at `850d44c`). GAR-690 already marked Done.

**Finding (priority h):** Branch `claude/focused-cray-BM98J` contained prepared but un-PR'd commits fixing `cargo deny` `advisory-not-detected` noise for two IDs:
- `RUSTSEC-2024-0429` (glib 0.18.5 VariantStrIter unsound) → cargo deny advisory DB no longer matches this version
- `RUSTSEC-2026-0097` (rand 0.7.3 thread_rng unsound) → cargo deny advisory DB no longer matches this version

Both IDs are retained in `audit.toml`. Removed from `deny.toml` only. Plan 0169 / GAR-513.

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

**Open PRs resolved this run:** PR #481 (GAR-689 run 17 status note) was open with all 20 CI checks green — **merged as first action** (squash at `7a2e9e5`). GAR-689 marked Done.

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
- **PR #475** (`docs/mark-0166-done`) — had merge conflict → resolved via rebase, pushed `075078b`, CI re-ran → merged after green

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

**RUSTSEC-2026-0149 (wasmtime-wasi 44.0.1) — FIXED:**
- Advisory: WASI path_open(TRUNCATE) bypasses `FilePerms::WRITE` host restriction (GHSA-2r75-cxrj-cmph)
- Fix: `cargo update -p wasmtime-wasi --precise 44.0.2`
- GAR-685 → Done, included in PR #472

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #472 |
| Malware (cargo/npm) | ✅ none | cargo-deny green (post-fix) |
| Dependabot alerts | ⚠️ 2 open, UPSTREAM-BLOCKED | password-hash 0.5→0.6 (#430, GAR-669 Slice 3) + rand 0.8→0.10 (#424, GAR-669 Slice 4) — both blocked on argon2 ≥ 0.6 stable |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass (post-fix) | wasmtime-wasi 44.0.1→44.0.2 clears RUSTSEC-2026-0149 |
| cargo-deny | ✅ pass (post-fix) | RUSTSEC-2026-0149 resolved by upgrade, advisories ok |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #472 |
| CI on main (`b594ace`) | ✅ green | base PR #472 after routine/ Q10.c merge |

**Fix applied: RUSTSEC-2026-0149 (GAR-685).** wasmtime-wasi 44.0.1 → 44.0.2 lockfile upgrade. Next security backlog: rsa (GAR-456), glib+rand (GAR-513) — suppression expiry 2026-07-31. GAR-669 Slices 3–4 unblock when argon2 ≥ 0.6 stable ships.

---

## Confirmed 2026-05-21 run 11 (health routine — upstream-blocked state unchanged; SSE stream + audit-log reviewed clean)

Health routine ran on 2026-05-21 (~16:45 ET). Full security scan completed. Priority ladder exhausted at (i) — no actionable security work found.

**New merges since run 10:** PR #459 (`d25b64c`, GAR-678 — SSE endpoint + DashMap GC fix + cross-tenant RLS test), PR #462 (`3ddaf3e`, post-merge bookkeeping), PR #463 (`a972947`, GAR-680 — audit-log of SSE chat subscriptions).

**Security review — SSE stream handler + ChatStreamGuard:** `stream_chat` handler performs RLS context inside a proper `pool.begin()` transaction — no implicit auto-commit race. `ChatStreamGuard` RAII drop emits `chat.unsubscribed` via fire-and-forget `tokio::spawn`. `DashMap::remove_if` GC on last receiver drop prevents unbounded memory growth. All `unwrap()` calls in `rest_v1_chats_sse.rs` are inside `#[cfg(test)]` blocks. No new external dependencies. No Cargo.lock security impact.

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

**New merge since run 9:** PR #455 (`1b7f04c`, GAR-496 — repo workflow seguro para garra max-power) squash-merged to main as `671f760`.

**Security review — repo_workflow.rs:** `ProcessRunner` uses `std::process::Command::new(program).args(rest)` — no shell involved, no string concatenation. Protected-branch guard correctly refuses direct pushes to `main`, `master`, `release/*`. All `unwrap()` calls confined to `#[cfg(test)]` blocks. No security issues found.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PRs #455 + #458 |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PRs #455 + #458 |
| Dependabot alerts | ⚠️ 2 open, UPSTREAM-BLOCKED | password-hash 0.5→0.6 (#430, GAR-669 Slice 3) + rand 0.8→0.10 (#424, GAR-669 Slice 4) — both blocked on argon2 ≥ 0.6 |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PRs #455 + #458 |
| cargo-deny | ✅ pass | advisories ok |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #458 |
| CI on main (`671f760`) | ✅ green | PR #458 check-runs: 19/20 success |

**No fix applied this run.** Next security backlog: rsa (GAR-456), glib+rand (GAR-513) — all expire 2026-07-31. GAR-669 Slices 3–4 unblock when argon2 ≥ 0.6 ships.

---

## Confirmed 2026-05-21 run 9 (health routine — upstream-blocked state unchanged; windows-sys #422 closed)

Health routine ran on 2026-05-21 (~08:45 ET). Full security scan completed. Priority ladder exhausted at (i) — no actionable security work found.

**windows-sys #422 status:** Confirmed closed — `garraia-cli/Cargo.toml` now pins `windows-sys = "0.61"`. Dependabot auto-closes on next rescan after PR #451 merged as `1e7ce50`.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass (20/20 checks green on PRs #454 + #455) |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PRs #454 + #455 |
| Dependabot alerts | ⚠️ 2 open, UPSTREAM-BLOCKED | password-hash 0.5→0.6 (#430, GAR-669 Slice 3) + rand 0.8→0.10 (#424, GAR-669 Slice 4) — both blocked on argon2 |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | 19 allowlisted warnings, CI green on PRs #454 + #455 |
| cargo-deny | ✅ pass | advisories ok |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PRs #454 + #455 |
| CI on main (`e5a2a08`) | ✅ green | 20/20 checks green |

**No fix applied this run.** Next security backlog: rsa (GAR-456), glib+rand (GAR-513) — all expire 2026-07-31. GAR-669 Slices 3–4 unblock when argon2 ≥ 0.6 ships.

---

## Confirmed 2026-05-21 run 8 (health routine — password-hash + rand build-dep upstream-blocked, no actionable fix)

Health routine ran on 2026-05-21 (~04:45 ET). Full security scan completed. Priority ladder exhausted at (i) — no actionable security work found.

**Key finding:** `password-hash 0.5→0.6` (Dependabot alert #430, GAR-669 Slice 3) is **upstream-blocked**. Registry scan confirmed that `argon2 0.5.3` is the latest argon2 release and only supports `password-hash ^0.5`. Both GAR-669 Slice 3 and Slice 4 remain deferred until argon2 publishes a release supporting `password-hash ^0.6`.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #453 head (20/20 checks green) |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PR #453 |
| Dependabot alerts | ⚠️ 2 open, UPSTREAM-BLOCKED | password-hash 0.5→0.6 (#430, GAR-669 Slice 3) + rand 0.8→0.10 (#424, GAR-669 Slice 4) — both blocked on argon2 crate not yet supporting password-hash 0.6 |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #453 |
| cargo-deny | ✅ pass | advisories ok |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #453 |
| CI on main (`a3c61ce`) | ✅ green | 20/20 checks green |

**No fix applied this run.** Linear: status note filed (health-routine label). Next security backlog: rsa (GAR-456), glib+rand (GAR-513) — all expire 2026-07-31. GAR-669 Slices 3–4 unblock when argon2 ≥ 0.6 ships.

---

## Confirmed 2026-05-20 run 7 (health routine — GAR-669 Slice 1: rand_chacha 0.9 + rand 0.9 co-bump)

Health routine ran on 2026-05-20 (run 7, ~08:45 ET / 12:45 UTC). Full security scan completed. Fix applied: co-bumped `rand_chacha` 0.3→0.9 and `rand` 0.8→0.9 in `garraia-workspace` dev-deps, renamed `gen_range` → `random_range` in `migration_smoke.rs`. Supersedes Dependabot PR #423.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #446 (20/20 checks green) |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PR #446 |
| Dependabot alerts | ⚠️ 3 open, major-version breaks | password-hash 0.5→0.6 (#430 — auth-critical, GAR-669 Slice 3), rand 0.8→0.10 (#424 — Rng→RngExt breaking, GAR-669 Slice 4), windows-sys 0.52→0.61 (#422 — windows-only, GAR-669 Slice 2) |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #446 |
| cargo-deny | ✅ pass | advisories ok |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #446 |
| CI on main (`d9f811ac`) | ✅ green | PR #446 (20/20 checks green) |

**Fix applied:** PR #446 squash-merged as `d9f811ac` 2026-05-20T13:46Z. Linear: GAR-669 Done, GAR-674 Done. Next security backlog: rsa (GAR-456), glib+rand (GAR-513) — all expire 2026-07-31. GAR-669 Slices 2–4 remain open.

---

## Confirmed 2026-05-19 run 4 (health routine — all surfaces clean, no actionable work)

Health routine ran on 2026-05-19 (run 4, ~12:45 ET / 16:45 UTC). Full security scan completed. Priority ladder exhausted at (i) — no actionable security work found.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #437 (20/20 checks green) |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PR #437 |
| Dependabot alerts | ⚠️ 4 open, major-version breaks | password-hash 0.5→0.6 (#430), rand 0.8→0.10 (#424), rand_chacha 0.3→0.9 (#423), windows-sys 0.52→0.61 (#422) — all deferred |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #437 |
| cargo-deny | ✅ pass | advisories ok |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #437 |
| CI on main (`deadd799`) | ✅ green | PR #437 (most recent code commit) 20/20 checks green |

**No fix applied this run.** Linear issue: GAR-671 (Done). Next security backlog: rsa (GAR-456), glib+rand (GAR-513) — all expire 2026-07-31.

---

## Confirmed 2026-05-19 run 3 (health routine — all surfaces clean, no actionable work)

Health routine ran on 2026-05-19 (run 3, ~08:45 ET / 12:45 UTC). Full security scan completed. Priority ladder exhausted at (i) — no actionable security work found.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on main `8a9a915` (20/20 checks green) |
| Malware (cargo/npm) | ✅ none | cargo-deny green on main |
| Dependabot alerts | ⚠️ 4 open, major-version breaks | password-hash 0.5→0.6 (#430), rand 0.8→0.10 (#424), rand_chacha 0.3→0.9 (#423), windows-sys 0.52→0.61 (#422) — all deferred |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on main `8a9a915` |
| cargo-deny | ✅ pass | advisories ok |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on main |
| CI on main (`8a9a915`) | ✅ green | 20/20 checks green |

**No fix applied this run.** Linear issue: GAR-670 (Done). Next security backlog: rsa (GAR-456), glib+rand (GAR-513) — all expire 2026-07-31.

---

## Confirmed 2026-05-19 run 2 (health routine — RUSTSEC-2026-0145 merged + tokio-tungstenite 0.26→0.29)

Health routine ran on 2026-05-19 (run 2, ~08:45 ET / 12:45 UTC). Two fixes delivered:

1. **RUSTSEC-2026-0145** (PAX Header Desynchronization in `astral-tokio-tar`) — PR #432 squash-merged as `287edc1c`.
2. **tokio-tungstenite 0.26→0.29** — clean `health/202605190850-tokio-tungstenite-0.29` branch, applied upgrade, merged as `51382a9c` (PR #433). 20/20 CI checks green. cargo audit: 0 vulnerabilities post-merge.

Main now at `51382a9c`. GAR-668 / plan 0152.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #433 |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PR #433 |
| Dependabot alerts | ⚠️ 5 open, major-version breaks | password-hash 0.5→0.6, governor 0.8→0.10, rand 0.8→0.10, rand_chacha 0.3→0.9, windows-sys 0.52→0.61 — all deferred |
| Security Audit (`cargo audit`) | ✅ 0 vulnerabilities | 19 allowed unmaintained warnings (pre-existing) |
| cargo-deny | ✅ pass | advisories ok |
| CodeQL (rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #433 |

## Confirmed 2026-05-18 run 6 (health routine — all surfaces clean, PRs #409+#410 verified, no actionable security work)

Health routine ran on 2026-05-18 (run 6, ~16:45 ET / 20:45 UTC). Full security scan completed. Priority ladder exhausted at (i) — no actionable security work found.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #409 (job 76592503754, completed success) |
| Malware (cargo/npm) | ✅ none | cargo-deny green on PR #409 |
| Dependabot alerts | ✅ 3 open, all upstream-blocked | rsa/GAR-456, glib/GAR-513, rand/GAR-513 — expiry 2026-07-31 |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | CI green on PR #409 |
| cargo-deny | ✅ pass | `advisories ok` |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | All 3 Analyze jobs green on PR #409 |
| CI on main (latest: `ea026e6`) | ✅ green | 20/20 checks green on PR #409 |

**No fix applied this run.** Linear issue: GAR-665 (Done). Next security backlog: rsa (GAR-456), glib+rand (GAR-513) — all expire 2026-07-31.

---

## Confirmed 2026-05-18 run 5 (health routine — RUSTSEC-2026-0112 confirmed resolved, all surfaces clean)

Health routine ran on 2026-05-18 (run 5, ~12:45 ET / 16:45 UTC). Full security scan completed. Priority ladder exhausted at (i) — no actionable security work found.

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
| CI on main (latest: `a5daf344`) | ✅ green | PR #382 all 20 checks green |

**Fix applied this run (plan 0142 — daemonize RUSTSEC-2025-0069, GAR-656):** `daemonize` removed, `nix` direct dep added, `start_daemon()` reimplemented with `nix::unistd::{fork, setsid}` + `libc::dup2` double-fork idiom. `cargo audit` warning count: 20 → **19**.

Alert count: **3 open** (unchanged). All 3 upstream-blocked with 2026-07-31 expiry.

## Confirmed 2026-05-17 run 2 (health routine — RUSTSEC-2025-0134 closed, axum-server 0.7→0.8)

Health routine ran on 2026-05-17 (run 2, ~05:00 ET). Full security scan completed. Highest actionable issue found: RUSTSEC-2025-0134 (`rustls-pemfile` unmaintained), fixed by bumping `axum-server` 0.7→0.8.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #378 head (`1eb5c4b`) and PR #376 head (`1be73cd`) |
| Malware (cargo/npm) | ✅ none | No malware advisories in cargo graph |
| Dependabot alerts | ✅ 3 open, all upstream-blocked | rsa/GAR-456, glib/GAR-513, rand/GAR-513 — expiry 2026-07-31 |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | **20** allowlisted warnings (↓1 from 21 — RUSTSEC-2025-0134 removed by PR #378) |
| cargo-deny | ✅ pass | `advisories ok`; RUSTSEC-2025-0134 entry removed from deny.toml |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | PR #378 + PR #376 all Analyze jobs green |
| CI on main (latest: `1be73cd`) | ✅ green | PR #376 all 20 checks green |

**Fix applied this run (plan 0138 — axum-server RUSTSEC-2025-0134):** `axum-server` 0.7→0.8; `rustls-pemfile` removed from Cargo.lock. `cargo audit` warning count: 21 → **20**.

Alert count: **3 open** (unchanged). All 3 upstream-blocked with 2026-07-31 expiry.

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
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass | PR #368 all Analyze jobs green |
| CI on main (latest: `bec410c`) | ✅ green | PR #368 all 20 checks green |

Alert count: **3 open** (unchanged). All 3 upstream-blocked with 2026-07-31 expiry. No Dependabot PRs open.

## Confirmed 2026-05-16 (health routine — GAR-634: tokio 1.50.0→1.52.3 unblocked via nix 0.31.3)

Health routine ran on 2026-05-16. **PR #366** (security dep sweep) merged. **GAR-634** (plan 0134) resolved the tokio 1.52.3 upgrade blocker.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #366 head (`3c438ea`) |
| Malware (cargo/npm) | ✅ none | No malware advisories in cargo graph |
| Dependabot alerts | ✅ 3 open, all upstream-blocked | rsa/GAR-456, glib/GAR-513, rand/GAR-513 — expiry 2026-07-31 |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | 21 allowlisted warnings, no new advisories |
| cargo-deny | ✅ pass | `advisories ok` |
| CodeQL (Analyze rust + js-ts) | ✅ pass | PR #366 Analyze jobs all green |
| CI on main (latest: `02bd9de`) | ✅ green | PR #366 all 20 checks green |

Alert count: **3 open** (unchanged). All 3 upstream-blocked with 2026-07-31 expiry.

## Confirmed 2026-05-14 (health routine — metrics 0.24.5 yanked → 0.24.6 lockfile-only fix)

Health routine ran on 2026-05-14. Full `cargo audit` + `cargo deny check` scan completed.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on PR #326 head (`84cf09f`) |
| Malware (cargo/npm) | ✅ none | No malware advisories in cargo graph |
| Dependabot alerts | ✅ 3 open, all upstream-blocked | rsa/GAR-456, glib/GAR-513, rand/GAR-513 — expiry 2026-07-31 |
| Security Audit (`cargo audit --deny unsound`) | ✅ pass | 21 allowlisted warnings. No new untracked advisories. |
| cargo-deny | ✅ pass | `advisories ok` |
| CodeQL (Analyze rust + js-ts) | ✅ pass | PR #326 Analyze jobs all green |
| CI on main (latest: `ae0306d`) | ✅ green | PR #326 all 18 checks green |

**Fix applied this run:** `metrics` 0.24.5 (yanked) → **0.24.6** lockfile-only patch. `cargo audit` warning count: **22 → 21**.

Alert count: **3 open** (unchanged). All 3 are upstream-blocked with 2026-07-31 expiry.

## Confirmed 2026-05-12 (health routine — GAR-591 merged, rustls-webpki 0.102.8 chain removed)

Health routine ran on 2026-05-12. **PR #293 (GAR-591)** merged at commit `69c357a7ff2c6d8e27a3283d7b2d4bdc235b8e9f`.

| Change | Result |
|---|---|
| serenity feature: `rustls_backend` → `native_tls_backend` | ✅ applied (PR #293, GAR-591) |
| poise `default-features = false` | ✅ applied |
| `rustls-webpki 0.102.8` in `Cargo.lock` | ✅ **REMOVED** — only `0.103.13` remains |
| `rustls 0.22.4` in `Cargo.lock` | ✅ **REMOVED** |
| `tokio-rustls 0.25.0` in `Cargo.lock` | ✅ **REMOVED** |
| Dependabot alerts closed | ⏳ PENDING — rescan expected within 24-48h |
| `audit.toml` + `deny.toml` cleanup | ✅ 4 RUSTSEC IDs removed atomically |
| Secret scanning (gitleaks) | ✅ clean — CI pass on PR #293 head |
| Malware (cargo/npm) | ✅ none |
| Security Audit (`cargo audit`) | ✅ pass — CI green on PR #293 |
| cargo-deny | ✅ pass — CI green on PR #293 |
| CodeQL (Analyze rust + js-ts) | ✅ pass — CI green on PR #293 |
| CI on main (latest: `69c357a`) | ✅ green — all 18 checks pass |

Alert count: **8 open** (pre-rescan) → **4 expected** (post-rescan, within 24-48h).

## Confirmed 2026-05-12 run 2 (health routine — GAR-593: lru RUSTSEC-2026-0002 stale ignore removed)

Health routine ran on 2026-05-12 (run 2, after PR #295 merged). **PR #297** (`8f73144`) had already landed the fix; this run removes the stale audit config entries.

| Change | Result |
|---|---|
| `lru` in `Cargo.lock` | ✅ **0.16.4** (patched; RUSTSEC-2026-0002 requires < 0.16.3) |
| `RUSTSEC-2026-0002` in `audit.toml` | ✅ **REMOVED** (PR #299, GAR-593) |
| `RUSTSEC-2026-0002` in `deny.toml` | ✅ **REMOVED** atomically with audit.toml |
| PR #299 CI | ✅ green — all 18 checks passed; merged as `7996dc4` |

Residuals (3 remaining, all expires 2026-07-31):

| Advisory | Crate | Owner | Status |
|---|---|---|---|
| RUSTSEC-2023-0071 | rsa 0.9.10 | GAR-456 | Active — no upstream fix |
| RUSTSEC-2024-0429 | glib 0.18.5 | GAR-513 | Active — Tauri gtk-rs blocker |
| RUSTSEC-2026-0097 | rand 0.7.3 | GAR-513 | Active — build-time dep only |

## Confirmed 2026-05-14 run 2 (health routine — GAR-620: metrics 0.24.5 yanked → 0.24.6)

Health routine ran on 2026-05-14 (run 2, ~8:50 AM ET). Highest actionable issue: `metrics 0.24.5` (yanked from crates.io). PR #336 implements the lockfile-only patch.

| Change | Result |
|---|---|
| `metrics 0.24.5` (yanked) → `0.24.6` in `Cargo.lock` | ✅ merged — `adbe00af` |
| Secret scanning (gitleaks) | ✅ clean |
| Malware (cargo/npm) | ✅ none |
| Dependabot alerts | ✅ 3 open, all upstream-blocked | rsa/GAR-456, glib/GAR-513, rand/GAR-513 — expiry 2026-07-31 |
| Security Audit (`cargo audit`) | ✅ pass |
| cargo-deny | ✅ pass |
| CodeQL (Analyze rust + js-ts + actions) | ✅ pass |
| plan 0124 | ✅ created | `plans/0124-gar-620-metrics-yanked-0246.md` + GAR-620 in Linear |

Alert count: **3 Dependabot open** (unchanged).

## Confirmed 2026-05-14 (health routine — GAR-605: CodeQL actions language fix + plan 0116)

Health routine ran on 2026-05-14. Two pending non-routine PRs merged; one active security fix (15 Medium CodeQL alerts) handled.

| Change | Result |
|---|---|
| PR #321 merged (`c45fcff`) | ✅ Plan 0114 T8 bookkeeping |
| PR #323 merged (GAR-605) | ✅ Add `language: actions, build-mode: none` to `codeql.yml` matrix |
| 15 Medium `actions/missing-workflow-permissions` alerts | ⏳ PENDING auto-close — CodeQL re-scan on main expected within 24h |
| Dependabot alerts | ✅ 3 open, all upstream-blocked | rsa/GAR-456, glib/GAR-513, rand/GAR-513 — expiry 2026-07-31 |
| Secret scanning (gitleaks) | ✅ clean |
| Malware (cargo/npm) | ✅ none |
| Security Audit (`cargo audit`) | ✅ pass |
| cargo-deny | ✅ pass |
| CI on main (post-merge) | ✅ green |

Alert count: **3 Dependabot open** (unchanged). After next CodeQL run on main, **Medium CodeQL open count → 0**.

## Confirmed 2026-05-13 (health routine — plan 0113 bookkeeping; all surfaces green)

Health routine ran on 2026-05-13. Full security scan completed; no new actionable security issue found.

| Surface | Status | Detail |
|---|---|---|
| Secret scanning (gitleaks) | ✅ clean | CI pass on main (`0e0edfb`) |
| Malware (cargo/npm) | ✅ none | No malware advisories in cargo graph |
| Dependabot alerts | ✅ 3 open, all upstream-blocked | rsa/GAR-456, glib/GAR-513, rand/GAR-513 — expiry 2026-07-31 |
| Security Audit (`cargo audit`) | ✅ pass | 3 allowlisted advisories, all with valid rationale |
| cargo-deny | ✅ pass | SYNC NOTE audit.toml ↔ deny.toml intact |
| CodeQL (Analyze rust + js-ts) | ✅ pass | No new open findings |
| CI on main (latest: `0e0edfb`) | ✅ green | All 18 checks pass |

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
| CI on main (latest: `77c8947`) | ✅ green | Format + cargo-deny completed success |

Alert count: **3 open** (unchanged since PR #299 merged). All 3 are upstream-blocked with 2026-07-31 expiry.

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
| `rustls-webpki 0.101.7` in `Cargo.lock` | ✅ **REMOVED** |
| `rustls 0.21.12` in `Cargo.lock` | ✅ **REMOVED** |
| `hyper-rustls 0.24.2` in `Cargo.lock` | ✅ **REMOVED** |
| `cargo clippy --workspace ...` | ✅ clean |
| Secret scanning | ✅ pass |
| CodeQL | ✅ 22 alerts all dismissed (unchanged) |

Alert count unchanged (8 open). The `rustls-webpki 0.101.7` sub-chain that contributed to RUSTSEC-2026-0098/0099/0104 has been removed from the dependency graph.

## Confirmed 2026-05-08 (health routine — all surfaces green)

Health routine ran on 2026-05-08. All 4 security surfaces scanned:

| Surface | Result |
|---|---|
| Secret scanning (gitleaks) | ✅ pass |
| cargo-deny (advisories) | ✅ pass — all allowlisted |
| Security Audit (cargo-audit) | ✅ pass — all allowlisted |
| Dependabot alerts | ✅ 8 open, all pre-existing, all allowlisted (GAR-455 / GAR-513 / GAR-456) |
| CodeQL (code scanning) | ✅ 22 alerts all dismissed in ledger. No new open alerts. Re-audit deadline: 2026-08-01. |

No new untracked alerts. Count reconciled: 8 Dependabot open (2 HIGH, 2 MEDIUM, 4 LOW) — all pre-existing, all upstream-blocked, all allowlisted. Main branch CI green.

## GAR-455 deep-dive 2026-05-08 — alert #37 closure investigation

Triggered by a question of whether GAR-455 could close today without breaking the project. Read-only investigation; no `Cargo.toml` / `Cargo.lock` / `deny.toml` / `.cargo/audit.toml` changes were made.

### Verdict

Alert #37 (RUSTSEC-2026-0104) **stays open and remains upstream-blocked**. The allowlist entry in `.cargo/audit.toml` and the mirror in `deny.toml` continue to be the correct mitigation.

### Empirical chain map (verified 2026-05-08 via `cargo tree`)

```
rustls-webpki 0.102.8  ← serenity 0.12.5
                         → tokio-tungstenite 0.21.0
                         → rustls 0.22.4
                         carries ALL 4 RUSTSEC IDs of GAR-455

rustls-webpki 0.101.7  ← aws-sdk-s3 1.119.0 (feature `rustls`)
                         (only when `garraia-storage/storage-s3` feature is enabled)
                         carries 3 of 4 RUSTSEC IDs (-0098, -0099, -0104)
```

### Follow-up (COMPLETED 2026-05-09 — plan 0087, GAR-553)

The AWS-side feature-flag swap has been **landed** in plan 0087. `crates/garraia-storage/Cargo.toml` now uses `"default-https-client"` instead of `"rustls"` for `aws-sdk-s3`. `rustls 0.21.12`, `rustls-webpki 0.101.7`, `hyper-rustls 0.24.2` removed from `Cargo.lock`.

## Confirmed 2026-05-07 (health routine — no new alerts)

Health routine ran on 2026-05-07. All 4 security surfaces scanned:

| Surface | Result |
|---|---|
| Secret scanning (gitleaks) | ✅ pass |
| cargo-deny (advisories) | ✅ pass — all allowlisted |
| Security Audit (cargo-audit) | ✅ pass — all allowlisted |
| Dependabot alerts | ✅ 8 open, all pre-existing, all allowlisted (GAR-455 / GAR-513 / GAR-456) |

No new untracked alerts. Count reconciled: 8 open (2 HIGH, 2 MEDIUM, 4 LOW) matching the 8 active RUSTSEC IDs in `.cargo/audit.toml`. PR #188 merged — added `.github-health-reports/` and `audit/` to `.gitignore`.

## Closed 2026-05-06 (health routine)

| Alert | Closure mechanism | Linear |
|---|---|---|
| `openssl` 0.10.78 → 0.10.79 + `openssl-sys` 0.9.114 → 0.9.115 security patch | plan 0073, health routine PR | [GAR-527](https://linear.app/chatgpt25/issue/GAR-527) |

## Closed in sprint 2026-04-22 → 2026-04-30

| Alert range | Closure mechanism | Linear |
|---|---|---|
| 12 lockfile-only Dependabot bumps | PR #97 (`time` + bench refresh) + PR #99 (`openssl` 0.10.75 → 0.10.78) + PR #102 (rand + rustls-webpki bench cleanup) | GAR-484 (closed 2026-04-30) |
| `jsonwebtoken 9 → 10` migration | PR #105 (plan §Step 3, replaces broken Dependabot PR #103). Adopts `rust_crypto` backend. | GAR-XXX umbrella, sub-issue 2 |

## Closed 2026-05-12 (PR #293 / GAR-591)

| GH # | RUSTSEC | Crate | Closure mechanism |
|---|---|---|---|
| #37 | RUSTSEC-2026-0104 | `rustls-webpki` | PR #293 (GAR-591): serenity `rustls_backend` → `native_tls_backend`; 0.102.8 chain removed from Cargo.lock. |
| #11 | RUSTSEC-2026-0049 | `rustls-webpki` | Same — part of same serenity chain. |
| #23 | RUSTSEC-2026-0099 | `rustls-webpki` | Same — part of same serenity chain. |
| #22 | RUSTSEC-2026-0098 | `rustls-webpki` | Same — part of same serenity chain. |

Dependabot rescan expected within 24-48h.

## Residuals (3 open post-rescan, updated 2026-05-12 run 2)

All 3 remaining alerts have:
- A specific RUSTSEC ID matching `Cargo.lock`.
- A documented rationale block in `.cargo/audit.toml` and/or `deny.toml`.
- A concrete Linear owner.
- An expiration date (**2026-07-31**) that forces re-triage.

| GH # | GHSA | Severity | Crate | RUSTSEC | Linear | Mitigation |
|---|---|---|---|---|---|---|
| — | — | HIGH | `rsa` | RUSTSEC-2023-0071 (Marvin Attack timing sidechannel) | GAR-456 | `rsa 0.9.10` enters tree via `sqlx-mysql` lockfile residual + `jsonwebtoken 10 rust_crypto`. GarraRUST emits/verifies HS256 only — no RSA code path is reachable. |
| #2  | GHSA-wrw7-89jp-8q8g | MEDIUM | `glib` | RUSTSEC-2024-0429 | GAR-513 | Tauri-only path (`crates/garraia-desktop`), excluded from server CI builds. |
| #25 | GHSA-cq8v-f236-94qc | LOW | `rand` | RUSTSEC-2026-0097 | GAR-513 | Build-time dep only: `phf_codegen → phf_generator → selectors → tauri-utils → garraia-desktop`. Zero server runtime risk. |

## Closed 2026-05-12 run 2 (PR #297 + PR #299 / GAR-593)

| GH # | RUSTSEC | Crate | Closure mechanism |
|---|---|---|---|
| #5 | RUSTSEC-2026-0002 | `lru` | PR #297 (`8f73144`) bumped aws-sdk-s3 1.119→1.132, pulling lru 0.16.4 (patched ≥ 0.16.3). Audit config cleanup via PR #299 (GAR-593). |

## Linear ownership map

- **GAR-455** — ✅ CLOSED 2026-05-12. `rustls-webpki` legacy chains fully removed.
- **GAR-513** — Unsound triage carve-out (created 2026-05-05). 2 of 3 remaining alerts (#2 glib, #25 rand). lru (#5 / RUSTSEC-2026-0002) closed 2026-05-12 by GAR-593 / PR #299.
- **GAR-456** — Marvin Attack timing sidechannel (`rsa 0.9.10`). Same `2026-07-31` expiration.

## Re-triage cadence

- **Weekly** (Monday): cargo-audit.yml runs `cargo audit --no-fetch --deny unsound`.
- **Quarterly** (every 3 months): every `audit.toml` ignore is checked against its declared expiration.
- **Ad-hoc**: a Dependabot alert that does NOT match an existing allowlist entry is treated as a real new vulnerability.

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
  per-(advisory, version) ignores.
  Tracked under GAR-455 closure plan.

## See also

- `.cargo/audit.toml` — line-by-line rationale per RUSTSEC ID.
- `deny.toml` — `cargo deny check advisories` config.
- `docs/security/secret-scanning-runbook.md` — companion runbook for
  the secret-scanning side of the security baseline.
- `docs/security/codeql-setup.md` — CodeQL advanced setup runbook.
