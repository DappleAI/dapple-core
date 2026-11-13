INSERT INTO rev.dispatch_task_attempts (dispatch_step_id, task_id, status, context_data, context_metadata)
VALUES ($1::uuid, $2::uuid, $3::text, $4::jsonb, $5::jsonb)
RETURNING
	id,
	dispatch_step_id,
	task_id,
	status,
	context_data,
	context_metadata,
	created_at,
	(
		SELECT d.id FROM rev.dispatch_steps s
		JOIN rev.dispatch_stages st ON s.dispatch_stage_id = st.id
		JOIN rev.dispatches d ON st.dispatch_id = d.id
		WHERE s.id = rev.dispatch_task_attempts.dispatch_step_id
	) AS parent_dispatch_object_id;
