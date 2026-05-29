# Plan 0219 — GAR-737: REST /v1 search slice 15 — `types=labels` task label name FTS

## Goal

Add `types=labels` to `GET /v1/search`, enabling callers to search for task labels
by name using full-text search. Group scope only — cross-tenant isolation via FORCE RLS
(`task_labels_group_isolation` policy, migration 006) + explicit `AND group_id = $2`
defense-in-depth.

## Scope

- Extend `GET /v1/search?q=...&scope_type=group&scope_id=<uuid>&types=labels`
- `fetch_labels()` searches `task_labels.name` via runtime `to_tsvector('simple', name)`
- No new migration — `task_labels` table and RLS policy (migration 006) already in place
- Group scope only (`scope_type=group` required; user scope → 400)
- `result.type = "label"`, `excerpt` = name, `kind` = color (`#RRGGBB`),
  `sender_user_id` = `created_by` (nullable, ON DELETE SET NULL),
  `chat_id` = null. `group_id` = `caller_group_id`.

## Implementation

### `search.rs` changes

1. `SearchResultType::Label` variant (after `Group`)
2. `include_labels: bool` in `ValidatedSearch`
3. `parse_and_validate`:
   - Recognizes `"labels"` type
   - Rejects `scope_type != Group` with 400
4. `LabelSearchRow` struct (`id`, `score`, `name`, `color`, `created_by`, `created_at`)
5. `fetch_labels()` async function — runtime tsvector on `tl.name`
6. Handler block: `if validated.include_labels { ... }`

### Why no additional membership filter in SQL

Migration 006 adds `FORCE ROW LEVEL SECURITY` to `task_labels` with policy
`task_labels_group_isolation`. The USING clause checks `group_id = app.current_group_id`.
The `garraia_app` role runs all queries inside the request transaction where `SET LOCAL
app.current_group_id` is set. Additionally the query carries an explicit
`AND tl.group_id = $2` for defense-in-depth (belt + suspenders). Combined these ensure
results are strictly scoped to the caller's group.

## Tests (5 unit)

| ID | Name | Expectation |
|----|------|-------------|
| U1 | `types_labels_group_scope_accepted` | group scope → `include_labels = true` |
| U2 | `types_labels_user_scope_rejected` | user scope → Err |
| U3 | `types_labels_chat_scope_rejected` | chat scope → Err |
| U4 | `types_labels_and_tasks_group_scope_accepted` | labels+tasks, group scope → both true |
| U5 | `types_labels_in_supported_types_error_message` | unknown type error mentions "labels" |

## Risks

- None: no migration, no schema change. FORCE RLS pre-exists in migration 006.

## Tasks

- [x] T1: Implement `SearchResultType::Label`, `include_labels`, `fetch_labels()`, handler block
- [x] T2: Fix clippy `doc-lazy-continuation` warning in module doc
- [x] T3: Add 5 unit tests
- [x] T4: Update ROADMAP.md + plans/README.md
- [x] T5: Commit + push on `routine/202605290025-search-slice15-labels`
- [ ] T6: Open PR, await CI green, merge
- [ ] T7: Mark GAR-737 Done in Linear
