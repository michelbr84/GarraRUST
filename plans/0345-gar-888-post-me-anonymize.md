# Plan 0345 — `POST /v1/me/anonymize` — LGPD art. 12 / GDPR art. 4(5) personal data anonymization

**Issue:** [GAR-888](https://linear.app/chatgpt25/issue/GAR-888)
**Parent epic:** [GAR-400](https://linear.app/chatgpt25/issue/GAR-400) — Endpoints de export e delete (slice 3/3)
**Branch:** `routine/202606150615-anonymize-me`
**Estimativa:** ~350 LOC net-new

---

## Goal

Implement `POST /v1/me/anonymize` — LGPD art. 12 / GDPR art. 4(5) endpoint.

Under LGPD art. 12, anonymised data is not personal data. This endpoint replaces
PII fields with non-identifiable tokens, making the account's identity unrecoverable:

- `user_identities.login` → `anon-<8 hex chars>@garraanon.local`
- `users.display_name` → `'Usuário Anônimo'`
- `users.status` → `'anonymized'`
- Active sessions revoked.
- Audit event `account.anonymized` emitted.

Distinct from `DELETE /v1/me` (plan 0343): that tombstones the account; this
anonymises PII while keeping the account row for group history integrity.

---

## Architecture

### Schema change (migration 030)

`users.status` CHECK extended to include `'anonymized'`:

```sql
ALTER TABLE users DROP CONSTRAINT IF EXISTS users_status_check;
ALTER TABLE users ADD CONSTRAINT users_status_check
    CHECK (status IN ('active', 'suspended', 'deleted', 'anonymized'));
```

### Identity anonymisation — `garraia-auth/src/password.rs`

New `pub async fn anonymize_identity(login_pool: &LoginPool, user_id: Uuid)`:
- Runs via `LoginPool` (BYPASSRLS needed to UPDATE `user_identities`).
- Single UPDATE: `SET login = 'anon-<uuid8chars>@garraanon.local'`.
- Password hash left intact (irrelevant once `status = 'anonymized'` is set).

### Handler — `garraia-gateway/src/rest_v1/me.rs`

`pub async fn anonymize_me(State, Principal) -> Result<StatusCode, RestError>`:
1. `SET LOCAL` context (user_id + nil group_id) for RLS.
2. `SELECT status FROM users WHERE id = $1 FOR UPDATE` — guard against double calls.
3. 409 if `status IN ('deleted', 'anonymized')`.
4. `anonymize_identity(login_pool, user_id)` — best-effort, outside app_pool tx.
5. Atomic tx: UPDATE users + revoke sessions + `AccountAnonymized` audit event.
6. Return 204.

### Audit event

`WorkspaceAuditAction::AccountAnonymized` → `"account.anonymized"`.
Metadata: `{}` — no PII.

---

## Design invariants

- **NO PII in audit metadata** — display_name and login are NEVER in the JSON.
- **Idempotency guard** — 409 if already anonymized or deleted.
- **BYPASSRLS for login** — only LoginPool touches `user_identities`.
- **Migration forward-only** — new CHECK value; no data rewrite needed.
- **Irreversible by design** — LGPD art. 12 requires permanent anonymisation.

---

## Out of scope

- Hard delete of data (Fase 5.3 retention worker).
- Group message content (sender_user_id persists; display_name shown as 'Usuário Anônimo').
- Re-activation of anonymized accounts.
- Cross-group message/file content anonymization (slice 3 of GAR-400 scope limit).

---

## Tasks

- [x] T1 — Migration 030: `users.status` CHECK extended
- [x] T2 — `anonymize_identity` in `garraia-auth/src/password.rs` + re-export
- [x] T3 — `AccountAnonymized` variant + `as_str()` + exhaustiveness test
- [x] T4 — `anonymize_me` handler in `me.rs` + 7 unit tests
- [x] T5 — Wire in `mod.rs` (3 routing branches)
- [x] T6 — Register in `openapi.rs`
- [x] T7 — Plan doc + `plans/README.md` update
- [ ] T8 — PR + green CI + merge + ROADMAP check

---

## Acceptance criteria

- `POST /v1/me/anonymize` → 204 for active user.
- 409 if already anonymized; 409 if already deleted.
- `user_identities.login` replaced with `anon-<8hex>@garraanon.local`.
- `users.display_name = 'Usuário Anônimo'`, `users.status = 'anonymized'`.
- Active sessions revoked.
- `account.anonymized` audit event emitted, metadata `{}`.
- Cargo check clean, clippy clean, 7 new unit tests green.

---

## Cross-references

- Parent: GAR-400 (Endpoints de export e delete)
- Slice 1: GAR-884 / plan 0343 — `DELETE /v1/me`
- Slice 2: GAR-885 / plan 0344 — `GET /v1/me/export`
- Migration: `crates/garraia-workspace/migrations/030_users_anonymized_status.sql`
- LGPD: art. 12 (dados anonimizados), art. 18 (direitos do titular)
- GDPR: art. 4(5) (anonymisation), art. 17 (right to erasure)
