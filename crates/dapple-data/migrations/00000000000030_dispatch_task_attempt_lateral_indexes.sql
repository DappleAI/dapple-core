-- ============================================================================
-- Composite indexes supporting the nested-LATERAL latest-attempt /
-- latest-event lookups in select_dispatches_by_object_id.sql.
--
-- The query repeatedly evaluates, per (dispatch_step_id, task_id) pair:
--
--   SELECT phase from latest dispatch_task_attempts row matching
--   (dispatch_step_id, task_id) ordered by created_at DESC,
--   then for that attempt's id, the latest dispatch_task_attempt_events
--   row ordered by (created_at DESC, id DESC).
--
-- The pre-existing single-column indexes (dispatch_step_id) and (task_id)
-- on dispatch_task_attempts force PG to fetch every matching row by step,
-- filter on task_id in memory, then sort by created_at — quadratic-ish
-- behaviour on a large dispatch tree. Same shape on dispatch_task_attempt_events.
--
-- The composite indexes below let the planner do a single index seek per
-- LATERAL invocation instead.
-- ============================================================================

CREATE INDEX dispatch_task_attempts_step_task_created_idx
    ON rev.dispatch_task_attempts (dispatch_step_id, task_id, created_at DESC);

CREATE INDEX dispatch_task_attempt_events_attempt_created_id_idx
    ON rev.dispatch_task_attempt_events (attempt_id, created_at DESC, id DESC);
