# 005-attack-surface

| | |
|---|---|
| **Category** | security-audit |
| **Status** | active |

## Objective

Prove, by inspection of pinned checkouts, the default exposure posture of
each framework (bind address, gateway authentication) and the size of its
dependency tree — dimensions a self-hoster weighs before running an
always-on agent.

Dependency counts are **per-ecosystem** and not directly comparable
across ecosystems: for Rust targets we count resolved crates in
`Cargo.lock`; for OpenClaw we count direct production dependencies in
`package.json` (the production closure method is documented in the check).
A Rust lockfile crate and an npm package are different units — the table
must always label which unit it is using.

## Targets

Same inputs as [004-credentials-at-rest](004-credentials-at-rest.md).

## Expected command

```bash
OPENCLAW_CHECKOUT=/path/to/openclaw ZEROCLAW_CHECKOUT=/path/to/zeroclaw \
  ./run.sh --scenario-005
```

## Claims table

| # | Target | Claim | Check |
|---|---|---|---|
| 1 | GarraIA | Default bind is 127.0.0.1 | `grep -n '127.0.0.1' crates/garraia-config/src/model.rs` (default_host) |
| 2 | GarraIA | Honest caveat: local WS/HTTP API is open by default on loopback — `gateway.api_key` and `session_tokens_required` are opt-in | `grep -n 'api_key: None' crates/garraia-config/src/model.rs; grep -n 'session_tokens_required' crates/garraia-config/src/model.rs` |
| 3 | GarraIA | Messaging channels are deny-by-default (pairing codes) | `grep -n 'restricted' crates/garraia-security/src/allowlist.rs` |
| 4 | GarraIA | Resolved crates in Cargo.lock | `grep -c '^name = ' Cargo.lock` |
| 5 | OpenClaw | Default bind mode is loopback | `grep -n 'loopback' src/gateway/net.ts` |
| 6 | OpenClaw | Gateway auth defaults to token mode and fails closed without a token | `grep -n 'no token was configured' src/gateway/auth.ts` |
| 7 | OpenClaw | Direct production dependencies in package.json | `python3 -c "import json;print(len(json.load(open('package.json'))['dependencies']))"` |
| 8 | ZeroClaw | Default bind is 127.0.0.1 and pairing is required by default | `grep -n 'require_pairing' crates/zeroclaw-config/src/schema.rs` |
| 9 | ZeroClaw | Honest caveat: public bind is warn-only, not refused | `grep -n 'allow_public_bind' crates/zeroclaw-config/src/schema.rs` |
| 10 | ZeroClaw | Resolved crates in Cargo.lock | `grep -c '^name = ' Cargo.lock` |
| 11 | GarraIA | WASM plugin sandbox: per-plugin memory caps + execution deadlines (wasmtime, opt-in feature) | `grep -n 'StoreLimits\|epoch_interruption' crates/garraia-plugins/src/runtime.rs` |
| 12 | OpenClaw | Plugins load in-process as trusted code (their own threat model) | `grep -n 'in-process' SECURITY.md` |
| 13 | ZeroClaw | Plugin Ed25519 signing exists but defaults to Disabled | `grep -n 'Disabled' crates/zeroclaw-plugins/src/signature.rs` |

## Expected result

`pass` when every claim resolves as stated; `skipped` per absent
checkout; `fail` on drift (fix the table and the root README together).

## Minimum evidence

- `005-attack-surface/summary.json` — per-claim `{name, ok, detail}` +
  numeric metrics (`garraia_lock_crates`, `zeroclaw_lock_crates`,
  `openclaw_prod_deps`)
- `005-attack-surface/raw/*.txt` — raw command outputs
- `environment.txt` — pinned commits
