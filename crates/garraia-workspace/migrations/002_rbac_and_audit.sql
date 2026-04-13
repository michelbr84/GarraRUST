-- 002_rbac_and_audit.sql
-- GAR-386 — Migration 002: RBAC tables (roles/permissions/role_permissions)
-- plus audit_events + single-owner partial unique index (closes GAR-414 M1, M3).
-- Plan:     plans/0004-gar-386-migration-002-rbac.md
-- Depends:  migration 001 (users, group_members for single-owner index).
-- Forward-only. No DROP TABLE, no destructive ALTER.

-- ─── 6.1 roles ─────────────────────────────────────────────────────────
CREATE TABLE roles (
    id           text        PRIMARY KEY,
    display_name text        NOT NULL,
    description  text        NOT NULL,
    tier         int         NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now()
);

COMMENT ON TABLE roles IS 'Static lookup table of capability tiers. Runtime authority is group_members.role CHECK constraint from migration 001. Rows are seeded here and NOT mutable via runtime API in v1 — any runtime endpoint that tries to INSERT/UPDATE/DELETE roles violates the v1 contract. Mutability deferred to a future migration + explicit admin API.';
COMMENT ON COLUMN roles.tier IS 'Numeric ordering: higher = more privilege. owner=100, admin=80, member=50, guest=20, child=10.';

INSERT INTO roles (id, display_name, description, tier) VALUES
    ('owner',  'Owner',         'Full control. Can delete the group, manage members, manage billing. Exactly one per group enforced by partial unique index.', 100),
    ('admin',  'Admin',         'Manage members and settings, moderate chats, manage folders.', 80),
    ('member', 'Member',        'Create/edit content, send messages, upload to permitted folders.', 50),
    ('guest',  'Guest',         'Read + limited contribution to explicitly-shared resources.', 20),
    ('child',  'Child',         'Chat + Tasks only. No files, memory, docs, export, or share — child_perms == 4 enforced by smoke test.', 10);

-- ─── 6.2 permissions ───────────────────────────────────────────────────
CREATE TABLE permissions (
    id          text        PRIMARY KEY,
    resource    text        NOT NULL,
    action      text        NOT NULL,
    description text        NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (resource, action)
);

COMMENT ON TABLE permissions IS 'Canonical capability strings. Static seed — runtime mutation deferred to a future migration + explicit admin API. Do NOT INSERT/UPDATE/DELETE via ad-hoc queries in v1. Application layer (garraia-auth, GAR-391) loads these into a compile-time map and uses them for fn can(principal, action) checks.';

INSERT INTO permissions (id, resource, action, description) VALUES
    -- Files (Fase 3.5)
    ('files.read',          'files',    'read',     'List and download files in permitted folders.'),
    ('files.write',         'files',    'write',    'Upload, rename, and replace files.'),
    ('files.delete',        'files',    'delete',   'Soft-delete files (moves to trash).'),
    ('files.share',         'files',    'share',    'Create share links or grant cross-group access.'),
    -- Chats (Fase 3.6)
    ('chats.read',          'chats',    'read',     'Read messages in subscribed channels.'),
    ('chats.write',         'chats',    'write',    'Send messages and replies.'),
    ('chats.moderate',      'chats',    'moderate', 'Delete others messages, pin, ban spammers.'),
    -- Memory (Fase 3.7)
    ('memory.read',         'memory',   'read',     'Query shared memory items by scope.'),
    ('memory.write',        'memory',   'write',    'Insert new memory items.'),
    ('memory.delete',       'memory',   'delete',   'Remove memory items (subject to retention policy).'),
    -- Tasks (Fase 3.8 Tier 1)
    ('tasks.read',          'tasks',    'read',     'View task lists and cards.'),
    ('tasks.write',         'tasks',    'write',    'Create and edit tasks.'),
    ('tasks.assign',        'tasks',    'assign',   'Assign tasks to members.'),
    ('tasks.delete',        'tasks',    'delete',   'Delete tasks or lists.'),
    -- Docs (Fase 3.8 Tier 2)
    ('docs.read',           'docs',     'read',     'Read collaborative doc pages.'),
    ('docs.write',          'docs',     'write',    'Edit doc pages and blocks.'),
    ('docs.delete',         'docs',     'delete',   'Archive or delete doc pages.'),
    -- Group admin
    ('members.manage',      'members',  'manage',   'Invite, remove, suspend, change roles of members.'),
    ('group.settings',      'group',    'settings', 'Modify group settings, retention policies, external sharing.'),
    ('group.delete',        'group',    'delete',   'Permanently delete the group and all its data.'),
    -- Export / compliance
    ('export.self',         'export',   'self',     'Export own data (LGPD / GDPR right to data portability).'),
    ('export.group',        'export',   'group',    'Export all group data.');

