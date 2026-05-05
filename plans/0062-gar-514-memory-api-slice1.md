# Plan 0062 — GAR-514: REST /v1 Memory slice 1 (GET + POST + DELETE /v1/memory)

**Status:** Em execução
**Autor:** Claude Sonnet 4.6 (garra-routine 2026-05-05, America/New_York)
**Data:** 2026-05-05 (America/New_York)
**Issue:** [GAR-514](https://linear.app/chatgpt25/issue/GAR-514)
**Branch:** `routine/202605050815-memory-api`
**Epic:** `epic:ws-memory` / `epic:ws-api`

---

## §1 Goal

Land the first slice of the `/v1/memory` surface (ROADMAP §3.7), delivering three
endpoints on the `garraia_app` RLS-enforced pool:

- `GET /v1/memory?scope_type=…&scope_id=…&cursor=…&limit=…` — cursor-paginated list
- `POST /v1/memory` — create memory item
- `DELETE /v1/memory/{id}` — soft-delete

## §2 Architecture

`memory_items` lives under FORCE RLS (migration 007) with a **dual policy**
(`memory_items_group_or_self`): branch 1 covers `scope_type ∈ {group, chat}` via
`group_id = current_setting('app.current_group_id')`; branch 2 covers
`scope_type = user` (personal) via `created_by = current_setting('app.current_user_id')
AND group_id IS NULL`.

Both SET LOCAL calls must be issued in every tx, regardless of scope — the same invariant
as all other FORCE-RLS handlers (plan 0056 `set_config` pattern).

`memory_embeddings` is filtered implicitly by a JOIN policy through `memory_items`. It is
not touched by this slice (no embedding operations).

## §3 Tech stack

- Axum 0.8 + `RestV1FullState` (same as `chats.rs`, `messages.rs`)
- `sqlx::query!` / `sqlx::query_as!` — parameterized Postgres
- `garraia_auth::{Action, Principal, WorkspaceAuditAction, audit_workspace_event}`
- `utoipa` for OpenAPI path/schema registration

## §4 Design invariants

1. Every SELECT includes `AND deleted_at IS NULL AND sensitivity <> 'secret'
   AND (ttl_expires_at IS NULL OR ttl_expires_at > now())`.
2. SET LOCAL both `app.current_user_id` AND `app.current_group_id` in every tx.
3. Scope validation (app-layer) on top of RLS:
   - `scope_type='user'` → `scope_id` must equal `principal.user_id`.
   - `scope_type='group'` → `scope_id` must equal `principal.group_id`.
   - `scope_type='chat'` → verify `chats.group_id = principal.group_id` in-tx.
4. `group_id` stored in `memory_items`:
   - `scope_type='group'` → `group_id = scope_id` (= principal.group_id).
   - `scope_type='chat'` → `group_id = principal.group_id`.
   - `scope_type='user'` → `group_id = NULL`.
5. Audit metadata is STRUCTURAL only — no content text (PII). Store `content_len`, `kind`, `scope_type`.
6. Cross-tenant: 404 (not 403) for unknown/other-tenant items in DELETE — same pattern as chats/messages.

## §5 Validações pré-plano

- [x] `memory_items` FORCE RLS migration 007 in main ✅
- [x] `Action::MemoryRead`, `Action::MemoryWrite`, `Action::MemoryDelete` in `action.rs` ✅
- [x] `set_config` parameterized SQL pattern established by plan 0056 ✅
- [x] `garraia_app` AppPool newtype available via `state.app_pool` ✅
- [x] No `pinned` column exists → pin endpoint is out of scope for this plan ✅
- [x] GAR-514 created in Linear ✅

## §6 Scope

**In scope:**
- `GET /v1/memory` — cursor-paginated list with scope filtering
- `POST /v1/memory` — create
- `DELETE /v1/memory/{id}` — soft-delete

**Out of scope:**
- `POST /v1/memory/{id}:pin` — no schema column; plan 0063+
- Embedding creation / ANN search (Fase 2.1, GAR-372)
- TTL expiration worker (Fase 2.1)
- `GET /v1/memory/{id}` — single item fetch (plan 0063)

## §7 Affected files

```
crates/garraia-auth/src/audit_workspace.rs         (+ 2 variants + 2 unit tests)
crates/garraia-gateway/src/rest_v1/memory.rs       (new, ~280 LOC)
crates/garraia-gateway/src/rest_v1/mod.rs          (route wiring, 3 modes)
crates/garraia-gateway/src/rest_v1/openapi.rs      (2 paths + 4 schemas)
crates/garraia-gateway/Cargo.toml                  (+ [[test]] rest_v1_memory)
crates/garraia-gateway/tests/rest_v1_memory.rs     (new, ~260 LOC)
plans/README.md                                    (+ row 0062)
```

## §8 Rollback plan

Revert the branch. No schema migration, no data mutation. Fully reversible.

## §9 Risk register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Dual RLS policy missed (SET LOCAL omitted) | Low | High | Protocol enforced by explicit set_config pattern; integration tests verify cross-group isolation |
| `sensitivity='secret'` leaks in list | Low | Critical | `AND sensitivity <> 'secret'` in every SELECT; integration test asserts this |
| scope_id mismatch not caught | Low | High | App-layer validation before INSERT; tests cover wrong scope_id |
| content PII in audit metadata | Low | Medium | Audit carries `content_len` only; snapshot test validates no content |

## §10 Acceptance criteria

- `GET /v1/memory?scope_type=group&scope_id=<group_uuid>` returns 200 with list
- `POST /v1/memory` returns 201 with the created item
- `DELETE /v1/memory/{id}` returns 204; item no longer returned in list
- Cross-group DELETE returns 404
- `sensitivity='secret'` items not returned in list even when caller owns them
- TTL-expired items not returned
- `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` passes
- All CI checks pass

## §11 Cross-references

- ROADMAP §3.7 — Memória IA compartilhada
- Plan 0054 (chats slice 1), 0055 (messages slice 2), 0058 (threads slice 3) — precedent
- Plan 0056 — `set_config` parameterized SQL pattern (RLS injection fix)
- Migration 005 (`memory_items` + `memory_embeddings`)
- Migration 007 (`memory_items_group_or_self` RLS)
- `Action::MemoryRead/Write/Delete` in `crates/garraia-auth/src/action.rs`

## §12 Open questions

None — schema and RLS already shipped; pattern established by plan 0056.

## §13 Estimativa

- T1: Audit variants: 15 min
- T2-T5: memory.rs handlers (~280 LOC): 45 min
- T6-T7: Routing + OpenAPI: 20 min
- T8: Integration tests (~260 LOC): 40 min
- CI + review follow-ups: 30 min
- **Total: ~2.5 hours**

## M1 Tasks

- [x] T1: Add `MemoryCreated` + `MemoryDeleted` to `WorkspaceAuditAction` + unit tests
- [x] T2: DTOs: `CreateMemoryRequest` + `MemoryItemResponse` + `MemoryItemSummary` + `ListMemoryResponse` + `ListMemoryQuery`
- [x] T3: `GET /v1/memory` handler — scope-filtered cursor-paginated list
- [x] T4: `POST /v1/memory` handler — create + scope validation + audit
- [x] T5: `DELETE /v1/memory/{id}` handler — soft-delete + audit
- [x] T6: Route wiring in `mod.rs` (mode 1 real + mode 2 fail-soft + mode 3 stub)
- [x] T7: OpenAPI paths + schemas in `openapi.rs`
- [x] T8: Integration tests `rest_v1_memory.rs` (12 scenarios, all green CI)
