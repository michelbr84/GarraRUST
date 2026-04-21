-- 003_files_and_folders.sql
-- GAR-387 — Migration 003: folders, files, file_versions (v1 slice of Fase 3.5).
-- Plan:     plans/0033-gar-387-migration-003-files.md
-- Depends:  migration 001 (users, groups), ADR 0004 (object storage).
-- Unblocks: GAR-394 (crate garraia-storage), GAR-395 (tus),
--           message_attachments / task_attachments (deferred migrations).
-- Forward-only. No DROP, no destructive ALTER.
--
-- ─── Scope decisions ──────────────────────────────────────────────────────
--
-- IN scope (3 tables with ENABLE + FORCE + CREATE POLICY with explicit
-- WITH CHECK):
--   folders, files, file_versions
--
-- OUT of scope:
--   file_shares            → deferred, ADR 0004 v1 drops sharing
--   message_attachments    → deferred, separate migration when
--                            garraia-storage contract is stable
--   task_attachments       → deferred, same reason
--   Object content itself  → not in DB (lives in S3/MinIO/LocalFs via GAR-394)
--   Presigned URL handlers → gateway layer, GAR-394+
--
-- ─── Numbering gap (003 after 004..013) ───────────────────────────────────
--
-- Slot 003 was reserved by migration 004 (comment: "slot 003 intentionally
-- reserved for GAR-387 (files) once ADR 0004 (object storage) is written").
-- sqlx::migrate! iterates migrations in source-order and applies any
-- version not present in _sqlx_migrations. A fresh database (CI test-
-- container, new deploy) applies 001, 002, 003, 004, ..., 013 in order.
-- An existing database with 004..013 already applied will pick up 003 on
-- the next migrate run without reordering — verified against sqlx-core
-- 0.8.6 src/migrate/migrator.rs:173-182.
--
-- ─── Role dependency ──────────────────────────────────────────────────────
--
-- Migration 003 runs BEFORE 007 in lexicographic order. Therefore:
--   1. The `garraia_app NOLOGIN` role does not yet exist when 003 runs
--      on a fresh database — we create it idempotently (same DO block
--      pattern as migrations 006 and 007).
--   2. The `ALTER DEFAULT PRIVILEGES ... TO garraia_app` from 007 only
--      covers tables created AFTER 007 runs. We explicitly GRANT
--      SELECT/INSERT/UPDATE/DELETE on the 3 new tables.
-- Same pattern as migration 006.

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'garraia_app') THEN
        CREATE ROLE garraia_app NOLOGIN;
    END IF;
END
$$;

-- ─── 3.1 folders ──────────────────────────────────────────────────────────

CREATE TABLE folders (
    id               uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    group_id         uuid        NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    parent_id        uuid        REFERENCES folders(id) ON DELETE CASCADE,
    name             text        NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    created_by       uuid        REFERENCES users(id) ON DELETE SET NULL,
    created_by_label text        NOT NULL CHECK (length(created_by_label) BETWEEN 1 AND 200),
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    deleted_at       timestamptz,
    CONSTRAINT folders_id_group_unique UNIQUE (id, group_id)
);

CREATE INDEX folders_group_parent_idx
    ON folders(group_id, parent_id)
    WHERE deleted_at IS NULL;

CREATE INDEX folders_parent_idx
    ON folders(parent_id)
    WHERE parent_id IS NOT NULL AND deleted_at IS NULL;

-- Unique folder name within a parent (scoped to group). Root-level folders
-- (parent_id IS NULL) coalesce to a fixed sentinel UUID so the partial
-- unique index still fires. Soft-deleted folders drop out so the user can
-- recreate a name after restoring a sibling.
--
-- Sentinel safety: '00000000-0000-0000-0000-000000000000' is the RFC 4122
-- nil UUID. gen_random_uuid() NEVER returns the nil UUID, so no real folder
-- can have id = nil, which means the sentinel cannot collide with a real
-- parent_id. If a future restore/backup tool inserts rows with explicit ids,
-- it MUST preserve this invariant — otherwise a folder with parent_id=nil
-- would cause siblings of root folders in the same group to incorrectly
-- collide with it.
CREATE UNIQUE INDEX folders_unique_name_per_parent_idx
    ON folders(group_id, COALESCE(parent_id, '00000000-0000-0000-0000-000000000000'::uuid), name)
    WHERE deleted_at IS NULL;

