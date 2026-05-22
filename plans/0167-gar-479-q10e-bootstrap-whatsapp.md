# Plan 0167 — GAR-479 — Q10.e: Extract `bootstrap/whatsapp.rs`

## Goal

Extract `build_whatsapp_channels` (180 LOC) from `bootstrap/mod.rs` into a new
sibling module `bootstrap/whatsapp.rs`. Re-export at the same public path so all
call sites stay unchanged — zero behaviour change.

## Architecture

Follows the extract-and-re-export pattern from slices 10.a–10.d.

```
bootstrap/
  config.rs     ← slice 10.a (config loading + path resolvers)
  channels.rs   ← slice 10.b (channel registry orchestrator)
  discord.rs    ← slice 10.c (Discord wiring + command handler)
  slack.rs      ← slice 10.d (Slack wiring)
  whatsapp.rs   ← slice 10.e (WhatsApp wiring) [NEW]
  mod.rs        ← re-exports; build_agent_runtime + build_mcp_tools + Telegram + iMessage still here
```

## Tech stack

- Rust stable (MSRV 1.92)
- `garraia_channels::{WhatsAppChannel, WhatsAppOnMessageFn}` (explicit type annotation in closure)
- `garraia_security::{Allowlist, PairingManager}` (+ full-path `InputValidator::sanitize` / `check_prompt_injection`)
- `garraia_agents::ChatMessage`
- `super::config::{default_allowlist_path, resolve_api_key}`

## Design invariants

- `pub use whatsapp::build_whatsapp_channels;` in `mod.rs` — call site unchanged.
- Return type `Vec<Arc<WhatsAppChannel>>` preserved (not `Vec<Box<dyn Channel>>`).
- No new logic, no renamed types, no behaviour change.
- No secrets hardcoded; env vars read via `resolve_api_key` and `std::env::var` (unchanged).
- `bootstrap/mod.rs` shrinks ~1878 → ~1698 LOC (−180).

## Out of scope

- Q10.f (imessage.rs) — separate slice (GAR-480).
- Any logic change to WhatsApp handling.

## Rollback

`git revert <sha>` — one commit, zero DB change, zero config change.

## M1 tasks

- [ ] Create `bootstrap/whatsapp.rs` with `build_whatsapp_channels`
- [ ] Add `mod whatsapp;` + `pub use whatsapp::build_whatsapp_channels;` to `mod.rs`
- [ ] Remove function + import clean-up from `mod.rs` (`WhatsAppChannel`, `WhatsAppOnMessageFn`)
- [ ] `cargo check -p garraia-gateway` green
- [ ] `cargo clippy --workspace ... -- -D warnings` green
- [ ] Update `plans/README.md`
- [ ] Commit, push, open PR

## Acceptance criteria

- `cargo check -p garraia-gateway` exits 0
- `git grep "fn build_whatsapp_channels" crates/garraia-gateway/src/bootstrap/mod.rs` → 0 hits
- `git grep "fn build_whatsapp_channels" crates/garraia-gateway/src/bootstrap/whatsapp.rs` → 1 hit
- All CI checks green

## Cross-references

- Parent epic: GAR-440 (Q10 bootstrap modularisation)
- Slice 10.a: PR #89 (config.rs)
- Slice 10.b: PR #470 (channels.rs)
- Slice 10.c: PR #471 (discord.rs) — plan 0165
- Slice 10.d: PR #474 (slack.rs) — plan 0166
- Linear: GAR-479

## Estimativa

~180 LOC moved, ~10 LOC bookkeeping. Risk: MEDIUM (WhatsApp webhook tokens + env vars).
