# Plan 0214 — GAR-730: REST /v1 search slice 13 — `types=users` group member display_name FTS

## Goal

Add `types=users` to `GET /v1/search`, enabling callers to search for group members
by `display_name` using Postgres full-text search. Useful for @-mention autocomplete,
member discovery, and user lookup within a workspace.

## Architecture

Same pattern as slices 5-12: add a new `SearchResultType` variant, a `UserSearchRow`
struct, a `fetch_users()` async query function, and wire it through
`parse_and_validate` + the handler block.

```
GET /v1/search?q=joao&scope_type=group&scope_id=<uuid>&types=users

→ fetch_users(pool, q, group_id, from_date, to_date, limit, offset)
  SELECT u.id, ts_rank(…) as score, u.display_name as excerpt,
         $group_id as group_id, gm.joined_at as created_at
  FROM group_members gm JOIN users u ON u.id = gm.user_id
  WHERE gm.group_id = $group_id AND gm.status = 'active'
    AND to_tsvector('simple', u.display_name) @@ websearch_to_tsquery('simple', $q)
    [AND gm.joined_at >= $from_date] [AND gm.joined_at <= $to_date]
  ORDER BY score DESC, gm.joined_at DESC
  LIMIT $limit OFFSET $offset
```

## Tech stack

- Rust / SQLx (garraia-gateway/src/rest_v1/search.rs) — no new crates
- No new migration (users + group_members tables exist since migration 001)
- RLS tenant-context: `SET LOCAL app.current_user_id` + `SET LOCAL app.current_group_id`
  already set before any SELECT in this handler

## Design invariants

- `types=users` is **group scope only**: rejected with 400 for `scope_type=chat` and
  `scope_type=user` (consistent with all other group-only types: files, tasks, etc.)
- Inactive members excluded: `gm.status = 'active'` WHERE clause
- Cross-tenant isolation: `gm.group_id = $scope_id` is the SQL constraint; the handler
  validates `scope_id == principal.group_id` (same pattern as other group-scope types)
- `author_id` filter is rejected (400) when `types=users` is the only or combined type,
  as it applies only to messages. Actually: only reject if user requests types=users +
  no messages AND author_id is set. Follow the existing pattern: `author_id` is accepted
  silently and simply not applied to user results (consistent with how it works for
  non-message types in slices 5-11).
- `has_attachment` filter is message-only and already validated before fetch_users;
  users results are unaffected
- excerpt = `display_name` only (no email in FTS or excerpt — PII safety)
- `sender_user_id` = `u.id` (the matched user's own id, not a "sender" but reuses the field
  for the resource owner id pattern established by file/chat/folder slices)

## Out of scope

- Fuzzy/trigram matching (pg_trgm): deferred, runtime tsvector 'simple' is consistent
  with all other slices
- Returning email in excerpt: PII concern, display_name only
- Searching across groups (global user search): requires elevated permission, deferred
- Searching by email: deferred, email is PII

## Rollback

Revert the search.rs changes: remove the `User` variant, the `UserSearchRow` struct,
`fetch_users()`, the `include_users` field, and the parse/handler blocks.

## File structure

Only one file changes:

```
crates/garraia-gateway/src/rest_v1/search.rs  (+~100 LOC)
plans/0212-gar-730-search-slice13-users.md    (this file)
plans/README.md                               (row 0212 added)
ROADMAP.md                                    (slice 13 entry in §3.4 busca)
```

## M1 — Implementation

- [ ] T1: Add `SearchResultType::User` variant + doc comment
- [ ] T2: Add `UserSearchRow { id, score, excerpt, group_id, created_at }` struct
- [ ] T3: Add `include_users: bool` to `ValidatedSearch`
- [ ] T4: `parse_and_validate`: parse `"users"` in types loop, add group-scope-only validation,
       add `include_users` to the "must include at least one" check and the error message
- [ ] T5: Implement `fetch_users(pool, q, group_id, from_date, to_date, limit, offset)`
       using `sqlx::query_as!` macro and `websearch_to_tsquery`
- [ ] T6: Wire `fetch_users` in the handler block (same pattern as `fetch_threads` etc.)
- [ ] T7: Add ≥6 unit tests in `#[cfg(test)] mod tests {}`:
       - `users_type_parsed_ok`
       - `users_type_rejected_for_chat_scope`
       - `users_type_rejected_for_user_scope`
       - `users_in_supported_types_error_message`
       - `unknown_type_still_rejected` (regression: ensure "users" not included in old error)
       - `users_combined_with_messages_ok`
- [ ] T8: Update ROADMAP.md + plans/README.md + plan header
- [ ] T9: `cargo clippy --workspace --tests --exclude garraia-desktop --no-deps -- -D warnings`
- [ ] T10: Commit + push

## Risk register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Cross-tenant leak via group_id bypass | Low | Critical | `gm.group_id = $scope_id` in SQL + handler scope validation |
| Email leakage in excerpt | N/A | High | Only `display_name` in excerpt, email not in FTS |
| RUSTSEC in transitive deps | Low | Low | CI cargo-deny gate |

## Acceptance criteria

- `types=users` accepted for `scope_type=group`, rejected for `chat`/`user` scopes
- Results contain only members of the caller's group (active status)
- ≥6 unit tests green
- Clippy clean
- CI green (20/20 checks)

## Cross-references

- Slice 12 (GAR-726 / plan 0211): types=threads — immediate predecessor
- Slice 11 (GAR-721 / plan 0208): types=task_lists — pattern reference
- GAR-730: Linear issue for this slice
- ROADMAP §3.4 Busca unificada

## Estimativa

- Low: 45 min
- Likely: 1h
- High: 1h30
