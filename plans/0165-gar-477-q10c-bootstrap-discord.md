# Plan 0165 — GAR-477 — Q10.c: Extract `bootstrap/discord.rs`

## Goal

Extract `build_discord_channels` + `handle_discord_command` (270 LOC) from
`bootstrap/mod.rs` into a new sibling module `bootstrap/discord.rs`. Re-export
at the same public path so all call sites stay unchanged — zero behaviour change.

## Architecture

Follows the extract-and-re-export pattern from slices 10.a (`bootstrap/config.rs`)
and 10.b (`bootstrap/channels.rs`).

```
bootstrap/
  config.rs     ← slice 10.a (config loading + path resolvers)
  channels.rs   ← slice 10.b (channel registry orchestrator)
  discord.rs    ← slice 10.c (Discord wiring + command handler) [NEW]
  mod.rs        ← re-exports; build_agent_runtime + build_mcp_tools still here
```

## Tech stack

- Rust stable (MSRV 1.92)
- `garraia_channels::discord::{DiscordChannel, DiscordOnMessageFn}` (full-path)
- `garraia_security::{Allowlist, InputValidator, PairingManager}`
- `garraia_agents::ChatMessage`

## Design invariants

- `pub use discord::build_discord_channels;` in `mod.rs` — call site unchanged.
- `handle_discord_command` stays `fn` (private to `discord.rs`).
- No new logic, no renamed types, no behaviour change.
- No secrets hardcoded; env vars read inside `build_discord_channels` (unchanged).
- `bootstrap/mod.rs` shrinks 2316 → ~2046 LOC (−270).

## Out of scope

- Q10.d (slack.rs), Q10.e (whatsapp.rs), Q10.f (imessage.rs) — separate slices.
- Any logic change to Discord handling.

## Rollback

`git revert <sha>` — one commit, zero DB change, zero config change.

## M1 tasks

- [x] Create `bootstrap/discord.rs` with `build_discord_channels` + `handle_discord_command`
- [x] Add `mod discord;` + `pub use discord::build_discord_channels;` to `mod.rs`
- [x] Remove functions from `mod.rs`
- [x] `cargo check -p garraia-gateway` green
- [x] `cargo clippy --workspace ... -- -D warnings` green
- [x] Update `plans/README.md` (bookkeeping for 0163 + 0164 + add 0165)
- [x] Commit, push, open PR

## Acceptance criteria

- `cargo check -p garraia-gateway` exits 0
- `git grep "fn build_discord_channels" crates/garraia-gateway/src/bootstrap/mod.rs` → 0 hits
- `git grep "fn build_discord_channels" crates/garraia-gateway/src/bootstrap/discord.rs` → 1 hit
- All CI checks green

## Cross-references

- Parent epic: GAR-440 (Q10 bootstrap modularisation)
- Slice 10.a: PR #89 (config.rs)
- Slice 10.b: PR #470 (channels.rs) — plan 0164 (note: 0164 also used for GAR-456 sqlx partial fix; first-assigned wins)
- Linear: GAR-477

## Estimativa

~270 LOC moved, ~30 LOC bookkeeping. Risk: LOW (pure extract, no logic change).
