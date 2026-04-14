-- 009_user_identities_hash_upgraded_at.sql
-- GAR-391b prereq — Adiciona user_identities.hash_upgraded_at para suportar
-- o lazy upgrade PBKDF2→Argon2id no verify_credential do crate garraia-auth.
--
-- Plan:     plans/0011.5-gar-391b-migration-009-hash-upgraded-at.md
-- ADR:      docs/adr/0005-identity-provider.md (§"InternalProvider implementation outline")
-- Depends:  migration 001 (user_identities table).
-- Forward-only. Strictly additive nullable column. No DROP, no destructive ALTER,
-- no backfill. NULL = "this row has never been rotated".
--
-- ─── Why a separate migration instead of folding into 391b ────────────────
-- User rule (2026-04-14): structural corrections discovered during Wave 0
-- reconnaissance must NOT be bundled with the feature implementation that
-- needs them. Migration 009 ships alone, then GAR-391b lands on top.
--
-- ─── Caller responsibility ────────────────────────────────────────────────
-- Only the lazy upgrade path in InternalProvider::verify_credential
-- (garraia-auth, GAR-391b) writes to this column. Production writes happen
-- inside the same transaction as the UPDATE password_hash, under a
-- SELECT ... FOR NO KEY UPDATE OF ui row lock. Any other writer is a bug.
--
-- ─── Lock cost ────────────────────────────────────────────────────────────
-- ALTER TABLE ... ADD COLUMN ... NULL (no DEFAULT) is a metadata-only
-- operation in Postgres 11+: it acquires an ACCESS EXCLUSIVE lock briefly
-- but does NOT rewrite the table. Safe for online deployment even on
-- production-sized user_identities.

ALTER TABLE user_identities
    ADD COLUMN IF NOT EXISTS hash_upgraded_at timestamptz NULL;

COMMENT ON COLUMN user_identities.hash_upgraded_at IS
    'Timestamp of the last password_hash algorithm rotation (e.g., PBKDF2 → Argon2id '
    'lazy upgrade). NULL means the hash has never been rotated since the row was '
    'inserted. Written ONLY by garraia-auth::InternalProvider::verify_credential '
    'inside the lazy upgrade transaction. See ADR 0005 §"InternalProvider implementation '
    'outline" and plan 0011 (GAR-391b).';
