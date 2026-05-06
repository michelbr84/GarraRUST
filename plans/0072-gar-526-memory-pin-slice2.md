# Plan 0072 — GAR-526: REST /v1 memory slice 2 (POST /v1/memory/{id}:pin + :unpin)

**Status:** Em execução
**Autor:** Claude Sonnet 4.6 (garra-routine 2026-05-06, America/New_York)
**Data:** 2026-05-06 (America/New_York)
**Issue:** [GAR-526](https://linear.app/chatgpt25/issue/GAR-526)
**Branch:** `routine/202605061620-memory-pin`
**Epic:** `epic:ws-memory` / `epic:ws-api`

---

## §1 Goal

Land the second slice of the `/v1/memory` surface (ROADMAP §3.4), delivering two
lifecycle endpoints on top of plan 0062 (GAR-514):

- `POST /v1/memory/{id}:pin` — marks a memory item as permanent (no TTL expiry)
- `POST /v1/memory/{id}:unpin` — removes the pin; item resumes TTL behavior

Pinned items never expire: the `pin` operation atomically sets `pinned_at = now()`
and nulls `ttl_expires_at`. Unpin sets `pinned_at = NULL` without restoring the
previous TTL — callers that want a TTL on an unpinned item must re-set it explicitly.

## §2 Architecture

`memory_items` already lives under FORCE RLS (migration 007). This slice adds one
nullable column (`pinned_at timestamptz`) via migration 015 — forward-only, no data loss.

Both handlers reuse the `set_config` RLS pattern (plan 0056): SET LOCAL both
`app.current_user_id` AND `app.current_group_id` in every tx. RLS automatically
filters cross-tenant rows to zero rows, so a cross-tenant pin attempt returns 404
exactly like a cross-tenant delete (plan 0062, invariant 6).

`memory_embeddings` is not touched by this slice.

## §3 Tech stack

- Axum 0.8 + `RestV1FullState` (same as `memory.rs` slice 1)
- `sqlx::query_as` — parameterized Postgres (no string concat)
- `garraia_auth::{Action, Principal, WorkspaceAuditAction, audit_workspace_event}`
- `utoipa` for OpenAPI path/schema registration

## §4 Design invariants

1. **Pin is idempotent**: pinning an already-pinned item is a no-op (returns 200 with
   current state). Avoids races without advisory locks.
2. **Pin nulls TTL atomically**: `SET pinned_at = now(), ttl_expires_at = NULL` in a
   single UPDATE. No intermediate state where item is pinned but still expires.
3. **Unpin does NOT restore TTL**: caller responsibility. Documents clearly in OpenAPI.
4. **Action::MemoryWrite** for both pin and unpin — no new permission variant needed.
5. **Cross-tenant → 404**: RLS filters the UPDATE … RETURNING to 0 rows → `NotFound`.
6. **Deleted items → 404**: `WHERE deleted_at IS NULL` guard on both UPDATE queries.
7. **Audit metadata is structural only**: `{ "kind": "...", "scope_type": "..." }`.
   No content, no PII.
8. SET LOCAL both `app.current_user_id` AND `app.current_group_id` in every tx.

## §5 Validações pré-plano

- [x] `memory_items` FORCE RLS migration 007 in main ✅
- [x] `set_config` parameterized RLS pattern proven (plan 0056) ✅
- [x] `Action::MemoryWrite` exists in `garraia_auth::Action` ✅
- [x] `WorkspaceAuditAction::MemoryCreated/MemoryDeleted` already in enum ✅
- [x] `RestV1FullState` + router wiring pattern understood ✅
- [x] No existing Linear issue for pin (confirmed search 2026-05-06) ✅

## §6 Out of scope

- Bulk pin/unpin endpoints.
- Filtering `GET /v1/memory` by `pinned=true` (slice 3 candidate).
- `sensitivity='secret'` pin — `secret` items not exposed via API.
- TTL restoration on unpin — explicit design decision; caller re-sets TTL.
- Embedding search interaction — embeddings are unchanged by pin state.

## §7 Rollback

Migration 015 is forward-only. If the PR must be reverted:
1. `git revert <merge-sha>` on main.
2. Run `ALTER TABLE memory_items DROP COLUMN IF EXISTS pinned_at CASCADE;`
   manually on the database (dev/staging only — in production use a new migration).

The new endpoints simply return 404 when the column doesn't exist (they'll fail to
compile, so they never reach production in that state).

## §12 Open questions

- Q1: Should unpin also accept a `DELETE /v1/memory/{id}:pin` style? Decision: NO —
  use two symmetrical POSTs (`:pin` / `:unpin`) matching the `:setRole` convention
  already established in `groups.rs`. Consistent with Google AIP-136 custom methods.

## §13 File structure

```
crates/
  garraia-workspace/migrations/
    015_memory_items_pinned_at.sql   ← NEW
  garraia-auth/src/
    audit_workspace.rs               ← add MemoryPinned, MemoryUnpinned variants
  garraia-gateway/src/rest_v1/
    memory.rs                        ← add pinned_at field to DTOs + pin/unpin handlers
    mod.rs                           ← register routes in all 3 modes
plans/
  0072-gar-526-memory-pin-slice2.md  ← this file
  README.md                          ← add row
```

## §14 M1 tasks

- [x] T1: Write plan 0072 + create Linear issue GAR-526
- [x] T2: Migration 015 (`015_memory_items_pinned_at.sql`)
- [x] T3: Add `MemoryPinned`/`MemoryUnpinned` to `WorkspaceAuditAction`
- [x] T4: Add `pinned_at` field to `MemoryItemResponse` + `MemoryItemSummary` + `MemoryListRow`
- [x] T5: Implement `pin_memory` handler
- [x] T6: Implement `unpin_memory` handler
- [x] T7: Register routes in `mod.rs` (all 3 modes)
- [x] T8: Update ROADMAP §3.4 + plans/README.md
- [ ] T9: CI green, PR merged

## §15 Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Migration 015 conflicts on concurrent branch | Low | migration name is unique |
| Axum route conflict `:pin` vs `:unpin` (same prefix) | Low | separate routes; Axum exact-match |
| `MemoryListRow` tuple ordering broken by adding `pinned_at` | Medium | update SELECT + type alias in one commit |

## §16 Acceptance criteria

- `POST /v1/memory/{id}:pin` returns 200 with `pinned_at != null` and `ttl_expires_at = null`.
- `POST /v1/memory/{id}:unpin` returns 200 with `pinned_at = null`.
- 404 on non-existent or deleted memory id.
- Cross-group: eve gets 404 trying to pin alice's memory (RLS filters).
- `cargo clippy --workspace --tests --exclude garraia-desktop -- -D warnings` clean.
- CI green ≥ 16 checks.

## §17 Cross-references

- Plan 0062 (GAR-514): slice 1 — GET/POST/DELETE /v1/memory
- Plan 0056 (GAR-508): parameterized set_config RLS pattern
- Migration 005: `memory_items` schema
- Migration 007: FORCE RLS on `memory_items`
- ROADMAP §3.4 Memory checklist

## §18 Estimativa

Baixa: 2 / 3 / 4 horas. Schema add + 2 handlers + route wiring.
