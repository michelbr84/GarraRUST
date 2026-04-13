-- 004_chats_and_messages.sql
-- GAR-388 — Migration 004: chats, chat_members, messages (with Portuguese FTS
-- tsvector STORED generated column + GIN index), message_threads.
-- Plan:     plans/0005-gar-388-migration-004-chats-fts.md
-- Depends:  migration 001 (groups, users) + migration 002 (none required,
--           sequential ordering only).
-- Note: slot 003 intentionally reserved for GAR-387 (files) once ADR 0004
--       (object storage) is written; sqlx::migrate! handles numbering gaps.
-- Forward-only. No DROP TABLE, no destructive ALTER.

-- ─── 6.1 chats ─────────────────────────────────────────────────────────────
CREATE TABLE chats (
    id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    group_id    uuid        NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    type        text        NOT NULL CHECK (type IN ('channel', 'dm', 'thread')),
    name        text        NOT NULL,
    topic       text,
    created_by  uuid        NOT NULL REFERENCES users(id),
    settings    jsonb       NOT NULL DEFAULT '{}'::jsonb,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    archived_at timestamptz,
    CONSTRAINT chats_id_group_unique UNIQUE (id, group_id)
);

CREATE INDEX chats_group_id_idx ON chats(group_id) WHERE archived_at IS NULL;
CREATE INDEX chats_group_type_idx ON chats(group_id, type) WHERE archived_at IS NULL;

COMMENT ON TABLE chats IS 'Chat containers within a group. RLS added in migration 007 (GAR-408).';
COMMENT ON COLUMN chats.type IS 'channel → public within group; dm → private between specific members; thread → reserved for thread-root chats, use message_threads for flat threading';
COMMENT ON COLUMN chats.name IS 'Display name. For dm type, caller may set this to the other participant''s display_name at creation time.';
COMMENT ON COLUMN chats.archived_at IS 'Soft delete. NULL = active, non-NULL = archived (hidden by default in UI but messages remain).';
COMMENT ON COLUMN chats.updated_at IS 'Caller responsibility — no trigger. Rust code must SET updated_at = now() explicitly on UPDATE. Same pattern as users.updated_at and groups.updated_at from migration 001.';

-- ─── 6.2 chat_members ──────────────────────────────────────────────────────
CREATE TABLE chat_members (
    chat_id      uuid        NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    user_id      uuid        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role         text        NOT NULL DEFAULT 'member'
                 CHECK (role IN ('owner', 'moderator', 'member', 'viewer')),
    joined_at    timestamptz NOT NULL DEFAULT now(),
    last_read_at timestamptz,
    muted        boolean     NOT NULL DEFAULT false,
    PRIMARY KEY (chat_id, user_id)
);

CREATE INDEX chat_members_user_id_idx ON chat_members(user_id);
CREATE INDEX chat_members_unread_idx
    ON chat_members(user_id, chat_id)
    WHERE muted = false;

COMMENT ON TABLE chat_members IS 'Per-chat membership. Independent from group_members — a user may be in a group but not subscribed to every chat (opt-in channels). RLS NOT enabled in this migration; migration 007 (GAR-408) must JOIN chats to enforce group scope on this table because chat_members has no direct group_id column.';
COMMENT ON COLUMN chat_members.role IS 'Chat-local role. Distinct from group_members.role. Used for moderator-only actions inside a channel. garraia-auth (GAR-391) must check BOTH chat_members.role and group_members.role for two-level RBAC.';
COMMENT ON COLUMN chat_members.last_read_at IS 'Cursor for unread count: messages WHERE created_at > last_read_at are unread.';

-- ─── 6.3 messages ──────────────────────────────────────────────────────────
CREATE TABLE messages (
    id              uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    chat_id         uuid        NOT NULL,
    group_id        uuid        NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    sender_user_id  uuid        NOT NULL REFERENCES users(id),
    sender_label    text        NOT NULL,
    body            text        NOT NULL CHECK (length(body) BETWEEN 1 AND 100000),
    body_tsv        tsvector    GENERATED ALWAYS AS (to_tsvector('portuguese', body)) STORED,
    reply_to_id     uuid        REFERENCES messages(id) ON DELETE SET NULL,
    thread_id       uuid,
    created_at      timestamptz NOT NULL DEFAULT now(),
    edited_at       timestamptz,
    deleted_at      timestamptz,
    FOREIGN KEY (chat_id, group_id) REFERENCES chats(id, group_id) ON DELETE CASCADE
);