COMMENT ON TABLE folders IS 'Per-group folder tree. parent_id IS NULL means root of the group. Cross-group drift is prevented by files.folder_id compound FK, and by the subtree invariant that parent_id must reference a folder in the same group — enforcement is app-layer (no composite self-FK because Postgres does not support (id, group_id) self-reference without triggers). RLS direct policy via group_id denormalized, WITH CHECK explicit.';
COMMENT ON COLUMN folders.parent_id IS 'Self-FK for subfolders. ON DELETE CASCADE: deleting a folder hard-deletes its subtree. App layer must validate that parent.group_id = child.group_id at insert time (schema does not — compound self-FK is non-trivial). GAR-394 SHOULD include an audit query: SELECT c.id FROM folders c JOIN folders p ON c.parent_id = p.id WHERE c.group_id <> p.group_id AND c.deleted_at IS NULL — cross-group drift indicates an app-layer bug.';
COMMENT ON COLUMN folders.created_by IS 'Nullable FK with ON DELETE SET NULL. GDPR erasure survival: hard-deleting a user does not violate the FK; folder row survives with created_by = NULL and attribution preserved via created_by_label.';
COMMENT ON COLUMN folders.created_by_label IS 'Cached display_name at creation — preserved across user hard-delete. Same pattern as messages.sender_label / tasks.created_by_label.';
COMMENT ON COLUMN folders.deleted_at IS 'Soft delete. Subfolders and files inside a soft-deleted folder are NOT automatically hidden at the DB layer — the UI query filters by WHERE deleted_at IS NULL on every level. Hard DELETE cascades to subfolders and files (via files.folder_id compound FK ON DELETE SET NULL).';

-- ─── 3.2 files ────────────────────────────────────────────────────────────

CREATE TABLE files (
    id               uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    group_id         uuid        NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    folder_id        uuid,
    name             text        NOT NULL CHECK (length(name) BETWEEN 1 AND 500),
    current_version int         NOT NULL DEFAULT 1 CHECK (current_version >= 1),
    total_versions   int         NOT NULL DEFAULT 1 CHECK (total_versions >= 1),
    size_bytes       bigint      NOT NULL CHECK (size_bytes >= 0 AND size_bytes <= 5368709120),
    mime_type        text        NOT NULL CHECK (length(mime_type) BETWEEN 1 AND 200),
    settings         jsonb       NOT NULL DEFAULT '{}'::jsonb,
    created_by       uuid        REFERENCES users(id) ON DELETE SET NULL,
    created_by_label text        NOT NULL CHECK (length(created_by_label) BETWEEN 1 AND 200),
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    deleted_at       timestamptz,
    -- Compound FK: folder_id MUST live in the same group_id as the file.
    -- Cross-group drift is structurally impossible at the DB layer — mirrors
    -- the pattern in messages (chat_id, group_id) and tasks (list_id, group_id).
    -- MATCH SIMPLE semantics (SQL default): when folder_id IS NULL the FK
    -- predicate is vacuously satisfied, so root-level files (no folder)
    -- bypass the compound constraint by design — which is exactly what we
    -- want. If MATCH FULL were ever desired (all-or-nothing), it would
    -- require rejecting root files entirely, which contradicts ADR 0004
    -- §Key schema.
    FOREIGN KEY (folder_id, group_id) REFERENCES folders(id, group_id) ON DELETE SET NULL,
    CONSTRAINT files_id_group_unique UNIQUE (id, group_id),
    -- Single-row multi-column CHECK — Postgres evaluates on every INSERT
    -- and UPDATE to this table. It enforces the pointer invariant (current
    -- version index cannot exceed the total count) at the schema layer.
    -- The cross-table invariant current_version = MAX(file_versions.version
    -- WHERE file_id = files.id) is app-layer only: a schema CHECK cannot
    -- reference another table without a trigger. GAR-394 CRUD must maintain
    -- both bumps atomically in a single transaction.
    CONSTRAINT files_current_le_total CHECK (current_version <= total_versions)
);

