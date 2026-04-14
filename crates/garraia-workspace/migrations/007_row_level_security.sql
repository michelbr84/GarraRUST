-- 007_row_level_security.sql
-- GAR-408 — Migration 007: Row-Level Security em 10 tabelas tenant-scoped.
-- Plan:     plans/0007-gar-408-migration-007-rls.md
-- Depends:  migrations 001 (users/groups), 002 (audit_events), 004 (chats/messages),
--           005 (memory_items/memory_embeddings).
-- Forward-only. No DROP, no destructive ALTER.
--
-- ─── Scope decisions ──────────────────────────────────────────────────────
--
-- IN scope (10 tables with ENABLE + FORCE + CREATE POLICY):
--   messages, chats, chat_members, message_threads,
--   memory_items, memory_embeddings, audit_events,
--   sessions, api_keys, user_identities
--
-- OUT of scope (no RLS — app-layer enforcement only):
--   users               → a user needs to see members of their groups;
--                         recursive RLS is expensive. garraia-auth controls
--                         via SELECT projection with partial columns.
--   groups              → a user sees groups they are a member of; requires
--                         correlated subquery to group_members with its own
--                         RLS. App-layer join is cleaner in v1.
--   group_members       → same reasoning.
--   group_invites       → token-based access, handled by the invite endpoint.
--   roles, permissions, role_permissions → static lookup, public read.
--
-- ─── Contract ──────────────────────────────────────────────────────────────
-- Every request that touches RLS-protected tables must, at the start of the
-- transaction, execute:
--     SET LOCAL app.current_group_id = '<uuid>';
--     SET LOCAL app.current_user_id  = '<uuid>';
-- A connection pool (garraia-workspace::Workspace) MUST NOT reuse a cached
-- setting across transactions. garraia-auth extractor (GAR-391) is the
-- canonical caller.
--
-- Fail-closed: current_setting(..., true) returns '' (empty string) for a
-- custom GUC that has never been set in the current session, and NULL only
-- for entirely unknown parameters. Both cases must degrade to "not visible".
-- We wrap the call in NULLIF(..., '') so '' → NULL, then cast to uuid. NULL
-- = <uuid> yields NULL in the USING clause, which RLS treats as not-visible
-- → 0 rows. This is a feature, not a bug: a missing SET LOCAL causes queries
-- to silently return empty sets instead of leaking cross-tenant data or
-- raising a 22P02 invalid_text_representation error. The Axum extractor
-- (GAR-391) MUST return 500 if the Principal has no group_id instead of
-- trusting the empty result.
--
-- FORCE ROW LEVEL SECURITY is REQUIRED on every table — without it, the
-- table owner (typically the role running migrations, and commonly the
-- same role the app pool uses) bypasses the policy entirely. Benchmark
-- B6 in benches/database-poc/ proves this.

-- Role used by smoke tests to demote from superuser. Not used in production.
-- Production apps create a separate least-privilege role (documented in
-- crate README). IF NOT EXISTS because migrations run once per DB lifetime
-- and the role persists across application restarts in real deployments.
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'garraia_app') THEN
        CREATE ROLE garraia_app NOLOGIN;
    END IF;
END
$$;

-- Grant minimal perms to garraia_app so SELECT/write through RLS works in tests.
-- ⚠️ PRODUCTION NOTE: Production deployments MUST use a distinct role with
-- finer-grained grants. The broad INSERT/UPDATE/DELETE here is intentional
-- for test demote purposes; GAR-413 (garraia-cli migrate workspace) will
-- define the hardened production role pattern.
GRANT USAGE ON SCHEMA public TO garraia_app;
GRANT SELECT ON ALL TABLES IN SCHEMA public TO garraia_app;
GRANT INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO garraia_app;

-- Forward-compat: any table created by a FUTURE migration (e.g., GAR-387
-- files, GAR-390 tasks) will automatically receive the same DML grants
-- without needing to re-GRANT in each migration. This also applies to the
-- garraia_app role and ONLY to the public schema.
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO garraia_app;

-- ─── messages — direct policy ────────────────────────────────────────────
ALTER TABLE messages ENABLE ROW LEVEL SECURITY;
ALTER TABLE messages FORCE ROW LEVEL SECURITY;

CREATE POLICY messages_group_isolation ON messages
    USING (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid);

COMMENT ON POLICY messages_group_isolation ON messages IS
    'Class: direct. Context: app.current_group_id. Fail-closed when unset (NULL=NULL yields NULL → not visible). Uses denormalized group_id column to avoid JOIN overhead.';

-- ─── chats — direct policy ───────────────────────────────────────────────
ALTER TABLE chats ENABLE ROW LEVEL SECURITY;
ALTER TABLE chats FORCE ROW LEVEL SECURITY;

CREATE POLICY chats_group_isolation ON chats
    USING (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid);

COMMENT ON POLICY chats_group_isolation ON chats IS
    'Class: direct. Context: app.current_group_id. Fail-closed when unset. Direct filter on group_id column.';

-- ─── chat_members — JOIN policy via chats ────────────────────────────────
ALTER TABLE chat_members ENABLE ROW LEVEL SECURITY;
ALTER TABLE chat_members FORCE ROW LEVEL SECURITY;

CREATE POLICY chat_members_through_chats ON chat_members
    USING (
        chat_id IN (
            SELECT id FROM chats
            WHERE group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid
        )
    );

