INSERT INTO rev.dispatch_task_attempt_events (attempt_id, phase_code, context_data, context_metadata, context_temp_data)
VALUES (
  (SELECT id FROM rev.dispatch_task_attempts WHERE id = $1::uuid),
  $2::text,
  $3::jsonb,
  $4::jsonb,
  $5::jsonb
)
RETURNING
  id,
  attempt_id,
  phase_code,
  context_data,
  context_metadata,
  context_temp_data,
  created_at;
