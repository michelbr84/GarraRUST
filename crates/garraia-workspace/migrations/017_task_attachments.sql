-- 017_task_attachments.sql
-- GAR-572 — Migration 017: task_attachments join table (plan 0096).
-- Depends:  migration 003 (files table) + migration 006 (tasks table).
-- Unblocks: ROADMAP §3.8 Tier 1 `[ ] task_attachments (task_id, file_id)`.
-- Forward-only. No DROP, no destructive ALTER.

CREATE TABLE task_attachments (
    task_id         uuid        NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    file_id         uuid        NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    -- Denormalized group_id mirrors the tasks.group_id pattern used by
    -- task_activity. Kept in sync by the caller (no trigger). Enables
    -- cheap cross-group leak audits: SELECT ta.id FROM task_attachments ta
    -- JOIN tasks t ON ta.task_id = t.id WHERE ta.group_id <> t.group_id.
    group_id        uuid        NOT NULL,
    attached_by     uuid        REFERENCES users(id) ON DELETE SET NULL,
    attached_by_label text      NOT NULL DEFAULT '',
    attached_at     timestamptz NOT NULL DEFAULT now(),

    PRIMARY KEY (task_id, file_id)
);

COMMENT ON TABLE task_attachments IS
    'M:N between tasks and files. A file can be attached to multiple tasks; a '
    'task can have multiple file attachments. group_id is denormalized for audit '
    'queries. RLS class: JOIN via tasks (same as task_assignees, task_label_assignments). '
    'Unblocked by GAR-387 (files schema), implemented in plan 0096 / GAR-572.';

COMMENT ON COLUMN task_attachments.group_id IS
    'Denormalized from tasks.group_id. Kept in sync by the caller. Used for '
    'audit drift detection — if ta.group_id != t.group_id, the handler has a bug.';

COMMENT ON COLUMN task_attachments.attached_by IS
    'User who performed the attach. SET NULL on hard-delete so history survives '
    'without attribution. Display fallback: attached_by_label (cached at insert).';

CREATE INDEX task_attachments_file_idx
    ON task_attachments(file_id, attached_at DESC);

COMMENT ON INDEX task_attachments_file_idx IS
    'Supports "find all tasks this file is attached to" queries. Not used by '
    'initial slice but avoids seqscan on files.id lookups.';

-- FORCE RLS — JOIN class via tasks (same pattern as task_assignees, 006 §7).
-- tasks is itself RLS-protected by tasks_group_isolation, so this composition
-- filters to the current group without duplicating the group_id predicate.
ALTER TABLE task_attachments ENABLE ROW LEVEL SECURITY;
ALTER TABLE task_attachments FORCE ROW LEVEL SECURITY;

CREATE POLICY task_attachments_through_tasks ON task_attachments
    USING (task_id IN (SELECT id FROM tasks));

COMMENT ON POLICY task_attachments_through_tasks ON task_attachments IS
    'Class: JOIN (implicit recursive). The subquery against tasks is itself '
    'RLS-protected by tasks_group_isolation (migration 006), so the composition '
    'filters to app.current_group_id transparently. Matches the pattern used by '
    'task_assignees, task_label_assignments, task_comments, task_subscriptions.';

-- Grant to garraia_app (ALTER DEFAULT PRIVILEGES from migration 007 does NOT
-- cover tables created in later migrations — explicit GRANT required here).
GRANT SELECT, INSERT, DELETE ON task_attachments TO garraia_app;
