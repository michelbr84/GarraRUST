-- 010_signup_role_and_session_select.sql
-- GAR-391c — Signup pool role + session SELECT grant for refresh flow.
-- Plan:     plans/0012-gar-391c-extractor-and-wiring.md
-- ADR:      docs/adr/0005-identity-provider.md (will receive amendment in 391c)
-- Depends:  migrations 001 (users, user_identities, sessions, audit_events),
--           002 (audit_events shape), 008 (garraia_login role).
-- Forward-only. No DROP, no destructive ALTER.
--
-- Two purposes in one migration:
-- (a) Create dedicated `garraia_signup` BYPASSRLS role for the signup flow.
--     The login role MUST NOT be reused: signup needs INSERT on user_identities
--     and the login role's whole point is minimal credential-verification scope.
-- (b) Add `GRANT SELECT ON sessions TO garraia_login` to fix Gap A discovered
--     during GAR-391b (INSERT ... RETURNING id and verify_refresh both need
--     SELECT on the returned/queried columns).
--
-- ─── Threat model (signup role) ─────────────────────────────────────────
-- COMPROMISE OF garraia_signup = ability to create arbitrary identities.
-- Less critical than login pool (which exposes existing credentials) but
-- still a tenant-onboarding attack vector. Mitigation:
-- - Network isolation (separate Unix socket / firewall rule).
-- - Distinct vault entry (GAR-410).
-- - Rate limiting on signup endpoint (deferred to 391c follow-up).
-- - pgaudit on user_identities INSERT.

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'garraia_signup') THEN
        CREATE ROLE garraia_signup NOLOGIN BYPASSRLS;
    END IF;
END
$$;

GRANT USAGE ON SCHEMA public TO garraia_signup;

-- Read users to detect duplicate-email + RETURNING id on INSERT.
GRANT SELECT, INSERT ON users TO garraia_signup;

-- Read user_identities to detect duplicate-provider_sub + RETURNING id on INSERT.
GRANT SELECT, INSERT ON user_identities TO garraia_signup;

-- Insert audit_events to log every signup attempt (success and failure).
GRANT INSERT ON audit_events TO garraia_signup;

-- ─── Login role: add SELECT on sessions (closes 391b Gap A) ───────────
-- INSERT ... RETURNING id requires SELECT on the returned columns.
-- verify_refresh requires SELECT on refresh_token_hash for lookup.
-- Both operations are part of the login flow (issue + verify), so the
-- grant is consistent with the login role's purpose.
GRANT SELECT ON sessions TO garraia_login;

-- ─── Login role: add SELECT on group_members (closes 391c Gap C) ──────
-- The garraia-auth Principal extractor (GAR-391c) resolves the active
-- group via `SELECT role FROM group_members WHERE group_id = $1
-- AND user_id = $2 AND status = 'active'` to typecheck membership and
-- populate Principal.role. This was discovered empirically during 391c
-- Wave 1.5: the extractor test `non_member_group_returns_403` was
-- returning 401 because the query failed with "permission denied for
-- table group_members" before the row could be checked.
--
-- group_members is NOT under RLS (migration 007 §scope: recursive RLS
-- is expensive, app-layer enforced via JOIN with the membership query
-- above), so granting SELECT here does not bypass any tenant isolation.
-- The grant is consistent with the login role's "resolve who you are"
-- purpose. Plan 0012 amendment §"Gap C correction" documents the
-- discovery + the user's Option 1 approval (2026-04-14).
GRANT SELECT ON group_members TO garraia_login;

COMMENT ON ROLE garraia_signup IS
    'BYPASSRLS dedicated role for the garraia-auth SIGNUP flow. NOLOGIN by '
    'default — production deployments must promote via ALTER ROLE WITH LOGIN '
    'PASSWORD. Compromise = ability to create arbitrary identities. NOT a '
    'substitute for garraia_login: this role has INSERT on users/user_identities '
    'but no read access to sessions or any tenant data. See ADR 0005 amendment '
    '(GAR-391c) and migration 010 comment block.';
