-- Migration 026 — Doc Pages table (Docs Tier 2, plan 0297 / GAR-834)
--
-- Creates the `doc_pages` table: a Notion-like hierarchical document store
-- per group. Only the scaffold (table + RLS + grants + indexes) is added here.
-- Block-level content (`doc_blocks`) comes in a future slice.
--
-- FORCE RLS: all SELECT/INSERT via `garraia_app` role are filtered by
-- `app.current_group_id` (set via SET LOCAL in every handler before SQL).

CREATE TABLE doc_pages (
    id               UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    group_id         UUID        NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    parent_page_id   UUID        REFERENCES doc_pages(id) ON DELETE SET NULL,
    title            TEXT        NOT NULL CHECK(char_length(title) BETWEEN 1 AND 255),
    icon             TEXT,
    cover_file_id    UUID        REFERENCES files(id) ON DELETE SET NULL,
    created_by       UUID        REFERENCES users(id) ON DELETE SET NULL,
    created_by_label TEXT        NOT NULL DEFAULT '',
    settings         JSONB       NOT NULL DEFAULT '{}',
    archived_at      TIMESTAMPTZ,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Compound unique for compound FK targets from child tables (future slices).
    UNIQUE (id, group_id)
);

-- FORCE RLS: row-level isolation by group.
ALTER TABLE doc_pages ENABLE ROW LEVEL SECURITY;
ALTER TABLE doc_pages FORCE ROW LEVEL SECURITY;

-- USING policy: SELECT / UPDATE / DELETE filtered to caller's group.
-- NULLIF avoids returning rows when app.current_group_id is unset ('').
CREATE POLICY doc_pages_group_isolation ON doc_pages
    USING (
        group_id = NULLIF(
            current_setting('app.current_group_id', true),
            ''
        )::uuid
    );

-- WITH CHECK policy: INSERT restricted to caller's group.
CREATE POLICY doc_pages_group_isolation_insert ON doc_pages
    AS RESTRICTIVE
    FOR INSERT
    WITH CHECK (
        group_id = NULLIF(
            current_setting('app.current_group_id', true),
            ''
        )::uuid
    );

-- Grant to the app role (garraia_app).
GRANT SELECT, INSERT, UPDATE ON doc_pages TO garraia_app;

-- Index: list pages by group, newest-first keyset cursor.
CREATE INDEX doc_pages_group_created_idx
    ON doc_pages (group_id, created_at DESC, id DESC);

-- Index: parent page traversal (sparse — most pages are root level).
CREATE INDEX doc_pages_parent_idx
    ON doc_pages (parent_page_id)
    WHERE parent_page_id IS NOT NULL;
