-- Migration 025: Add declined_at + declined_by to group_invites; update partial unique index.
--
-- An invitee-declined invite is one the recipient explicitly rejected via
-- POST /v1/me/invites/{invite_id}/decline (plan 0258 / GAR-783).
-- Both columns are nullable; NULL means not declined.
--
-- The partial unique index from migration 024 excluded accepted and revoked rows.
-- Declined rows (accepted_at IS NULL, revoked_at IS NULL, declined_at IS NOT NULL)
-- would still participate in the index, blocking re-invite after decline.
-- This migration drops and recreates it to also exclude declined rows,
-- enabling re-invite after decline without a 409 conflict.
--
-- Plan 0258 (GAR-783) — Fase 3.4 invite decline.

ALTER TABLE group_invites
    ADD COLUMN declined_at timestamptz,
    ADD COLUMN declined_by uuid REFERENCES users(id);

COMMENT ON COLUMN group_invites.declined_at IS 'Set to now() when the invitee explicitly declines the invite. NULL = not declined.';
COMMENT ON COLUMN group_invites.declined_by IS 'User who performed the decline (the invitee). NULL = not declined.';

-- Recreate the pending-unique index to also exclude declined rows,
-- enabling re-invite after decline without a 409 collision.
DROP INDEX group_invites_pending_unique;
CREATE UNIQUE INDEX group_invites_pending_unique
    ON group_invites(group_id, invited_email)
    WHERE accepted_at IS NULL AND revoked_at IS NULL AND declined_at IS NULL;

COMMENT ON INDEX group_invites_pending_unique IS 'Plan 0258 (amends plan 0257 / migration 024): prevents duplicate pending invites for the same email in a group. Excludes accepted, revoked, and declined rows so re-invite is allowed after any terminal action.';
