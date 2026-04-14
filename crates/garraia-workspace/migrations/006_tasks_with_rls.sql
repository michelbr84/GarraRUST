-- 006_tasks_with_rls.sql
-- GAR-390 — Migration 006: Tasks Tier 1 (Notion-like) com RLS FORCE
-- embutido desde o dia zero (sem retrofit via migration 007+).
-- Plan:     plans/0008-gar-390-migration-006-tasks-with-rls.md
-- Depends:  migrations 001 (users, groups), 002 (audit_events pattern ref),
--           004 (compound FK pattern from messages), 005 (scope patterns).
-- Forward-only. No DROP TABLE, no destructive ALTER.
--
-- ─── Scope decisions ──────────────────────────────────────────────────────
--
-- IN scope (8 tables with ENABLE + FORCE + CREATE POLICY):
--   task_lists, tasks, task_assignees, task_labels, task_label_assignments,
--   task_comments, task_subscriptions, task_activity
--
-- OUT of scope:
--   task_attachments → blocked by GAR-387 (files) + ADR 0004 (object storage)
--   Recurrence engine → app layer, column exists with format CHECK only
--   Notification sending → GAR-397 digest worker handles fan-out
--   Rust CRUD API → GAR-393
--   position column (kanban ordering) → out of scope, future migration
--
-- ─── Role dependency ──────────────────────────────────────────────────────
--
-- This migration runs BEFORE 007_row_level_security.sql in lexicographic
-- order. That means `garraia_app` role does not yet exist when 006 runs,
-- AND the `ALTER DEFAULT PRIVILEGES ... TO garraia_app` from 007 does not
-- cover tables created here (ADP only affects tables created after its
-- emission). Therefore migration 006:
--   1. Creates `garraia_app NOLOGIN` idempotently (same DO block as 007)
--   2. Explicitly GRANTs SELECT/INSERT/UPDATE/DELETE on every new table
-- When 007 runs, its own idempotent block is a no-op for the role, and its
-- ALTER DEFAULT PRIVILEGES still applies going forward for tables created
-- by any migration > 007.

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'garraia_app') THEN
        CREATE ROLE garraia_app NOLOGIN;
    END IF;
END
$$;

-- ─── 6.1 task_lists ────────────────────────────────────────────────────────

CREATE TABLE task_lists (
    id               uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    group_id         uuid        NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    name             text        NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    type             text        NOT NULL CHECK (type IN ('list', 'board', 'calendar')),
    description      text,
    created_by       uuid        REFERENCES users(id) ON DELETE SET NULL,
    created_by_label text        NOT NULL CHECK (length(created_by_label) BETWEEN 1 AND 200),
    settings         jsonb       NOT NULL DEFAULT '{}'::jsonb,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    archived_at      timestamptz,
    CONSTRAINT task_lists_id_group_unique UNIQUE (id, group_id)
);

CREATE INDEX task_lists_group_idx ON task_lists(group_id)
    WHERE archived_at IS NULL;

COMMENT ON TABLE task_lists IS 'Container for tasks, analogous to a Notion database or a Linear project. type determines the default view. RLS direct policy via group_id; archived lists are filtered at query layer (not RLS).';
COMMENT ON COLUMN task_lists.type IS 'list → flat todo list; board → kanban with status columns; calendar → calendar view driven by due_at.';
COMMENT ON COLUMN task_lists.created_by IS 'Nullable FK with ON DELETE SET NULL. When a user is hard-deleted (GDPR erasure), list remains but loses attribution. created_by_label caches the display name at creation time.';
COMMENT ON COLUMN task_lists.created_by_label IS 'Cached display_name at creation — never NULL even after created_by is SET NULL via user erasure. Same pattern as tasks.created_by_label and audit_events.actor_label.';
COMMENT ON COLUMN task_lists.archived_at IS 'Soft archive. Archived lists remain queryable but hidden from default UI.';
COMMENT ON COLUMN task_lists.updated_at IS 'Caller responsibility — no trigger. Same pattern as users.updated_at.';

-- ─── 6.2 tasks ─────────────────────────────────────────────────────────────

