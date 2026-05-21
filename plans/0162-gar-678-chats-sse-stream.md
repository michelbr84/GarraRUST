# Plan 0162 — GAR-678: REST /v1/chats SSE stream

**Linear issue:** [GAR-678](https://linear.app/chatgpt25/issue/GAR-678) — "REST /v1 chats slice 3: GET /v1/chats/{chat_id}/stream (SSE) — real-time message events" (Backlog → In Progress → **Done** 2026-05-21). Labels: `epic:ws-chat`, `epic:ws-api`. Project: Fase 3 — Group Workspace.

> **Linear ID correction (2026-05-21):** the original PR #459 body and this plan
> file initially referenced GAR-670, which is in fact "Health Routine run 3"
> bookkeeping. The autonomous routine misattributed the ID during PR
> creation. [GAR-678](https://linear.app/chatgpt25/issue/GAR-678) was created
> specifically to track this work with full context; the plan file was renamed
> from `0162-gar-670-chats-sse-stream.md` → `0162-gar-678-chats-sse-stream.md`
> in this bookkeeping pass.

**Status:** ✅ Merged 2026-05-21 via PR [#459](https://github.com/michelbr84/GarraRUST/pull/459) (`d25b64c`).

## Post-merge addendum (audit fixes)

Three audit findings closed on the same branch before merge:

| Finding | Severity | Fix commit | Note |
|---|---|---|---|
| F-1 | ALTO | `aaae3c5` | `RestV1FullState::cleanup_chat_subscription` + `ChatStreamGuard` RAII — DashMap entry GC'd when last receiver disconnects. Closes the leak originally documented in §"Broadcast table" below. |
| F-2 | MÉDIO | `20216e0` | New `tests/rest_v1_chats_sse.rs` — 4 scenarios (cross-tenant 404, happy-path 200, missing X-Group-Id 400, archived chat 404). CLAUDE.md regra #10. |
| RLS-tx bug (exposed by F-2) | ALTO | `19ec966` | `stream_chat` was calling `SELECT set_config(_,_,true)` outside a tx → setting reverted in auto-commit → FORCE RLS rejected all rows. Fix: wrap in `pool.begin()` matching `messages.rs`. |

Open follow-ups (post-merge):

- [GAR-679](https://linear.app/chatgpt25/issue/GAR-679) — F-3: SSE rate-limit per user/group (Medium).
- [GAR-680](https://linear.app/chatgpt25/issue/GAR-680) — F-4: audit-log of SSE subscriptions (Low).

## Goal

Ship `GET /v1/chats/{chat_id}/stream` — a Server-Sent Events (SSE) endpoint that
delivers real-time `message.created` events to connected clients whenever a new
message is posted to a chat via `POST /v1/chats/{chat_id}/messages`. This closes
the only remaining unchecked item in ROADMAP §3.4 API surface that is shovel-ready
(<500 LOC, schema + auth already shipped).

## Architecture

### Broadcast table
Add `chat_events: Arc<DashMap<Uuid, tokio::sync::broadcast::Sender<serde_json::Value>>>`
to both `AppState` (owned) and `RestV1FullState` (clone of the Arc). The DashMap is
created lazily — an entry appears the first time a client subscribes to a given
`chat_id`, and stays indefinitely (no GC needed; channels are cheap). When no
subscribers are connected, `Sender::send` is never called (only the publish path
checks for existence). When a broadcast `Sender` has no receivers, the call is
silently dropped (`send` returns `Err(SendError)` which we ignore).

### Subscribe flow (`GET /v1/chats/{chat_id}/stream`)
1. Auth: `Principal` extractor — group membership + `Action::ChatsRead`.
2. RLS sanity: verify `chat_id` belongs to `principal.group_id` (same query as `send_message`).
3. Call `state.subscribe_chat(chat_id)` → lazily creates the broadcast entry and returns a `Receiver`.
4. Convert receiver to `impl Stream<Item = Result<Event, Infallible>>` via `futures::stream::unfold`.
5. Wrap in `Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(30)))`.

### Publish flow (`POST /v1/chats/{chat_id}/messages`)
After `tx.commit()`, call `state.publish_chat_event(chat_id, json_value)` where
`json_value` is the full `MessageResponse` fields. If no subscriber exists, the call
is a no-op (`DashMap::get` returns `None`).

### Event wire format
```
event: message.created
data: {"id":"...","chat_id":"...","group_id":"...","sender_user_id":"...","sender_label":"...","body":"...","reply_to_id":null,"created_at":"2026-05-21T...Z"}

event: stream.lagged
data: 3
```

`stream.lagged` is emitted when the broadcast channel drops messages due to a slow
consumer (tokio's `RecvError::Lagged`). The data is the count of dropped messages.

### Keep-alive
`KeepAlive::new().interval(30s)` sends a `:` comment line (blank SSE comment) every
30 seconds so load-balancers and proxies don't close idle connections.

## Tech stack

- **Axum 0.8** `axum::response::sse::{Event, KeepAlive, Sse}` — no extra feature flag needed.
- **futures 0.3** `futures::stream::unfold` — already in gateway Cargo.toml.
- **tokio::sync::broadcast** — already used for `log_tx` in AppState.
- **DashMap** — already used for `sessions`, `channel_models`, etc.
- No new crate dependencies.

## Design invariants

1. **Auth parity** — same `Principal` + `Action::ChatsRead` + RLS chat-membership check as `list_messages`.
2. **No PII in audit events** — `stream_chat` does not write to `audit_events` (read-only subscription).
3. **Backpressure** — broadcast channel capacity 64; slow consumers receive `stream.lagged` and continue.
4. **Fail-soft** — no subscriber channel? `publish_chat_event` is a no-op. Handler absent (app_pool not wired)? Returns 503 same as other `/v1/*` handlers.
5. **No `unwrap()` in production** — all broadcast errors are silently dropped or mapped to SSE events.
6. **No SQL string concatenation** — UUIDs interpolated only in `SET LOCAL` (injection-safe by Uuid::Display, same pattern as all other handlers).

## Validações pré-plano

- [x] `chats` table under FORCE RLS with `chat_members` membership (migration 007).
- [x] `chat_members` checked by Principal extractor (group membership).
- [x] Handler `send_message` already verifies `chat_id` ∈ `group_id` (0 rows → 404).
- [x] `log_tx: broadcast::Sender<Value>` in AppState proves the pattern compiles.
- [x] `futures` in gateway Cargo.toml — `stream::unfold` available.
- [x] Axum 0.8 ships `response::sse` in core crate (no feature flag).
- [x] Router already has `/v1/chats/{chat_id}/messages` — adding `/stream` on same prefix.

## Out of scope

- WebSocket upgrade (WS is listed in ROADMAP but is a distinct, heavier slice).
- Fan-out to multiple groups simultaneously.
- Message edit/delete events (only `message.created` in this slice).
- Garbage-collecting empty broadcast entries.
- Integration test against a real Postgres testcontainer (unit tests + mock cover the slice).

## Rollback

Revert the 4 file changes (`state.rs`, `mod.rs`, `messages.rs`, `chats.rs`). No schema
migration — purely in-process state.

## File structure

```
crates/garraia-gateway/src/
  state.rs              — add chat_events field + subscribe_chat + publish_chat_event
  rest_v1/
    mod.rs              — add chat_events to RestV1FullState + from_app_state + helpers
    messages.rs         — add publish after tx.commit()
    chats.rs            — add stream_chat SSE handler + unit tests
```

## Task list

- [x] T1 — Add `chat_events` to `AppState` + `subscribe_chat` + `publish_chat_event` methods
- [x] T2 — Add `chat_events` to `RestV1FullState` + wire in `from_app_state`
- [x] T3 — Broadcast in `send_message` after commit
- [x] T4 — Add `stream_chat` SSE handler + `get.route("/v1/chats/{chat_id}/stream")`
- [x] T5 — Unit tests (broadcast publish/subscribe, auth guard, event format)
- [x] T6 — `cargo check -p garraia-gateway` + clippy clean
- [x] T7 — PR, CI green, squash-merge
- [x] T8 — Bookkeeping: ROADMAP §3.4 check-off + plans/README row

## Risk register

| Risk | Probability | Mitigation |
|---|---|---|
| SSE connection limit per client (browser 6-conn cap) | Low | Clients open one SSE per active chat tab — typical usage is 1-2 |
| Lagged consumers causing noisy `stream.lagged` events | Low | Channel cap 64 covers >10 msg/s for 6s lag, typical chat cadence much lower |
| `RestV1FullState::from_app_state` returns `None` when `AppPool` absent | By design | Fail-soft mode already returns 503 for all `/v1/*` routes |

## Acceptance criteria

1. `GET /v1/chats/{id}/stream` returns `200 text/event-stream` for authenticated member.
2. After `POST /v1/chats/{id}/messages`, subscriber receives `event: message.created` SSE.
3. Non-member of the group receives 403.
4. Unknown `chat_id` receives 404.
5. `cargo clippy --workspace --tests --exclude garraia-desktop -- -D warnings` passes.

## Cross-references

- ROADMAP §3.4 `[ ] WebSocket /v1/chats/{chat_id}/stream` → closes as SSE (same functional slot, WS deferred).
- Plans 0054 (GAR-506 chats slice 1), 0055 (GAR-507 messages slice 2) — predecessors.
- `AppState::log_tx` pattern (`state.rs:214`) — reference impl for broadcast.

## Estimativa

- LOC: ~350 (state.rs ~30, mod.rs ~30, messages.rs ~15, chats.rs ~200, tests ~75)
- Tempo: 1 sessão (~2h)