COMMENT ON POLICY chat_members_through_chats ON chat_members IS
    'Class: JOIN. Resolution: chat_id → chats.group_id → app.current_group_id. The subquery against chats is itself RLS-protected, so the composition is safe (chats RLS filters to current group first, then chat_members FK subquery uses that filtered set). Slight overhead vs direct policy; acceptable because chat_members is small (bounded by members per group).';

-- ─── message_threads — JOIN policy via chats ─────────────────────────────
ALTER TABLE message_threads ENABLE ROW LEVEL SECURITY;
ALTER TABLE message_threads FORCE ROW LEVEL SECURITY;

CREATE POLICY message_threads_through_chats ON message_threads
    USING (
        chat_id IN (
            SELECT id FROM chats
            WHERE group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid
        )
    );

COMMENT ON POLICY message_threads_through_chats ON message_threads IS
    'Class: JOIN. Resolution: chat_id → chats.group_id → app.current_group_id. Same pattern as chat_members_through_chats.';

-- ─── memory_items — dual policy ──────────────────────────────────────────
ALTER TABLE memory_items ENABLE ROW LEVEL SECURITY;
ALTER TABLE memory_items FORCE ROW LEVEL SECURITY;

CREATE POLICY memory_items_group_or_self ON memory_items
    USING (
        (group_id IS NOT NULL
         AND group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid)
        OR
        (group_id IS NULL
         AND created_by = NULLIF(current_setting('app.current_user_id', true), '')::uuid)
    );

COMMENT ON POLICY memory_items_group_or_self ON memory_items IS
    'Class: dual. Branch 1 (group): group_id IS NOT NULL AND group_id = app.current_group_id — covers scope_type=group and scope_type=chat. Branch 2 (user): group_id IS NULL AND created_by = app.current_user_id — covers scope_type=user personal memories. Both settings must be set for full coverage; missing either branch silently filters that branch away (fail-closed).';

-- ─── memory_embeddings — JOIN policy via memory_items ────────────────────
ALTER TABLE memory_embeddings ENABLE ROW LEVEL SECURITY;
ALTER TABLE memory_embeddings FORCE ROW LEVEL SECURITY;

CREATE POLICY memory_embeddings_through_items ON memory_embeddings
    USING (
        memory_item_id IN (SELECT id FROM memory_items)
    );

COMMENT ON POLICY memory_embeddings_through_items ON memory_embeddings IS
    'Class: JOIN (implicit recursive). The subquery against memory_items is itself RLS-protected, so it already returns only rows visible to the current scope (group + self branches). The FK subquery then filters embeddings to those visible items. This means ANN queries that go directly against memory_embeddings respect RLS automatically — but per the COMMENT on memory_embeddings, direct ANN queries are still discouraged until GAR-391 ships proper retrieval helpers.';

-- ─── audit_events — dual policy ──────────────────────────────────────────
ALTER TABLE audit_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE audit_events FORCE ROW LEVEL SECURITY;

CREATE POLICY audit_events_group_or_self ON audit_events
    USING (
        (group_id IS NOT NULL
         AND group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid)
        OR
        (group_id IS NULL
         AND actor_user_id = NULLIF(current_setting('app.current_user_id', true), '')::uuid)
    );

COMMENT ON POLICY audit_events_group_or_self ON audit_events IS
    'Class: dual. Branch 1 (group audit): events bound to a group visible when inside that group scope. Branch 2 (user audit): user-level events (login/logout/self-export) visible only to the actor themselves. NOTE: admin viewing another user audit trail is NOT covered by this policy — requires a BYPASSRLS role or a security-definer function, deferred to GAR-391 admin endpoints.';

-- ─── sessions — user policy ──────────────────────────────────────────────
ALTER TABLE sessions ENABLE ROW LEVEL SECURITY;
ALTER TABLE sessions FORCE ROW LEVEL SECURITY;

CREATE POLICY sessions_owner_only ON sessions
    USING (user_id = NULLIF(current_setting('app.current_user_id', true), '')::uuid);

COMMENT ON POLICY sessions_owner_only ON sessions IS
    'Class: user. A session row is visible only to its owning user. Fail-closed when app.current_user_id is unset.';

-- ─── api_keys — user policy ──────────────────────────────────────────────
ALTER TABLE api_keys ENABLE ROW LEVEL SECURITY;
ALTER TABLE api_keys FORCE ROW LEVEL SECURITY;

CREATE POLICY api_keys_owner_only ON api_keys
    USING (user_id = NULLIF(current_setting('app.current_user_id', true), '')::uuid);

COMMENT ON POLICY api_keys_owner_only ON api_keys IS
    'Class: user. Same as sessions_owner_only. Protects key_hash from cross-user reads.';

-- ─── user_identities — user policy ───────────────────────────────────────
ALTER TABLE user_identities ENABLE ROW LEVEL SECURITY;
ALTER TABLE user_identities FORCE ROW LEVEL SECURITY;

CREATE POLICY user_identities_owner_only ON user_identities
    USING (user_id = NULLIF(current_setting('app.current_user_id', true), '')::uuid);

COMMENT ON POLICY user_identities_owner_only ON user_identities IS
    'Class: user. Critical because this table holds password_hash. A user can only read their own identity records. Login flow in garraia-auth (GAR-391) must temporarily bypass RLS (via BYPASSRLS role or security-definer fn) to verify credentials, because at login time app.current_user_id is not yet known. Documented contract.';
