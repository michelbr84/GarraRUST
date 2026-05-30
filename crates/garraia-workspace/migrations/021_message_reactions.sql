-- 021_message_reactions.sql
-- GAR-747 — Migration 021: message_reactions table (plan 0229).
-- Depends:  migration 004 (messages table) + migration 001 (users table).
-- Unblocks: ROADMAP §3.6 `[ ] Reações, menções (@user, @channel), typing indicators`
--           (this slice delivers reactions).
-- Forward-only. No DROP, no destructive ALTER.

CREATE TABLE message_reactions (
    message_id      uuid        NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    user_id         uuid        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- emoji: 1–10 Unicode grapheme clusters (covers skin tone modifiers, ZWJ sequences)
    emoji           varchar(64) NOT NULL CHECK (char_length(emoji) BETWEEN 1 AND 10),
    -- group_id is denormalized from messages.group_id at INSERT time.
    -- Enables cheap cross-tenant audit queries without joining back to messages:
    --   SELECT mr.* FROM message_reactions mr
    --   JOIN messages m ON mr.message_id = m.id
    --   WHERE mr.group_id <> m.group_id;
    group_id        uuid        NOT NULL,
    reacted_at      timestamptz NOT NULL DEFAULT now(),

    PRIMARY KEY (message_id, user_id, emoji)
);

COMMENT ON TABLE message_reactions IS
    'Emoji reactions on messages. PK (message_id, user_id, emoji) enforces one reaction '
    'per (user, emoji, message). group_id is denormalized for audit queries and FORCE RLS '
    'policy. RLS class: direct via group_id (same pattern as message_attachments). '
    'Implemented in plan 0229 / GAR-747.';

-- Index for efficient GROUP BY queries in GET /v1/messages/{id}/reactions
CREATE INDEX message_reactions_message_emoji_idx ON message_reactions (message_id, emoji);

-- Index for efficient user-based deletion in DELETE /v1/messages/{id}/reactions/{emoji}
CREATE INDEX message_reactions_user_idx ON message_reactions (user_id, message_id);

-- ─── Row Level Security ───────────────────────────────────────────────────────

ALTER TABLE message_reactions ENABLE ROW LEVEL SECURITY;
ALTER TABLE message_reactions FORCE ROW LEVEL SECURITY;

-- Isolation policy: callers can only see/write reactions within their group.
-- The group_id column is denormalized (set by the application at INSERT time)
-- and validated by the JOIN guard in the handler before INSERT.
-- NULLIF fail-closed: if app.current_group_id is not set, NULLIF returns NULL
-- which never equals group_id, so no rows are visible/writable.
CREATE POLICY message_reactions_group_isolation ON message_reactions
    USING (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid)
    WITH CHECK (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid);

-- Grant to garraia_app role (same grants as other application tables)
GRANT SELECT, INSERT, DELETE ON message_reactions TO garraia_app;