-- ─── 6.3 role_permissions ──────────────────────────────────────────────
CREATE TABLE role_permissions (
    role_id       text NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    permission_id text NOT NULL REFERENCES permissions(id) ON DELETE CASCADE,
    PRIMARY KEY (role_id, permission_id)
);

-- Owner: full control (all permissions).
INSERT INTO role_permissions (role_id, permission_id)
SELECT 'owner', id FROM permissions;

-- Admin: everything except group.delete and export.group.
INSERT INTO role_permissions (role_id, permission_id) VALUES
    ('admin', 'files.read'),    ('admin', 'files.write'),    ('admin', 'files.delete'),    ('admin', 'files.share'),
    ('admin', 'chats.read'),    ('admin', 'chats.write'),    ('admin', 'chats.moderate'),
    ('admin', 'memory.read'),   ('admin', 'memory.write'),   ('admin', 'memory.delete'),
    ('admin', 'tasks.read'),    ('admin', 'tasks.write'),    ('admin', 'tasks.assign'),    ('admin', 'tasks.delete'),
    ('admin', 'docs.read'),     ('admin', 'docs.write'),     ('admin', 'docs.delete'),
    ('admin', 'members.manage'),('admin', 'group.settings'),
    ('admin', 'export.self');

-- Member: content create/edit, no member management, no delete on others.
INSERT INTO role_permissions (role_id, permission_id) VALUES
    ('member', 'files.read'),   ('member', 'files.write'),
    ('member', 'chats.read'),   ('member', 'chats.write'),
    ('member', 'memory.read'),  ('member', 'memory.write'),
    ('member', 'tasks.read'),   ('member', 'tasks.write'),
    ('member', 'docs.read'),    ('member', 'docs.write'),
    ('member', 'export.self');

-- Guest: read + contribute to explicit channels only (semantic guard in app layer).
INSERT INTO role_permissions (role_id, permission_id) VALUES
    ('guest', 'files.read'),
    ('guest', 'chats.read'),    ('guest', 'chats.write'),
    ('guest', 'tasks.read'),
    ('guest', 'docs.read'),
    ('guest', 'export.self');

-- Child: read + write chats/tasks (supervised), NO file/memory/docs/export/share.
INSERT INTO role_permissions (role_id, permission_id) VALUES
    ('child', 'chats.read'),    ('child', 'chats.write'),
    ('child', 'tasks.read'),    ('child', 'tasks.write');

-- ─── 6.4 audit_events ──────────────────────────────────────────────────
CREATE TABLE audit_events (
    id              uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    group_id        uuid,
    actor_user_id   uuid,
    actor_label     text,
    action          text        NOT NULL,
    resource_type   text        NOT NULL,
    resource_id     text,
    ip              inet,
    user_agent      text,
    metadata        jsonb       NOT NULL DEFAULT '{}'::jsonb,
    created_at      timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX audit_events_group_created_idx ON audit_events(group_id, created_at DESC)
    WHERE group_id IS NOT NULL;
CREATE INDEX audit_events_actor_created_idx ON audit_events(actor_user_id, created_at DESC)
    WHERE actor_user_id IS NOT NULL;
CREATE INDEX audit_events_action_idx ON audit_events(action);

COMMENT ON TABLE audit_events IS 'Central audit trail. No FK — rows survive CASCADE deletes so erasure is demonstrable. RLS added in migration 007 (GAR-408) so members only see their group events.';
COMMENT ON COLUMN audit_events.group_id IS 'NULL for user-level events (login, logout, self-export). Set for group-scoped events.';
COMMENT ON COLUMN audit_events.actor_user_id IS 'Plain uuid with NO foreign key — intentionally. Rows must survive hard deletion of the user so LGPD art. 8 §5 / GDPR art. 17(1) erasure is demonstrable. actor_label caches the display name at event time for post-deletion auditability.';
COMMENT ON COLUMN audit_events.actor_label IS 'Cached display label at event time. Lets auditors see "who did what" after user deletion.';
COMMENT ON COLUMN audit_events.action IS 'Verb string: users.delete, files.download, roles.setRole, etc. Canonical from permissions.id where applicable.';
COMMENT ON COLUMN audit_events.resource_id IS 'Text, not uuid, to support non-UUID resources (paths, external IDs).';
COMMENT ON COLUMN audit_events.metadata IS 'Free-form jsonb for event-specific context. Must never contain secrets or raw PII beyond what audit requires.';

-- ─── 6.5 single-owner partial unique index (closes GAR-414 M1) ─────────
-- Enforces exactly one 'owner' per group at the database layer. Complements
-- the CHECK constraint from migration 001 which only validates the enum value.
-- A second INSERT with role='owner' for the same group_id fails with SQLSTATE 23505.
CREATE UNIQUE INDEX group_members_single_owner_idx
    ON group_members(group_id)
    WHERE role = 'owner';
