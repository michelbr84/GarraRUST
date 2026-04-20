-- Plan:     plans/0021-gar-425-workspace-security-hardening.md
-- Issue:    GAR-425
-- Depends:  migration 007 (audit_events_group_or_self policy).
-- Forward-only. Idempotent via DROP + CREATE.
--
-- Security review follow-up F-01: make the `WITH CHECK` clause
-- explicit on the `audit_events_group_or_self` policy.
--
-- The Postgres docs (§5.8.3) specify that for a PERMISSIVE policy
-- without an explicit `WITH CHECK`, the `USING` expression is used
-- as the implicit WITH CHECK. Migration 007 relied on this implicit
-- behavior. Plan 0021 security auditor flagged this as a design
-- fragility: if someone later converts the policy to `AS RESTRICTIVE`,
-- the implicit WITH CHECK becomes TRUE (permissive total),
-- silently removing the write guard.
--
-- This migration drops and re-creates the policy with an explicit
-- `WITH CHECK` clause identical to the `USING` — so future
-- alterations must touch both clauses deliberately, making the
-- regression-path obvious.
--
-- Behavior unchanged: same predicate on both sides means INSERT/
-- UPDATE/DELETE are authorized iff SELECT would see the row,
-- which matches the previous implicit semantics.

DROP POLICY IF EXISTS audit_events_group_or_self ON audit_events;

CREATE POLICY audit_events_group_or_self ON audit_events
    AS PERMISSIVE
    FOR ALL
    USING (
        (group_id IS NOT NULL
         AND group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid)
        OR
        (group_id IS NULL
         AND actor_user_id = NULLIF(current_setting('app.current_user_id', true), '')::uuid)
    )
    WITH CHECK (
        (group_id IS NOT NULL
         AND group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid)
        OR
        (group_id IS NULL
         AND actor_user_id = NULLIF(current_setting('app.current_user_id', true), '')::uuid)
    );

COMMENT ON POLICY audit_events_group_or_self ON audit_events IS
    'Class: dual. Branch 1 (group audit): events bound to a group visible when inside that group scope. Branch 2 (user audit): user-level events (login/logout/self-export) visible only to the actor themselves. NOTE: admin viewing another user audit trail is NOT covered by this policy — requires a BYPASSRLS role or a security-definer function, deferred to GAR-391 admin endpoints. Plan 0021 migration 013 made WITH CHECK explicit (previously implicit per Postgres §5.8.3) to defend against accidental downgrade if the policy is later converted to RESTRICTIVE.';
