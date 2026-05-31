-- 022_message_mentions.sql
-- GAR-755 — Migration 022: message_mentions table (plan 0237).
-- Depends:  migration 004 (messages table) + migration 001 (users table).
-- Unblocks: ROADMAP §3.6 `[ ] Menções (@user, @channel)`
-- Forward-only. No DROP, no destructive ALTER.

CREATE TABLE message_mentions (
    message_id          uuid        NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    mentioned_user_id   uuid        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- group_id is denormalized from messages.group_id at INSERT time.
    -- Enables cheap cross-tenant audit queries without joining back to messages.
    group_id            uuid        NOT NULL,
    created_at          timestamptz NOT NULL DEFAULT now(),

    PRIMARY KEY (message_id, mentioned_user_id)
);

COMMENT ON TABLE message_mentions IS
    'Per-message user mentions (@user). PK (message_id, mentioned_user_id) enforces one mention '
    'per (user, message). group_id is denormalized for audit queries and FORCE RLS policy. '
    'RLS class: direct via group_id (same pattern as message_reactions). '
    'Implemented in plan 0237 / GAR-755.';

-- Index for efficient GET /v1/me/mentions (ordered by newest mention first).
CREATE INDEX message_mentions_user_created_idx
    ON message_mentions (mentioned_user_id, created_at DESC);

-- ─── Row Level Security ───────────────────────────────────────────────────────

ALTER TABLE message_mentions ENABLE ROW LEVEL SECURITY;
ALTER TABLE message_mentions FORCE ROW LEVEL SECURITY;

-- Isolation policy: callers can only see/write mentions within their group.
-- NULLIF fail-closed: if app.current_group_id is not set, NULLIF returns NULL
-- which never equals group_id, so no rows are visible/writable.
CREATE POLICY message_mentions_group_isolation ON message_mentions
    USING (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid)
    WITH CHECK (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid);

-- Grant to garraia_app role (same grants as other application tables)
GRANT SELECT, INSERT, DELETE ON message_mentions TO garraia_app;
