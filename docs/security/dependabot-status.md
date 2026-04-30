# Dependabot Status

> Last updated: **2026-04-30** (Green Security Baseline sprint).
> Source of truth: `.cargo/audit.toml` and `deny.toml` (the suppression
> rationale lives there, this file is the alert-to-rationale index).

## Snapshot

| Metric | 2026-04-22 | 2026-04-30 (this sprint) |
|---|---|---|
| Total Dependabot alerts open | 20 | **7** |
| High severity | 1 | 1 |
| Medium severity | 4 | 2 |
| Low severity | 4 | 4 |
| With Linear ownership | mixed | **7 / 7** |

## Closed in this sprint (2026-04-22 → 2026-04-30)

| Alert range | Closure mechanism | Linear |
|---|---|---|
| 12 lockfile-only Dependabot bumps | PR #97 (`time` + bench refresh) + PR #99 (`openssl` 0.10.75 → 0.10.78) + PR #102 (rand + rustls-webpki bench cleanup) | GAR-484 (closed 2026-04-30) |
| `jsonwebtoken 9 → 10` migration | PR #105 (this sprint, plan `personal-api-key-revogada-vectorized-matsumoto` §Step 3, replaces broken Dependabot PR #103). Adopts `rust_crypto` backend + decouples `garraia-auth` from `rand` churn via direct `getrandom::fill`. | GAR-XXX umbrella, sub-issue 2 |

## Residuals (7 open, 2026-04-30)

All 7 alerts already have:
- A specific RUSTSEC ID matching `Cargo.lock`.
- A documented rationale block in `.cargo/audit.toml` and/or `deny.toml`.
- A concrete Linear owner.
- An expiration date (**2026-07-31**) that forces re-triage.

The `cargo audit` and `cargo deny` CI gates pass green because each entry
is intentionally allowlisted, not silenced.

| GH # | GHSA | Severity | Crate | RUSTSEC | Linear | Mitigation |
|---|---|---|---|---|---|---|
| #37 | GHSA-82j2-j2ch-gfr8 | HIGH | `rustls-webpki` | RUSTSEC-2026-0104 (panic in CRL parsing) | GAR-455 | Production hot path patched to `rustls-webpki 0.103.13` in plan 0053 PR-1 (PR #75). Residual lives in legacy `rustls-webpki 0.102.8` (serenity 0.12.5 EOL chain) and `0.101.7` (aws-smithy-http-client `storage-s3` feature). Closes when serenity 0.13 + aws-smithy upgrade lands. |
| #11 | GHSA-pwjx-qhcg-rvj4 | MEDIUM | `rustls-webpki` | RUSTSEC-2026-0049 (CRL Distribution Point matching) | GAR-455 | Same legacy chains as #37. Closes with the same upgrade. |
| #2  | GHSA-wrw7-89jp-8q8g | MEDIUM | `glib` | RUSTSEC-2024-0429 (`VariantStrIter` Iterator unsoundness) | GAR-437 | Tauri-only path (`crates/garraia-desktop`), excluded from server CI builds. Low runtime risk in deployments. Fix path: bump glib OR gate ignore behind `desktop` feature. |
| #25 | GHSA-cq8v-f236-94qc | LOW | `rand` | RUSTSEC-2026-0097 (custom logger unsoundness in `rand::rng()`) | GAR-437 | Pre-bump audit of call sites required because `rand 0.8 → 0.9 → 0.10` are semver-incompatible. PR B (#105) reduced exposure by removing runtime `rand` from `garraia-auth` (now uses `getrandom::fill` direct). |
| #5  | GHSA-rhfx-m35p-ff5j | LOW | `lru` | RUSTSEC-2026-0002 (`IterMut` Stacked Borrows violation) | GAR-437 | Transitive via `aws-sdk-s3 1.119.0` (feature `storage-s3` of `garraia-storage`). `Cargo.lock` resolution is feature-agnostic — alert appears even when feature off. Closes when aws-sdk-s3 bumps lru, OR when `storage-s3` is excluded from cargo audit surface. |
| #23 | GHSA-xgp8-3hg3-c2mh | LOW | `rustls-webpki` | RUSTSEC-2026-0099 (wildcard in name-constrained) | GAR-455 | Same legacy chains as #37. Closes with the same upgrade. |
| #22 | GHSA-965h-392x-2mh5 | LOW | `rustls-webpki` | RUSTSEC-2026-0098 (URI name constraints incorrectly accepted) | GAR-455 | Same legacy chains as #37. Closes with the same upgrade. |

## Linear ownership map

- **GAR-455** — `rustls-webpki` legacy chains. 4 of 7 alerts (#37, #11, #23, #22). Closes when `serenity 0.12.5 → 0.13` AND `aws-smithy-http-client 1.1.12 → next` upgrades land. Both are upstream-blocked today.
- **GAR-437** — Q7 RUSTSEC unsound triage epic. 3 of 7 alerts (#2 glib, #25 rand, #5 lru). Each tracked under a sub-issue or carved out individually as upstream fixes ship.
- **GAR-456** — Marvin Attack timing sidechannel (`rsa 0.9.10`). NOT in the Dependabot alerts above (cargo audit lists it as a workspace-wide allow under a separate ID flow), but documented here for completeness because PR B's `rust_crypto` backend pulled `rsa 0.9.10` into `garraia-auth`'s production tree as a second entry path. Invariant verified: GarraRUST emits and verifies HS256 only; no RSA call site exists. Same `2026-07-31` expiration.

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
