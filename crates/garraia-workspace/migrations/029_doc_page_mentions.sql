-- 029_doc_page_mentions.sql
-- GAR-858 — Migration 029: doc_page_mentions table (plan 0318).
-- Depends:  migration 026 (doc_pages table) + migration 001 (users table).
-- Unblocks: ROADMAP §3.8 Tier 2 `[ ] doc_page_mentions` schema checklist item.
-- Forward-only. No DROP, no destructive ALTER.

CREATE TABLE doc_page_mentions (
    page_id             uuid        NOT NULL REFERENCES doc_pages(id) ON DELETE CASCADE,
    mentioned_user_id   uuid        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- group_id is denormalized from doc_pages.group_id at INSERT time.
    -- Enables direct RLS isolation without joining back to doc_pages.
    group_id            uuid        NOT NULL,
    created_at          timestamptz NOT NULL DEFAULT now(),

    PRIMARY KEY (page_id, mentioned_user_id)
);

COMMENT ON TABLE doc_page_mentions IS
    'Per-page user mentions (@user in a doc page). PK (page_id, mentioned_user_id) enforces one '
    'mention per (user, page). group_id is denormalized for direct FORCE RLS policy. '
    'RLS class: direct via group_id (same pattern as message_mentions / message_reactions). '
    'Implemented in plan 0318 / GAR-858.';

-- Index for efficient GET /v1/me/doc-page-mentions (ordered by newest mention first).
CREATE INDEX doc_page_mentions_user_created_idx
    ON doc_page_mentions (mentioned_user_id, created_at DESC);

-- ─── Row Level Security ───────────────────────────────────────────────────────

ALTER TABLE doc_page_mentions ENABLE ROW LEVEL SECURITY;
ALTER TABLE doc_page_mentions FORCE ROW LEVEL SECURITY;

-- Isolation policy: callers can only see/write mentions within their group.
-- NULLIF fail-closed: if app.current_group_id is not set, NULLIF returns NULL
-- which never equals group_id, so no rows are visible/writable.
CREATE POLICY doc_page_mentions_group_isolation ON doc_page_mentions
    USING (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid)
    WITH CHECK (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid);

-- Grant to garraia_app role (same grants as other application tables)
GRANT SELECT, INSERT, DELETE ON doc_page_mentions TO garraia_app;
