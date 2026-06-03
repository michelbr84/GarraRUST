# Plan 0261 — GET /v1/me/threads (GAR-790)

## Goal

Add `GET /v1/me/threads` — cursor-paginated inbox of message threads in which
the authenticated caller has participated (creator or reply author).

## Architecture

- Handler in `crates/garraia-gateway/src/rest_v1/me.rs` (no new crate).
- No new migration — uses existing `message_threads` (migration 004) + FORCE RLS
  from `message_threads_through_chats` (migration 007).
- 4-branch static SQL: cursor × include_resolved.
- Keyset cursor on `(mt.created_at DESC, mt.id DESC)` via subquery — fail-closed
  when `after_id` is deleted or cross-group (NULL comparison → 0 rows).
- `role` field computed in Rust: `"creator"` if `created_by == caller_user_id`,
  else `"participant"`.

## Tech stack

- Rust / Axum 0.8 / sqlx / utoipa (OpenAPI)

## Design invariants

- FORCE RLS: both `SET LOCAL app.current_user_id` and `SET LOCAL app.current_group_id`.
- No `unwrap()` outside tests.
- No SQL string concatenation.
- `title` and `resolved_at` are `#[serde(skip_serializing_if = "Option::is_none")]`.

## Out of scope

- Pagination across multiple groups.
- Subscription/notification management.
- Mutation endpoints for threads (those are in `chats.rs`).

## Rollback

Revert the me.rs handler + mod.rs routes + openapi.rs registrations. No migration
to revert.

## File structure

```
crates/garraia-gateway/src/rest_v1/me.rs      — handler + structs + tests
crates/garraia-gateway/src/rest_v1/mod.rs     — route registration (mode 1 + 2)
crates/garraia-gateway/src/rest_v1/openapi.rs — paths + components
ROADMAP.md                                     — §3.4 entry
plans/README.md                                — this plan row
```

## M1 tasks

- [x] Add `ThreadRow`, `ListMyThreadsQuery`, `MyThreadSummary`, `MyThreadsResponse` structs
- [x] Implement `list_my_threads` handler (4-branch SQL, FORCE RLS)
- [x] Register route in `mod.rs` mode 1 (real handler) and mode 2 (stub)
- [x] Register path + components in `openapi.rs`
- [x] 8 unit tests covering serialization, role field, limit clamp, optional fields
- [x] `cargo check -p garraia-gateway` clean
- [x] `cargo clippy --workspace --no-deps -- -D warnings` clean
- [x] ROADMAP.md + plans/README.md updated

## Acceptance criteria

- `cargo check -p garraia-gateway` passes.
- `cargo clippy --workspace --no-deps -- -D warnings` passes.
- 63 unit tests green in `rest_v1::me::tests`.
- CI 20/20 green.

## Cross-references

- ROADMAP.md §3.4 Groups/Me
- GAR-790 (Linear)
- Predecessor: plan 0260 / GAR-788 (me/reactions inbox)
- message_threads schema: migration 004
- FORCE RLS policy: migration 007 (`message_threads_through_chats`)

## Estimativa

~2h implementation, ~15min review.
