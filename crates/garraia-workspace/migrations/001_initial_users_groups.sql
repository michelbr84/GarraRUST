-- 001_initial_users_groups.sql
-- GAR-407 — garraia-workspace bootstrap
-- Decision: docs/adr/0003-database-for-workspace.md (Postgres 16 + pgvector)
-- Plan:     plans/0003-gar-407-workspace-schema-bootstrap.md
-- Forward-only. No DROP TABLE, no destructive ALTER.
--
-- Post-approval deltas from plan §6:
--   - users.legacy_sqlite_id added per plan §12 Q5 (GAR-413 migration tool bridge)
--   - sessions.refresh_token_hash UNIQUE per security review H1 (defense-in-depth
--     against silent collisions and duplicate-insert bugs)
--   - api_keys.key_hash comment pinned to Argon2id only per security review H2
--     (SHA-256 ambiguity removed; consistency across auth-bearing tables)
--   - Caller-responsibility note on updated_at columns per security review M4

-- ─── Extensions ────────────────────────────────────────────────────────────
-- pgcrypto provides gen_random_uuid() (v4). Rust code generates uuid_v7 for
-- time-ordered inserts; the DB default is a v4 fallback for SQL-level inserts
-- (migrations, debugging, psql).
CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- citext gives case-insensitive email comparison without lower() tricks.
CREATE EXTENSION IF NOT EXISTS citext;

-- ─── users ─────────────────────────────────────────────────────────────────
CREATE TABLE users (
    id                uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    email             citext      NOT NULL UNIQUE,
    display_name      text        NOT NULL,
    status            text        NOT NULL DEFAULT 'active'
                      CHECK (status IN ('active', 'suspended', 'deleted')),
    legacy_sqlite_id  text,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now()
);

COMMENT ON TABLE  users IS 'Tenant-root user identities. RLS NOT enabled — visible to app layer.';
COMMENT ON COLUMN users.email IS 'Case-insensitive via citext. Unique across the instance.';
COMMENT ON COLUMN users.status IS 'active → normal; suspended → blocked login; deleted → tombstone pending hard delete';
COMMENT ON COLUMN users.legacy_sqlite_id IS 'Used by GAR-413 SQLite→Postgres migration tool to map rowids; NULL for native Postgres users.';
COMMENT ON COLUMN users.updated_at IS 'Caller responsibility — no trigger. Rust code must SET updated_at = now() explicitly on UPDATE.';

-- ─── user_identities ───────────────────────────────────────────────────────
-- Maps a user to one or more identity providers (internal JWT, OIDC, SAML).
-- Shape matches ADR 0003 and leaves room for ADR 0005 (identity provider)
-- without forcing a schema change when OIDC adapters land.
CREATE TABLE user_identities (
    id             uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id        uuid        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider       text        NOT NULL,
    provider_sub   text        NOT NULL,
    password_hash  text,
    created_at     timestamptz NOT NULL DEFAULT now(),
    UNIQUE (provider, provider_sub)
);
CREATE INDEX user_identities_user_id_idx ON user_identities(user_id);

COMMENT ON COLUMN user_identities.provider IS 'internal | oidc | saml. Default internal for self-host.';
COMMENT ON COLUMN user_identities.provider_sub IS 'Stable subject identifier from the provider. For internal provider, equals users.id::text.';
COMMENT ON COLUMN user_identities.password_hash IS 'Only used when provider=internal. NULL for external IdPs. Algorithm decided in garraia-auth (GAR-391).';

