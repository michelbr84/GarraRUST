# Plan 0168 — GAR-480 — Q10.f: Extract `bootstrap/imessage.rs`

## Goal

Extract `build_imessage_channels` (~123 LOC, macOS-only) from `bootstrap/mod.rs` into a new
sibling module `bootstrap/imessage.rs`. Re-export at the same public path so all
call sites stay unchanged — zero behaviour change.

## Architecture

Follows the extract-and-re-export pattern from slices 10.a–10.e.

```
bootstrap/
  config.rs     ← slice 10.a (config loading + path resolvers)
  channels.rs   ← slice 10.b (channel registry orchestrator)
  discord.rs    ← slice 10.c (Discord wiring + command handler)
  slack.rs      ← slice 10.d (Slack wiring)
  whatsapp.rs   ← slice 10.e (WhatsApp wiring)
  imessage.rs   ← slice 10.f (iMessage wiring, macOS-only) [NEW]
  mod.rs        ← re-exports; build_agent_runtime + build_mcp_tools + Telegram still here
```

## Tech stack

- Rust stable (MSRV 1.92)
- `garraia_channels::{IMessageChannel, IMessageOnMessageFn}` (macOS-gated)
- `garraia_security::{Allowlist, PairingManager}` (+ full-path `InputValidator::sanitize` / `check_prompt_injection`)
- `garraia_config::AppConfig`
- `super::config::default_allowlist_path`
- `#![cfg(target_os = "macos")]` at top of new module

## Design invariants

- `#[cfg(target_os = "macos")] pub use imessage::build_imessage_channels;` in `mod.rs` — call site in `server.rs` unchanged.
- Return type `Vec<Box<dyn garraia_channels::Channel>>` preserved.
- No new logic, no renamed types, no behaviour change.
- No hardcoded secrets; no sensitive env vars used (just `poll_interval_secs` from config).
- `bootstrap/mod.rs` shrinks ~1699 → ~1576 LOC (−123).
- The `#[cfg(target_os = "macos")] use garraia_channels::{IMessageChannel, IMessageOnMessageFn};` import removed from `mod.rs`.

## Out of scope

- Any further slices of mod.rs (Telegram still ~900 LOC, out of scope here).
- Any logic change to iMessage handling.

## Rollback

`git revert <sha>` — one commit, zero DB change, zero config change.

## M1 tasks

- [ ] Create `bootstrap/imessage.rs` with `build_imessage_channels` (macOS-gated via `#![cfg(target_os = "macos")]`)
- [ ] Add `#[cfg(target_os = "macos")] mod imessage;` + `#[cfg(target_os = "macos")] pub use imessage::build_imessage_channels;` to `mod.rs`
- [ ] Remove function body + cfg-gated import from `mod.rs`
- [ ] `cargo check -p garraia-gateway` green
- [ ] `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` green
- [ ] Update `plans/README.md`
- [ ] Commit, push, open PR

## Acceptance criteria

- `cargo check -p garraia-gateway` exits 0
- `git grep "fn build_imessage_channels" crates/garraia-gateway/src/bootstrap/mod.rs` → 0 hits
- `git grep "fn build_imessage_channels" crates/garraia-gateway/src/bootstrap/imessage.rs` → 1 hit
- All CI checks green

## Cross-references

- Parent epic: GAR-440 (Q10 bootstrap modularisation)
- Slice 10.a: PR #89 (config.rs)
- Slice 10.b: PR #470 (channels.rs)
- Slice 10.c: PR #471 (discord.rs) — plan 0165
- Slice 10.d: PR #474 (slack.rs) — plan 0166
- Slice 10.e: PR #476 (whatsapp.rs) — plan 0167
- Linear: GAR-480

## Estimativa

~123 LOC moved, ~10 LOC bookkeeping. Risk: LOW (macOS-only, no sensitive env vars, no tokens).
