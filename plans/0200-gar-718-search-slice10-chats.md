# Plan 0200 — GAR-718: REST /v1 search slice 10 — `types=chats` chat name/topic FTS

## Goal

Extend `GET /v1/search` with `types=chats`, enabling callers to search chat names
and topics using full-text search on the `chats` table. This is the tenth slice of
the unified search surface.

## Architecture

Same pattern as slices 5 (files), 6 (tasks), 7 (task_comments), 9 (folders):
- New `SearchResultType::Chat` variant
- New `include_chats: bool` in `ValidatedSearch`
- New `ChatSearchRow` struct (`sqlx::FromRow`)
- New `fetch_chats()` async function (runtime `to_tsvector('simple', ...)`)
- Group-scope-only restriction (400 for chat/user scope)
- Handler wires up the new block after the existing `include_folders` block
- No migration needed — `chats` table + FORCE RLS already in place (migrations 004 + 007)

FTS column: `chats.name || ' ' || coalesce(chats.topic, '')` allows searching
both display name and description in a single `to_tsvector` call.

## Tech stack

- Rust async handler in `crates/garraia-gateway/src/rest_v1/search.rs`
- `utoipa` `ToSchema` on `SearchResultType::Chat` variant
- No SQL migration; no Cargo.toml changes

## Design invariants

- `scope_type=group` only — chats are group-scoped; `scope_type=chat/user` → 400.
- `archived_at IS NULL` enforced — archived chats excluded.
- Explicit `group_id = $2` defense-in-depth alongside `chats_group_isolation` FORCE RLS.
- `excerpt` = `name` (not the full concatenation; `topic` is internal FTS boost only).
- `sender_user_id` = `created_by` (chat creator).
- `kind` = `type` ('channel', 'dm', 'thread').
- `chat_id` = None (the result IS the chat, not nested under one).
- `'simple'` tokenizer (chat names/topics are identifiers, not prose).

## Validações pré-plano

- `cargo check -p garraia-gateway --features test-helpers` → 0 errors
- `cargo test --lib -- rest_v1::search::tests` → ≥70 passing before this slice

## Out of scope

- Searching `chat_members` or message counts for chats.
- Cursor-based pagination (deferred since slice 1).
- Hybrid BM25 + ANN vectorial re-rank (GAR-WS-SEARCH §3.9 long-term).

## Rollback

Revert the diff. No schema changes, no migration, no production risk.

## §12 Open questions

None.

## File structure

```
crates/garraia-gateway/src/rest_v1/search.rs   — augmented (~180 LOC net)
plans/0200-gar-718-search-slice10-chats.md     — this file
plans/README.md                                — row 0200 added
ROADMAP.md                                     — [x] for types=chats
```

## M1 tasks

- [x] T1 — Add `SearchResultType::Chat` variant with doc comment
- [x] T2 — Add `include_chats: bool` to `ValidatedSearch`
- [x] T3 — Add `"chats"` branch to `parse_and_validate` type loop
- [x] T4 — Update "at least one type" guard + error message
- [x] T5 — Add `include_chats && scope_type != Group` → 400 restriction
- [x] T6 — Add `include_chats` to `ValidatedSearch { ... }` constructor
- [x] T7 — Update module-level doc comment (slice 10 paragraph)
- [x] T8 — Add `ChatSearchRow` struct
- [x] T9 — Add `fetch_chats()` async function
- [x] T10 — Wire `include_chats` block in handler (after `include_folders`)
- [x] T11 — Add ≥6 unit tests
- [x] T12 — Update ROADMAP.md `[x]` + plans/README.md row 0200

## Risk register

| Risk | Mitigation |
|------|-----------|
| DM chat names leak across groups | FORCE RLS `chats_group_isolation` + explicit `group_id = $2` |
| Topic NULL concatenation breaks FTS | `coalesce(topic, '')` guard |
| Archived chats appear in results | `archived_at IS NULL` predicate |

## Acceptance criteria

- `types_chats_group_scope_accepted` — `include_chats = true`, others false.
- `types_chats_chat_scope_rejected` — 400.
- `types_chats_user_scope_rejected` — 400.
- `types_chats_and_folders_group_scope_accepted` — both flags true.
- `types_chats_and_tasks_group_scope_accepted` — both flags true.
- `types_all_seven_group_scope_accepted` — messages, memory, files, tasks, task_comments, folders, chats all true.
- All prior tests continue to pass.
- `cargo clippy --workspace --no-deps -- -D warnings` clean.
- CI 100% green.
- PR ≤ 250 LOC.

## Cross-references

- GAR-718 Linear issue: https://linear.app/chatgpt25/issue/GAR-718
- Slice 5 (files): PR #505 / plan 0185
- Slice 6 (tasks): PR #526 / plan 0192
- Slice 7 (task_comments): PR #532 / plan 0193
- Slice 8 (sort_by): PR #535 / plan 0195
- Slice 9 (folders): PR #540 / plan 0199
- plans/0084-gar-ws-search-slice1.md (canonical search architecture)

## Estimativa

45 min — same boilerplate pattern as slice 9, no schema changes.
