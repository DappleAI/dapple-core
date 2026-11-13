INSERT INTO rev.dispatch_task_attempts (dispatch_step_id, task_id, context_data, context_metadata)
VALUES (
  (SELECT id FROM rev.dispatch_steps WHERE id = $1::uuid),
  (SELECT id FROM rev.tasks WHERE id = $2::uuid),
  $3::jsonb,
  $4::jsonb
)
RETURNING
  id,
  dispatch_step_id,
  task_id,
  context_data,
  context_metadata,
  created_at;
