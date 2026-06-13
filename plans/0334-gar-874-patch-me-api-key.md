# Plan 0334 — GAR-874: PATCH /v1/me/api-keys/{key_id}

**Issue:** [GAR-874](https://linear.app/chatgpt25/issue/GAR-874)
**Branch:** `routine/202606131845-patch-me-api-key`
**Date:** 2026-06-13 (America/New_York)

---

## Goal

Add `PATCH /v1/me/api-keys/{key_id}` to complete the CRUD for user API keys.
POST/GET/DELETE were delivered in GAR-871 (plan 0331). PATCH (rename label,
update scopes) was explicitly listed as "deferred" in plan 0331 §"Out of scope".

---

## Architecture

```
PATCH  /v1/me/api-keys/{key_id}  → 200 ApiKeySummary
```

- `PatchMyApiKeyRequest { label: Option<String>, scopes: Option<Vec<String>> }` — at least one field required.
- Returns 400 if both fields absent or validation fails.
- Returns 404 if key not found / cross-user (FORCE RLS `api_keys_owner_only`).
- Returns 409 if key is already revoked (cannot mutate a revoked key).
- Returns 200 + updated `ApiKeySummary` on success.
- Audit `WorkspaceAuditAction::ApiKeyUpdated` with `label_len` (PII-safe, no raw label).
- `app.current_group_id` = nil-uuid by convention (same as CREATE/LIST/DELETE).

---

## Tech stack

- Rust/Axum 0.8 handler in `crates/garraia-gateway/src/rest_v1/me.rs`
- `WorkspaceAuditAction::ApiKeyUpdated` added to `garraia-auth/src/audit_workspace.rs`
- Routes wired in all 3 `mod.rs` branches (full / auth-stub / no-auth stub)

---

## Design invariants

1. **At-least-one-field.** Both `label` and `scopes` absent → 400 "at least one field required".
2. **Revoked guard.** UPDATE includes `AND revoked_at IS NULL`; if 0 rows affected, a follow-up SELECT distinguishes "revoked" (409 Conflict) from "not found / cross-user" (404).
3. **Label validation.** 1–255 chars, trimmed.
4. **Scopes validation.** Non-empty strings only; empty string in array → 400.
5. **COALESCE pattern.** `label = COALESCE($1, label)`, `scopes = COALESCE($2, scopes)` — only provided fields change.
6. **PII-safe audit.** `label_len: label.len()` only — never the label text.
7. **No key_hash mutation.** The raw key is unchanged; only metadata mutates.

---

## Validações pré-plano

- [x] `api_keys.label` (text) + `api_keys.scopes` (jsonb) are mutable — no NOT NULL without DEFAULT
- [x] FORCE RLS `api_keys_owner_only` guards cross-user access (migration 007)
- [x] `ApiKeyCreated` + `ApiKeyRevoked` patterns established in plan 0331
- [x] Pattern: 409 Conflict for "already-revoked" similar to idempotent DELETE pattern

---

## Out of scope

- Scopes enforcement
- API key authentication with `gai_*` bearer token
- Rotating/regenerating the raw key value

---

## Rollback

Pure Rust handler + 1 audit variant. No schema migration. Drop the branch to revert.

---

## Tasks

- [x] T1: Add `WorkspaceAuditAction::ApiKeyUpdated` to `audit_workspace.rs` + test
- [x] T2: Add `PatchMyApiKeyRequest` struct + handler `patch_my_api_key` in `me.rs`
- [x] T3: Wire route in all 3 `mod.rs` branches
- [x] T4: Add 6 unit tests in `me.rs #[cfg(test)]`
- [x] T5: Register in `openapi.rs`
- [x] T6: Update ROADMAP + plans/README.md

---

## Acceptance criteria

- `PATCH /v1/me/api-keys/{id}` with `{"label":"renamed"}` returns 200 + updated label.
- `PATCH` with `{}` (empty body) returns 400.
- `PATCH` on a revoked key returns 409.
- `PATCH` on a non-existent or cross-user key returns 404.
- `cargo clippy --workspace` clean.
- 6 unit tests pass.

---

## Cross-references

- Plan 0331 (GAR-871) — POST/GET/DELETE api-keys
- Plan 0327 (GAR-866) — GET/DELETE /v1/me/sessions (pattern reference for nil-uuid RLS)
