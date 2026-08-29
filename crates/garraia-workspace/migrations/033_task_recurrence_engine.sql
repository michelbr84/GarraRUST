-- 033_task_recurrence_engine.sql
--
-- Depends:  migration 006 (tasks, task_lists, FORCE RLS)
-- Forward-only per CLAUDE.md regra 9.
--
-- Migration 006 shipped `tasks.recurrence_rrule` with a charset CHECK and an
-- explicit note that "full parsing and expansion is app-layer responsibility
-- (future recurrence engine)". Nothing ever expanded it: no parser existed and
-- no worker fired on `due_at`. This migration adds the state that engine needs
-- and the SECURITY DEFINER sweep it runs on.
--
-- Two additive columns (no backfill needed — NULL means "not recurring yet"):
--   recurrence_tz          IANA timezone the RRULE is evaluated in. Without it
--                          a 09:00 task drifts by an hour across DST.
--   recurrence_spawned_at  when the next occurrence was last materialised,
--                          making the sweep idempotent if a tick is retried.

ALTER TABLE tasks ADD COLUMN IF NOT EXISTS recurrence_tz text;
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS recurrence_spawned_at timestamptz;

COMMENT ON COLUMN tasks.recurrence_tz IS
    'IANA timezone used to expand recurrence_rrule (default America/New_York when NULL). Charset-only validation here; the engine in garraia-workspace::recurrence does the real parsing.';
COMMENT ON COLUMN tasks.recurrence_spawned_at IS
    'Set when the recurrence engine materialised the following occurrence. Makes the sweep idempotent across retried ticks.';

-- Recurring tasks that are done and still owe their next occurrence.
CREATE INDEX IF NOT EXISTS tasks_recurrence_pending_idx
    ON tasks (completed_at)
    WHERE deleted_at IS NULL
      AND recurrence_rrule IS NOT NULL
      AND recurrence_spawned_at IS NULL;

-- Cross-tenant sweep for the background worker.
--
-- `tasks` is under FORCE RLS (`tasks_group_isolation`, fail-closed when
-- `app.current_group_id` is unset), so a worker on `garraia_app` sees zero
-- rows with a plain query. Same pattern as `expire_tus_uploads_sweep`
-- (migration 032): encapsulated SECURITY DEFINER, granted to garraia_app.
--
-- Read-only on purpose: the function only reports candidates. Creating the
-- next occurrence needs RRULE expansion, which lives in Rust, and the insert
-- is then done per-row with `app.current_group_id` set so the normal RLS
-- WITH CHECK applies.
CREATE OR REPLACE FUNCTION due_task_recurrences(
    p_now   timestamptz,
    p_limit int
)
RETURNS TABLE (
    id                uuid,
    group_id          uuid,
    list_id           uuid,
    title             text,
    description_md    text,
    priority          text,
    due_at            timestamptz,
    recurrence_rrule  text,
    recurrence_tz     text,
    created_by        uuid,
    created_by_label  text
)
LANGUAGE sql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
    SELECT t.id, t.group_id, t.list_id, t.title, t.description_md, t.priority,
           t.due_at, t.recurrence_rrule, t.recurrence_tz,
           t.created_by, t.created_by_label
    FROM tasks t
    WHERE t.deleted_at IS NULL
      AND t.recurrence_rrule IS NOT NULL
      AND t.recurrence_spawned_at IS NULL
      AND t.status = 'done'
      AND t.completed_at IS NOT NULL
      AND t.completed_at <= p_now
    ORDER BY t.completed_at ASC
    LIMIT p_limit;
$$;

COMMENT ON FUNCTION due_task_recurrences(timestamptz, int) IS
    'Cross-tenant read-only sweep for the recurrence worker. SECURITY DEFINER because tasks is under FORCE RLS and the worker runs on garraia_app with no tenant GUC. Returns completed recurring tasks whose next occurrence has not been materialised yet.';

REVOKE ALL ON FUNCTION due_task_recurrences(timestamptz, int) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION due_task_recurrences(timestamptz, int) TO garraia_app;
