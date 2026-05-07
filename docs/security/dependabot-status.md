# Dependabot Status

> Last updated: **2026-05-07** (health routine — post-PR #188 scan).
> Source of truth: `.cargo/audit.toml` and `deny.toml` (the suppression
> rationale lives there, this file is the alert-to-rationale index).

## Snapshot

| Metric | 2026-04-22 | 2026-04-30 (last sprint) | 2026-05-06 | 2026-05-07 (today) |
|---|---|---|---|---|
| Total Dependabot alerts open | 20 | **7** | **8** (rsa added as 2nd entry via jsonwebtoken 10 rust_crypto backend 2026-04-30; openssl patched and closed) | **8** (confirmed) |
| High severity | 1 | 1 | **2** (rustls-webpki #37 + rsa RUSTSEC-2023-0071) | **2** |
| Medium severity | 4 | 2 | 2 | **2** |
| Low severity | 4 | 4 | 4 | **4** |
| With Linear ownership | mixed | **7 / 7** | **8 / 8** | **8 / 8** |

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

## Residuals (8 open, updated 2026-05-07)

All 8 alerts already have:
- A specific RUSTSEC ID matching `Cargo.lock`.
- A documented rationale block in `.cargo/audit.toml` and/or `deny.toml`.
- A concrete Linear owner.
- An expiration date (**2026-07-31**) that forces re-triage.

The `cargo audit` and `cargo deny` CI gates pass green because each entry
is intentionally allowlisted, not silenced.

| GH # | GHSA | Severity | Crate | RUSTSEC | Linear | Mitigation |
|---|---|---|---|---|---|---|
| #37 | GHSA-82j2-j2ch-gfr8 | HIGH | `rustls-webpki` | RUSTSEC-2026-0104 (panic in CRL parsing) | GAR-455 | Production hot path patched to `rustls-webpki 0.103.13` in plan 0053 PR-1 (PR #75). Residual lives in legacy `rustls-webpki 0.102.8` (serenity 0.12.5 EOL chain) and `0.101.7` (aws-smithy-http-client `storage-s3` feature). Closes when serenity 0.13 + aws-smithy upgrade lands. |
| — | — | HIGH | `rsa` | RUSTSEC-2023-0071 (Marvin Attack timing sidechannel) | GAR-456 | `rsa 0.9.10` enters tree via two paths: (1) `sqlx-mysql` lockfile residual even with `default-features = false` on all sqlx deps; (2) `jsonwebtoken 10 rust_crypto` backend (added 2026-04-30). GarraRUST emits/verifies HS256 only (`Algorithm::HS256` in `garraia-auth/src/jwt.rs`) — no RSA code path is reachable. Fix paths: (a) `jsonwebtoken` upstream isolates `rsa` behind `asymmetric` feature; (b) migrate to `sqlx-postgres` direct or sqlx 0.9. |
| #11 | GHSA-pwjx-qhcg-rvj4 | MEDIUM | `rustls-webpki` | RUSTSEC-2026-0049 (CRL Distribution Point matching) | GAR-455 | Same legacy chains as #37. Closes with the same upgrade. |
| #2  | GHSA-wrw7-89jp-8q8g | MEDIUM | `glib` | RUSTSEC-2024-0429 (`VariantStrIter` Iterator unsoundness) | GAR-513 | Tauri-only path (`crates/garraia-desktop`), excluded from server CI builds. Low runtime risk in deployments. Fix path: bump glib OR gate ignore behind `desktop` feature. |
| #25 | GHSA-cq8v-f236-94qc | LOW | `rand` | RUSTSEC-2026-0097 (custom logger unsoundness in `rand::rng()`) | GAR-513 | Build-time dep only: `phf_codegen → phf_generator → selectors → tauri-utils → garraia-desktop`. Zero server runtime risk. No 0.7.x patch; fix requires phf_codegen to bump rand. |
| #5  | GHSA-rhfx-m35p-ff5j | LOW | `lru` | RUSTSEC-2026-0002 (`IterMut` Stacked Borrows violation) | GAR-513 | Transitive via `aws-sdk-s3 1.119.0` (feature `storage-s3` of `garraia-storage`). `Cargo.lock` resolution is feature-agnostic — alert appears even when feature off. Closes when aws-sdk-s3 bumps lru, OR when `storage-s3` is excluded from cargo audit surface. |
| #23 | GHSA-xgp8-3hg3-c2mh | LOW | `rustls-webpki` | RUSTSEC-2026-0099 (wildcard in name-constrained) | GAR-455 | Same legacy chains as #37. Closes with the same upgrade. |
| #22 | GHSA-965h-392x-2mh5 | LOW | `rustls-webpki` | RUSTSEC-2026-0098 (URI name constraints incorrectly accepted) | GAR-455 | Same legacy chains as #37. Closes with the same upgrade. |

## Linear ownership map

- **GAR-455** — `rustls-webpki` legacy chains. 4 of 8 alerts (#37, #11, #23, #22). Closes when `serenity 0.12.5 → 0.13` AND `aws-smithy-http-client 1.1.12 → next` upgrades land. Both are upstream-blocked today.
- **GAR-513** — Unsound triage carve-out (created 2026-05-05; GAR-437 closed 2026-04-27). 3 of 8 alerts (#2 glib, #25 rand, #5 lru). Each tracked individually as upstream fixes ship.
- **GAR-456** — Marvin Attack timing sidechannel (`rsa 0.9.10`). 1 of 8 alerts (RUSTSEC-2023-0071; GH alert number unknown — cargo audit detects it as workspace advisory). GarraRUST emits and verifies HS256 only; no RSA call site is reachable. Same `2026-07-31` expiration.

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