CREATE TABLE tasks (
    id                uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    list_id           uuid        NOT NULL,
    group_id          uuid        NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    parent_task_id    uuid        REFERENCES tasks(id) ON DELETE CASCADE,
    title             text        NOT NULL CHECK (length(title) BETWEEN 1 AND 500),
    description_md    text        CHECK (description_md IS NULL OR length(description_md) BETWEEN 1 AND 50000),
    status            text        NOT NULL DEFAULT 'todo'
                      CHECK (status IN ('backlog', 'todo', 'in_progress', 'review', 'done', 'canceled')),
    priority          text        NOT NULL DEFAULT 'none'
                      CHECK (priority IN ('none', 'low', 'medium', 'high', 'urgent')),
    due_at            timestamptz,
    started_at        timestamptz,
    completed_at      timestamptz,
    estimated_minutes int         CHECK (estimated_minutes IS NULL OR estimated_minutes BETWEEN 0 AND 100000),
    recurrence_rrule  text        CHECK (recurrence_rrule IS NULL OR recurrence_rrule ~ '^[A-Z:;,=0-9+-]+$'),
    created_by        uuid        REFERENCES users(id) ON DELETE SET NULL,
    created_by_label  text        NOT NULL CHECK (length(created_by_label) BETWEEN 1 AND 200),
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now(),
    deleted_at        timestamptz,
    FOREIGN KEY (list_id, group_id) REFERENCES task_lists(id, group_id) ON DELETE CASCADE,
    CONSTRAINT tasks_id_group_unique UNIQUE (id, group_id)
);

CREATE INDEX tasks_list_status_idx ON tasks(list_id, status) WHERE deleted_at IS NULL;
CREATE INDEX tasks_group_status_idx ON tasks(group_id, status) WHERE deleted_at IS NULL;
CREATE INDEX tasks_due_idx ON tasks(due_at) WHERE deleted_at IS NULL AND due_at IS NOT NULL;
CREATE INDEX tasks_parent_idx ON tasks(parent_task_id) WHERE parent_task_id IS NOT NULL AND deleted_at IS NULL;
CREATE INDEX tasks_completed_idx ON tasks(group_id, completed_at DESC) WHERE deleted_at IS NULL AND status = 'done';

COMMENT ON TABLE tasks IS 'Unit of work. Subtasks via parent_task_id self-FK (ON DELETE CASCADE — subtask dies with parent). Compound FK (list_id, group_id) fails-closed against cross-group drift — application code cannot silently insert a task claiming a different group_id than its list. RLS direct policy via group_id denormalized from task_lists.';
COMMENT ON COLUMN tasks.group_id IS 'Denormalized from task_lists.group_id and enforced by compound FK. Enables RLS direct policy without JOIN. Same pattern as messages.group_id.';
COMMENT ON COLUMN tasks.parent_task_id IS 'Self-FK for subtasks. Cross-list parenting is NOT enforced at schema level — app layer (GAR-391/GAR-393) must validate parent.list_id = child.list_id at insert time. Schema depth is unlimited; UI typically caps at 3 levels. GAR-391 MUST include an audit query: SELECT c.id FROM tasks c JOIN tasks p ON c.parent_task_id = p.id WHERE c.list_id <> p.list_id AND c.deleted_at IS NULL — orphan/cross-list subtasks indicate an app-layer bug.';
COMMENT ON COLUMN tasks.status IS 'backlog → unrefined; todo → ready; in_progress → actively worked; review → awaiting review; done → complete; canceled → not going to happen. Transition rules (e.g., canceled↛done) are app-layer.';
COMMENT ON COLUMN tasks.description_md IS 'Markdown-rendered description. CHECK (1..50000) is a DoS mitigation — larger than message body but smaller than collaborative docs (Tier 2).';
COMMENT ON COLUMN tasks.recurrence_rrule IS 'Optional RFC 5545 RRULE string. CHECK validates charset only — full parsing and expansion is app-layer responsibility (future recurrence engine).';
COMMENT ON COLUMN tasks.created_by IS 'Nullable FK with ON DELETE SET NULL. GDPR erasure path: hard-deleting a user does NOT violate this FK; the task row survives with created_by = NULL and attribution preserved via created_by_label.';
COMMENT ON COLUMN tasks.created_by_label IS 'Cached display_name. Erasure survival — task remains readable after user hard-delete. CHECK (length 1..200) mirrors users.display_name bounds.';
COMMENT ON COLUMN tasks.deleted_at IS 'Soft delete. deleted_at IS NOT NULL hides from lists but subtasks and comments remain queryable for audit. Hard delete cascades via ON DELETE CASCADE to children and related rows.';

