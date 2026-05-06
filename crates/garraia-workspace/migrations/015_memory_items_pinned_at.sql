-- 015_memory_items_pinned_at.sql
-- GAR-526 — Migration 015: add pinned_at to memory_items.
-- Plan:     plans/0072-gar-526-memory-pin-slice2.md
-- Depends:  migration 005 (memory_items table)
-- Forward-only. No DROP, no destructive ALTER.

ALTER TABLE memory_items
    ADD COLUMN IF NOT EXISTS pinned_at timestamptz;

-- Partial index for efficient "list all pinned items" queries.
-- Not used by slice 2 directly but avoids a seqscan when slice 3
-- adds GET /v1/memory?pinned=true.
CREATE INDEX IF NOT EXISTS memory_items_pinned_idx
    ON memory_items(group_id, pinned_at DESC)
    WHERE pinned_at IS NOT NULL AND deleted_at IS NULL;

COMMENT ON COLUMN memory_items.pinned_at IS
    'Non-NULL when the item is pinned. Pin atomically sets pinned_at = now() '
    'and clears ttl_expires_at so the item never expires. Unpin sets pinned_at '
    'to NULL; ttl_expires_at is NOT restored — caller must re-set it explicitly. '
    'Pinning is idempotent: re-pinning refreshes pinned_at to now().';
