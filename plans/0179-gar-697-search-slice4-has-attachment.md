# Plan 0179 — GAR-697: REST /v1 search slice 4 (has_attachment filter + migration 020)

**Status:** ✅ Merged 2026-05-25
**Autor:** Claude Sonnet 4.6 (garra-routine 2026-05-25, America/New_York)
**Data:** 2026-05-25 (America/New_York)
**Issue:** [GAR-697](https://linear.app/chatgpt25/issue/GAR-697) — Done
**Branch:** `routine/202605250015-search-has-attachment`
**Epic:** `epic:ws-search`, `epic:ws-api`
**Predecessor:** plan 0086 (GAR-552, slice 3)

---

## §1 Goal

Deliver the final ROADMAP §3.9 search filter: `has_attachment=true|false` for
`GET /v1/search`. Callers can now ask:

```
GET /v1/search?q=hello&scope_type=group&scope_id=<group>&types=messages&has_attachment=true
GET /v1/search?q=hello&scope_type=group&scope_id=<group>&types=messages&has_attachment=false
```

Also creates migration 020 (`message_attachments`), the M:N join table between
`messages` and `files`, which was deferred by ROADMAP §3.2 until `files` (migration
003) was stable.

---

## §2 Architecture

```
crates/garraia-workspace/migrations/
  020_message_attachments.sql        ← NEW — M:N join table with FORCE RLS

crates/garraia-gateway/src/rest_v1/
  search.rs                          ← EDIT — has_attachment field + SQL predicate

crates/garraia-gateway/tests/
  rest_v1_search.rs                  ← EDIT — S18/S19/S20 integration scenarios

plans/
  0179-gar-697-search-slice4-has-attachment.md  ← this file
plans/README.md                                  ← add row
ROADMAP.md                                       ← mark ✅ Done
```

---

## §3 Schema: migration 020

`message_attachments` is a pure M:N join table:

| Column | Type | Notes |
|--------|------|-------|
| `message_id` | uuid PK | FK → messages ON DELETE CASCADE |
| `file_id` | uuid PK | FK → files ON DELETE CASCADE |
| `group_id` | uuid NOT NULL | Denormalized from messages.group_id for audit queries |
| `attached_by` | uuid NULLABLE | FK → users ON DELETE SET NULL |
| `attached_by_label` | text NOT NULL DEFAULT '' | Cached display name |
| `attached_at` | timestamptz NOT NULL DEFAULT now() | |

Indexes:
- `message_attachments_file_idx (file_id, attached_at DESC)` — "find all messages this file is attached to"
- `message_attachments_message_idx (message_id)` — EXISTS subquery path in search (avoids seqscan)

RLS:
- `ENABLE ROW LEVEL SECURITY` + `FORCE ROW LEVEL SECURITY`
- Policy `message_attachments_through_messages`: `message_id IN (SELECT id FROM messages)` — JOIN class, same pattern as `task_attachments` (migration 017)

Grant: `SELECT, INSERT, DELETE ON message_attachments TO garraia_app`

---

## §4 Search extension

### New field

`SearchQuery` and `ValidatedSearch` gain:
```rust
pub has_attachment: Option<bool>,
```

### Validation

```
has_attachment set + types excludes 'messages'  → 400
```

### SQL predicate (EXISTS-equality trick)

```sql
AND ($7::boolean IS NULL
     OR EXISTS (SELECT 1 FROM message_attachments ma
                WHERE ma.message_id = m.id) = $7)
```

When `$7 IS NULL` the guard short-circuits to TRUE (no filter).
When `$7 = true` returns only messages with ≥1 attachment.
When `$7 = false` returns only messages with 0 attachments.

---

## §5 Test coverage

### Unit tests (search.rs #[cfg(test)])

New tests (slice 4 block):
- `has_attachment_true_with_messages_accepted`
- `has_attachment_false_with_messages_accepted`
- `has_attachment_none_default_accepted`
- `has_attachment_with_memory_only_rejected`
- `has_attachment_with_default_types_messages_memory_accepted`

Updated tests (struct literal completeness):
- `offset_too_large_rejected` — `has_attachment: None` added
- `limit_clamped_to_max` — `has_attachment: None` added
- `make_params_full` helper — `has_attachment: None` added
- `author_id_with_user_scope_rejected` — `has_attachment: None` added

### Integration tests (rest_v1_search.rs)

- S18 — `has_attachment=true` returns only the message with an attachment
- S19 — `has_attachment=false` returns only the message without an attachment
- S20 — `has_attachment=true` + `types=memory` → 400

Integration test fixture inserts a stub file and `message_attachments` row
directly via `h.admin_pool` (superuser / BYPASSRLS) to avoid depending on a
file-upload endpoint.

---

## §6 Design invariants

- `has_attachment` filter is silently a no-op when `has_attachment IS NULL` (absent or not passed) — avoids breaking any caller that already omits the field.
- Filter only applies to message results; memory results are unaffected.
- The EXISTS subquery is covered by `message_attachments_message_idx` (migration 020) — one index lookup per message candidate, not a seqscan.
- JOIN-class RLS on `message_attachments` (`through_messages` policy) means the EXISTS subquery is automatically tenant-scoped — no explicit `group_id` predicate needed in the EXISTS clause.

---

## §7 Rollback

Drop migration 020 and revert `search.rs` to remove the `has_attachment` field
and SQL clause. No other crates are affected.

---

## §8 M1 tasks

- [x] T1: Create migration 020 (`message_attachments` table + RLS + grants)
- [x] T2: Extend `SearchQuery` + `ValidatedSearch` with `has_attachment: Option<bool>`
- [x] T3: Add `has_attachment` validation in `parse_and_validate`
- [x] T4: Extend `fetch_messages` signature + SQL predicate
- [x] T5: Wire `validated.has_attachment` through handler call-site
- [x] T6: Update unit test helpers and raw struct literals to include `has_attachment: None`
- [x] T7: Add 5 unit tests for slice 4 validation
- [x] T8: Add S18/S19/S20 integration scenarios
- [x] T9: Create plan file + update plans/README.md + ROADMAP.md
- [x] T10: Commit + push; open PR; wait for green CI; merge; bookkeeping

---

## §9 Risk register

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| EXISTS = $7 fails when $7 IS NULL | Low | Low | Outer `$7::boolean IS NULL OR ...` guard short-circuits before EXISTS fires |
| message_attachments RLS blocks EXISTS in search | Low | Medium | JOIN-class policy routes through messages (already RLS-scoped) — composition is transparent |
| files table FK requires file_versions row | Low | Low | Integration test inserts stub file only (no file_versions row required — FK is only files→groups) |

---
