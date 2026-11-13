WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[]
    ) WITH ORDINALITY
    AS o(step_id)
)
SELECT
    objs.id,
    s.id AS step_id,
    t.id AS task_id,
    objs.created_at
FROM rev.step_tasks objs
JOIN rev.steps s ON objs.step_id = s.id
JOIN args ON s.id = args.step_id
JOIN rev.tasks t ON t.id = objs.task_id
ORDER BY objs.step_id, objs.ord;