CREATE INDEX files_group_folder_idx
    ON files(group_id, folder_id)
    WHERE deleted_at IS NULL;

CREATE INDEX files_group_created_idx
    ON files(group_id, created_at DESC)
    WHERE deleted_at IS NULL;

CREATE INDEX files_folder_idx
    ON files(folder_id)
    WHERE folder_id IS NOT NULL AND deleted_at IS NULL;

COMMENT ON TABLE files IS 'Per-group file entity. One row per logical file; content and per-version metadata live in file_versions. current_version points at the active version; total_versions caches the count for O(1) UI. Soft delete via deleted_at. Compound FK (folder_id, group_id) → folders(id, group_id) prevents cross-group folder assignment. RLS direct policy via denormalized group_id, WITH CHECK explicit.';
COMMENT ON COLUMN files.folder_id IS 'NULL = root of group (ADR 0004 §Key schema: root files have object_key {group_id}/{file_uuid}/vN). ON DELETE SET NULL so hard-deleting a folder does not cascade-wipe its files — they become root-level and remain recoverable.';
COMMENT ON COLUMN files.current_version IS 'Pointer to the active version in file_versions. Must satisfy current_version <= total_versions (CHECK) and should equal MAX(file_versions.version WHERE file_id = files.id AND version <= current_version) — the latter is an app-layer invariant. GAR-394 CRUD must bump current_version and total_versions atomically when a new version is uploaded. Audit query: SELECT f.id FROM files f JOIN (SELECT file_id, MAX(version) mv, COUNT(*) ct FROM file_versions GROUP BY file_id) v ON f.id = v.file_id WHERE f.current_version <> v.mv OR f.total_versions <> v.ct;';
COMMENT ON COLUMN files.size_bytes IS 'Size of the CURRENT version. Denormalized from file_versions.size_bytes for fast UI. CHECK 0..5368709120 (5 GiB) is a DoS mitigation — prevents storage amplification from a single malicious PATCH. Individual versions may be larger if app layer permits, but runtime cap is already enforced in ADR 0004 §Security Policy.';
COMMENT ON COLUMN files.mime_type IS 'MIME type of the CURRENT version. Denormalized from file_versions. Allow-list enforcement lives in GAR-394 (crate garraia-storage); schema accepts any 1..200 char string for future-proofing.';
COMMENT ON COLUMN files.settings IS 'Per-file settings blob. Reserved for: {"encryption":"sse-s3"|"sse-kms"|"operator","retention_override_days":int}. Schema does NOT validate shape — validation is GAR-394 responsibility.';
COMMENT ON COLUMN files.created_by IS 'Nullable FK with ON DELETE SET NULL. GDPR erasure survival.';
COMMENT ON COLUMN files.created_by_label IS 'Cached display_name at creation. Preserved across user hard-delete.';
COMMENT ON COLUMN files.deleted_at IS 'Soft delete. file_versions remain queryable for audit and restoration — deleted_at does NOT cascade to file_versions. Hard DELETE cascades to file_versions via ON DELETE CASCADE compound FK on the version side.';

-- ─── 3.3 file_versions ────────────────────────────────────────────────────