-- ─── 6.3 task_assignees (M:N user ↔ task) ──────────────────────────────────

CREATE TABLE task_assignees (
    task_id     uuid        NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    user_id     uuid        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    assigned_at timestamptz NOT NULL DEFAULT now(),
    assigned_by uuid        REFERENCES users(id) ON DELETE SET NULL,
    PRIMARY KEY (task_id, user_id)
);

CREATE INDEX task_assignees_user_idx ON task_assignees(user_id, assigned_at DESC);

COMMENT ON TABLE task_assignees IS 'M:N between tasks and users. Assignment is symmetric — no role (assignee vs owner vs watcher). Notification routing is task_subscriptions, not this table. RLS JOIN policy via tasks.group_id.';
COMMENT ON COLUMN task_assignees.assigned_by IS 'Who triggered the assignment. SET NULL on hard-delete so the history row survives but loses attribution.';

-- ─── 6.4 task_labels + 6.5 task_label_assignments ──────────────────────────

CREATE TABLE task_labels (
    id               uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    group_id         uuid        NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    name             text        NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    color            text        NOT NULL CHECK (color ~ '^#[0-9a-fA-F]{6}$'),
    created_by       uuid        REFERENCES users(id) ON DELETE SET NULL,
    created_by_label text        NOT NULL CHECK (length(created_by_label) BETWEEN 1 AND 200),
    created_at       timestamptz NOT NULL DEFAULT now(),
    UNIQUE (group_id, name)
);

CREATE INDEX task_labels_group_idx ON task_labels(group_id);

COMMENT ON TABLE task_labels IS 'Per-group task labels. UNIQUE (group_id, name) prevents duplicate label names within a group. RLS direct policy via group_id.';
COMMENT ON COLUMN task_labels.color IS 'Hex color for UI rendering, CHECK validates #RRGGBB format.';
COMMENT ON COLUMN task_labels.created_by IS 'Nullable FK with ON DELETE SET NULL. GDPR erasure survives — label remains with cached attribution via created_by_label.';
COMMENT ON COLUMN task_labels.created_by_label IS 'Cached display_name at creation — preserved across user hard-delete.';

CREATE TABLE task_label_assignments (
    task_id     uuid        NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    label_id    uuid        NOT NULL REFERENCES task_labels(id) ON DELETE CASCADE,
    assigned_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (task_id, label_id)
);

CREATE INDEX task_label_assignments_label_idx ON task_label_assignments(label_id);

COMMENT ON TABLE task_label_assignments IS 'M:N between tasks and labels. RLS JOIN policy via tasks.group_id (NOT via task_labels — both composites must match, but tasks is the anchor).';

-- ─── 6.6 task_comments ─────────────────────────────────────────────────────

CREATE TABLE task_comments (
    id              uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id         uuid        NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    author_user_id  uuid        REFERENCES users(id) ON DELETE SET NULL,
    author_label    text        NOT NULL CHECK (length(author_label) BETWEEN 1 AND 200),
    body_md         text        NOT NULL CHECK (length(body_md) BETWEEN 1 AND 50000),
    created_at      timestamptz NOT NULL DEFAULT now(),
    edited_at       timestamptz,
    deleted_at      timestamptz
);

CREATE INDEX task_comments_task_created_idx
    ON task_comments(task_id, created_at DESC)
    WHERE deleted_at IS NULL;

