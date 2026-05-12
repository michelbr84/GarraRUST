# Dependabot Status

> Last updated: **2026-05-12** (health routine — PR #293 / GAR-591 merged; 4 rustls-webpki alerts pending auto-close via Dependabot rescan within 24-48h).
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

## Residuals (4 open post-rescan, updated 2026-05-12)

All 4 remaining alerts have:
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
| #5  | GHSA-rhfx-m35p-ff5j | LOW | `lru` | RUSTSEC-2026-0002 (`IterMut` Stacked Borrows violation) | GAR-513 | Transitive via `aws-sdk-s3 1.119.0` (feature `storage-s3` of `garraia-storage`). `Cargo.lock` resolution is feature-agnostic — alert appears even when feature off. Closes when aws-sdk-s3 bumps lru, OR when `storage-s3` is excluded from cargo audit surface. |

## Linear ownership map

- **GAR-455** — ✅ CLOSED 2026-05-12. `rustls-webpki` legacy chains fully removed. Both chains eliminated: aws-smithy (plan 0087, 2026-05-09) + serenity (PR #293, 2026-05-12). 4 of 8 former alerts (#37, #11, #23, #22) closing pending Dependabot rescan.
- **GAR-513** — Unsound triage carve-out (created 2026-05-05; GAR-437 closed 2026-04-27). 3 of 4 remaining alerts (#2 glib, #25 rand, #5 lru). Each tracked individually as upstream fixes ship.
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
# (the two MUST share the wasmtime IDs (15) AND rustls-webpki residuals (4)
#  per .cargo/audit.toml SYNC NOTE.)

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
