# Plan 0166 — GAR-478 — Q10.d: Extract `bootstrap/slack.rs`

## Goal

Extract `build_slack_channels` (164 LOC) from `bootstrap/mod.rs` into a new
sibling module `bootstrap/slack.rs`. Re-export at the same public path so all
call sites stay unchanged — zero behaviour change.

## Architecture

Follows the extract-and-re-export pattern from slices 10.a–10.c.

```
bootstrap/
  config.rs     ← slice 10.a (config loading + path resolvers)
  channels.rs   ← slice 10.b (channel registry orchestrator)
  discord.rs    ← slice 10.c (Discord wiring + command handler)
  slack.rs      ← slice 10.d (Slack wiring) [NEW]
  mod.rs        ← re-exports; build_agent_runtime + build_mcp_tools + Telegram still here
```

## Tech stack

- Rust stable (MSRV 1.92)
- `garraia_channels::{SlackChannel, SlackOnMessageFn}` (full-path inside closure)
- `garraia_security::{Allowlist, InputValidator, PairingManager}`
- `garraia_agents::ChatMessage`
- `super::config::{default_allowlist_path, resolve_api_key}`

## Design invariants

- `pub use slack::build_slack_channels;` in `mod.rs` — call site unchanged.
- No new logic, no renamed types, no behaviour change.
- No secrets hardcoded; env vars read via `resolve_api_key` (unchanged).
- `bootstrap/mod.rs` shrinks 2044 → ~1880 LOC (−164).

## Out of scope

- Q10.e (whatsapp.rs), Q10.f (imessage.rs) — separate slices.
- Any logic change to Slack handling.

## Rollback

`git revert <sha>` — one commit, zero DB change, zero config change.

## M1 tasks

- [x] Create `bootstrap/slack.rs` with `build_slack_channels`
- [x] Add `mod slack;` + `pub use slack::build_slack_channels;` to `mod.rs`
- [x] Remove function from `mod.rs`
- [x] `cargo check -p garraia-gateway` green
- [x] `cargo clippy --workspace ... -- -D warnings` green
- [x] Update `plans/README.md`
- [x] Commit, push, open PR

## Acceptance criteria

- `cargo check -p garraia-gateway` exits 0
- `git grep "fn build_slack_channels" crates/garraia-gateway/src/bootstrap/mod.rs` → 0 hits
- `git grep "fn build_slack_channels" crates/garraia-gateway/src/bootstrap/slack.rs` → 1 hit
- All CI checks green

## Cross-references

- Parent epic: GAR-440 (Q10 bootstrap modularisation)
- Slice 10.a: PR #89 (config.rs)
- Slice 10.b: PR #470 (channels.rs)
- Slice 10.c: PR #471 (discord.rs) — plan 0165
- Linear: GAR-478

## Estimativa

~164 LOC moved, ~10 LOC bookkeeping. Risk: LOW (pure extract, no logic change).

## Result

Merged 2026-05-22 via PR #474 (commit `4a51841`). CI fix included: gated 8
testcontainer-dependent auth tests behind `required-features = ["test-support"]`
in `crates/garraia-auth/Cargo.toml` to prevent Docker Hub rate-limit failures
in plain `cargo test --workspace`.
