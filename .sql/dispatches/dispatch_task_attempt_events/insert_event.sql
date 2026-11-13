INSERT INTO rev.dispatch_task_attempt_events (attempt_id, phase_code, context_data, context_metadata, context_temp_data)
VALUES ($1::uuid, $2::text, $3::jsonb, $4::jsonb, $5::jsonb)
RETURNING
	id,
	attempt_id,
	phase_code,
	context_data,
	context_metadata,
	context_temp_data,
	created_at,
		(
		SELECT d.id FROM rev.dispatch_task_attempts a
		JOIN rev.dispatch_steps s ON a.dispatch_step_id = s.id
		JOIN rev.dispatch_stages st ON s.dispatch_stage_id = st.id
		JOIN rev.dispatches d ON st.dispatch_id = d.id
		WHERE a.id = rev.dispatch_task_attempt_events.attempt_id
	) AS parent_dispatch_object_id;
