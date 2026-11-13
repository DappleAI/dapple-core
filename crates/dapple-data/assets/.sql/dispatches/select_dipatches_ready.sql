WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(dispatch_id, ord)
)
SELECT
    dt.id AS id,
    ds.id AS dispatch_step_id,
    t.id AS task_id,
    dt.created_at,
    a.ord
FROM rev.dispatch_tasks dt
JOIN rev.dispatch_steps ds ON dt.dispatch_step_id = ds.id
JOIN rev.dispatches d ON ds.dispatch_id = d.id
JOIN rev.tasks t ON dt.task_id = t.id
JOIN args a ON d.id = a.dispatch_id
ORDER BY a.ord;
