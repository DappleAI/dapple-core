SELECT d.id
FROM rev.dispatches d
WHERE d.journey_id = $1::UUID
  AND NOT EXISTS (
      SELECT 1
      FROM rev.dispatch_stages dst
      JOIN rev.dispatch_steps ds ON ds.dispatch_stage_id = dst.id
      JOIN rev.dispatch_task_attempts a ON a.dispatch_step_id = ds.id
      WHERE dst.dispatch_id = d.id
  )
ORDER BY d.created_at;
