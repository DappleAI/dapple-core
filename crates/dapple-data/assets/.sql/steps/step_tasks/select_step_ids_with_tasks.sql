SELECT DISTINCT step_id
FROM rev.step_tasks
WHERE step_id = ANY($1::UUID[]);
