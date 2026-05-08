# Plan 0084 — GAR-549: REST /v1 search slice 1 (unified FTS)

**Status:** ✅ Merged 2026-05-08 via PR #223 (`79199ab`)
**Autor:** Claude Sonnet 4.6 (garra-routine 2026-05-08, America/New_York)
**Data:** 2026-05-08 (America/New_York)
**Issue:** [GAR-549](https://linear.app/chatgpt25/issue/GAR-549) — Done
**Branch:** `routine/202605081815-search-api-slice10` (deletada após merge)
**Epic:** `epic:ws-search`, `epic:ws-api`

---

## §1 Goal

Expose `GET /v1/search` — unified full-text search across messages and
memory_items scoped to a group. This is the only remaining non-blocked
Fase 3.4 endpoint (WebSocket and Files are deferred).

```
GET /v1/search
  ?q=<query>
  &scope_type=group
  &scope_id=<group_uuid>
  &types=messages,memory          (default: both)
  &limit=<1-50>                   (default 20)
  &offset=<n>                     (default 0)
```

Response: `SearchResponse { items: Vec<SearchResult>, has_more: bool }`.

---

## §2 Architecture

```
crates/garraia-gateway/src/
  rest_v1/
    search.rs          ← NEW: handler + DTOs + unit tests
    mod.rs             ← add pub mod search; routes in all 3 modes
    openapi.rs         ← add search::search to paths(...)
  tests/
    rest_v1_search.rs  ← NEW: 8 integration test scenarios
```

### FTS strategy

| Table         | Column        | Index         | FTS function               |
|---------------|---------------|---------------|----------------------------|
| `messages`    | `body_tsv`    | GIN (mig 004) | `body_tsv @@ q`            |
| `memory_items`| `content`     | none (runtime)| `to_tsvector('portuguese', content) @@ q` |

`q` is always built via `websearch_to_tsquery('portuguese', $1)` — never
`to_tsquery` (operator injection risk documented in migration 004 comment).

### RLS protocol (plan 0056 pattern)

Both `messages` and `memory_items` are FORCE RLS tables. Two `set_config`
calls before any SELECT:

```sql
SELECT set_config('app.current_user_id', $1, true)
SELECT set_config('app.current_group_id', $1, true)
```

`true` = transaction-local (cleared on COMMIT/ROLLBACK).

### Result merging

Run both type queries independently within the same transaction. Collect
into a `Vec<SearchResult>`, sort by `(score DESC, created_at DESC, id DESC)`,
apply `[offset .. offset+limit]`, set `has_more = total_collected > offset + limit`.

Offset-based pagination is standard for FTS (cursor pagination across
heterogeneous ranked results is pathological).

---

## §3 Tech Stack

- sqlx `QueryBuilder` (parameterized, no string concat)
- `websearch_to_tsquery('portuguese', $1)` — safe user-input FTS
- `ts_rank(body_tsv, query)` / `ts_rank(to_tsvector('portuguese', content), query)` → `f32`
- utoipa `#[utoipa::path]` for OpenAPI docs
- `garraia_auth::{Principal, AppPool}` — same as all other slices

---

## §4 Design Invariants

1. **NO SQL string concat** — all params via `push_bind` or `$N` placeholders.
2. **RLS both vars** — `app.current_user_id` + `app.current_group_id` SET LOCAL before every query.
3. **Cross-group 404** — `scope_id ≠ principal.group_id` → 404 (not 403).
4. **`q` sanitisation** — empty `q` → 400; max 256 chars → 400; `websearch_to_tsquery` handles the rest.
5. **`sensitivity='secret'` excluded** from memory results (same filter as memory.rs).
6. **`deleted_at IS NULL`** applied to both messages and memory_items.
7. **No audit event** for search reads (no circular noise).
8. **Max limit 50** — prevents runaway queries; offset max 10 000 (DoS mitigation).

---

## §5 Validações pré-plano

- [x] `messages.body_tsv` GIN index exists (migration 004, `messages_body_tsv_idx`)
- [x] `memory_items.content` queryable at runtime via `to_tsvector`
- [x] `AppPool` + `Principal` + `set_config` pattern established (plans 0056+)
- [x] No new migration required
- [x] Linear search confirms no duplicate issue for `GET /v1/search` in GAR

---

## §6 Out of Scope

- FTS on tasks, files, audit_events
- `scope_type=user` or `scope_type=chat` (group-only for slice 1)
- Cursor-based pagination
- Embedding/vector search (GAR-372)
- WebSocket streaming results
- Highlighting/snippet extraction (future slice)

---

## §7 Rollback

Delete the branch. The only change is an additive `search.rs` module +
route registrations. No migration, no schema change. Reverting is `git revert`.

---

## §8 File Structure

```
crates/garraia-gateway/src/rest_v1/search.rs   (NEW ~280 LOC)
crates/garraia-gateway/src/rest_v1/mod.rs      (EDIT: +pub mod + routes)
crates/garraia-gateway/src/rest_v1/openapi.rs  (EDIT: +search::search)
crates/garraia-gateway/tests/rest_v1_search.rs (NEW ~150 LOC)
plans/0084-gar-549-search-api-slice1.md        (this file)
```

---

## §9 M1 Tasks

- [ ] T1: `search.rs` — DTOs, validation, unit tests (red)
- [ ] T2: FTS query helpers — messages + memory queries with `set_config` RLS
- [ ] T3: Handler + `SearchResult` merge/sort + router wiring (all 3 modes)
- [ ] T4: Integration tests (`rest_v1_search.rs`)
- [ ] T5: OpenAPI registration + `mod.rs` `pub mod search`
- [ ] T6: ROADMAP §3.4 checkbox + plans/README.md row

---

## §10 Risk Register

| Risk | Mitigation |
|------|-----------|
| `to_tsvector` on memory.content is slow without GIN index | Acceptable for slice 1 (content ≤ 10k, RLS filters first); add index in future slice if p95 > 100ms |
| `websearch_to_tsquery` returns empty tsquery for nonsense input | Handled: Postgres returns 0 rows, not an error |
| Large offset causes full-table scan | offset MAX = 10 000 → 400 |
| Cross-language content ranks poorly with 'portuguese' config | Known limitation; config is per-DB; noted in plan |

---

## §11 Acceptance Criteria

1. `GET /v1/search?q=hello&scope_type=group&scope_id=<uuid>&types=messages` returns messages matching "hello" in group.
2. `GET /v1/search?q=hello&scope_type=group&scope_id=<uuid>&types=memory` returns memory_items matching "hello".
3. `types=messages,memory` returns merged results sorted by score DESC.
4. Cross-group: `scope_id` ≠ `principal.group_id` → 404.
5. Empty `q` → 400.
6. Unknown type → 400.
7. `has_more=true` when results exceed limit.
8. `sensitivity='secret'` memory items not returned.
9. `cargo clippy --workspace ... -D warnings` green.
10. All CI checks green.

---

## §12 Cross-references

- plan 0056 (GAR-508): `set_config` RLS protocol
- plan 0055 (GAR-507): messages patterns
- plan 0062 (GAR-514): memory patterns
- migration 004: `messages.body_tsv` (tsvector, 'portuguese', GIN)
- migration 005: `memory_items.content` (text, CHECK 10k)
- ADR 0006: search strategy decision (Postgres FTS → Tantivy → Meilisearch)
- ROADMAP §3.4: `GET /v1/search` checkbox

---

## §13 Estimativa

- Implementação: 3–4 h
- LOC: ~430 (search.rs 280 + mod 30 + openapi 5 + tests 115)
- Risco: baixo (padrão estabelecido, sem migração)
