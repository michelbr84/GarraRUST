# Plan 0195 — GAR-713: REST /v1 search slice 8: sort_by parameter

## Goal

Add an optional `sort_by` query parameter to `GET /v1/search` so callers can
control the ordering of merged results across all five resource types (messages,
memory, files, tasks, task_comments).

Default is `relevance` — identical to the current behavior (slices 1–7), so
there is no breaking change.

## Architecture

`sort_by` is applied on the Rust side after all per-type SQL fetches complete and
results are merged into `Vec<SearchResult>`. Each SQL fetch already returns more
rows than the page (i.e., `fetch_up_to = offset + limit + 1`) so there are
enough candidates to re-order correctly.

No SQL changes. No migration needed.

## Tech stack

- Rust async handler in `crates/garraia-gateway/src/rest_v1/search.rs`
- `utoipa` OpenAPI annotation updated for `sort_by` param
- `serde::Deserialize` on `SearchQuery` (existing pattern)

## Design invariants

- `sort_by` absent OR `sort_by=relevance` → identical to prior behavior.
- `sort_by=created_at_desc` → `created_at DESC, score DESC, id DESC`.
- `sort_by=created_at_asc`  → `created_at ASC, score DESC, id DESC`.
- Any other value → 400 Bad Request.
- No PII exposed. No SQL injection surface (parameter is validated to an enum before SQL touch).

## Validações pré-plano

- `cargo check -p garraia-gateway` → 0 errors
- `cargo test -p garraia-gateway -- search` locally → ≥38 passing (was 34 before this slice)

## Out of scope

- Cursor-based pagination (deferred in slice 1 comment).
- Per-type sort (e.g., only sort messages by date while files stay by relevance).
- `sort_by` applied to SQL ORDER BY inside fetch functions (not needed — Rust-side merge is sufficient).

## Rollback

Revert the diff. No schema changes, no production risk.

## §12 Open questions

None.

## File structure

```
crates/garraia-gateway/src/rest_v1/search.rs  — augmented
plans/0195-gar-713-search-slice8-sort-by.md   — this file
plans/README.md                               — row 0195 added
```

## M1 tasks

- [x] T1 — Add `SortBy` enum (`Relevance`, `CreatedAtDesc`, `CreatedAtAsc`)
- [x] T2 — Add `sort_by: Option<String>` to `SearchQuery`
- [x] T3 — Add `sort_by: SortBy` to `ValidatedSearch`
- [x] T4 — Parse + validate `sort_by` in `parse_and_validate()`
- [x] T5 — Apply `SortBy` in handler sort closure
- [x] T6 — Update `make_params` + `make_params_full` helpers (add `sort_by: None`)
- [x] T7 — Add 4 unit tests (default, relevance explicit, created_at_desc, created_at_asc, invalid)
- [x] T8 — Update ROADMAP.md checklist + plans/README.md row

## Risk register

| Risk | Mitigation |
|------|-----------|
| Stable sort ordering for ties | `id` as final tiebreaker ensures deterministic order |
| Breaking change for existing callers | Default is `relevance` = current behavior |

## Acceptance criteria

- `sort_by` absent → `relevance` default, same results order as slices 1–7.
- `sort_by=relevance` → same as default.
- `sort_by=created_at_desc` → newest first.
- `sort_by=created_at_asc` → oldest first.
- `sort_by=invalid_value` → 400 Bad Request.
- All existing 34 unit tests continue to pass.
- 4 new unit tests pass.
- CI 100% green.
- PR ≤ 150 LOC.

## Cross-references

- GAR-713 Linear issue
- GAR-549 (slice 1), GAR-551 (slice 2), GAR-552 (slice 3), GAR-697 (slice 4),
  GAR-703 (slice 5), GAR-707 (slice 6), GAR-710 (slice 7)
- plans/0084-gar-ws-search-slice1.md (canonical search architecture)

## Estimativa

1h — no schema changes, no SQL changes, tests-only in structure.