COMMENT ON TABLE task_comments IS 'Thread of markdown comments per task. Soft delete via deleted_at. Author attribution survives user erasure via cached author_label + ON DELETE SET NULL on author_user_id. RLS JOIN policy via tasks.group_id.';
COMMENT ON COLUMN task_comments.author_user_id IS 'ON DELETE SET NULL so GDPR right to erasure does not violate FK. author_label preserves who wrote the comment post-erasure.';
COMMENT ON COLUMN task_comments.body_md IS 'Markdown comment body. CHECK (1..50000) is a DoS mitigation and also forces empty-body UI validation.';

-- ─── 6.7 task_subscriptions ────────────────────────────────────────────────

CREATE TABLE task_subscriptions (
    task_id       uuid        NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    user_id       uuid        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    subscribed_at timestamptz NOT NULL DEFAULT now(),
    muted         boolean     NOT NULL DEFAULT false,
    PRIMARY KEY (task_id, user_id)
);

CREATE INDEX task_subscriptions_user_idx ON task_subscriptions(user_id) WHERE muted = false;

COMMENT ON TABLE task_subscriptions IS 'Who receives notifications when a task changes. Decoupled from task_assignees — a user can watch a task they are not assigned to. The actual fan-out to garraia-channels (Telegram/Discord/mobile push) is handled by the digest worker in GAR-397. RLS JOIN policy via tasks.group_id.';
COMMENT ON COLUMN task_subscriptions.user_id IS 'ON DELETE CASCADE — GDPR hard-deletion of a user silently drops all their subscriptions with no audit trail here. Subscription events are ephemeral state (not historical record), so cascade is semantically correct, but GAR-397 digest worker must handle the case where a subscriber disappears mid-fan-out.';

-- ─── 6.8 task_activity ─────────────────────────────────────────────────────

