# Plan 0190 — GAR-707: Search Slice 6 — types=tasks (task title/description FTS in unified search)

## Goal

Add `types=tasks` to `GET /v1/search`, enabling callers to search task titles and
descriptions via full-text search. Follows the same pattern as slice 5 (`types=files`,
plan 0185 / GAR-703): runtime `to_tsvector('simple', ...)` on an existing table with no
new migration required.

## Architecture

Pure extension of `crates/garraia-gateway/src/rest_v1/search.rs`. No new files, no new
migrations.

Changes:
- `SearchResultType::Task` variant (new discriminant in existing enum)
- `include_tasks: bool` in `ValidatedSearch` (parsed from `types` param)
- `parse_and_validate`: recognizes `"tasks"`, group-scope-only restriction (same as files),
  updated "supported" error messages
- `TaskSearchRow` struct (sqlx::FromRow)
- `fetch_tasks` async function: queries `tasks` table with runtime tsvector, optional
  `from_date`/`to_date`/`author_id` filters, excludes `deleted_at IS NOT NULL`
- Handler: `if validated.include_tasks { ... }` block mapping to `SearchResult`
- Unit tests: update `unknown_type_rejected` + 6 new tests for tasks type

## Tech Stack

- Rust / Axum 0.8 / sqlx (Postgres)
- Runtime `to_tsvector('simple', title || ' ' || coalesce(description_md, ''))` — no GIN index
  (same approach as files; acceptable for MVP given task tables are smaller than messages)
- `websearch_to_tsquery('simple', $1)` — consistent with files tokenizer choice

## Design Invariants

- NO new migration. `tasks.title` and `tasks.description_md` are existing columns.
- `types=tasks` is **group-scope only**: rejected for `scope_type=chat` and `scope_type=user`.
- Deleted tasks (`deleted_at IS NOT NULL`) always excluded.
- RLS (`tasks_group_rls_policy`, migration 006 + 007) transparently filters to
  `app.current_group_id`; explicit `t.group_id = $2` is defense-in-depth.
- `author_id` maps to `tasks.created_by` (same semantics as messages).
- `kind` in `SearchResult` = task status (so callers can display status badge).
- `excerpt` in `SearchResult` = task title (truncated at Postgres layer to 500 chars max
  — tasks.title CHECK ensures this).
- No PII: `created_by` is a UUID (not a name); `actor_label` caches are not exposed.
- Parameterized SQL only — no string concatenation.

## Validações pré-plano

- `cargo check -p garraia-gateway` → 0 errors (verified via CI on prior PRs)
- `tasks` table schema confirmed in migration 006: `title text NOT NULL CHECK (1..500)`,
  `description_md text` (nullable), `group_id uuid NOT NULL`, `deleted_at timestamptz`,
  `created_by uuid` (ON DELETE SET NULL, nullable), `status text NOT NULL`, `created_at timestamptz`

## Out of Scope

- Persistent GIN index on tasks FTS (deferred — no evidence of performance issue at current scale)
- `types=tasks` for `scope_type=chat` or `scope_type=user`
- Returning assignees, labels, or other task fields in results
- Snippet highlighting in excerpt (future slice)
- Integration tests (unit tests cover parse_and_validate; integration tests behind testcontainers
  follow the same pattern as slices 1–5 but are not required for this slice's acceptance criteria)

## Rollback

Revert search.rs. No schema changes, no migration, no production data risk.

## Open Questions

None.

## File Structure

```
crates/garraia-gateway/src/rest_v1/search.rs   — augmented (tasks support)
plans/0192-gar-707-search-slice6-tasks.md      — this file
plans/README.md                                — row 0192 added
ROADMAP.md                                     — §3.9 slice 6 row added
```

## M1 Tasks

- [x] T1 — Add `SearchResultType::Task` variant to enum
- [x] T2 — Add `include_tasks: bool` to `ValidatedSearch`
- [x] T3 — Update `parse_and_validate` (recognize `"tasks"`, group-only guard, error messages)
- [x] T4 — Add `TaskSearchRow` struct
- [x] T5 — Add `fetch_tasks` async function
- [x] T6 — Add `include_tasks` branch in handler
- [x] T7 — Update `unknown_type_rejected` test (change `"tasks"` → `"docs"`)
- [x] T8 — Add 6 new unit tests for tasks type
- [x] T9 — Update ROADMAP.md §3.9 + plans/README.md

## Risk Register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Runtime tsvector slow on large tasks table | Low | Same pattern as files; acceptable for MVP |
| NULL `created_by` breaks author_id filter | None | SQL uses `$N::uuid IS NULL OR t.created_by = $N` pattern |
| Conflict with existing `unknown_type_rejected` test | Certain | Test updated to use `"docs"` as unknown type |

## Acceptance Criteria

- `types=tasks` with `scope_type=group` → accepted, returns task matches
- `types=tasks` with `scope_type=chat` → 400
- `types=tasks` with `scope_type=user` → 400
- `types=tasks,messages` → accepted (mixed types work)
- `from_date`/`to_date`/`author_id` accepted alongside `types=tasks`
- Deleted tasks excluded (enforced by SQL `deleted_at IS NULL`)
- 20/20 CI checks green

## Cross-references

- Plan 0185 (GAR-703, slice 5/files) — pattern followed
- Plan 0086 (GAR-552, slice 3/date+author filters) — filter pattern
- ROADMAP.md §3.4 "Busca unificada" + §3.9
- GAR-707 Linear issue

## Estimativa

1–2h — single-file change, pattern established by slice 5.
