-- 018_rls_groups_and_members.sql
-- GAR-589 — Add FORCE ROW LEVEL SECURITY to `groups` and `group_members`.
-- Plan:     plans/0106-gar-589-rls-groups-and-members.md
-- Depends:  migration 007 (garraia_app role + RLS on 10 tenant tables),
--           migration 001 (groups + group_members schema).
-- Forward-only. No DROP TABLE, no destructive ALTER.
--
-- ─── Motivation ──────────────────────────────────────────────────────────
--
-- Migration 007 intentionally excluded `groups` and `group_members` from
-- FORCE RLS (see migration 007 comment lines 15-25). Plan 0105 (GAR-580)
-- added `GET /v1/groups` which sets `app.current_user_id` "defensively",
-- and GAR-589 was filed to make that defensive hook load-bearing.
--
-- Without FORCE RLS, cross-group isolation relies entirely on the WHERE
-- clause in each handler (app-layer enforcement). With FORCE RLS, the
-- database enforces isolation at the garraia_app role level even if a
-- handler bug omits the WHERE clause (defense-in-depth).
--
-- ─── Policy design ───────────────────────────────────────────────────────
--
-- `groups` — membership-visible policy
--
--   USING:      visible iff the current user has an active `group_members` row.
--   WITH CHECK: for UPDATE — same as USING (group stays visible after edit).
--               for INSERT — same OR `created_by = current_user_id`.
--               The OR branch covers the window between INSERT INTO groups
--               and INSERT INTO group_members in create_group: the new group
--               has no `group_members` row yet, so the subquery returns 0 rows,
--               but `created_by = current_user_id` passes.
--
-- `group_members` — dual-context policy
--
--   Branch 1 (group-scoped endpoints): `group_id = app.current_group_id`
--     — covers list_members, set_member_role, delete_member.
--   Branch 2 (cross-group endpoints): `user_id = app.current_user_id`
--     — covers list_groups (user sees only their own membership rows)
--       and create_group (INSERT creator's own owner row).
--
--   No circular dependency: groups → group_members (one direction only).
--   group_members policy does NOT reference groups.
--
-- ─── Handler contract update ─────────────────────────────────────────────
--
-- These two handlers must be updated in the same PR (groups.rs):
--
--   get_group:    currently runs SELECT on `groups` WITHOUT a transaction or
--                 SET LOCAL — silently returns 404 with FORCE RLS.
--                 Fix: wrap in pool.begin() + set_config(user_id + group_id).
--
--   list_members: sets app.current_user_id but NOT app.current_group_id.
--                 Fix: add set_config('app.current_group_id', id, true).
--
-- All other group handlers already set the required context variables.
--
-- ─── Idempotency ─────────────────────────────────────────────────────────
--
-- DROP POLICY IF EXISTS is used before CREATE POLICY (same pattern as
-- migration 013). ENABLE/FORCE ROW LEVEL SECURITY are naturally idempotent
-- in Postgres (re-running is a no-op without error).

-- ═══════════════════════════════════════════════════════════════════════════
-- Part 1: groups
-- ═══════════════════════════════════════════════════════════════════════════

ALTER TABLE groups ENABLE ROW LEVEL SECURITY;
ALTER TABLE groups FORCE ROW LEVEL SECURITY;

-- Drop before re-create for idempotency (same pattern as migration 013).
DROP POLICY IF EXISTS groups_member_access ON groups;

-- Single PERMISSIVE FOR ALL policy.
-- USING  applies to SELECT, the old-row filter in UPDATE, and DELETE.
-- WITH CHECK applies to INSERT and the new-row filter in UPDATE.
CREATE POLICY groups_member_access ON groups
    AS PERMISSIVE
    FOR ALL
    USING (
        -- A group is visible to the current user iff they have an active
        -- membership row in group_members.
        -- Fail-closed: if app.current_user_id is not set, NULLIF returns NULL,
        -- the cast to uuid succeeds (NULL::uuid = NULL), and the subquery
        -- returns 0 rows → group is not visible.
        id IN (
            SELECT group_id
            FROM group_members
            WHERE user_id = NULLIF(current_setting('app.current_user_id', true), '')::uuid
              AND status = 'active'
        )
    )
    WITH CHECK (
        -- UPDATE: group_id is unchanged by patch_group → USING branch still holds
        --         after the update → same subquery passes.
        -- INSERT (create_group): the new group has no group_members row yet.
        --         Branch 1 (subquery) returns 0 rows. Branch 2 (created_by) passes
        --         because the handler binds principal.user_id = current_user_id.
        id IN (
            SELECT group_id
            FROM group_members
            WHERE user_id = NULLIF(current_setting('app.current_user_id', true), '')::uuid
              AND status = 'active'
        )
        OR
        created_by = NULLIF(current_setting('app.current_user_id', true), '')::uuid
    );

COMMENT ON POLICY groups_member_access ON groups IS
    'Class: membership. USING: group visible iff caller has active group_members row '
    '(app.current_user_id). WITH CHECK: same for UPDATE; for INSERT, also allows '
    'created_by = current_user_id (new group has no members row yet). '
    'Part of GAR-589 / migration 018. No circular dependency: group_members policy '
    'does not reference groups.';

-- ═══════════════════════════════════════════════════════════════════════════
-- Part 2: group_members
-- ═══════════════════════════════════════════════════════════════════════════

ALTER TABLE group_members ENABLE ROW LEVEL SECURITY;
ALTER TABLE group_members FORCE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS group_members_visible ON group_members;

-- Dual-context PERMISSIVE FOR ALL policy.
-- Branch 1 (group-scoped): list_members / set_member_role / delete_member
--   all set app.current_group_id → all members of the current group are visible.
-- Branch 2 (cross-group): list_groups / create_group
--   only app.current_user_id is set → only the caller's own membership rows are
--   visible, which is exactly what those handlers need.
CREATE POLICY group_members_visible ON group_members
    AS PERMISSIVE
    FOR ALL
    USING (
        -- Branch 1: group-scoped context (set_member_role, delete_member, list_members).
        group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid
        OR
        -- Branch 2: cross-group context (list_groups, create_group INSERT).
        user_id = NULLIF(current_setting('app.current_user_id', true), '')::uuid
    )
    WITH CHECK (
        -- UPDATE (set_member_role / delete_member soft-delete): group_id unchanged →
        --   Branch 1 still satisfies after the update.
        -- INSERT (create_group owner row): user_id = creator = current_user_id →
        --   Branch 2 satisfies.
        group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid
        OR
        user_id = NULLIF(current_setting('app.current_user_id', true), '')::uuid
    );

COMMENT ON POLICY group_members_visible ON group_members IS
    'Class: dual-context. Branch 1 (group-scoped): row visible when group_id = '
    'app.current_group_id — covers list_members, set_member_role, delete_member. '
    'Branch 2 (cross-group): row visible when user_id = app.current_user_id — '
    'covers list_groups (user sees own memberships) and create_group INSERT. '
    'No circular reference back to groups table. '
    'Part of GAR-589 / migration 018.';
