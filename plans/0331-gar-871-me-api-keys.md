# Plan 0331 — GAR-871: POST/GET /v1/me/api-keys + DELETE /v1/me/api-keys/{key_id}

**Issue:** [GAR-871](https://linear.app/chatgpt25/issue/GAR-871)
**Branch:** `routine/202606131222-me-api-keys`
**Date:** 2026-06-13 (America/New_York)

---

## Goal

Add user-facing API key management to the REST API. The `api_keys` table (migration 001)
and FORCE RLS policy `api_keys_owner_only` (migration 007) already exist; this plan
wires the four user-facing endpoints to complete the CRUD surface.

---

## Architecture

The `api_keys` table is user-scoped (not group-scoped). RLS uses only
`app.current_user_id`; `app.current_group_id` is set to nil-uuid by convention
(same as sessions, doc_mentions, etc.). The raw key is returned **once only**
on creation and is never recoverable afterwards — only the Argon2id hash is stored.

```
POST   /v1/me/api-keys              → 201 CreateApiKeyResponse
GET    /v1/me/api-keys              → 200 MyApiKeysResponse (cursor-paginated)
GET    /v1/me/api-keys/{key_id}     → 200 ApiKeySummary
DELETE /v1/me/api-keys/{key_id}     → 204 (idempotent)
```

---

## Tech stack

- Rust/Axum 0.8 handler in `crates/garraia-gateway/src/rest_v1/me.rs`
- `garraia_auth::hash_argon2id` for Argon2id hashing (consistent with `api_keys.key_hash` column comment)
- `password_hash::rand_core::{OsRng, RngCore}` for key generation (already in Cargo.toml via `password-hash { features = ["getrandom"] }`)
- `base64::engine::general_purpose::URL_SAFE_NO_PAD` for encoding
- Audit via `WorkspaceAuditAction::ApiKeyCreated` + `ApiKeyRevoked`
- Routes wired in all 3 `mod.rs` branches (full / auth-stub / no-auth stub)

---

## Design invariants

1. **Raw key returned only once.** `CreateApiKeyResponse.key` is present only in POST 201. All other responses use `ApiKeySummary` which has no `key` field.
2. **`key_hash` never in responses.** The column is SELECT-excluded by not querying it.
3. **RLS via user_id only.** `SET LOCAL app.current_user_id = {caller_id}` before every query. `app.current_group_id` set to nil-uuid (RLS policy `api_keys_owner_only` doesn't check group_id, but nil-uuid keeps the pattern consistent and satisfies the audit_events `WITH CHECK`).
4. **Soft revoke.** `DELETE` sets `revoked_at = now()`; the row is never deleted. `GET /v1/me/api-keys` returns all keys (active + revoked) so the user can see history.
5. **Idempotent revoke.** If `revoked_at IS NOT NULL`, return 204 (already done). If row not found / cross-user (RLS), return 404.
6. **Label validation.** Label must be 1–255 chars. Scopes must be a JSON array of non-empty strings; default `[]`.
7. **Key format.** `gai_` prefix + 32 random bytes as URL-safe base64-no-pad → e.g. `gai_YWJjZGVmZ2hpamtsbW5vcHFyc3R1dndh`.

---

## Validações pré-plano

- [x] `api_keys` table exists with FORCE RLS `api_keys_owner_only` (migration 001 + 007)
- [x] `garraia_auth::hash_argon2id` exported from `garraia-auth/src/lib.rs`
- [x] `password-hash` with `getrandom` feature in `garraia-gateway/Cargo.toml`
- [x] `base64` in workspace deps, used by gateway
- [x] `WorkspaceAuditAction` enum in `garraia-auth/src/audit_workspace.rs`
- [x] Pattern established by `list_my_sessions` / `revoke_my_session` (plan 0327)

---

## Out of scope

- API key **authentication** (using a `gai_*` key to authenticate a request) — separate slice
- Scopes enforcement — keys are created with scopes array but enforcement is future work
- Rate-limiting per API key
- `PATCH /v1/me/api-keys/{key_id}` (label/scopes update) — deferred

---

## Rollback

Pure Rust handler addition + new audit variants. No schema migrations. Drop the branch to revert.

---

## File structure

```
crates/garraia-auth/src/audit_workspace.rs   ← add ApiKeyCreated + ApiKeyRevoked variants + as_str() + can_be_emitted_by_user_scope()
crates/garraia-gateway/src/rest_v1/me.rs     ← add 4 handler functions + request/response types
crates/garraia-gateway/src/rest_v1/mod.rs    ← wire routes in all 3 branches
crates/garraia-gateway/src/rest_v1/openapi.rs ← register paths + schemas
plans/0331-gar-871-me-api-keys.md            ← this file
plans/README.md                              ← add row 0331
```

---

## M1 tasks

- [ ] T1: Add `ApiKeyCreated` + `ApiKeyRevoked` to `WorkspaceAuditAction`
- [ ] T2: Implement `create_my_api_key` handler (POST)
- [ ] T3: Implement `list_my_api_keys` handler (GET list, cursor-paginated)
- [ ] T4: Implement `get_my_api_key` handler (GET single)
- [ ] T5: Implement `revoke_my_api_key` handler (DELETE)
- [ ] T6: Wire routes + OpenAPI registration
- [ ] T7: Unit tests (≥6)
- [ ] T8: Update ROADMAP + plans/README.md

---

## Risk register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Argon2id slow in tests | Low | Low | Tests only exercise response shape, not real hashing |
| Key collision (same raw token generated twice) | Negligible | Medium | 32 bytes = 2^256 collision resistance; UNIQUE constraint on `key_hash` provides DB-level guard |

---

## Acceptance criteria

1. `POST /v1/me/api-keys` → 201 with `{ id, label, scopes, created_at, key: "gai_..." }`
2. `GET /v1/me/api-keys` → 200 paginated list without `key` field
3. `GET /v1/me/api-keys/{key_id}` → 200 single item without `key` field
4. `DELETE /v1/me/api-keys/{key_id}` → 204 (idempotent)
5. `cargo clippy --workspace --tests --exclude garraia-desktop --no-deps -- -D warnings` green
6. ≥6 unit tests pass

---

## Cross-references

- Migration 001 (`api_keys` table) + migration 007 (FORCE RLS)
- Plan 0327 (GAR-866) — sessions list/revoke pattern (reference implementation)
- `garraia-auth::hash_argon2id` — Argon2id hasher
- GAR-871 Linear issue

---

## Estimativa

- Otimista: 1h
- Realista: 1.5h
- Pessimista: 2h
