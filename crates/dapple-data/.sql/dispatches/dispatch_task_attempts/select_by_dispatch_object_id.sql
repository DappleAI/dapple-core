-- selects attempts for a dispatch by dispatch.id
-- The LEFT JOIN LATERAL pulls the latest event so callers see the
-- effective phase/context even when the attempt row is still the
-- "running" placeholder. error_detail is the same event's `error`
-- field when the phase is a failure state, so the UI can render the
-- actual error string without an extra round-trip to the events table.
SELECT
  a.id,
  a.dispatch_step_id,
  a.task_id,
  COALESCE(e.phase_code, a.status) AS status,
  COALESCE(e.context_data, a.context_data) AS context_data,
  COALESCE(e.context_metadata, a.context_metadata) AS context_metadata,
  CASE
    WHEN e.phase_code IN ('failed', 'incomplete')
    THEN e.context_data->>'error'
    ELSE NULL
  END AS error_detail,
  a.created_at
FROM rev.dispatch_task_attempts a
LEFT JOIN LATERAL (
  SELECT phase_code, context_data, context_metadata, created_at
  FROM rev.dispatch_task_attempt_events ev
  WHERE ev.attempt_id = a.id
  ORDER BY ev.created_at DESC, ev.id DESC
  LIMIT 1
) e ON true
WHERE a.dispatch_step_id IN (
  SELECT s.id
  FROM rev.dispatch_steps s
  JOIN rev.dispatch_stages st ON s.dispatch_stage_id = st.id
  JOIN rev.dispatches d ON st.dispatch_id = d.id
  WHERE d.id = $1::uuid
)
ORDER BY a.id DESC;

