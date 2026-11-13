WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],   -- dispatch_step_id[]
        $2::UUID[]    -- step_task_id[]
    ) WITH ORDINALITY AS o(dispatch_step_id, step_task_id, ord)
),
returned AS (
    INSERT INTO rev.dispatch_tasks (dispatch_step_id, step_task_id)
    SELECT ds.id, stk.id
    FROM args a
    JOIN rev.dispatch_steps ds ON a.dispatch_step_id = ds.id
    JOIN rev.step_tasks stk ON a.step_task_id = stk.id
    ORDER BY a.ord
    RETURNING *
)
SELECT
    returned.*,
    stk.task_id AS task_id
FROM returned
JOIN rev.dispatch_steps ds ON returned.dispatch_step_id = ds.id
JOIN rev.step_tasks stk ON returned.step_task_id = stk.id
JOIN args a ON ds.id = a.dispatch_step_id
             AND stk.id = a.step_task_id
ORDER BY returned.dispatch_step_id, returned.id;
