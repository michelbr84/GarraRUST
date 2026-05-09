# Plan 0087 — GAR-553: Drop aws-sdk-s3 legacy rustls 0.21 chain via feature-flag swap

| Field | Value |
|---|---|
| **Status** | 🟡 In Progress |
| **Linear** | [GAR-553](https://linear.app/chatgpt25/issue/GAR-553) |
| **Branch** | `health/202605090047-aws-sdk-s3-drop-legacy-rustls` |
| **Parent epic** | GAR-430 (Quality Gates Phase 3.6) |
| **Follows from** | GAR-455 deep-dive PR #230 (docs only, 2026-05-08) |
| **Estimativa** | 1–2h |

## 1. Goal

Remove `rustls-webpki 0.101.7`, `rustls 0.21.12`, and `hyper-rustls 0.24.2`
from the compiled dependency tree of the `storage-s3` feature by swapping the
`aws-sdk-s3` feature `"rustls"` (which aliases the legacy `aws-smithy-http-client/legacy-rustls-ring`
chain) for `"default-https-client"` (which activates the modern rustls 0.23 /
aws-lc chain already present in `Cargo.lock`).

This is **defense-in-depth only**: the serenity 0.12.5 chain
(`rustls-webpki 0.102.8`) independently carries all 4 RUSTSEC IDs
(RUSTSEC-2026-0049/0098/0099/0104), so Dependabot alerts #37/#11/#22/#23
remain open until serenity 0.13 ships. No alerts close from this PR.

## 2. Architecture

```
Before:
  crates/garraia-storage/Cargo.toml
    aws-sdk-s3 feature "rustls"
      → aws-smithy-runtime/tls-rustls
      → aws-smithy-http-client/legacy-rustls-ring
      → rustls 0.21.12 + hyper-rustls 0.24.2 + rustls-webpki 0.101.7  ← 3 RUSTSEC IDs

After:
  crates/garraia-storage/Cargo.toml
    aws-sdk-s3 feature "default-https-client"
      → aws-smithy-runtime/default-https-client
      → aws-smithy-http-client/rustls-aws-lc (modern)
      → rustls 0.23.36 + rustls-webpki 0.103.13  ← already present, already patched
```

## 3. Tech stack

- `crates/garraia-storage/Cargo.toml` — one feature flag change in `[dependencies]`
- `Cargo.lock` — re-resolved automatically by `cargo check`
- `docs/security/dependabot-status.md` — updated snapshot + rationale

## 4. Design invariants

- S3 HTTPS functionality is preserved via the modern rustls 0.23 chain
- `cargo audit` and `cargo deny check` allowlists are UNCHANGED (the 4 RUSTSEC IDs
  stay allowlisted; they're still triggered by the serenity chain)
- No code changes to `garraia-storage/src/` — purely a Cargo.toml feature tweak
- `garraia-config::StorageConfig` and the `AppState` wiring in `garraia-gateway` are unaffected

## 5. Out of scope

- Closing Dependabot alerts (serenity chain still triggers all 4)
- Updating `.cargo/audit.toml` or `deny.toml` (no change needed)
- Serenity upgrade (GAR-455 scope, waiting on upstream 0.13)

## 6. Rollback

Revert the one-line feature change in `crates/garraia-storage/Cargo.toml` and
re-run `cargo check` to restore `rustls 0.21.12` to `Cargo.lock`.

## 7. File structure

```
crates/garraia-storage/Cargo.toml     ← M1: feature swap
Cargo.lock                            ← auto-updated by cargo check
docs/security/dependabot-status.md   ← M2: snapshot update
plans/README.md                       ← bookkeeping row
```

## 8. Tasks (M1–M2)

### M1 — Feature-flag swap
- [ ] In `crates/garraia-storage/Cargo.toml`, change `aws-sdk-s3` dependency:
  - Remove feature `"rustls"`
  - Add feature `"default-https-client"`
- [ ] Run `cargo check --workspace --exclude garraia-desktop --features garraia-storage/storage-s3`
- [ ] Confirm `rustls-webpki 0.101.7` is absent from `Cargo.lock`
- [ ] Run `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings`
- [ ] Run `cargo audit` and `cargo deny check` (both should still exit 0 with allowlist)

### M2 — Docs + bookkeeping
- [ ] Update `docs/security/dependabot-status.md`:
  - Add 2026-05-09 snapshot row to the table
  - Add "Confirmed 2026-05-09" section noting AWS sub-chain removal
  - Update alert #37 mitigation column to note `0.101.7` chain removed
- [ ] Update `plans/README.md` with plan 0087 row

## 9. Risk register

| Risk | Probability | Mitigation |
|---|---|---|
| `"default-https-client"` feature does not exist in `aws-sdk-s3 1.119.0` | Low | cargo check fails immediately with clear feature-not-found error; revert |
| Feature swap breaks S3 connectivity at runtime | Low | `aws-smithy-runtime/default-https-client` activates modern rustls 0.23 chain, which is already present and tested |
| Other workspace member independently activates `legacy-rustls-ring` | Low | Verify with `cargo tree --features garraia-storage/storage-s3` post-change |
| Cargo.lock conflict when merged | Low | One-line change; routine PR #231 (plan 0086) only touches other files |

## 10. Acceptance criteria

- [ ] `cargo check --workspace --exclude garraia-desktop --features garraia-storage/storage-s3` exits 0
- [ ] `cargo clippy ... -D warnings` exits 0
- [ ] `cargo audit` exits 0 (all 4 RUSTSEC IDs still allowlisted)
- [ ] `cargo deny check` exits 0
- [ ] `grep "rustls-webpki" Cargo.lock` no longer contains `version = "0.101.7"`
- [ ] CI green on PR (all ≥16 checks pass)
- [ ] `docs/security/dependabot-status.md` updated with 2026-05-09 snapshot

## 11. Cross-references

- GAR-455 — parent investigation (Done, PR #230, 2026-05-08)
- GAR-430 — Quality Gates Phase 3.6 epic
- GAR-553 — this issue
- RUSTSEC-2026-0098/0099/0104 — IDs covered by the removed `0.101.7` chain
- `docs/security/dependabot-status.md` §"GAR-455 deep-dive 2026-05-08" §"New finding"

## 12. Open questions

None — the feature-flag approach was empirically validated by the 2026-05-08
deep-dive. Only risk: feature name in `aws-sdk-s3 1.119.0` — caught immediately
by `cargo check`.
