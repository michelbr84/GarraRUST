-- 005_memory_items_and_embeddings.sql
-- GAR-389 — Migration 005: AI memory with tri-level scope + pgvector HNSW.
-- Plan:     plans/0006-gar-389-migration-005-memory-pgvector.md
-- Depends:  migration 001 (users, groups), migration 004 (chats, messages for
--           provenance columns).
-- Forward-only. No DROP TABLE, no destructive ALTER.

CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE memory_items (
    id               uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    scope_type       text        NOT NULL
                     CHECK (scope_type IN ('user', 'group', 'chat')),
    scope_id         uuid        NOT NULL,
    group_id         uuid        REFERENCES groups(id) ON DELETE CASCADE,
    created_by       uuid        REFERENCES users(id) ON DELETE SET NULL,
    created_by_label text        NOT NULL,
    kind             text        NOT NULL
                     CHECK (kind IN ('fact', 'preference', 'note', 'reminder', 'rule', 'profile')),
    content          text        NOT NULL CHECK (length(content) BETWEEN 1 AND 10000),
    sensitivity      text        NOT NULL DEFAULT 'private'
                     CHECK (sensitivity IN ('public', 'group', 'private', 'secret')),
    source_chat_id   uuid        REFERENCES chats(id) ON DELETE SET NULL,
    source_message_id uuid       REFERENCES messages(id) ON DELETE SET NULL,
    ttl_expires_at   timestamptz,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    deleted_at       timestamptz,
    CHECK (ttl_expires_at IS NULL OR ttl_expires_at > created_at)
);

CREATE INDEX memory_items_scope_idx
    ON memory_items(scope_type, scope_id)
    WHERE deleted_at IS NULL;

CREATE INDEX memory_items_group_idx
    ON memory_items(group_id, created_at DESC)
    WHERE deleted_at IS NULL AND group_id IS NOT NULL;

CREATE INDEX memory_items_ttl_idx
    ON memory_items(ttl_expires_at)
    WHERE ttl_expires_at IS NOT NULL AND deleted_at IS NULL;

CREATE INDEX memory_items_created_by_idx ON memory_items(created_by);

-- Non-partial intentionally — used for GDPR right-to-erasure scans that
-- must find both live and soft-deleted rows owned by a departing user.

-- Forcing function for sensitivity='secret' discoverability: this partial
-- index makes secret rows visible in EXPLAIN output, so any retrieval query
-- that forgets to exclude them is caught by query-plan review. garraia-auth
-- (GAR-391) retrieval paths MUST include `AND sensitivity <> 'secret'` unless
-- the user explicitly requested secret access.
CREATE INDEX memory_items_secret_sensitivity_idx
    ON memory_items(id)
    WHERE sensitivity = 'secret' AND deleted_at IS NULL;

