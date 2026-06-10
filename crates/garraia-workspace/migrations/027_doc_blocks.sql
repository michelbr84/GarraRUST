-- Migration 027 — Doc Blocks table (Docs Tier 2, plan 0302 / GAR-840)
--
-- Adds `doc_blocks`: the block-level content store for `doc_pages`.
-- One page contains N blocks; blocks may nest via `parent_block_id`.
-- `group_id` is denormalized from `doc_pages` for direct FORCE RLS
-- isolation (same pattern as `messages` → `chats`).
--
-- The compound FK `(page_id, group_id) → doc_pages(id, group_id)`
-- guarantees referential integrity even under RLS where RLS-filtered
-- rows look like "not found" to the app role.
--
-- FORCE RLS: all SELECT/INSERT/UPDATE/DELETE via `garraia_app` are
-- filtered by `app.current_group_id`.

CREATE TABLE doc_blocks (
    id               UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    page_id          UUID        NOT NULL REFERENCES doc_pages(id) ON DELETE CASCADE,
    group_id         UUID        NOT NULL,
    -- Compound FK: ensures page belongs to the same group.
    -- doc_pages has UNIQUE(id, group_id) so this is valid.
    FOREIGN KEY (page_id, group_id) REFERENCES doc_pages(id, group_id) ON DELETE CASCADE,
    parent_block_id  UUID        REFERENCES doc_blocks(id) ON DELETE SET NULL,
    -- Sparse float position: values like 1.0, 2.0, 3.0 allow
    -- insertion between by picking midpoint (e.g. 1.5).
    position         FLOAT8      NOT NULL DEFAULT 0,
    block_type       TEXT        NOT NULL CHECK(block_type IN (
                         'heading', 'paragraph', 'todo', 'bullet', 'numbered',
                         'code', 'quote', 'callout', 'divider',
                         'file_embed', 'task_embed', 'chat_embed', 'image')),
    content_jsonb    JSONB       NOT NULL DEFAULT '{}',
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- FORCE RLS: row-level isolation by group.
ALTER TABLE doc_blocks ENABLE ROW LEVEL SECURITY;
ALTER TABLE doc_blocks FORCE ROW LEVEL SECURITY;

-- USING policy: SELECT / UPDATE / DELETE filtered to caller's group.
-- NULLIF avoids returning rows when app.current_group_id is unset ('').
CREATE POLICY doc_blocks_group_isolation ON doc_blocks
    USING (
        group_id = NULLIF(
            current_setting('app.current_group_id', true),
            ''
        )::uuid
    );

-- WITH CHECK policy: INSERT restricted to caller's group.
CREATE POLICY doc_blocks_group_isolation_insert ON doc_blocks
    AS RESTRICTIVE
    FOR INSERT
    WITH CHECK (
        group_id = NULLIF(
            current_setting('app.current_group_id', true),
            ''
        )::uuid
    );

-- Grant to the app role (garraia_app).
GRANT SELECT, INSERT, UPDATE, DELETE ON doc_blocks TO garraia_app;

-- Index: list blocks by page, ordered by position + id (deterministic).
CREATE INDEX doc_blocks_page_position_idx
    ON doc_blocks (page_id, position ASC, id ASC);

-- Index: parent block traversal (sparse — most blocks are root-level).
CREATE INDEX doc_blocks_parent_idx
    ON doc_blocks (parent_block_id)
    WHERE parent_block_id IS NOT NULL;
