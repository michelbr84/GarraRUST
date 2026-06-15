-- migration 030: add 'anonymized' to users.status CHECK constraint
--
-- LGPD art. 12 / GDPR art. 4(5): anonymised data is no longer personal data.
-- POST /v1/me/anonymize (plan 0345 / GAR-888) sets status = 'anonymized' to
-- signal that PII fields have been replaced with non-identifiable tokens.
--
-- Forward-only: existing rows all have status IN ('active','suspended','deleted')
-- so the new constraint validates without table rewrites.
--
-- Approach: drop + re-add named CHECK constraint (Postgres does not support
-- ALTER CHECK ... USING directly). This acquires an AccessExclusiveLock on
-- `users` for a brief period — acceptable for a one-time schema migration.

ALTER TABLE users
    DROP CONSTRAINT IF EXISTS users_status_check;

ALTER TABLE users
    ADD CONSTRAINT users_status_check
        CHECK (status IN ('active', 'suspended', 'deleted', 'anonymized'));

COMMENT ON COLUMN users.status IS
    'active → normal; suspended → blocked login; deleted → tombstone pending hard delete; anonymized → PII replaced, account irreversibly anonymised (LGPD art. 12 / GDPR art. 4(5))';
