# Plan 0086 — GAR-552: REST /v1 search slice 3 (date-range + author_id filters)

**Status:** ✅ Merged 2026-05-09 via PR #231 (`49c4a6b`)
**Autor:** Claude Sonnet 4.6 (garra-routine 2026-05-09, America/New_York)
**Data:** 2026-05-09 (America/New_York)
**Issue:** [GAR-552](https://linear.app/chatgpt25/issue/GAR-552) — Done
**Branch:** `routine/202605090015-search-slice3-date-author-filters`
**Epic:** `epic:ws-search`, `epic:ws-api`
**Predecessor:** plan 0085 (GAR-551, slice 2)

---

## §1 Goal

Extend `GET /v1/search` with three optional filter parameters that implement
part of the ROADMAP §3.9 checklist ("Filtros: scope, types, from_date, author,
has_attachment"):

```
GET /v1/search
  ?q=<query>
  &scope_type=group|chat|user
  &scope_id=<uuid>
  &from_date=2026-01-01T00:00:00Z   # NEW — created_at >= from_date
  &to_date=2026-06-01T00:00:00Z     # NEW — created_at <= to_date
  &author_id=<uuid>                 # NEW — message sender_user_id (not user scope)
```

All three parameters are optional and additive with existing slice 1+2 filters.

---

## §2 Architecture

```
crates/garraia-gateway/src/rest_v1/
  search.rs   ← EDIT only: extend SearchQuery, ValidatedSearch,
                parse_and_validate, fetch_messages, fetch_memory
plans/
  0086-...md  ← this file
plans/README.md ← add row
```

No schema changes, no new migrations, no new crates. Pure Rust extension.

### Validation rules

| Condition | Response |
|-----------|----------|
| `from_date > to_date` (both present) | 400 |
| `author_id` + `scope_type=user` | 400 (user scope has no messages) |
| `from_date` or `to_date` alone | valid |
| `author_id` with `scope_type=group` or `=chat` | valid |

### SQL strategy

Both queries use nullable predicates via sqlx bind:

```sql
-- messages
AND ($N::timestamptz IS NULL OR m.created_at >= $N)
AND ($M::timestamptz IS NULL OR m.created_at <= $M)
AND ($P::uuid IS NULL OR m.sender_user_id = $P)

-- memory_items
AND ($N::timestamptz IS NULL OR mi.created_at >= $N)
AND ($M::timestamptz IS NULL OR mi.created_at <= $M)
```

The `author_id` filter is NOT applied to memory results (no author concept there).

---

## §3 Tech stack

- Rust / Axum 0.8 / sqlx 0.8
- `chrono::DateTime<Utc>` — already in workspace with `serde` feature, parsed
  automatically by Axum's `Query<T>` deserializer from ISO 8601 strings
- No new dependencies

---

## §4 Design invariants

- `author_id` on user-scope is rejected (400) even though it's technically harmless
  (no messages to filter): fail loudly to avoid misleading callers.
- Date filters apply to BOTH messages and memory rows — consistent `created_at`
  semantics across types.
- Offset pagination semantics unchanged: `fetch_up_to = offset + limit + 1` still
  drives `has_more` detection after Rust-side sort.

---

## §5 Validações pré-plano

- [x] chrono serde feature enabled in workspace (`Cargo.toml:chrono = { version = "0.4", features = ["serde"] }`)
- [x] `DateTime<Utc>` already imported and used in `search.rs`
- [x] `fetch_messages` and `fetch_memory` use sqlx positional params — safe to extend
- [x] No integration tests for search (unit tests only so far) — unit test coverage is the target
- [x] GAR-552 created in Linear, no duplicate found

---

## §6 Out of scope

- `has_attachment` filter — requires schema change (attachments column on messages)
- Hybrid BM25 + ANN vector re-ranking
- Cursor-based pagination
- `author_id` filter on memory results

---

## §7 Rollback

Pure extension to existing module — removing the 3 new params from `SearchQuery`
and reverting the SQL predicates restores slice 2 behavior exactly. No migration
needed to rollback.

---

## §8 Open questions

None. Approach is directly analogous to existing nullable-predicate pattern already
in `fetch_messages` (`$3::uuid IS NULL OR m.chat_id = $3`).

---

## §9 File structure

```
crates/garraia-gateway/src/rest_v1/search.rs   (edit)
plans/0086-gar-552-search-api-slice3-date-author-filters.md  (this)
plans/README.md                                               (edit: add row)
```

---

## §10 M1 tasks

- [x] T1: Extend `SearchQuery` + `ValidatedSearch` with 3 new fields; update `parse_and_validate`
- [x] T2: Update `fetch_messages` SQL + signature for date + author params
- [x] T3: Update `fetch_memory` SQL + signature for date params
- [x] T4: Wire new params through handler
- [x] T5: Add ≥ 10 unit tests for new validation branches
- [x] T6: `cargo check -p garraia-gateway` + `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings`
- [x] T7: Commit + push; open PR
- [x] T8: Update plans/README.md + ROADMAP checklist after merge

---

## §11 Risk register

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| chrono serde parse fails on non-UTC input | Low | Low | Serde rejects non-UTC automatically; caller gets 422 |
| Nullable predicate `$N::timestamptz IS NULL` rejected by sqlx macro | Low | Low | Using `sqlx::query_as` (not `query!` macro) — dynamic bind is fine |
| Offset-based pagination over filtered results differs from caller expectation | Low | Low | Documented in §4: offset applied after Rust sort, consistent with slices 1+2 |

---

## §12 Acceptance criteria

- [x] `?from_date=2026-01-01T00:00:00Z` filters results to `created_at >= 2026-01-01`
- [x] `?to_date=2026-06-01T00:00:00Z` filters results to `created_at <= 2026-06-01`
- [x] `from_date > to_date` → 400 with descriptive message
- [x] `?author_id=<uuid>` on group/chat scope → filters messages to that sender
- [x] `?author_id=<uuid>` on user scope → 400
- [x] All slice 1 + slice 2 unit tests pass unchanged
- [x] `cargo clippy` clean, no warnings

---

## Cross-references

- ROADMAP §3.9 "Busca unificada" `[ ] Filtros: scope, types, from_date, author, has_attachment`
- plan 0084 (GAR-549) — slice 1 (group scope)
- plan 0085 (GAR-551) — slice 2 (chat + user scope)
- plan 0056 (GAR-508) — set_config RLS pattern used throughout

---

## Estimativa

- T1–T5: ~2h (extension of existing code, no new concepts)
- T6–T7: ~30min (CI run time)
- Total: ~3h calendar