CREATE TABLE file_versions (
    file_id          uuid        NOT NULL,
    group_id         uuid        NOT NULL,
    version          int         NOT NULL CHECK (version >= 1),
    object_key       text        NOT NULL CHECK (length(object_key) BETWEEN 1 AND 1024),
    etag             text        NOT NULL CHECK (length(etag) BETWEEN 1 AND 200),
    checksum_sha256  text        NOT NULL CHECK (checksum_sha256 ~ '^[0-9a-f]{64}$'),
    integrity_hmac   text        NOT NULL CHECK (integrity_hmac ~ '^[0-9a-f]{64}$'),
    size_bytes       bigint      NOT NULL CHECK (size_bytes >= 0 AND size_bytes <= 5368709120),
    mime_type        text        NOT NULL CHECK (length(mime_type) BETWEEN 1 AND 200),
    created_by       uuid        REFERENCES users(id) ON DELETE SET NULL,
    created_by_label text        NOT NULL CHECK (length(created_by_label) BETWEEN 1 AND 200),
    created_at       timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (file_id, version),
    -- Compound FK: version's group_id MUST match the parent file's group_id.
    -- ON DELETE CASCADE: hard-deleting the file wipes all versions (GDPR
    -- right-to-erasure path when app layer issues DELETE FROM files).
    FOREIGN KEY (file_id, group_id) REFERENCES files(id, group_id) ON DELETE CASCADE,
    -- Global uniqueness of object_key: ADR 0004 key schema is globally
    -- collision-free by construction ({group_id}/{file_uuid}/v{N}), and a
    -- UNIQUE index converts accidental overwrite bugs into 23505 errors
    -- rather than silent object clobbering in the storage backend.
    CONSTRAINT file_versions_object_key_key UNIQUE (object_key)
);

CREATE INDEX file_versions_file_idx
    ON file_versions(file_id, version DESC);

CREATE INDEX file_versions_group_created_idx
    ON file_versions(group_id, created_at DESC);

COMMENT ON TABLE file_versions IS 'Immutable per-version metadata row. One row per uploaded version; never UPDATE in the happy path — new uploads INSERT new rows with monotonically increasing version numbers. group_id is denormalized for RLS direct policy; compound FK (file_id, group_id) → files(id, group_id) keeps it consistent. ON DELETE CASCADE wipes versions when the parent file is hard-deleted (GDPR art. 17 / LGPD art. 18 path). Soft delete of files.deleted_at does NOT affect this table — versions remain queryable for audit and potential restore. RLS direct policy via group_id, WITH CHECK explicit.';
COMMENT ON COLUMN file_versions.version IS 'Monotonic version number starting at 1. App layer (GAR-394) enforces monotonicity — schema allows any int >= 1 but the PRIMARY KEY (file_id, version) catches duplicate inserts as 23505.';
COMMENT ON COLUMN file_versions.object_key IS 'S3/LocalFs object key. ADR 0004 §Key schema: "{group_id}/{folder_path}/{file_uuid}/v{N}". Sanitization rules (allow-list charset, no .. traversal, no // empty components, max 512 chars for folder_path) are enforced in GAR-394 ObjectKey::new() before insert — the schema accepts any 1..1024 char string to avoid double-regex overhead.';
COMMENT ON COLUMN file_versions.etag IS 'S3 ETag returned by the backend after successful put. Opaque string; do NOT parse. Length 1..200 is deliberately permissive because S3 emits variable formats (MD5 hex for single-part, md5-N for multipart, custom for R2/B2) and LocalFs emits hex(sha256(body))[:32]. Short test fixtures (e.g., "abc123") pass the CHECK on purpose — in production, GAR-394 always writes a real backend-provided ETag. Used as weak cache key; integrity_hmac is the authoritative tamper check.';
COMMENT ON COLUMN file_versions.checksum_sha256 IS 'Lowercase hex SHA-256 of object content. Computed at upload time. CHECK regex ^[0-9a-f]{64}$ is lowercase-only: callers MUST normalize to lowercase before INSERT (documented contract). Any content-integrity verification on read MUST recompute and compare byte-for-byte — ETag alone is insufficient.';
COMMENT ON COLUMN file_versions.integrity_hmac IS 'Lowercase hex HMAC-SHA256 over "{object_key}:{version}:{checksum_sha256}" signed by the server-side key (ADR 0004 §Security Policy 4). A tamper of the blob in the storage backend does NOT match — the HMAC recomputed with the server key fails verification. Critical for LocalFs (which has no server-side integrity). Key material rotation path: add new HMAC column when rotation is needed (forward-only).';
COMMENT ON COLUMN file_versions.size_bytes IS 'Size of THIS version in bytes. CHECK 0..5368709120 (5 GiB) aligns with files.size_bytes runtime cap.';
COMMENT ON COLUMN file_versions.mime_type IS 'MIME type of THIS version. Allow-list enforcement is GAR-394 (garraia-storage) responsibility. Per-version MIME allows a file name to stay stable while content type evolves (e.g., PDF → PDF/A).';
COMMENT ON COLUMN file_versions.created_by IS 'Who uploaded this version. ON DELETE SET NULL for GDPR erasure — the version survives but loses attribution.';
COMMENT ON COLUMN file_versions.created_by_label IS 'Cached display_name at upload time. Preserved across user hard-delete.';

