SELECT id, attempt_id, phase_code, context_data, context_metadata, context_temp_data, created_at
FROM rev.dispatch_task_attempt_events
WHERE attempt_id = (
  SELECT id FROM rev.dispatch_task_attempts WHERE id = $1::uuid
)
ORDER BY id ASC;