CREATE INDEX messages_chat_created_idx
    ON messages(chat_id, created_at DESC)
    WHERE deleted_at IS NULL;

CREATE INDEX messages_body_tsv_idx
    ON messages USING GIN (body_tsv)
    WHERE deleted_at IS NULL;

CREATE INDEX messages_group_created_idx
    ON messages(group_id, created_at DESC)
    WHERE deleted_at IS NULL;

CREATE INDEX messages_thread_id_idx
    ON messages(thread_id)
    WHERE thread_id IS NOT NULL AND deleted_at IS NULL;

CREATE INDEX messages_sender_idx ON messages(sender_user_id);

COMMENT ON TABLE messages IS 'Chat messages with Portuguese FTS. group_id is denormalized from chats.group_id for fast cross-chat queries and future RLS policy (GAR-408). RLS NOT enabled in this migration — tenant isolation comes in migration 007.';
COMMENT ON COLUMN messages.group_id IS 'Denormalized from chats.group_id. Kept in sync by compound FK (chat_id, group_id) REFERENCES chats(id, group_id) — enforced at the DB layer, not by application code. Enables fast group-scoped queries and RLS without a join.';
COMMENT ON COLUMN messages.sender_label IS 'Cached display_name at send time. Lets messages remain readable after user is deleted (erasure survival path).';
COMMENT ON COLUMN messages.body IS 'User-generated content. CHECK (length BETWEEN 1 AND 100000) is a DoS mitigation, not a product limit — prevents storage amplification from a single malicious INSERT. Any observability span that captures this column OR body_tsv MUST route through garraia-telemetry redaction (see garraia-telemetry::redact). Do NOT log raw message bodies in error handlers, panic messages, or span fields.';
COMMENT ON COLUMN messages.body_tsv IS 'Generated column (STORED) with Portuguese tokenizer. GIN indexed for full-text search. Do NOT write to this column — Postgres maintains it from body. SECURITY: always use plainto_tsquery() or websearch_to_tsquery() for user-supplied search input. NEVER pass user input to raw to_tsquery() — it parses operators (&, |, !, <->, label:) which allows query-structure injection that can probe index coverage.';
COMMENT ON COLUMN messages.reply_to_id IS 'Parent message for 1:1 reply. ON DELETE SET NULL so reply chains survive the parent being soft-deleted.';
COMMENT ON COLUMN messages.thread_id IS 'Thread grouping via message_threads table. NULL means top-level message. Plain uuid (no FK) to avoid circular dependency with message_threads.root_message_id. Application layer enforces that every thread_id points to a valid message_threads.id; audit queries in GAR-391 will detect orphans.';
COMMENT ON COLUMN messages.deleted_at IS 'Soft delete. deleted_at IS NOT NULL → message hidden from lists but retained for audit. Hard DELETE reserved for GDPR right to erasure; propagates via ON DELETE SET NULL (reply_to_id) and ON DELETE CASCADE (message_threads.root_message_id).';

-- ─── 6.4 message_threads ───────────────────────────────────────────────────
CREATE TABLE message_threads (
    id              uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    chat_id         uuid        NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    root_message_id uuid        NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    title           text,
    created_by      uuid        NOT NULL REFERENCES users(id),
    created_at      timestamptz NOT NULL DEFAULT now(),
    resolved_at     timestamptz,
    UNIQUE (root_message_id)
);

CREATE INDEX message_threads_chat_idx ON message_threads(chat_id) WHERE resolved_at IS NULL;

COMMENT ON TABLE message_threads IS 'Thread as first-class entity. A thread groups messages via messages.thread_id. Each root message has exactly one thread (UNIQUE). Deferred to app layer: FK from messages.thread_id to message_threads.id would create a circular dependency with root_message_id; we leave messages.thread_id as plain uuid and rely on application invariants + audit queries (GAR-391). RLS NOT enabled in this migration; migration 007 (GAR-408) scopes via JOIN to chats → groups.';
COMMENT ON COLUMN message_threads.resolved_at IS 'Optional: lets a thread be explicitly marked "resolved" (like a GitHub PR thread). UI decision.';
