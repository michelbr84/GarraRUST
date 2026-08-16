-- 032_expire_tus_uploads_sweep.sql
--
-- Issue:    GAR-820 / Issue #820
-- Depends:  migration 014 (tus_uploads, garraia_app grants)
-- Forward-only per CLAUDE.md regra 9.
--
-- Provides a SECURITY DEFINER maintenance function for the uploads expiration
-- worker (`uploads_worker.rs`). Because `tus_uploads` is under FORCE RLS with
-- `tus_uploads_group_isolation` (fail-closed when `app.current_group_id` is
-- unset), a cross-tenant background worker running on `garraia_app` cannot
-- see or update expired rows across all groups using plain table queries.
--
-- The function runs with the privileges of its creator (owner/superuser running
-- the migrations), bypassing RLS on `tus_uploads` in a controlled, encapsulated
-- manner, and is granted EXECUTE to `garraia_app`.

CREATE OR REPLACE FUNCTION expire_tus_uploads_sweep(
    p_now timestamptz,
    p_limit int
)
RETURNS TABLE (
    id            uuid,
    group_id      uuid,
    created_by    uuid,
    object_key    text,
    upload_offset bigint,
    upload_length bigint,
    age_secs      bigint
)
LANGUAGE sql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
    WITH victim AS (
        SELECT t.id
        FROM tus_uploads t
        WHERE t.status = 'in_progress' AND t.expires_at < p_now
        ORDER BY t.expires_at ASC
        LIMIT p_limit
        FOR UPDATE SKIP LOCKED
    )
    UPDATE tus_uploads AS t
    SET status = 'expired', updated_at = now()
    FROM victim
    WHERE t.id = victim.id
    RETURNING t.id, t.group_id, t.created_by, t.object_key,
              t.upload_offset, t.upload_length,
              EXTRACT(EPOCH FROM (now() - t.created_at))::bigint AS age_secs;
$$;

COMMENT ON FUNCTION expire_tus_uploads_sweep(timestamptz, int) IS
    'SECURITY DEFINER maintenance function for the background tus uploads expiration worker. '
    'Sweeps up to p_limit in_progress tus_uploads rows with expires_at < p_now, updates their '
    'status to expired, and returns the transitioned rows for staging file cleanup and audit logging.';

-- Grant EXECUTE to garraia_app so the gateway worker pool can invoke it.
GRANT EXECUTE ON FUNCTION expire_tus_uploads_sweep(timestamptz, int) TO garraia_app;
