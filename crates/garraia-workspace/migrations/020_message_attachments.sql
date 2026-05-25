-- 020_message_attachments.sql
-- GAR-697 — Migration 020: message_attachments join table (plan 0179).
-- Depends:  migration 003 (files table) + migration 004 (messages table).
-- Unblocks: ROADMAP §3.2 `[ ] message_attachments — deferido até GAR-387 (files) materializar`.
--           ROADMAP §3.9 `has_attachment` search filter (search slice 4).
-- Forward-only. No DROP, no destructive ALTER.

CREATE TABLE message_attachments (
    message_id      uuid        NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    file_id         uuid        NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    -- Denormalized group_id mirrors the messages.group_id pattern used by
    -- message_threads. Kept in sync by the caller (no trigger). Enables
    -- cheap cross-group leak audits:
    -- SELECT ma.* FROM message_attachments ma
    -- JOIN messages m ON ma.message_id = m.id
    -- WHERE ma.group_id <> m.group_id;
    group_id        uuid        NOT NULL,
    attached_by     uuid        REFERENCES users(id) ON DELETE SET NULL,
    attached_by_label text      NOT NULL DEFAULT '',
    attached_at     timestamptz NOT NULL DEFAULT now(),

    PRIMARY KEY (message_id, file_id)
);

COMMENT ON TABLE message_attachments IS
    'M:N between messages and files. A file can be attached to multiple messages; a '
    'message can have multiple file attachments. group_id is denormalized for audit '
    'queries. RLS class: JOIN via messages (same as task_attachments via tasks, '
    'migration 017). Unblocked by GAR-387 (files schema) + GAR-388 (messages schema), '
    'implemented in plan 0179 / GAR-697.';

COMMENT ON COLUMN message_attachments.group_id IS
    'Denormalized from messages.group_id. Kept in sync by the caller. Used for '
    'audit drift detection — if ma.group_id != m.group_id, the handler has a bug.';

COMMENT ON COLUMN message_attachments.attached_by IS
    'User who performed the attach. SET NULL on hard-delete so history survives '
    'without attribution. Display fallback: attached_by_label (cached at insert).';

CREATE INDEX message_attachments_file_idx
    ON message_attachments(file_id, attached_at DESC);

COMMENT ON INDEX message_attachments_file_idx IS
    'Supports "find all messages this file is attached to" queries.';

CREATE INDEX message_attachments_message_idx
    ON message_attachments(message_id);

COMMENT ON INDEX message_attachments_message_idx IS
    'Supports EXISTS subquery in GET /v1/search?has_attachment=true (search slice 4). '
    'Avoids seqscan on message_id lookups — critical because the search handler fires '
    'one EXISTS per message candidate in the FTS result set.';

-- FORCE RLS — JOIN class via messages (same pattern as task_attachments via tasks,
-- migration 017). messages is itself RLS-protected by messages_group_isolation
-- (migration 007), so the composition filters to the current group transparently.
ALTER TABLE message_attachments ENABLE ROW LEVEL SECURITY;
ALTER TABLE message_attachments FORCE ROW LEVEL SECURITY;

CREATE POLICY message_attachments_through_messages ON message_attachments
    USING (message_id IN (SELECT id FROM messages));

COMMENT ON POLICY message_attachments_through_messages ON message_attachments IS
    'Class: JOIN (implicit recursive). The subquery against messages is itself '
    'RLS-protected by messages_group_isolation (migration 007), so the composition '
    'filters to app.current_group_id transparently. Matches the pattern used by '
    'task_attachments (migration 017).';

-- Grant to garraia_app (ALTER DEFAULT PRIVILEGES from migration 007 does NOT
-- cover tables created in later migrations — explicit GRANT required here).
GRANT SELECT, INSERT, DELETE ON message_attachments TO garraia_app;
