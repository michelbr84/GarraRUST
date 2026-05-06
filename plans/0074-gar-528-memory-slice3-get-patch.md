# Plan 0074 — GAR-528: REST /v1 memory slice 3 (GET /v1/memory/{id} + PATCH /v1/memory/{id})

**Status:** Em execução
**Autor:** Claude Sonnet 4.6 (garra-routine 2026-05-06, America/New_York)
**Data:** 2026-05-06 (America/New_York)
**Issue:** [GAR-528](https://linear.app/chatgpt25/issue/GAR-528)
**Branch:** `routine/202505061820-memory-slice3-get-patch`
**Epic:** `epic:ws-memory` / `epic:ws-api`

---

## §1 Goal

Land the third slice of the `/v1/memory` surface (ROADMAP §3.4 / §3.7), delivering
two endpoints that complete the single-item CRUD for `memory_items`:

- `GET /v1/memory/{id}` — fetch a single memory item with **full content** (the list
  endpoint, plan 0062, returns only a 200-char preview).
- `PATCH /v1/memory/{id}` — partial update of mutable fields: `content`, `kind`,
  `sensitivity`, `ttl_expires_at`. Immutable fields (`scope_type`, `scope_id`,
  `group_id`, `created_by`) are rejected if present via `deny_unknown_fields`.

## §2 Architecture

`memory_items` already lives under FORCE RLS (migration 007). Both handlers reuse the
`set_config` RLS pattern (plan 0056): `SET LOCAL app.current_user_id` AND
`app.current_group_id` inside every transaction. Cross-tenant rows are invisible to
the caller (RLS returns 0 rows → 404).

No new migration needed: all required columns (`content`, `kind`, `sensitivity`,
`ttl_expires_at`, `pinned_at`, `updated_at`) already exist from migrations 005 + 015.

`WorkspaceAuditAction::MemoryUpdated` ("memory.updated") is a new audit variant
added to `garraia-auth` in this slice. No schema change — only a new Rust enum
variant + match arm.

## §3 Tech stack

- Axum 0.8 + `RestV1FullState` (same as `memory.rs` slices 1 + 2)
- `sqlx::query_as` / `sqlx::query` — parameterized Postgres (no string concat)
- `garraia_auth::{Action, Principal, WorkspaceAuditAction, audit_workspace_event}`
- `utoipa` for OpenAPI path/schema registration
- No new dependencies

## §4 Design invariants

1. **`sensitivity='secret'` items are never returned or patchable.** GET and PATCH
   treat them as 404 (same security filter as list endpoint).
2. **`deleted_at IS NOT NULL` items return 404** (same as delete + pin).
3. **Immutable fields** (`scope_type`, `scope_id`, `group_id`, `created_by`,
   `source_chat_id`, `source_message_id`, `pinned_at`, `created_at`) are NOT
   accepted in the PATCH body. `#[serde(deny_unknown_fields)]` enforces this.
4. **TTL validation**: if `ttl_expires_at` is provided it must be in the future.
5. **At least one mutable field required** in a PATCH request; empty body → 400.
6. **PATCH on pinned items**: allowed — callers may restore `ttl_expires_at` after
   an unpin. The pin status itself is not modified by PATCH (only by pin/unpin).
7. **Audit metadata never contains content** (PII) — carries `content_len`, `kind`,
   `scope_type` only.
8. **RLS SET LOCAL both vars** on every tx, even for GET (RLS is tx-scoped).

## §5 Validações pré-plano

- [x] Migrations 005 + 015 supply all required columns.
- [x] `set_rls_context` helper already in `memory.rs` — reused.
- [x] `MemoryItemResponse` DTO already defined — reused for GET response.
- [x] `ALLOWED_KINDS`, `ALLOWED_SENSITIVITIES` constants already in `memory.rs`.
- [x] `WorkspaceAuditAction` enum in `garraia-auth/src/audit_workspace.rs` — adding
  `MemoryUpdated` variant requires no schema change.
- [x] Router slots available in `rest_v1/mod.rs` (no conflict with existing routes).

## §6 Out of scope

- Embedding update (memory embeddings are managed by background workers).
- Bulk PATCH.
- `sensitivity='secret'` items (never exposed via this API).
- `source_chat_id`, `source_message_id` updates (immutable after creation).
- `scope_type` / `scope_id` changes (would break RLS policies — requires delete+create).

## §7 Rollback

No migration in this slice. Rollback = revert the PR. `MemoryUpdated` enum variant
removal is a pure code revert. No data loss.

## §8 File structure

```
crates/garraia-auth/src/audit_workspace.rs   (+2 lines — new variant + match arm)
crates/garraia-gateway/src/rest_v1/memory.rs (+~200 LOC — 2 new handlers + 1 DTO)
crates/garraia-gateway/src/rest_v1/mod.rs    (+6 lines — 3 router slots × 2 branches)
crates/garraia-gateway/tests/rest_v1_memory_get_patch.rs  (new integration test file)
plans/0074-gar-528-memory-slice3-get-patch.md (this file)
plans/README.md                               (new row)
```

## §9 M1 tasks

- [ ] T1: Add `MemoryUpdated` to `WorkspaceAuditAction` in `garraia-auth`
- [ ] T2: Write integration test stubs (red) in `rest_v1_memory_get_patch.rs`
  - T2a: `get_memory_by_id_happy_path` — 200 with full content
  - T2b: `get_memory_by_id_not_found` — 404
  - T2c: `get_memory_by_id_cross_tenant` — 404 (Eve cannot see Alice's item)
  - T2d: `patch_memory_content` — 200 with updated content
  - T2e: `patch_memory_sensitivity` — 200 sensitivity change
  - T2f: `patch_memory_ttl` — 200 TTL set
  - T2g: `patch_memory_empty_body` — 400
  - T2h: `patch_memory_not_found` — 404
  - T2i: `patch_memory_cross_tenant` — 404
- [ ] T3: Implement `GET /v1/memory/{id}` handler + OpenAPI + router wire
- [ ] T4: Implement `PATCH /v1/memory/{id}` handler + `PatchMemoryRequest` DTO + OpenAPI + router wire
- [ ] T5: `cargo check -p garraia-gateway && cargo check -p garraia-auth`
- [ ] T6: `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings`
- [ ] T7: `cargo test -p garraia-gateway --test rest_v1_memory_get_patch` (green)
- [ ] T8: Update `plans/README.md` row + `ROADMAP.md` §3.4 `GET /v1/memory/{id}` + `PATCH /v1/memory/{id}` checkboxes

## §10 Risk register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| `MemoryUpdated` breaks existing match in `garraia-auth` | Low | Low | `cargo check -p garraia-auth` catches it before gateway compile |
| Partial UPDATE SQL with nullable fields | Low | Medium | Use explicit COALESCE with $N binding; test with null TTL path |
| RLS sees 0 rows on GET after item creation in same tx | Low | Low | Each handler opens its own tx; no ambiguity |

## §11 Acceptance criteria

1. `GET /v1/memory/{id}` returns full `MemoryItemResponse` (200) or 404.
2. `PATCH /v1/memory/{id}` updates the specified fields, returns updated `MemoryItemResponse` (200).
3. Empty PATCH body returns 400.
4. Cross-tenant GET/PATCH return 404 (Eve cannot access Alice's item).
5. `sensitivity='secret'` items return 404 from both GET and PATCH.
6. Deleted items return 404.
7. `cargo clippy --workspace … -- -D warnings` clean.
8. All integration tests green on Postgres real container.

## §12 Open questions

_None blocking._

## §13 Cross-references

- plan 0062 (GAR-514) — memory slice 1: list + create + delete
- plan 0072 (GAR-526) — memory slice 2: pin + unpin
- plan 0056 (GAR-508) — `set_config` parameterized RLS pattern
- `docs/adr/0005-identity-provider.md` — BYPASSRLS roles

## §14 Estimativa

~250 LOC (handlers + DTO + tests). 1-2h.