-- ─── sessions ──────────────────────────────────────────────────────────────
CREATE TABLE sessions (
    id                  uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id             uuid        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    refresh_token_hash  text        NOT NULL UNIQUE,
    device_id           text,
    expires_at          timestamptz NOT NULL,
    revoked_at          timestamptz,
    created_at          timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX sessions_user_id_idx ON sessions(user_id);
CREATE INDEX sessions_active_expires_idx
    ON sessions(expires_at)
    WHERE revoked_at IS NULL;

COMMENT ON COLUMN sessions.refresh_token_hash IS 'Argon2id hash of the refresh token. Never the token itself. UNIQUE to prevent silent collisions and surface duplicate-insert bugs.';

-- ─── api_keys ──────────────────────────────────────────────────────────────
CREATE TABLE api_keys (
    id            uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id       uuid        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    label         text        NOT NULL,
    key_hash      text        NOT NULL UNIQUE,
    scopes        jsonb       NOT NULL DEFAULT '[]'::jsonb,
    created_at    timestamptz NOT NULL DEFAULT now(),
    revoked_at    timestamptz,
    last_used_at  timestamptz
);
CREATE INDEX api_keys_active_user_idx
    ON api_keys(user_id)
    WHERE revoked_at IS NULL;

COMMENT ON COLUMN api_keys.key_hash IS 'Argon2id hash of the API key (PHC string format). Never the key itself. Do NOT substitute SHA-256 — consistency across auth-bearing tables is enforced for defense-in-depth.';
COMMENT ON COLUMN api_keys.scopes IS 'JSON array of permission strings, e.g. ["workspace.read", "files.write"].';

-- ─── groups ────────────────────────────────────────────────────────────────
-- The tenant root. Every downstream table (messages, files, memory) will
-- reference group_id and enable RLS filtered by current_setting('app.current_group_id').
-- Migration 007 (GAR-408) applies FORCE ROW LEVEL SECURITY to those downstream tables.
-- groups itself is NOT under RLS — membership-based visibility lives in the app layer
-- (JOIN groups ↔ group_members WHERE group_members.user_id = current_user_id).
CREATE TABLE groups (
    id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    name        text        NOT NULL,
    type        text        NOT NULL CHECK (type IN ('family', 'team', 'personal')),
    created_by  uuid        NOT NULL REFERENCES users(id),
    settings    jsonb       NOT NULL DEFAULT '{}'::jsonb,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX groups_created_by_idx ON groups(created_by);

COMMENT ON TABLE groups IS 'Tenant root. All scoped data (messages/files/memory/tasks) references group_id.';
COMMENT ON COLUMN groups.type IS 'family → household; team → professional; personal → RESERVED for GAR-413 SQLite→PG migration tool fallback. The API layer (GAR-393) must not expose ''personal'' as a user-selectable option — owner-only, programmatic.';
COMMENT ON COLUMN groups.updated_at IS 'Caller responsibility — no trigger. Rust code must SET updated_at = now() explicitly on UPDATE.';

-- ─── group_members ─────────────────────────────────────────────────────────
CREATE TABLE group_members (
    group_id    uuid        NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    user_id     uuid        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role        text        NOT NULL CHECK (role IN ('owner', 'admin', 'member', 'guest', 'child')),
    status      text        NOT NULL DEFAULT 'active'
                CHECK (status IN ('active', 'invited', 'removed', 'banned')),
    joined_at   timestamptz NOT NULL DEFAULT now(),
    invited_by  uuid        REFERENCES users(id),
    PRIMARY KEY (group_id, user_id)
);
CREATE INDEX group_members_user_id_idx ON group_members(user_id);
CREATE INDEX group_members_active_by_group_idx
    ON group_members(group_id)
    WHERE status = 'active';

COMMENT ON COLUMN group_members.role IS 'Maps to capability matrix in ROADMAP §3.3 / GAR-391';

-- ─── group_invites ─────────────────────────────────────────────────────────
CREATE TABLE group_invites (
    id             uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    group_id       uuid        NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    invited_email  citext      NOT NULL,
    proposed_role  text        NOT NULL
                   CHECK (proposed_role IN ('admin', 'member', 'guest', 'child')),
    token_hash     text        NOT NULL UNIQUE,
    expires_at     timestamptz NOT NULL,
    created_by     uuid        NOT NULL REFERENCES users(id),
    created_at     timestamptz NOT NULL DEFAULT now(),
    accepted_at    timestamptz,
    accepted_by    uuid        REFERENCES users(id)
);
CREATE INDEX group_invites_group_id_idx ON group_invites(group_id);
CREATE INDEX group_invites_pending_email_idx
    ON group_invites(invited_email)
    WHERE accepted_at IS NULL;

COMMENT ON COLUMN group_invites.token_hash IS 'Argon2id hash of the invite token sent via email. Never the token itself.';
COMMENT ON COLUMN group_invites.proposed_role IS 'owner cannot be invited — only created by the bootstrap user.';