-- ─── 3.4 RLS — ENABLE + FORCE + 3 policies with explicit WITH CHECK ──────
--
-- All 3 policies use the "direct" class: they compare the denormalized
-- group_id column against the app.current_group_id GUC, fail-closed via
-- NULLIF when the GUC is unset. WITH CHECK is IDENTICAL to USING — this
-- is the pattern established by migration 013 (see its header) to defend
-- against a future AS RESTRICTIVE regression where an implicit WITH CHECK
-- would silently become TRUE (permissive total).
--
-- If a file is moved between folders, group_id MUST remain constant —
-- cross-group move is structurally impossible because the compound FK
-- (folder_id, group_id) would reject the UPDATE.

ALTER TABLE folders ENABLE ROW LEVEL SECURITY;
ALTER TABLE folders FORCE ROW LEVEL SECURITY;

CREATE POLICY folders_group_isolation ON folders
    AS PERMISSIVE
    FOR ALL
    USING (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid)
    WITH CHECK (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid);

COMMENT ON POLICY folders_group_isolation ON folders IS
    'Class: direct. Context: app.current_group_id. Fail-closed via NULLIF (empty GUC → NULL → no visible rows). USING and WITH CHECK are identical (pattern from migration 013) — any future conversion to AS RESTRICTIVE must touch both sides deliberately.';

ALTER TABLE files ENABLE ROW LEVEL SECURITY;
ALTER TABLE files FORCE ROW LEVEL SECURITY;

CREATE POLICY files_group_isolation ON files
    AS PERMISSIVE
    FOR ALL
    USING (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid)
    WITH CHECK (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid);

COMMENT ON POLICY files_group_isolation ON files IS
    'Class: direct. Context: app.current_group_id. Fail-closed via NULLIF. USING and WITH CHECK identical (migration 013 pattern). Compound FK (folder_id, group_id) → folders(id, group_id) additionally enforces that moving a file to a folder in a different group is structurally impossible — the FK validation rejects the UPDATE before RLS even runs.';

ALTER TABLE file_versions ENABLE ROW LEVEL SECURITY;
ALTER TABLE file_versions FORCE ROW LEVEL SECURITY;

CREATE POLICY file_versions_group_isolation ON file_versions
    AS PERMISSIVE
    FOR ALL
    USING (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid)
    WITH CHECK (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid);

COMMENT ON POLICY file_versions_group_isolation ON file_versions IS
    'Class: direct. Context: app.current_group_id. group_id is denormalized from files.group_id and kept in sync by the compound FK (file_id, group_id) → files(id, group_id). USING and WITH CHECK identical (migration 013 pattern).';

-- ─── 3.5 Grants for garraia_app ───────────────────────────────────────────
--
-- ALTER DEFAULT PRIVILEGES from migration 007 does NOT cover these tables
-- because 003 runs BEFORE 007 in lexicographic order. Same pattern as
-- migration 006.

GRANT SELECT, INSERT, UPDATE, DELETE ON folders, files, file_versions TO garraia_app;
