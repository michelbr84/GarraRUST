# Plan 0260 — GAR-788: GET /v1/me/reactions — emoji-reactions inbox

**Status:** In Progress
**Linear:** [GAR-788](https://linear.app/chatgpt25/issue/GAR-788)
**Branch:** `routine/202506031650-me-reactions-inbox`
**Date:** 2026-06-03 (America/New_York)

## Goal

Add `GET /v1/me/reactions` to the me/ inbox API family. Returns a cursor-paginated
list of messages on which the authenticated user has placed emoji reactions, grouped
by message (one row per message with all emojis via `ARRAY_AGG`). Required for the
mobile "My Activity" reactions tab.

## Root cause / motivation

The `message_reactions` table (migration 021) is fully deployed with FORCE RLS and a
`(user_id, message_id)` index, but there was no endpoint exposing the caller's own
reaction history. The me/ inbox pattern (group_id required, keyset cursor,
`(MAX(reacted_at) DESC, message_id DESC)` ordering) already existed for mentions,
tasks, chats, files, memory, and invites; reactions is the next natural slice.

## Design

- **No new migration** — `message_reactions` (migration 021) is already in place.
- **Grouping**: `GROUP BY (message_id, chat_id, group_id, sender_user_id, sender_label, body)`
  with `ARRAY_AGG(emoji ORDER BY reacted_at, emoji)` and `MAX(reacted_at)` as the ordering key.
- **Cursor**: `message_id` UUID — the HAVING clause subquery using
  `(MAX(reacted_at), message_id)` tuple comparison fails closed (returns NULL → no rows)
  when `after_id` is deleted or from a different group.
- **Named `#[derive(sqlx::FromRow)]` struct** (`ReactionGroupRow`) instead of a tuple
  type alias — required because `Vec<String>` cannot be decoded from a PostgreSQL array
  column via the anonymous tuple path.
- **Default limit**: 20 (smaller than the 50 default for other inboxes, since emoji-rich
  messages expand per-item payload).
- **RLS**: both `app.current_user_id` and `app.current_group_id` set via parameterized
  `SET LOCAL` before any query.

## File structure

| File | Change |
|------|--------|
| `crates/garraia-gateway/src/rest_v1/me.rs` | New types `ListReactionsQuery` / `ReactionGroupRow` / `MyReactionSummary` / `MyReactionsResponse` + handler `list_my_reactions` + 5 unit tests |
| `crates/garraia-gateway/src/rest_v1/mod.rs` | Route `.route("/v1/me/reactions", get(me::list_my_reactions))` in authenticated router + `get(unconfigured_handler)` stub in both unconfigured routers |
| `crates/garraia-gateway/src/rest_v1/openapi.rs` | Path `super::me::list_my_reactions` + schemas `MyReactionsResponse` / `MyReactionSummary` + import |
| `ROADMAP.md` | Add `/v1/me/reactions` entry + fix 3 stale `🔄 In Progress` labels |
| `plans/0260-gar-788-me-reactions-inbox.md` | This file |
| `plans/README.md` | Row 0260 added |

## M1 Tasks

- [x] T1: Handler + types + unit tests in `me.rs`
- [x] T2: Route registration in `mod.rs` (3 router variants)
- [x] T3: OpenAPI path + schema + import in `openapi.rs`
- [x] T4: ROADMAP entry + stale label cleanup
- [x] T5: Plan doc + `plans/README.md` row

## Acceptance criteria

- `cargo check -p garraia-gateway` clean.
- `cargo clippy -p garraia-gateway --tests --no-deps -- -D warnings` clean.
- `cargo test -p garraia-gateway --lib -- me::tests` — 55 tests pass (5 new).
- CI green on this PR.

## Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| `ARRAY_AGG` returns `NULL` for messages with 0 reactions | None | WHERE clause requires at least one `mr.user_id = $1` row — GROUP BY only produces rows where a reaction exists |
| Cursor subquery returns NULL on deleted/cross-group `after_id` | Low | Fail-closed: NULL comparison → no rows → safe empty result |
| `Vec<String>` decoding from PostgreSQL array column | None | Named `#[derive(sqlx::FromRow)]` struct bypasses the tuple limitation; unit test `my_reaction_summary_serializes_all_fields` covers roundtrip |

## Cross-references

- Migration 021 (`message_reactions`) — schema, indexes, FORCE RLS policy
- GAR-788 — Linear issue
- Plans 0237 / 0242 / 0245 / 0246 / 0249 / 0255 / 0258 — prior me/ inbox slices (same pattern)
