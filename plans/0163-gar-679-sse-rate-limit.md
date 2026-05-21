# Plan 0163 — GAR-679: SSE rate-limit per user/group (DoS hardening)

**Linear issue:** [GAR-679](https://linear.app/chatgpt25/issue/GAR-679) — "Follow-up F-3: SSE rate-limit per user/group on `/v1/chats/{id}/stream` — DoS hardening" (Backlog → In Progress → Done). Labels: `epic:ws-chat`. Project: Fase 3 — Group Workspace.

**Status:** In Progress (2026-05-21).

---

## Goal

Prevent DoS attacks where a single user opens an unbounded number of concurrent
SSE connections on `/v1/chats/{id}/stream`, exhausting file descriptors or broadcast
receiver slots. Ship an in-process, per-user connection-count gate that rejects the
*(N+1)*-th connection with **429 Too Many Requests** + `Retry-After: 60`.

This closes the open security finding F-3 from the audit of PR #459 (plan 0162).

---

## Architecture

### Rate-limit model

A **concurrent-connection cap** (not a request-rate cap) is the correct primitive
for long-lived SSE streams:

- A request-rate cap would block re-connections after a transient network hiccup.
- A concurrent-connection cap lets legitimate clients hold N streams indefinitely
  while still bounding the blast radius of a malicious client.

Limits (constants, not config — simple enough to hard-code, auditable in code):

| Key | Value | Rationale |
|-----|-------|-----------|
| `MAX_SSE_PER_USER` | 5 | Covers typical multi-tab / multi-device usage; above that it's abuse |

Group-level: not independently tracked — a group is bounded by the sum of its
members' per-user caps. With a 64-subscriber broadcast buffer already in place
(plan 0162), the per-user cap is the tighter constraint for realistic groups.

### State

```
AppState::sse_connections: Arc<DashMap<Uuid, Arc<AtomicUsize>>>
```

`Uuid` is `user_id`. The inner `Arc<AtomicUsize>` is shared with `ChatStreamGuard`
so the guard can decrement without holding a DashMap ref in async context.

Mirrored in `RestV1FullState::sse_connections`.

### Acquire / release protocol

**Acquire** (in `stream_chat`, before RLS check so the counter is not touched on
auth-failure paths):

1. `entry(user_id).or_insert_with(|| Arc::new(AtomicUsize::new(0)))` — get or create.
2. `prev = counter.fetch_add(1, Ordering::Relaxed)` — optimistically increment.
3. If `prev >= MAX_SSE_PER_USER`:
   - `counter.fetch_sub(1, Ordering::Relaxed)` — undo.
   - Return `RestError::TooManyRequests`.
4. Pass `Arc<AtomicUsize>` into `ChatStreamGuard`.

**Release** (in `ChatStreamGuard::drop`):

1. `counter.fetch_sub(1, Ordering::Relaxed)`.
2. If counter reaches 0 AND we are the only Arc holder, call
   `sse_connections.remove_if(&user_id, |_, c| c.load(Ordering::Relaxed) == 0)`
   to GC the map entry. Idempotent.

### Error response

`429 Too Many Requests` with:
- `Retry-After: 60` header (clients should wait ~1 minute before retrying).
- RFC 9457 Problem Details body (same pattern as `RestError::TooManyRequests`).

---

## Tech stack

- **`std::sync::atomic::{AtomicUsize, Ordering}`** — no new dependency.
- **`dashmap::DashMap`** — already in `Cargo.toml` for `AppState`.
- **`std::sync::Arc`** — already used throughout.

---

## Design invariants

1. Counter is incremented **before** the RLS transaction to keep the critical
   section minimal. On any error (RLS, chat-not-found, etc.), the counter is
   decremented in the same code path before returning — no RAII guard needed for
   the error branches.
2. The `ChatStreamGuard` is only constructed after the RLS transaction commits
   successfully. It always holds a valid `Arc<AtomicUsize>` with a live +1 credit.
3. `Ordering::Relaxed` is correct: there is no memory-ordering dependency between
   the counter and any other state. The counter is a pure counter, not a lock.
4. GC is safe: `remove_if` holds the DashMap shard lock while checking the
   predicate, so a concurrent `or_insert_with` cannot observe the partial-remove
   state.

---

## Out of scope

- Config-driven limits — hard constants are sufficient and simpler.
- Per-group aggregate limits — per-user limit + broadcast cap-64 already bound
  group-level blast radius.
- Persistent rate-limit state (Redis) — not needed for a connection-count gate.
- WebSocket streams — no WebSocket SSE endpoint exists; deferred.

---

## Rollback

Revert the three touched files: `state.rs`, `rest_v1/mod.rs`, `rest_v1/chats.rs`.
No migration, no schema change, no new dependency — full rollback in one commit.

---

## File structure

```
crates/garraia-gateway/src/
  state.rs                   ← add sse_connections field to AppState + init
  rest_v1/
    mod.rs                   ← add sse_connections to RestV1FullState + from_app_state
    chats.rs                 ← stream_chat: acquire/release + extend ChatStreamGuard
tests/
  rest_v1_chats_sse_rate_limit.rs  ← integration: 429 on 6th connection, 200 on 5th
```

---

## M1 — Implementation tasks

- [ ] **T1** — `state.rs`: add `sse_connections: Arc<DashMap<Uuid, Arc<AtomicUsize>>>` to `AppState`; init in `AppState::new`.
- [ ] **T2** — `rest_v1/mod.rs`: mirror `sse_connections` in `RestV1FullState`; wire in `from_app_state`.
- [ ] **T3** — `rest_v1/chats.rs`: add `RestError::TooManyRequests` variant if missing; add `MAX_SSE_PER_USER` constant; implement acquire in `stream_chat`; extend `ChatStreamGuard` with counter; implement release in `Drop`.
- [ ] **T4** — Integration test: open 5 SSE connections (all 200), 6th returns 429; after one disconnects, 6th attempt returns 200.
- [ ] **T5** — Bookkeeping: ROADMAP §3.4 `[ ]` → `[x]` for GAR-679; plans/README row.

---

## Risk register

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| AtomicUsize underflow if `fetch_sub` called without a matching `fetch_add` | Low | Medium | Guard is only constructed post-commit; error branches always call `fetch_sub` before returning |
| DashMap entry leaked if user never disconnects and process restarts | n/a | n/a | Process restart clears all in-memory state |
| Concurrent `or_insert_with` races | Low | Low | DashMap shard lock prevents double-init; `or_insert_with` is atomic |

---

## Acceptance criteria

1. `GET /v1/chats/{id}/stream` returns **429** with `Retry-After: 60` when a single
   user holds ≥ 5 concurrent SSE connections.
2. After one connection closes, the same user can open a new connection (200).
3. Five simultaneous connections from the same user all succeed (no false positives).
4. `cargo clippy --workspace --tests -- -D warnings` green.
5. All existing SSE integration tests remain green.

---

## Cross-references

- Plan 0162 (GAR-678) — SSE stream endpoint (parent feature).
- GAR-680 (✅ Done) — audit-log of SSE subscriptions (sibling follow-up F-4).
- ROADMAP §3.4 — chats API surface, open item F-3.

---

## Estimativa

- Tamanho: XS (~150 LOC new, ~30 LOC modified)
- Complexidade: Baixa
- Risco: Baixo
- Tempo estimado: 2-3 horas
