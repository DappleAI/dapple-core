SELECT id, dispatch_step_id, task_id, status, context_data, context_metadata, created_at
FROM rev.dispatch_task_attempts
WHERE id = $1::uuid;
