# Plan 0170 — GAR-691 — Q10.g: Extract `bootstrap/telegram.rs`

## Goal

Extract the three Telegram-wiring functions (~423 LOC) from `bootstrap/mod.rs` into a new
sibling module `bootstrap/telegram.rs`. Re-export `build_telegram_channels` at the same
public path so all call sites stay unchanged — zero behaviour change.

## Architecture

Follows the extract-and-re-export pattern from slices 10.a–10.f.

```
bootstrap/
  config.rs     ← slice 10.a (config loading + path resolvers)
  channels.rs   ← slice 10.b (channel registry orchestrator)
  discord.rs    ← slice 10.c (Discord wiring + command handler)
  slack.rs      ← slice 10.d (Slack wiring)
  whatsapp.rs   ← slice 10.e (WhatsApp wiring)
  imessage.rs   ← slice 10.f (iMessage wiring, macOS-only)
  telegram.rs   ← slice 10.g (Telegram wiring + voice handler) [NEW]
  mod.rs        ← re-exports; build_agent_runtime + build_mcp_tools still here
```

## Tech stack

- Rust stable (MSRV 1.92)
- `garraia_channels::{Channel, CommandContext, CommandError, OnMessageFn, OnVoiceFn, Role, TelegramChannel}`
- `garraia_security::{Allowlist, InputValidator, PairingManager}`
- `garraia_config::AppConfig`
- `garraia_db::SessionHints`
- `garraia_agents::ChatMessage`
- `teloxide::net::Download`, `teloxide::prelude::Requester`
- `super::config::default_allowlist_path`, `super::resolve_api_key`

## Design invariants

- `pub use telegram::build_telegram_channels;` in `mod.rs` — call site in `server.rs` unchanged.
- `handle_command` stays private (not re-exported).
- `build_telegram_voice_handler` is `pub` within the module but NOT re-exported from mod.rs
  (only called from within `build_telegram_channels` in the same module).
- Return type `Vec<Box<dyn garraia_channels::Channel>>` preserved.
- No new logic, no renamed types, no behaviour change.
- No hardcoded secrets; `TELEGRAM_BOT_TOKEN` env var read only via `super::resolve_api_key`.
- `bootstrap/mod.rs` shrinks ~1579 → ~1156 LOC (−423).
- Telegram-specific imports removed from `mod.rs`: `Mutex`, `OnVoiceFn`, `TelegramChannel`,
  `Allowlist`, `PairingManager`, `teloxide::net::Download`, `teloxide::prelude::Requester`,
  `ChatMessage` (only used in telegram functions).

## Out of scope

- Further slices (build_agent_runtime ~978 LOC, build_mcp_tools ~92 LOC — separate slices Q10.h, Q10.i).
- Any logic change to Telegram handling.

## Rollback

`git revert <sha>` — one commit, zero DB change, zero config change.

## Validações pré-plano

- [x] bootstrap/mod.rs contains `build_telegram_voice_handler`, `build_telegram_channels`, `handle_command`
- [x] `build_telegram_channels` is imported in `server.rs` line 17 (external caller)
- [x] `build_telegram_voice_handler` is only called internally by `build_telegram_channels`
- [x] `handle_command` is a private `fn` (not `pub fn`)
- [x] No call to these functions outside of `server.rs` + `bootstrap/mod.rs`

## M1 Tasks

- [x] T1: Create `bootstrap/telegram.rs` with all three functions + correct imports
- [x] T2: Add `mod telegram;` and `pub use telegram::build_telegram_channels;` to `mod.rs`
- [x] T3: Remove telegram-specific imports from `mod.rs`
- [x] T4: `cargo check -p garraia-gateway` green
- [x] T5: `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` green
- [x] T6: Update `plans/README.md` with this plan row
- [x] T7: Commit + push + open PR

## Risk register

| Risk | Mitigation |
|---|---|
| Import path break for `build_telegram_channels` in server.rs | Re-export ensures path unchanged |
| `super::config::default_allowlist_path` not visible from sibling module | Both are siblings under bootstrap; works if fn is pub in config.rs |
| `super::resolve_api_key` accessible from telegram.rs | Re-exported as `pub(crate)` in mod.rs, accessible as `super::resolve_api_key` |
| Clippy warnings in extracted code | Run clippy before commit |

## Acceptance criteria

- `git grep "fn build_telegram_channels" crates/garraia-gateway/src/bootstrap/mod.rs` → 0 hits
- `git grep "fn build_telegram_channels" crates/garraia-gateway/src/bootstrap/telegram.rs` → 1 hit
- `cargo check -p garraia-gateway` exits 0
- Full CI green (20 checks)
- `bootstrap/mod.rs` LOC ≤ 1160

## Cross-references

- Parent: GAR-440 (Q10 bootstrap modularization)
- Epic: GAR-430 (Quality Gates Phase 3.6)
- Slice 10.f: plan 0168 / GAR-480 / PR #484
- Slice 10.e: plan 0167 / GAR-479 / PR #476

## Estimativa

~30 min. Risco LOW-MEDIUM (toca tokens Telegram).
