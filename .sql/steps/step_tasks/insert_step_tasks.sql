WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::UUID[]
    ) WITH ORDINALITY
    AS o(
        step_id,
        task_id,
        ord
    )
),
-- Resolve IDs with ordinality preserved
resolved AS (
    SELECT
        args.ord,
        s.id AS step_id,
        t.id AS task_id
    FROM args
    JOIN rev.steps s ON args.step_id = s.id
    JOIN rev.tasks t ON args.task_id = t.id
),
objs AS (
    INSERT INTO rev.step_tasks (step_id, task_id)
    SELECT r.step_id, r.task_id
    FROM resolved r
    ORDER BY r.ord
    RETURNING *
),
-- Number objs to preserve ordinality
numbered_objs AS (
    SELECT o.*, row_number() OVER (ORDER BY o.id) AS ord
    FROM objs o
)
SELECT
    no.id,
    no.step_id,
    no.task_id,
    no.created_at
FROM numbered_objs no
JOIN resolved r ON r.ord = no.ord
ORDER BY no.id;