COMMENT ON TABLE memory_items IS 'AI memory with three-level scope (user/group/chat). RLS NOT enabled in this migration — migration 007 (GAR-408) adds the policy filtering by group_id AND by app.current_user_id for user-scope rows. Personal memories (scope_type=user) MUST NOT leak into group retrieval per LGPD art. 46 segregation requirement.';
COMMENT ON COLUMN memory_items.scope_type IS 'user → personal memory (only the creator sees it); group → shared within the group; chat → bound to a specific chat/channel. Resolution rule (see ROADMAP §3.3): Chat > Group > User when multiple scopes intersect.';
COMMENT ON COLUMN memory_items.scope_id IS 'Points to users.id, groups.id, or chats.id depending on scope_type. No FK — Postgres does not support conditional FKs on scalar columns. Application layer (garraia-auth, GAR-391) enforces that scope_id is a valid row of the table implied by scope_type. Audit orphans via: SELECT id FROM memory_items WHERE scope_type=''user'' AND scope_id NOT IN (SELECT id FROM users); analogous queries for group/chat scopes. Schedule in GAR-391.';
COMMENT ON COLUMN memory_items.group_id IS 'Denormalized group context for RLS in migration 007. For scope_type=group, equals scope_id. For scope_type=chat, equals chats.group_id. For scope_type=user, NULL (personal memories are not group-scoped).';
COMMENT ON COLUMN memory_items.created_by IS 'Plain FK with ON DELETE SET NULL — when the user is hard-deleted (GDPR right to erasure), memory rows survive but lose attribution. created_by_label is cached so post-erasure audit remains readable. Nullable by necessity — a NOT NULL constraint here would make LGPD art. 18 erasure blocked by FK violation.';
COMMENT ON COLUMN memory_items.created_by_label IS 'Cached display_name at save time. Lets memory remain attributable after user deletion (erasure survival path, same pattern as audit_events.actor_label). Never NULL — even after created_by is SET NULL on erasure, the label is retained.';
COMMENT ON COLUMN memory_items.kind IS 'Semantic category — helps the agent filter retrieval (e.g., fetch only profile facts when introducing the assistant).';
COMMENT ON COLUMN memory_items.content IS 'Plain text. CHECK (length 1..10000) is a DoS mitigation. Any observability span that captures this column MUST route through garraia-telemetry redaction — memories often contain PII (names, schedules, preferences, secrets).';
COMMENT ON COLUMN memory_items.sensitivity IS 'public → safe to include in any retrieval; group → only within the group; private → only with the creator present; secret → NEVER included in LLM prompts automatically (manual retrieval only). garraia-auth (GAR-391) MUST enforce: every retrieval query MUST include `AND sensitivity <> ''secret''` unless the caller explicitly opted in. The partial index memory_items_secret_sensitivity_idx exists as a forcing function — secret rows are visible in EXPLAIN output to catch query plans that forgot the exclusion.';
COMMENT ON COLUMN memory_items.ttl_expires_at IS 'Optional expiration. A scheduled worker (Fase 2.1 or later) hard-deletes rows where ttl_expires_at < now() after a grace period. CHECK (ttl_expires_at > created_at) prevents accidentally-expired-on-insert rows. IMPORTANT: retrieval queries MUST include `AND (ttl_expires_at IS NULL OR ttl_expires_at > now())` — rows remain physically present between expiration and the worker sweep, so filtering is the caller responsibility.';
COMMENT ON COLUMN memory_items.source_chat_id IS 'Optional provenance: which chat this memory was extracted from. ON DELETE SET NULL so memory survives chat deletion for audit.';
COMMENT ON COLUMN memory_items.source_message_id IS 'Optional provenance: which specific message triggered this memory.';
COMMENT ON COLUMN memory_items.updated_at IS 'Caller responsibility — no trigger. Same pattern as users.updated_at and groups.updated_at.';

CREATE TABLE memory_embeddings (
    memory_item_id uuid        NOT NULL REFERENCES memory_items(id) ON DELETE CASCADE,
    model          text        NOT NULL CHECK (length(model) BETWEEN 1 AND 256),
    embedding      vector(768) NOT NULL,
    created_at     timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (memory_item_id, model)
);

-- HNSW index for approximate nearest neighbor search, cosine distance.
-- Same config as benches/database-poc/ which benchmarked 5.53ms p95 on 100k vectors.
CREATE INDEX memory_embeddings_embedding_hnsw_idx
    ON memory_embeddings USING hnsw (embedding vector_cosine_ops);

CREATE INDEX memory_embeddings_model_idx ON memory_embeddings(model);

COMMENT ON TABLE memory_embeddings IS 'Dense embedding vectors for memory_items. Separated from the parent to support multiple models per item (PK includes model). HNSW cosine index gives ~5ms p95 top-k for 100k vectors per benchmark in benches/database-poc/ (GAR-373). SECURITY: this table has NO scope_type, group_id, or sensitivity column. Any ANN query MUST JOIN memory_items and filter on scope_type/scope_id/group_id/sensitivity BEFORE using the embedding distance. Querying memory_embeddings without the JOIN exposes cross-tenant vectors and secret-tier memories until migration 007 (GAR-408) RLS is active.';
COMMENT ON COLUMN memory_embeddings.model IS 'Embedding model identifier, e.g. "mxbai-embed-large-v1". Allows side-by-side comparison when migrating between models without re-embedding everything at once. CHECK (length ≤ 256) prevents pathological input.';
COMMENT ON COLUMN memory_embeddings.embedding IS 'vector(768) dimension matches mxbai-embed-large-v1 (Fase 2.1, GAR-372). Different-dim models require a new column or a new table — this migration does not support multi-dim in one column.';
