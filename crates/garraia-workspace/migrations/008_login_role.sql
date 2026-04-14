-- 008_login_role.sql
-- GAR-391a — Cria garraia_login NOLOGIN BYPASSRLS dedicated role para o
-- login flow do crate garraia-auth. Resolve o hard blocker arquitetural
-- documentado em GAR-408 e formalizado em ADR 0005 (GAR-375).
--
-- Plan:     plans/0010-gar-391a-garraia-auth-crate-skeleton.md
-- ADR:      docs/adr/0005-identity-provider.md (§"Login role specification")
-- Depends:  migrations 001 (users/sessions/api_keys/user_identities)
--           e 002 (audit_events).
-- Forward-only. No DROP, no destructive ALTER.
--
-- ─── Threat model ─────────────────────────────────────────────────────────
--
-- garraia_login is a BYPASSRLS role used EXCLUSIVELY by the login endpoint
-- via the LoginPool newtype in the garraia-auth crate. It is NOT used by
-- the main app pool (garraia_app), it is NOT used by migrations (which run
-- as superuser), and it is NOT used by any background worker.
--
-- COMPROMISE OF garraia_login = FULL CREDENTIAL STORE EXPOSURE.
-- Mitigation: network isolation, distinct vault entry (GAR-410), rotation,
-- and pgaudit logging on user_identities reads.
--
-- See ADR 0005 §"Login role specification" for the production deployment
-- requirements (separate Unix socket, separate firewall rule, distinct
-- credentials never shared with the main app pool).

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'garraia_login') THEN
        CREATE ROLE garraia_login NOLOGIN BYPASSRLS;
    END IF;
END
$$;

-- Minimal grants for the login flow. See ADR 0005 §"Login role specification"
-- for the rationale of each grant. Any future addition requires a new
-- migration AND a security review.
GRANT USAGE ON SCHEMA public TO garraia_login;

-- Read user_identities to verify credentials.
-- Update user_identities for lazy upgrade PBKDF2 → Argon2id (future 391b).
GRANT SELECT, UPDATE ON user_identities TO garraia_login;

-- Read users to look up by email and get display_name for audit_label.
GRANT SELECT ON users TO garraia_login;

-- Insert/update sessions to issue refresh tokens.
GRANT INSERT, UPDATE ON sessions TO garraia_login;

-- Insert audit_events to log every login attempt (success and failure).
GRANT INSERT ON audit_events TO garraia_login;

-- Sequence grants are intentionally NOT issued here. ADR 0005 §"Login role
-- specification" lists exactly the four table grants above + `USAGE ON SCHEMA
-- public`, and nothing else. If a future migration introduces a serial- or
-- sequence-backed column on one of the auth tables (user_identities, users,
-- sessions, audit_events), that migration MUST add a targeted
-- `GRANT USAGE ON SEQUENCE <name> TO garraia_login` AND go through a security
-- review. A blanket `GRANT USAGE ON ALL SEQUENCES IN SCHEMA public` would
-- silently broaden the role to every sequence in the public schema, including
-- those owned by api_keys, groups, roles, etc. — see GAR-391a security review
-- C-1. Forbidden.

-- ─── Idempotency note ─────────────────────────────────────────────────────
-- The `CREATE ROLE` is wrapped in a `DO $$ ... $$` guard so the first
-- statement is safe to re-run. The `GRANT` statements that follow are
-- naturally idempotent in PostgreSQL: re-issuing a grant that already exists
-- is a no-op (no warning, no error). sqlx::migrate records this migration
-- in `_sqlx_migrations` and never re-runs it on the happy path; the
-- idempotency is only relevant if an operator manually replays the migration
-- during disaster recovery.

-- The login role must NOT have access to:
--   - messages, chats, chat_members, message_threads (chat data)
--   - memory_items, memory_embeddings (AI memory)
--   - tasks, task_lists, task_assignees, task_labels, task_label_assignments,
--     task_comments, task_subscriptions, task_activity (work tracking)
--   - groups, group_members, group_invites (tenant management)
--   - api_keys (separate auth surface)
--   - roles, permissions, role_permissions (RBAC config)
-- These are NOT granted, so REVOKE is unnecessary.

COMMENT ON ROLE garraia_login IS
    'BYPASSRLS dedicated role used EXCLUSIVELY by the garraia-auth login flow. '
    'NOLOGIN by default — production deployments must promote via ALTER ROLE WITH '
    'LOGIN PASSWORD. Compromise = full credential store exposure. See ADR 0005 '
    '(docs/adr/0005-identity-provider.md) and GAR-391 implementation. Code outside '
    'the LoginPool newtype in garraia-auth MUST NOT use this role under any '
    'circumstances.';
