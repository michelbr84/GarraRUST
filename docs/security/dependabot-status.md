# Dependabot Status

> Last updated: **2026-05-21 run 10** (health routine — upstream-blocked state unchanged; GAR-496 repo-workflow merged (PR #455 / 671f760); repo_workflow.rs security review clean; 2 alerts remain. Previous: run 9 upstream-blocked state unchanged; run 8 password-hash + rand upstream-blocked; run 7 GAR-674 windows-sys 0.52→0.61; run 6 GAR-673; run 5 GAR-672; run 4 GAR-671; run 3 GAR-670; run 2 GAR-668 RUSTSEC-2026-0145 + tokio-tungstenite 0.29; run 1 GAR-667 all-clean; run 6 GAR-665; run 5 GAR-664; run 4 GAR-663; run 3 GAR-662; run 2 lockfile bump PR #401; run 1 GAR-661).
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
