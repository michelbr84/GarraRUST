-- Migration 024: Add revoked_at + revoked_by to group_invites; update partial unique index.
--
-- A revoked invite is one cancelled by an owner/admin before the invitee
-- acted on it. Both columns are nullable; NULL means not revoked.
--
-- The partial unique index from migration 011 only excluded accepted rows.
-- Revoked rows (accepted_at IS NULL, revoked_at IS NOT NULL) would still
-- participate in the index, blocking re-invite after revocation.
-- This migration drops and recreates it to also exclude revoked rows,
-- enabling re-invite after revocation without a 409 conflict.
--
-- Plan 0257 (GAR-780) — Fase 3.4 invite revocation.

ALTER TABLE group_invites
    ADD COLUMN revoked_at timestamptz,
    ADD COLUMN revoked_by uuid REFERENCES users(id);

COMMENT ON COLUMN group_invites.revoked_at IS 'Set to now() when an owner/admin revokes the pending invite. NULL = not revoked.';
COMMENT ON COLUMN group_invites.revoked_by IS 'User who performed the revocation. NULL = not revoked.';

-- Recreate the pending-unique index to also exclude revoked rows,
-- enabling re-invite after revocation without a 409 collision.
DROP INDEX group_invites_pending_unique;
CREATE UNIQUE INDEX group_invites_pending_unique
    ON group_invites(group_id, invited_email)
    WHERE accepted_at IS NULL AND revoked_at IS NULL;

COMMENT ON INDEX group_invites_pending_unique IS 'Plan 0257 (amends plan 0018 / migration 011): prevents duplicate pending invites for the same email in a group. Excludes accepted and revoked rows so re-invite is allowed after revocation.';
