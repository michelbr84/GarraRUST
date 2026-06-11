-- Migration 028 — Doc Page Versions table (Docs Tier 2, plan 0307 / GAR-845)
--
-- Adds `doc_page_versions`: point-in-time snapshots of `doc_pages` content.
-- Each row captures the page metadata + all current blocks at the moment of
-- snapshot creation (stored as `snapshot_jsonb`).
--
-- `group_id` is denormalized from `doc_pages` for direct FORCE RLS isolation
-- (same pattern as `doc_blocks` → `doc_pages` → `messages` → `chats`).
--
-- The compound FK `(page_id, group_id) → doc_pages(id, group_id)` guarantees
-- referential integrity even under RLS where filtered rows look like "not found".
--
-- `created_by` is a plain UUID (no FK) so the row survives user deletion;
-- `created_by_label` caches `display_name` at snapshot time.
--
-- FORCE RLS: all SELECT/INSERT via `garraia_app` are filtered by
-- `app.current_group_id`. No UPDATE or DELETE — versions are append-only.

CREATE TABLE doc_page_versions (
    id               UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    page_id          UUID        NOT NULL REFERENCES doc_pages(id) ON DELETE CASCADE,
    group_id         UUID        NOT NULL,
    -- Compound FK: ensures the page belongs to the same group.
    -- doc_pages has UNIQUE(id, group_id) so this is valid.
    FOREIGN KEY (page_id, group_id) REFERENCES doc_pages(id, group_id) ON DELETE CASCADE,
    -- Snapshot: {title, icon, parent_page_id, blocks: [{id, type, position, content}]}
    snapshot_jsonb   JSONB       NOT NULL,
    -- Plain UUID (no FK) — survives user deletion; label cached at snapshot time.
    created_by       UUID        NOT NULL,
    created_by_label TEXT        NOT NULL,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- FORCE RLS: row-level isolation by group.
ALTER TABLE doc_page_versions ENABLE ROW LEVEL SECURITY;
ALTER TABLE doc_page_versions FORCE ROW LEVEL SECURITY;

-- USING policy: SELECT filtered to caller's group.
-- NULLIF avoids returning rows when app.current_group_id is unset ('').
CREATE POLICY doc_page_versions_group_isolation ON doc_page_versions
    USING (
        group_id = NULLIF(
            current_setting('app.current_group_id', true),
            ''
        )::uuid
    );

-- WITH CHECK policy: INSERT restricted to caller's group.
CREATE POLICY doc_page_versions_group_isolation_insert ON doc_page_versions
    AS RESTRICTIVE
    FOR INSERT
    WITH CHECK (
        group_id = NULLIF(
            current_setting('app.current_group_id', true),
            ''
        )::uuid
    );

-- Grant SELECT and INSERT to the app role (garraia_app).
-- No UPDATE or DELETE — versions are immutable once created.
GRANT SELECT, INSERT ON doc_page_versions TO garraia_app;

-- Index: list versions by page ordered by creation time descending.
CREATE INDEX doc_page_versions_page_created_idx
    ON doc_page_versions (page_id, created_at DESC, id DESC);
