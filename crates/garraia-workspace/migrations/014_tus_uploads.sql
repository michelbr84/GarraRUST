-- Plan:     plans/0041-gar-395-tus-slice1.md
-- Issue:    GAR-395
-- Depends:  migration 001 (groups, users), migration 007 (RLS baseline),
--           migration 008 (garraia_app grants framework).
-- Forward-only per CLAUDE.md regra 9 (add columns/tables, never drop).
--
-- Creates the `tus_uploads` table backing the tus 1.0 resumable upload
-- server. Slice 1 of GAR-395 reserves the upload *resource* — the
-- blob-append path (PATCH) and ObjectStore commit land in slice 2.
--
-- Design notes (plan 0041 §5):
--
-- - `object_key` is allocated server-side in the shape
--   `{group_id}/uploads/{upload_id}/v1` (ADR 0004 §Key schema compatible)
--   and marked UNIQUE so concurrent upload creation can never collide.
-- - `upload_length` is hard-capped at 5 GiB (5 * 1024^3 = 5368709120)
--   mirroring `files.size_bytes` from migration 003, so a completed
--   upload is never refused when it's written into `file_versions`.
-- - `upload_offset` is CHECK'd against `upload_length` — schema-level
--   guard complementing the app-layer append-only invariant that lands
--   in slice 2.
-- - FORCE RLS + `tus_uploads_group_isolation` policy (explicit WITH
--   CHECK clause, migration 013 pattern) blocks cross-tenant reads AND
--   writes. The `WITH CHECK` is identical to `USING` so an accidental
--   `AS RESTRICTIVE` conversion stays permissive on both sides.
-- - `expires_at` is populated server-side (plan 0041 §5.5) — slice 3
--   will add the purge worker.

CREATE TABLE tus_uploads (
    id               uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    group_id         uuid        NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    created_by       uuid        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    object_key       text        NOT NULL UNIQUE
                                 CHECK (length(object_key) BETWEEN 1 AND 1024),
    upload_length    bigint      NOT NULL
                                 CHECK (upload_length > 0 AND upload_length <= 5368709120),
    upload_offset    bigint      NOT NULL DEFAULT 0
                                 CHECK (upload_offset >= 0 AND upload_offset <= upload_length),
    upload_metadata  text,
    filename         text        CHECK (filename IS NULL OR length(filename) BETWEEN 1 AND 500),
    mime_type        text        CHECK (mime_type IS NULL OR length(mime_type) BETWEEN 1 AND 200),
    status           text        NOT NULL DEFAULT 'in_progress'
                                 CHECK (status IN ('in_progress', 'completed', 'aborted', 'expired')),
    expires_at       timestamptz NOT NULL,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    -- Compound UNIQUE so a future FK from `files(id, group_id)` can
    -- reference (id, group_id) pattern used by migrations 003/004.
    CONSTRAINT tus_uploads_id_group_unique UNIQUE (id, group_id)
);

CREATE INDEX tus_uploads_group_status_idx
    ON tus_uploads(group_id, status);

-- Partial index supports the slice-3 purge job
-- (`DELETE FROM tus_uploads WHERE status = 'in_progress' AND expires_at < now()`).
CREATE INDEX tus_uploads_expires_in_progress_idx
    ON tus_uploads(expires_at)
    WHERE status = 'in_progress';

COMMENT ON TABLE tus_uploads IS
    'tus 1.0 resumable upload ledger. Slice 1 (plan 0041): Creation + HEAD reserve the row; PATCH + ObjectStore commit land in slice 2. Row is authoritative for upload state; ObjectStore is only hit on completion.';
COMMENT ON COLUMN tus_uploads.object_key IS
    'Server-allocated object key under which the blob will be flushed at completion (slice 2). Format: `{group_id}/uploads/{upload_id}/v1` — see ADR 0004 §Key schema.';
COMMENT ON COLUMN tus_uploads.upload_metadata IS
    'Raw tus `Upload-Metadata` header value (comma-separated `key base64-value` pairs). May contain PII (filename); never log inline — redact via tracing `fields(skip)`.';
COMMENT ON COLUMN tus_uploads.status IS
    'in_progress (default, bytes still incoming) → completed (ObjectStore commit OK) | aborted (DELETE termination ext, slice 3) | expired (purge worker, slice 3).';

-- ─── RLS — ENABLE + FORCE + group-isolation policy ──────────────────────
--
-- Mirrors the migration 003 pattern for `files`/`folders`/`file_versions`
-- (direct class, `app.current_group_id` context, NULLIF fail-closed,
-- explicit WITH CHECK identical to USING).

ALTER TABLE tus_uploads ENABLE ROW LEVEL SECURITY;
ALTER TABLE tus_uploads FORCE ROW LEVEL SECURITY;

CREATE POLICY tus_uploads_group_isolation ON tus_uploads
    AS PERMISSIVE
    FOR ALL
    USING (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid)
    WITH CHECK (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid);

COMMENT ON POLICY tus_uploads_group_isolation ON tus_uploads IS
    'Class: direct. Context: app.current_group_id. Fail-closed via NULLIF (empty GUC → NULL → no visible rows). USING and WITH CHECK identical (migration 013 pattern); a future AS RESTRICTIVE conversion must touch both deliberately.';

-- ─── garraia_app grants ──────────────────────────────────────────────────
GRANT SELECT, INSERT, UPDATE, DELETE ON tus_uploads TO garraia_app;
