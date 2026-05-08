-- 016_task_activity_moved_kind.sql
-- GAR-544 — Migration 016: extend task_activity.kind CHECK with 'moved'.
-- Plan:     plans/0082-gar-544-task-move-api.md
-- Depends:  migration 006 (task_activity table + original CHECK constraint)
-- Forward-only. The default Postgres-named constraint
-- `task_activity_kind_check` was generated inline by migration 006
-- (no `CONSTRAINT name` clause). If the runtime name differs we
-- look it up via pg_constraint instead of failing.

DO $$
DECLARE
    cname text;
BEGIN
    SELECT conname
    INTO cname
    FROM pg_constraint
    WHERE conrelid = 'public.task_activity'::regclass
      AND contype  = 'c'
      AND pg_get_constraintdef(oid) ILIKE '%kind%';

    IF cname IS NULL THEN
        RAISE EXCEPTION 'task_activity kind CHECK constraint not found';
    END IF;

    EXECUTE format(
        'ALTER TABLE task_activity DROP CONSTRAINT %I',
        cname
    );
END
$$;

ALTER TABLE task_activity
    ADD CONSTRAINT task_activity_kind_check
    CHECK (kind IN (
        'created', 'status_changed', 'priority_changed',
        'assigned', 'unassigned', 'labeled', 'unlabeled',
        'commented', 'due_changed', 'archived', 'deleted', 'restored',
        'moved'
    ));

COMMENT ON CONSTRAINT task_activity_kind_check ON task_activity IS
    'Closed set of activity event kinds. ''moved'' added in migration 016 '
    '(GAR-544 task move endpoint). Extending requires a new forward-only '
    'migration following the same DROP/ADD pattern.';
