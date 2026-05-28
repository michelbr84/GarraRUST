# Plan 0215 — GAR-733: REST /v1 search slice 14 — `types=groups` group name FTS

## Goal

Add `types=groups` to `GET /v1/search`, enabling callers to search for groups
by name using full-text search. User scope only — the caller sees only groups
they are a member of, enforced at the database layer by FORCE RLS
(migration 018, `groups_member_access` policy via `app.current_user_id`).

## Scope

- Extend `GET /v1/search?q=...&scope_type=user&scope_id=<uuid>&types=groups`
- `fetch_groups()` searches `groups.name` via runtime `to_tsvector('simple', name)`
- No new migration — `groups` table and RLS policy (`migration 018`) already in place
- User scope only (groups are cross-group from the user's perspective)
- `result.type = "group"`, `excerpt` = name, `group_id` = group's own id,
  `sender_user_id` = `created_by`, `kind` = type ('family'/'team'/'personal')

## Implementation

### `search.rs` changes

1. `SearchResultType::Group` variant (after `User`)
2. `include_groups: bool` in `ValidatedSearch`
3. `parse_and_validate`:
   - Recognizes `"groups"` type
   - Rejects `scope_type != User` with 400
4. `GroupSearchRow` struct (`id`, `score`, `name`, `kind`, `created_by`, `created_at`)
5. `fetch_groups()` async function — runtime tsvector on `g.name`
6. Handler block: `if validated.include_groups { ... }`

### Why no explicit membership filter in SQL

Migration 018 adds `FORCE ROW LEVEL SECURITY` to `groups` with policy
`groups_member_access`. The USING clause does a subquery into `group_members`
checking `user_id = app.current_user_id AND status = 'active'`. The `garraia_app`
role runs all queries inside the request transaction where `SET LOCAL
app.current_user_id` is set. So `SELECT ... FROM groups` automatically returns
only the caller's groups — no explicit `WHERE group_id IN (...)` needed.

## Tests (6 unit)

| ID | Name | Expectation |
|----|------|-------------|
| U1 | `types_groups_user_scope_accepted` | user scope → `include_groups = true` |
| U2 | `types_groups_group_scope_rejected` | group scope → Err |
| U3 | `types_groups_chat_scope_rejected` | chat scope → Err |
| U4 | `types_groups_and_memory_user_scope_accepted` | memory+groups, user scope → OK |
| U5 | `types_groups_in_supported_types_error_message` | unknown type error mentions "groups" |
| U6 | `types_eleven_with_groups_group_scope_rejected` | all 11 types + groups with group scope → Err |

## Risks

- None: no migration, no schema change. FORCE RLS pre-exists.

## Tasks

- [x] T1: Implement `SearchResultType::Group`, `include_groups`, `fetch_groups()`, handler block
- [x] T2: Add 6 unit tests
- [x] T3: Update ROADMAP.md + plans/README.md
- [x] T4: Commit + push on `routine/202605281240-search-slice14-groups`
- [ ] T5: Open PR, await CI green, merge
- [ ] T6: Mark GAR-733 Done in Linear
