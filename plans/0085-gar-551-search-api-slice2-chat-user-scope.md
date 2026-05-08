# Plan 0085 — GAR-551: REST /v1 search slice 2 (chat + user scope)

**Status:** 🟡 In Progress
**Autor:** Claude Opus 4.7 (garra-routine 2026-05-08, America/New_York)
**Data:** 2026-05-08 (America/New_York)
**Issue:** [GAR-551](https://linear.app/chatgpt25/issue/GAR-551) — Backlog → In Progress
**Branch:** `routine/202605082234-search-slice2-chat-user-scope`
**Epic:** `epic:ws-search`, `epic:ws-api`
**Predecessor:** plan 0084 (GAR-549, slice 1)

---

## §1 Goal

Lift the `scope_type=group` restriction in slice 1 by adding two new
scopes to `GET /v1/search`:

```
GET /v1/search
  ?q=<query>
  &scope_type=chat               # NEW
  &scope_id=<chat_uuid>
  &types=messages,memory         (default: both)
  &limit=<1-50>                  (default 20)
  &offset=<n>                    (default 0)

GET /v1/search
  ?q=<query>
  &scope_type=user               # NEW
  &scope_id=<user_uuid>          (must equal principal.user_id)
  &types=memory                  (messages invalid for user scope)
  &limit=<1-50>
  &offset=<n>
```

Slice 1 (`scope_type=group`) behavior is preserved unchanged.

---

## §2 Architecture

```
crates/garraia-gateway/src/rest_v1/
  search.rs                              ← EDIT: extend validation + queries
  openapi.rs                             ← unchanged (utoipa picks up new param values)
crates/garraia-gateway/tests/
  rest_v1_search.rs                      ← EDIT: add 8 new scenarios for chat/user scope
plans/0085-...md                          ← this file
```

### Scope dispatcher

A new internal enum encodes the validated scope:

```rust
enum Scope {
    Group { group_id: Uuid },
    Chat  { chat_id: Uuid, group_id: Uuid },
    User  { user_id: Uuid, group_id: Uuid },
}
```

`parse_and_validate(params, principal)` returns `Scope` plus a refined
`include_messages` / `include_memory` policy:

| scope_type | include_messages allowed? | include_memory allowed? |
|------------|---------------------------|-------------------------|
| group      | yes                       | yes                     |
| chat       | yes (filtered by chat_id) | yes (`scope_type='chat' AND scope_id=$chat_id`) |
| user       | **400** if requested      | yes (`scope_type='user' AND scope_id=$user_id`) |

### Cross-tenant 404 matrix

| Condition                                                  | Status |
|------------------------------------------------------------|--------|
| `scope_type=user` + `scope_id ≠ principal.user_id`         | 404    |
| `scope_type=chat` + `scope_id` not in caller's group       | 404    |
| `scope_type=group` + `scope_id ≠ principal.group_id` (1)   | 404    |

(1) preserved from slice 1.

### RLS protocol

Same as slice 1: both `app.current_user_id` and `app.current_group_id`
are SET LOCAL via parameterized `set_config` before any SELECT. RLS on
`memory_items` (policy `memory_items_group_or_self`, migration 007:133)
already covers all three scope_types via its dual branch. Messages RLS
is direct on `group_id`.

### Chat existence validation

For `scope_type=chat`, before any FTS query:

```sql
SELECT id FROM chats
WHERE id = $1 AND group_id = $2 AND archived_at IS NULL
```

`Some(_)` → continue; `None` → 404. Pattern mirrors `messages.rs` and
`memory.rs` chat scope validation. No `chat_members` lookup is added in
this slice — visibility is "any chat in caller's group", consistent with
slice 1 of the chat surface.

### Query filter changes

**Messages query** (slice 2 adds optional `chat_id` predicate):

```sql
SELECT m.id, ts_rank(...) AS score, m.body, m.group_id, m.chat_id,
       m.sender_user_id, m.created_at
FROM   messages m
WHERE  m.body_tsv @@ websearch_to_tsquery('portuguese', $1)
  AND  m.group_id = $2
  AND  ($3::uuid IS NULL OR m.chat_id = $3)   -- NEW
  AND  m.deleted_at IS NULL
ORDER BY score DESC, m.created_at DESC, m.id DESC
LIMIT $4
```

**Memory query** (slice 2 adds optional `scope_type`/`scope_id` predicates):

```sql
SELECT mi.id, ts_rank(to_tsvector('portuguese', mi.content), ...) AS score,
       mi.content, mi.group_id, mi.scope_type, mi.scope_id, mi.kind, mi.created_at
FROM   memory_items mi
WHERE  to_tsvector('portuguese', mi.content) @@ websearch_to_tsquery('portuguese', $1)
  AND  ($2::text IS NULL OR mi.scope_type = $2)   -- NEW
  AND  ($3::uuid IS NULL OR mi.scope_id   = $3)   -- NEW
  AND  mi.deleted_at IS NULL
  AND  mi.sensitivity <> 'secret'
  AND  (mi.ttl_expires_at IS NULL OR mi.ttl_expires_at > now())
ORDER BY score DESC, mi.created_at DESC, mi.id DESC
LIMIT $4
```

For `scope_type=group`, the new predicates are passed as `NULL` → no-op
filter, preserving slice 1 behavior bit-for-bit. Note that
`memory_items.group_id` is NOT in the WHERE clause for user scope (per
migration 005, user-scope rows have `group_id IS NULL`); RLS handles the
`group_id IS NULL AND created_by = current_user_id` branch automatically.

---

## §3 Tech Stack

- sqlx parameter binding (no string concat, no `format!` for SET LOCAL — we
  use `set_config('app.current_*_id', $1, true)` per the established pattern)
- `websearch_to_tsquery('portuguese', $1)` for safe FTS
- utoipa `#[utoipa::path]` documentation auto-rebuilt
- `garraia_auth::{Principal, AppPool}`

---

## §4 Design Invariants

1. **Slice 1 is regression-tested** — `scope_type=group` still works.
2. **NO SQL string concat** — all user-controlled values via `bind`.
3. **RLS both vars set** — `app.current_user_id` + `app.current_group_id`.
4. **Cross-tenant → 404** — never 403 for user/chat (avoid leaking
   existence of other-group chats or other users). Slice 1 used 404 for
   group; we keep it.
5. **`types=messages` + `scope_type=user` → 400** — explicit, not silent
   filter. The error message names which scope is incompatible.
6. **`sensitivity='secret'` excluded** from memory results in all scopes.
7. **`deleted_at IS NULL`** + `ttl_expires_at` filters apply in all scopes.
8. **No audit event** for search reads (no circular noise).
9. **Chat-scoped search of `messages.body`** — a non-member of the chat
   but in the group still gets results (acceptable for `type='channel'`;
   for `dm` rooms the future slice 3 will add `chat_members` enforcement
   if product calls for it).

---

## §5 Validações pré-plano

- [x] `messages.body_tsv` GIN index (migration 004)
- [x] `memory_items.content` queryable at runtime via `to_tsvector`
  (slice 1 already does this)
- [x] `chats` RLS on `group_id` (migration 007:90-94)
- [x] `memory_items_group_or_self` policy already covers user-scope rows
  via its second branch (migration 007:133-140)
- [x] No new migration required
- [x] Linear search confirms no duplicate issue (`v1/search slice 2`,
  `search scope_type chat user` returned only related slice 1 / memory
  slice 1; no GAR-551 conflict). Created GAR-551 fresh.

---

## §6 Out of Scope

- FTS on `tasks`, `task_comments`, `files`, `audit_events`
- Cursor-based pagination (offset is fine for FTS)
- Embedding/vector search (GAR-372)
- WebSocket streaming results
- Highlighting/snippet extraction (future slice)
- `chat_members` membership enforcement for DM-type chats (slice 3 if
  product asks for it)
- Sweep of slice-1 internal review follow-ups (none filed)

---

## §7 Rollback

The change is purely additive at the handler level: validation grows
from one `scope_type` branch to three, queries gain two NULL-able bind
predicates. No migration, no schema change, no new table. Reverting is
`git revert` of the squash-merge commit. Slice 1 callers using
`scope_type=group` continue working without recompile (DTO is unchanged
beyond the scope_type accepted set).

---

## §8 File Structure

```
crates/garraia-gateway/src/rest_v1/search.rs          (EDIT, ~+150 LOC)
crates/garraia-gateway/tests/rest_v1_search.rs        (EDIT, ~+200 LOC)
plans/0085-gar-551-search-api-slice2-chat-user-scope.md  (NEW)
plans/README.md                                        (EDIT: +row 0085)
ROADMAP.md                                             (EDIT: §3.4 search note)
```

No changes to `mod.rs`, `openapi.rs`, or migrations.

---

## §9 M1 Tasks

- [ ] T1: extend `parse_and_validate` to accept `scope_type ∈ {group, chat, user}`;
      add `Scope` enum; new unit tests covering: chat-scope happy, chat-scope
      messages-only, chat-scope memory-only, user-scope memory happy, user-scope
      `types=messages` rejected, user-scope `scope_id` mismatch rejected.
- [ ] T2: refactor `fetch_messages` and `fetch_memory` to accept the new
      optional bind predicates (`chat_id_filter`, `mem_scope_type`,
      `mem_scope_id`). Slice-1 callers pass `None` → identical SQL plan.
- [ ] T3: extend the `search` handler to dispatch on `Scope`. Add chat
      existence check in-tx for chat scope (mirrors `memory.rs:362`).
- [ ] T4: integration tests in `rest_v1_search.rs` — 8 new scenarios:
      (a) chat-scope happy returns only that chat's messages,
      (b) chat-scope cross-group → 404,
      (c) chat-scope archived chat → 404,
      (d) chat-scope memory only returns `scope_type='chat'` rows for that chat,
      (e) user-scope happy returns only personal memory of caller,
      (f) user-scope `scope_id` mismatch → 404,
      (g) user-scope `types=messages` → 400,
      (h) cross-user attempt → 404 (user A tries `scope_type=user` with B's id).
- [ ] T5: update plan/ROADMAP bookkeeping — `plans/README.md` row, ROADMAP §3.4
      note for slice 2 against the existing `GET /v1/search` checkbox.

---

## §10 Risk Register

| Risk | Mitigation |
|------|-----------|
| `($N::uuid IS NULL OR ...)` triggers a sqlx type-inference cliff | Cast inside SQL; `bind(Option<Uuid>)` is canonical and matches `memory.rs` patterns |
| Slice 1 regression by accident | T1 unit tests preserve all slice-1 cases verbatim; T4 keeps 1 group-scope happy as smoke |
| RLS user-branch unexpectedly filters chat-scope memory | RLS branch 1 covers `chat` (group_id NOT NULL = principal.group_id); explicit unit test |
| User-scope memory query slow without index | `memory_items_scope_idx (scope_type, scope_id)` already exists (migration 005:32) — partial WHERE deleted_at IS NULL |
| Cross-language tokenizer mismatch | Unchanged from slice 1; documented limitation |

---

## §11 Acceptance Criteria

1. `scope_type=chat` + `scope_id ∈ caller's group` → 200 with messages
   filtered by `chat_id` and memory_items with `scope_type='chat' AND scope_id=chat_id`.
2. `scope_type=chat` + `scope_id` not in caller's group → 404.
3. `scope_type=chat` + chat archived (`archived_at IS NOT NULL`) → 404.
4. `scope_type=user` + `scope_id == principal.user_id` → 200 with only
   personal memory of caller.
5. `scope_type=user` + `scope_id ≠ principal.user_id` → 404.
6. `scope_type=user` + `types=messages` (or `messages,memory`) → 400.
7. `scope_type=group` (slice 1) regression: identical responses pre/post.
8. `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` green.
9. All required CI checks (Format, Clippy, Test×3, Build, MSRV, cargo-deny,
   Security Audit, Coverage, Analyze rust, Analyze js-ts, Playwright, E2E,
   Secret Scan, Dependency Review) pass.

---

## §12 Open questions

None. All design decisions are derivable from migrations 004/005/007 and
existing handlers (`messages.rs`, `memory.rs`, `search.rs`).

---

## §13 Cross-references

- plan 0084 (GAR-549): slice 1, defers user/chat scope in §6
- plan 0062 (GAR-514): memory.rs scope dispatcher (`user`/`group`/`chat`)
- plan 0055 (GAR-507): messages.rs chat-existence in-tx pattern
- plan 0056 (GAR-508): `set_config()` parameterized SQL convention
- migration 004: `chats`, `chat_members`, `messages` schema
- migration 005: `memory_items` scope_type/scope_id semantics
- migration 007: RLS policies (`messages_group_isolation`, `chats_group_isolation`,
  `memory_items_group_or_self` dual-branch)
- ADR 0006: search strategy decision (Postgres FTS → Tantivy → Meilisearch)
- ROADMAP §3.4: `GET /v1/search` checkbox already ticked for slice 1

---

## §14 Estimativa

- Implementação: 2–3 h (small, well-scoped extension)
- LOC: ~350 (search.rs +150, tests +200)
- Risco: baixo (padrão estabelecido em memory.rs; sem migração; slice 1 preservado)
