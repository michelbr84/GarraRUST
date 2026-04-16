-- Migration 011: Partial unique index on group_invites for pending invites.
--
-- Prevents duplicate pending invites for the same (group_id, invited_email)
-- at the database level, eliminating TOCTOU race conditions in the
-- `POST /v1/groups/{id}/invites` handler (plan 0018, review fix).
--
-- The existing `group_invites_pending_email_idx` (migration 001) is a
-- non-unique partial index on `invited_email WHERE accepted_at IS NULL`.
-- This new index adds the uniqueness constraint on the compound key.

CREATE UNIQUE INDEX group_invites_pending_unique
    ON group_invites(group_id, invited_email)
    WHERE accepted_at IS NULL;

COMMENT ON INDEX group_invites_pending_unique IS 'Plan 0018: prevents duplicate pending invites for the same email in a group. Used by ON CONFLICT in create_invite handler.';
