WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[]
    ) WITH ORDINALITY
    AS o(stage_id, ord)
)
SELECT
    objs.id,
    objs.stage_id,
    objs.step_id,
    t.id AS task_id,
    objs.created_at
FROM rev.stage_steps objs
JOIN rev.stages st ON objs.stage_id = st.id
JOIN rev.steps s ON objs.step_id = s.id
JOIN args ON objs.stage_id = args.stage_id
LEFT JOIN rev.step_tasks s_t ON s_t.step_id = s.id
LEFT JOIN rev.tasks t ON t.id = s_t.task_id
ORDER BY objs.id;
