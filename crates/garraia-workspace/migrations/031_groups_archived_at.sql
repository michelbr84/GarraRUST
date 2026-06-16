-- migration 031: add archived_at to groups table
--
-- Enables soft-deletion of groups via DELETE /v1/groups/{id}
-- (plan 0346 / GAR-890). Owner-only operation (Action::GroupDelete
-- in garraia-auth/src/can.rs).
--
-- Design: nullable timestamptz column (NULL = active, non-NULL = archived).
-- Forward-only: existing rows remain active. No backfill needed.
-- No NOT NULL constraint here — this is a soft-delete sentinel only.

ALTER TABLE groups
    ADD COLUMN archived_at timestamptz DEFAULT NULL;

COMMENT ON COLUMN groups.archived_at IS
    'Set by DELETE /v1/groups/{id} (Owner-only). NULL = active group. '
    'Non-NULL = archived; group is hidden from list/get endpoints. '
    'Hard delete deferred to Fase 5.3 retention worker.';
