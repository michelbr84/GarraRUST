# 004-credentials-at-rest

| | |
|---|---|
| **Category** | security-audit |
| **Status** | active |

## Objective

Prove, by code and documentation inspection of pinned checkouts (never by
trusting anyone's README), how each framework stores credentials at rest —
and record the honest caveats of each, including GarraIA's own.

## Targets

| Target | Input |
|---|---|
| GarraIA | current checkout |
| OpenClaw | local clone at `OPENCLAW_CHECKOUT` (pin the commit in `environment.txt`) |
| ZeroClaw | local clone at `ZEROCLAW_CHECKOUT` (pin the commit in `environment.txt`) |

## Expected command

```bash
OPENCLAW_CHECKOUT=/path/to/openclaw ZEROCLAW_CHECKOUT=/path/to/zeroclaw \
  ./run.sh --scenario-004
```

## Claims table

Each claim is verified mechanically (grep against the pinned checkout) and
recorded as `{name, ok, detail}` in `summary.json`.

| # | Target | Claim | Check |
|---|---|---|---|
| 1 | GarraIA | Credential vault uses AES-256-GCM | `grep -n 'AES_256_GCM' crates/garraia-security/src/credentials.rs` |
| 2 | GarraIA | Vault key derived with PBKDF2 600k iterations | `grep -n 'PBKDF2_ITERATIONS' crates/garraia-security/src/credentials.rs` |
| 3 | GarraIA | Honest caveat: MCP secrets fall back to plaintext (with warning) when no vault passphrase is set | `grep -rn 'plaintext' crates/garraia-gateway/src/mcp/persistence.rs` |
| 4 | OpenClaw | Docs state the secret store is **not encrypted at rest** (protection = POSIX 0600/0700) | `grep -n 'not encrypted at rest' docs/gateway/secrets.md` |
| 5 | OpenClaw | Fairness: SecretRefs + 1Password/Vault integrations exist as opt-in | `grep -n 'SecretRefs' docs/gateway/secrets.md; ls -d extensions/onepassword extensions/vault` |
| 6 | ZeroClaw | Secrets encrypted at rest **by default** (ChaCha20-Poly1305, `encrypt: true` default) | `grep -n 'ChaCha20-Poly1305' crates/zeroclaw-config/src/secrets.rs` |
| 7 | ZeroClaw | Honest caveat: master key lives on the same filesystem as the ciphertext (no OS keyring — 0 `keyring` crates in Cargo.lock) | `grep -c '^name = "keyring"' Cargo.lock` (expect 0) |

## Expected result

`pass` when every claim above resolves as stated against the pinned
checkouts; `skipped` for a target whose checkout is not provided; `fail`
if any claim no longer matches (the upstream changed — update the table
AND the root README in the same PR).

## Minimum evidence

- `004-credentials-at-rest/summary.json` — per-claim `{name, ok, detail}`
- `004-credentials-at-rest/raw/*.txt` — raw grep outputs per claim
- `environment.txt` — pinned commits of both competitor checkouts