CREATE TABLE task_activity (
    id            uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id       uuid        NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    group_id      uuid        NOT NULL,
    actor_user_id uuid,
    actor_label   text        NOT NULL CHECK (length(actor_label) BETWEEN 1 AND 200),
    kind          text        NOT NULL
                  CHECK (kind IN (
                      'created', 'status_changed', 'priority_changed',
                      'assigned', 'unassigned', 'labeled', 'unlabeled',
                      'commented', 'due_changed', 'archived', 'deleted', 'restored'
                  )),
    payload       jsonb       NOT NULL DEFAULT '{}'::jsonb,
    created_at    timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX task_activity_task_created_idx ON task_activity(task_id, created_at DESC);
CREATE INDEX task_activity_group_created_idx ON task_activity(group_id, created_at DESC);
CREATE INDEX task_activity_kind_idx ON task_activity(kind);

COMMENT ON TABLE task_activity IS 'UI-facing timeline of task events. Complements audit_events (which is global compliance audit). task_activity cascades with tasks — if a task is hard-deleted, its activity goes with it. Actor_user_id is plain uuid (no FK) so rows survive user erasure. RLS direct policy via group_id denormalized.';
COMMENT ON COLUMN task_activity.group_id IS 'Denormalized from tasks.group_id. Kept in sync by the caller (no trigger). Enables RLS direct policy without JOIN. GAR-391 MUST include an audit query: SELECT ta.id FROM task_activity ta JOIN tasks t ON ta.task_id = t.id WHERE ta.group_id <> t.group_id — drift indicates the Axum extractor forgot to set group_id on INSERT, and rows drift-orphaned this way become invisible via RLS (silent data loss).';
COMMENT ON COLUMN task_activity.actor_user_id IS 'Plain uuid — NO FK. Rows survive hard deletion of the user (same pattern as audit_events.actor_user_id). actor_label caches the display name at insert time.';
COMMENT ON COLUMN task_activity.kind IS 'Event category. New event types require a migration to extend the CHECK constraint.';
COMMENT ON COLUMN task_activity.payload IS 'Event-specific context as jsonb (e.g., {old_status:"todo", new_status:"in_progress"} for a status_changed event). Must never contain secrets or unredacted PII.';

-- ─── 6.9 RLS — ENABLE + FORCE + 8 policies ─────────────────────────────────

-- Direct class (group_id denormalized)
ALTER TABLE task_lists ENABLE ROW LEVEL SECURITY;
ALTER TABLE task_lists FORCE ROW LEVEL SECURITY;
CREATE POLICY task_lists_group_isolation ON task_lists
    USING (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid);
COMMENT ON POLICY task_lists_group_isolation ON task_lists IS
    'Class: direct. Context: app.current_group_id. Fail-closed via NULLIF. Same pattern as chats_group_isolation from migration 007.';

ALTER TABLE tasks ENABLE ROW LEVEL SECURITY;
ALTER TABLE tasks FORCE ROW LEVEL SECURITY;
CREATE POLICY tasks_group_isolation ON tasks
    USING (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid);
COMMENT ON POLICY tasks_group_isolation ON tasks IS
    'Class: direct. Context: app.current_group_id. group_id is denormalized and enforced by compound FK (list_id, group_id) → task_lists.';

ALTER TABLE task_labels ENABLE ROW LEVEL SECURITY;
ALTER TABLE task_labels FORCE ROW LEVEL SECURITY;
CREATE POLICY task_labels_group_isolation ON task_labels
    USING (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid);
COMMENT ON POLICY task_labels_group_isolation ON task_labels IS
    'Class: direct. Context: app.current_group_id.';

ALTER TABLE task_activity ENABLE ROW LEVEL SECURITY;
ALTER TABLE task_activity FORCE ROW LEVEL SECURITY;
CREATE POLICY task_activity_group_isolation ON task_activity
    USING (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid);
COMMENT ON POLICY task_activity_group_isolation ON task_activity IS
    'Class: direct. Context: app.current_group_id. Denormalized group_id is set by the caller at insert time — no trigger enforces synchronization with tasks.group_id. Caller (GAR-391/GAR-393) is the single source of truth.';

-- JOIN class (via tasks)
ALTER TABLE task_assignees ENABLE ROW LEVEL SECURITY;
ALTER TABLE task_assignees FORCE ROW LEVEL SECURITY;
CREATE POLICY task_assignees_through_tasks ON task_assignees
    USING (task_id IN (SELECT id FROM tasks));
COMMENT ON POLICY task_assignees_through_tasks ON task_assignees IS
    'Class: JOIN (implicit recursive). The subquery against tasks is itself RLS-protected by tasks_group_isolation, so the composition filters to current group transparently.';

ALTER TABLE task_label_assignments ENABLE ROW LEVEL SECURITY;
ALTER TABLE task_label_assignments FORCE ROW LEVEL SECURITY;
CREATE POLICY task_label_assignments_through_tasks ON task_label_assignments
    USING (task_id IN (SELECT id FROM tasks));
COMMENT ON POLICY task_label_assignments_through_tasks ON task_label_assignments IS
    'Class: JOIN. Anchors on tasks (not task_labels) because tasks.group_id is denormalized and the direct policy is cheaper to evaluate.';

ALTER TABLE task_comments ENABLE ROW LEVEL SECURITY;
ALTER TABLE task_comments FORCE ROW LEVEL SECURITY;
CREATE POLICY task_comments_through_tasks ON task_comments
    USING (task_id IN (SELECT id FROM tasks));
COMMENT ON POLICY task_comments_through_tasks ON task_comments IS
    'Class: JOIN. Recursive RLS composition via tasks. Soft-deleted comments (deleted_at IS NOT NULL) are still visible through this policy — filtering out soft deletes is a query-layer responsibility.';

ALTER TABLE task_subscriptions ENABLE ROW LEVEL SECURITY;
ALTER TABLE task_subscriptions FORCE ROW LEVEL SECURITY;
CREATE POLICY task_subscriptions_through_tasks ON task_subscriptions
    USING (task_id IN (SELECT id FROM tasks));
COMMENT ON POLICY task_subscriptions_through_tasks ON task_subscriptions IS
    'Class: JOIN. A user only sees subscriptions for tasks they can see — cross-group subscription leakage is structurally impossible.';

-- Grants for the 8 new tables. ALTER DEFAULT PRIVILEGES from migration 007
-- does NOT cover these because 006 runs BEFORE 007 in lexicographic order.
GRANT SELECT, INSERT, UPDATE, DELETE ON
    task_lists, tasks, task_assignees, task_labels,
    task_label_assignments, task_comments, task_subscriptions, task_activity
    TO garraia_app;
