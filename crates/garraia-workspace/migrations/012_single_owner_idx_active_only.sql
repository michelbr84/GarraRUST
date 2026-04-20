-- Plan:     plans/0021-gar-425-workspace-security-hardening.md
-- Issue:    GAR-425
-- Depends:  migrations 001 (group_members), 002 (original partial UNIQUE index).
-- Forward-only. Idempotent via `IF EXISTS` / `IF NOT EXISTS` guards.
--
-- Goal: amend the partial UNIQUE index `group_members_single_owner_idx`
-- to also filter by `status = 'active'`. The original migration 002:146
-- created the index with `WHERE role = 'owner'` only, which left
-- soft-deleted owner rows (`role = 'owner', status = 'removed'`) still
-- occupying the single-owner slot.
--
-- Reality check:
--
-- The app-layer last-owner invariant in `set_member_role` and
-- `delete_member` (plan 0020) counts ACTIVE owners only:
--
--   SELECT COUNT(*)
--     FROM group_members
--    WHERE group_id = $1
--      AND role = 'owner'
--      AND status = 'active';
--
-- The DB-level constraint was strictly stricter than needed:
-- it forbade two `role = 'owner'` rows even when only one was
-- active. This created an artificial coupling in tests (D5 had
-- to hard-delete the soft-deleted row to rebuild the index)
-- and would become load-bearing if plan 0022+ ever enables
-- owner reactivation (`UPDATE status = 'active' WHERE
-- status = 'removed' AND role = 'owner'` would conflict with
-- an already-active owner of the same group).
--
-- Post-migration state:
--
-- The partial UNIQUE still enforces "at most one ACTIVE owner
-- per group" — identical to the app-layer invariant. A
-- soft-deleted owner is no longer counted. Re-inserting or
-- reactivating an owner in a group that had a prior soft-deleted
-- owner now succeeds at the DB layer. The app-layer guards
-- still prevent such reactivation via the API (setRole rejects
-- role='owner'), so production behavior is unchanged — only
-- the test-restore workaround (D5) becomes unnecessary.

-- Drop + recreate because Postgres has no `ALTER INDEX ... PREDICATE`.
DROP INDEX IF EXISTS group_members_single_owner_idx;

CREATE UNIQUE INDEX group_members_single_owner_idx
    ON group_members (group_id)
    WHERE role = 'owner' AND status = 'active';

COMMENT ON INDEX group_members_single_owner_idx IS
    'Partial UNIQUE — at most one ACTIVE owner per group. Plan 0021 '
    '(GAR-425) amended the predicate from WHERE role = owner to '
    'WHERE role = owner AND status = active so soft-deleted owner rows '
    'do not block reactivation or leave-group flows. Aligns the DB '
    'constraint with the app-layer last-owner invariant in '
    'set_member_role / delete_member (plan 0020).';
