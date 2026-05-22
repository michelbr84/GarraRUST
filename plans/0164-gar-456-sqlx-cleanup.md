# Plan 0164 — GAR-456: sqlx cleanup — jsonwebtoken rust_crypto → aws_lc_rs

**Issue:** [GAR-456](https://linear.app/chatgpt25/issue/GAR-456)
**Branch:** `routine/202605220028-gar-456-sqlx-cleanup`
**Date:** 2026-05-22 (America/New_York)
**Session:** garra-routine autonomous (serene-fermat-qOhkl)

---

## 1. Goal

Close RUSTSEC-2023-0071 (`rsa 0.9.10` — Marvin Attack timing sidechannel) by
removing `rsa` from the compiled dep tree. The companion goal (removing
`sqlx-mysql` from Cargo.lock) is a structural blocker; this plan closes the
partial fix and documents why the acceptance criteria require sqlx 0.9+.

---

## 2. Root Cause Analysis

Two independent paths brought `rsa 0.9.10` into the project:

### Path A — jsonwebtoken (ACTIVE, now closed)

```
garraia-auth / garraia-gateway
  → jsonwebtoken 10 (features = ["rust_crypto"])
    → rsa 0.9.10          ← RUSTSEC-2023-0071
```

The `rust_crypto` feature bundles all asymmetric algorithms together
(`ed25519-dalek`, `hmac`, `p256`, `p384`, `rand`, `rsa`, `sha2`). Only
`hmac` + `sha2` were ever used (HS256). The RSA code path was never
reachable at runtime, but the crate was compiled and linked.

**Fix applied in this plan:** switch `jsonwebtoken` to `aws_lc_rs` feature.
`aws-lc-rs 1.17.0` is already compiled by the workspace via
`rustls → aws-lc-rs` (axum-server / aws-sdk-s3 TLS). Zero additional build
cost. Removed from Cargo.lock: `rsa`, `ed25519-dalek`, `ed25519`,
`p256`, `p384`, `ecdsa`, `elliptic-curve`, `curve25519-dalek`,
`curve25519-dalek-derive`, `crypto-bigint`, `fiat-crypto`, `ff`, `group`,
`primeorder`, `rfc6979`, `sec1`, `base16ct` (17 packages).

### Path B — sqlx-macros-core optional dep (LOCKFILE GHOST, structural blocker)

```
sqlx-macros-core 0.8.6
  [dependencies]
  sqlx-mysql = { version = "=0.8.6", optional = true }
                   → rsa 0.9.10
```

Cargo lockfile semantics: optional dependencies of workspace packages are
resolved and recorded in `Cargo.lock` for reproducibility, even when no
active feature enables them. No crate in the workspace enables the `mysql`
feature of `sqlx`. `sqlx-mysql` is NEVER compiled. But `cargo audit` and
`cargo deny` walk the full `Cargo.lock` and flag `rsa 0.9.10` regardless.

**Structural blocker:** This cannot be fixed without:
1. **sqlx 0.9+** that restructures `sqlx-macros-core` to remove the
   `sqlx-mysql` optional dep entirely (or to a separate workspace crate), OR
2. A `[patch.crates-io]` redirect to a stub `sqlx-mysql` without `rsa` dep
   (fragile, not recommended), OR
3. Abandoning `sqlx` for raw `sqlx-postgres` + `sqlx-core` imports.

Option 1 is the correct fix. Watch sqlx 0.9.x release notes.

---

## 3. Design Invariants

- Only `Algorithm::HS256` is emitted/verified in `garraia-auth/src/jwt.rs`
  and `garraia-gateway/src/mobile_auth.rs`. The `aws_lc_rs` backend
  implements HS256 via AWS-LC HMAC-SHA256.
- No RSA encrypt/decrypt path is reachable at runtime. Marvin Attack
  non-reachable both before and after this change.
- Algorithm-confusion guard: `Validation::new(Algorithm::HS256)` rejects
  all non-HS256 tokens — unchanged.

---

## 4. Changes

| File | Change |
|------|--------|
| `Cargo.toml` (workspace) | `jsonwebtoken` features: `["rust_crypto"]` → `["aws_lc_rs"]` |
| `Cargo.lock` | 17 packages removed; `untrusted 0.7.1` dependency added to `aws-lc-rs` entry |
| `.cargo/audit.toml` | Updated GAR-456 ignore block with full analysis and AMENDMENT 2026-05-22 |
| `deny.toml` | Updated rsa entry comment to reflect lockfile-ghost-only status |
| `plans/README.md` | Added plan 0164 entry |

---

## 5. Acceptance Criteria Status

| Criterion | Status |
|-----------|--------|
| `cargo metadata --locked | grep '"name":"rsa"'` → empty | ❌ Still present (sqlx-macros-core optional dep, see §2 Path B) |
| `cargo metadata --locked | grep '"name":"sqlx-mysql"'` → empty | ❌ Still present (same root cause) |
| RUSTSEC-2023-0071 removed from `.cargo/audit.toml` | ❌ Entry retained (rsa in lockfile) |
| `rsa` removed from compiled dep tree | ✅ Achieved (jsonwebtoken path closed) |
| `cargo check -p garraia-auth` green | ✅ Verified locally |
| 17 unnecessary packages removed from Cargo.lock | ✅ Achieved |

Full acceptance criteria require sqlx 0.9+. Track in GAR-456 deadline 2026-07-31.

---

## 6. Follow-up

- Watch sqlx 0.9.x release; if it restructures `sqlx-macros-core` to not
  include `sqlx-mysql` as an optional dep, upgrade and drop RUSTSEC-2023-0071
  from both `audit.toml` and `deny.toml` atomically.
- If sqlx 0.9 is not out by 2026-07-31, refresh the expiration dates in
  both files to 2026-09-30 and update GAR-456 with a new comment.
