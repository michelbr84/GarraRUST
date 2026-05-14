-- 019_chats_dm_pair.sql
-- GAR-604 — Add dm_user_a / dm_user_b pair columns + unique index to `chats`.
-- Plan:     plans/0115-gar-604-chats-dm-creation.md
-- Depends:  migration 004 (chats schema), migration 007 (FORCE RLS on chats).
-- Forward-only. No DROP TABLE, no destructive ALTER.
--
-- ─── Motivation ──────────────────────────────────────────────────────────
--
-- Plan 0054 deferred type='dm' to a future slice, noting "DM precisa de
-- 2 chat_members + UNIQUE constraint para evitar duplicado". This migration
-- delivers the schema prerequisite: a sorted-pair unique index that prevents
-- duplicate DMs at the DB level (race-condition safe), and a CHECK constraint
-- that enforces the sorted-pair invariant (dm_user_a < dm_user_b).
--
-- ─── Column semantics ────────────────────────────────────────────────────
--
-- dm_user_a: the lexicographically SMALLER of the two DM participants.
-- dm_user_b: the lexicographically LARGER  of the two DM participants.
--
-- The application layer normalizes the pair via Rust Uuid::cmp (Ord impl
-- on 128-bit integer). Both columns are NULL for type='channel' and
-- type='thread' rows (enforced by the CHECK constraint below).
--
-- ─── Uniqueness guarantee ─────────────────────────────────────────────────
--
-- A pair of users can have at most one DM per group. The partial unique index
-- `chats_dm_pair_unique` fires only on rows WHERE type = 'dm', so it does not
-- affect channel or thread rows. On conflict (SQLSTATE 23505) the application
-- returns the existing DM (200 OK) rather than an error.
--
-- ─── Idempotency ─────────────────────────────────────────────────────────
--
-- ADD COLUMN IF NOT EXISTS prevents duplicate-column errors on re-run.
-- CREATE INDEX uses IF NOT EXISTS for the same reason.
-- Note: ADD CONSTRAINT does NOT support IF NOT EXISTS in PostgreSQL;
-- this migration is applied exactly once by sqlx so no guard is needed.

ALTER TABLE chats
    ADD COLUMN IF NOT EXISTS dm_user_a uuid,
    ADD COLUMN IF NOT EXISTS dm_user_b uuid;

-- CHECK: for type='dm' both columns must be set and in sorted order (a < b
-- is enforced so the pair is canonical and the UNIQUE index fires correctly).
-- For any other type both columns must be NULL.
ALTER TABLE chats
    ADD CONSTRAINT chats_dm_users_check CHECK (
        (type = 'dm'
            AND dm_user_a IS NOT NULL
            AND dm_user_b IS NOT NULL
            AND dm_user_a <> dm_user_b
            AND dm_user_a < dm_user_b)
        OR
        (type <> 'dm'
            AND dm_user_a IS NULL
            AND dm_user_b IS NULL)
    );

-- Partial unique index: at most one DM per (group, sorted user pair).
CREATE UNIQUE INDEX IF NOT EXISTS chats_dm_pair_unique
    ON chats (group_id, dm_user_a, dm_user_b)
    WHERE type = 'dm';

COMMENT ON COLUMN chats.dm_user_a IS
    'DM-only: lexicographically smaller participant UUID. NULL for channel/thread. '
    'Normalized at API layer: LEAST(caller_id, partner_id). Part of GAR-604.';

COMMENT ON COLUMN chats.dm_user_b IS
    'DM-only: lexicographically larger participant UUID. NULL for channel/thread. '
    'Normalized at API layer: GREATEST(caller_id, partner_id). Part of GAR-604.';
